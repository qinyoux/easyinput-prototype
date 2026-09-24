//! 执行器（Driver）契约、校验与注册表。
//!
//! # 第一性原理：这里为什么用「枚举分派」而不是 `trait + dyn`
//!
//! 方案文档把它写成了 `#[async_trait] trait AgentDriver`。落地时改了，理由如下：
//!
//! 1. **`async fn in trait` 不 dyn-safe。** Rust 稳定版至今不允许 `dyn Trait` 里出现
//!    `async fn`。要做 trait object 就得引入 `async_trait` crate，Cargo.toml 里没有它——
//!    为一层抽象加一个新依赖、还让每个方法都多一次装箱，不划算。
//! 2. **"形态"是封闭集，"Agent"才是开放集。** 这是关键的正交划分：
//!    - 形态：只有 `Api`（HTTP）和 `Cli`（子进程）两种 —— `DriverKind` 枚举本身就是证明。
//!    - Agent：bl / qwen / codex / claude / opencode / cursor … **全部属于 Cli 形态**，
//!      差异只在命令行模板与完成信号。
//! 3. **封闭集用枚举，开放集用配置。** 枚举带来编译期穷尽匹配——以后加第三种形态
//!    （比如 WebSocket driver），编译器会**强制**你在每个分派点处理它，不会静默漏掉。
//!    而 `dyn` 反而会把"有哪些 driver"从配置搬回代码，违背"加一行 JSON"的设计目标。
//!
//! ⇒ 方案 §2.3 的目标「加新 Agent = 加一行 JSON，不写 Rust 代码」**依然完全达成**，
//! 因为换 Agent 只改 [`crate::agent_desk::config::DriverConfig`]。
//!
//! # 世界门禁放在这一层
//!
//! 「信号从哪来」（Api / Cli）与「要不要过门禁」是**正交**的两件事，
//! 所以门禁不写在各自 driver 里，而是由 [`run_driver`] 统一施加。
//! 这样两种形态共用一套完成语义，不会出现"Api 有门禁、Cli 忘了"这类不一致。

pub mod api_driver;
pub mod cli_driver;

use serde::Serialize;

use crate::agent_desk::config::{CompletionKind, Confidence, DriverConfig, DriverKind};
use crate::agent_desk::queue::TaskId;
use crate::log_buffer;
use crate::runner::{self, CancelToken, RunnerOptions};

/// 默认执行超时（毫秒）。3 分钟——足够一次长文本生成，又不至于让槽位被永久占住。
pub const DEFAULT_TIMEOUT_MS: u64 = 180_000;

/// 世界门禁命令的超时（毫秒）。
///
/// 门禁自己也是子进程（跑测试、查文件、探测产物），给 60 秒足够；
/// 再长说明这条门禁本身有问题，不该拖着槽位不放。
pub const EVIDENCE_TIMEOUT_MS: u64 = 60_000;

// ---------------------------------------------------------------------------
// 请求 / 结果
// ---------------------------------------------------------------------------

/// 一次执行请求。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunRequest {
    /// 归属槽位（0 基）。
    pub slot: usize,
    /// 任务号（回执时用来校验，防迟到）。
    pub task_id: TaskId,
    /// 任务文本（来自 ASR）。
    pub prompt: String,
    /// 超时（毫秒）。0 表示用 [`DEFAULT_TIMEOUT_MS`]。
    pub timeout_ms: u64,
    /// 续接的会话 id。**`Some` 时 driver 应尽量在同一个会话上继续**。
    ///
    /// ⚠️ 这是"尽量"而不是"必须"：driver 是否支持续接由配置里的
    /// `continuation` 决定。带进来它却不支持 ⇒ 静默退化成新会话，
    /// 所以上层不必先自己去查配置。
    pub session_id: Option<String>,
}

impl RunRequest {
    pub fn new(slot: usize, task_id: TaskId, prompt: impl Into<String>) -> Self {
        Self {
            slot,
            task_id,
            prompt: prompt.into(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            session_id: None,
        }
    }

    /// 生效的超时（0 → 默认值）。
    pub fn effective_timeout_ms(&self) -> u64 {
        if self.timeout_ms == 0 {
            DEFAULT_TIMEOUT_MS
        } else {
            self.timeout_ms
        }
    }
}

/// 执行结束时进程/请求的终态。
///
/// ⚠️ **`Completed` 只表示"这个 driver 认为它做完了"**，它是否真的可靠由
/// [`RunOutcome::confidence`] 表达。把这两件事混成一个字段，是"虚假完成"的温床。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// 按完成信号判定为完成。
    Completed,
    /// 失败（含门禁未通过）。
    Failed,
    /// 被调用方取消。
    Cancelled,
    /// 超时。
    TimedOut,
}

impl RunStatus {
    /// 中文标签。
    ///
    /// 用在哪里：慢任务的可读性靠它——日志与调试面板显示的都是它，
    /// 而不是 `Debug` 出来的 `TimedOut` 这种英文枚举名。
    pub fn label(&self) -> &'static str {
        match self {
            RunStatus::Completed => "已完成",
            RunStatus::Failed => "失败",
            RunStatus::Cancelled => "已取消",
            RunStatus::TimedOut => "超时",
        }
    }
}

/// 世界门禁的执行结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EvidenceResult {
    /// 实际执行的命令。
    pub cmd: String,
    /// 是否通过（退出码 0）。
    pub passed: bool,
    /// 退出码。`None` = 被信号杀（超时）。
    pub exit_code: Option<i32>,
    /// 输出摘要（stdout + stderr 截断），失败时给用户看"为什么没通过"。
    pub detail: String,
    pub duration_ms: u64,
}

/// 一次执行的完整结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RunOutcome {
    pub status: RunStatus,
    /// 失败原因（`status == Failed` 时有意义）。
    pub reason: Option<String>,
    /// 原始输出：Cli 是 stdout，Api 是模型全文。
    pub output: String,
    pub duration_ms: u64,
    /// 完成可信度。**门禁通过会被升为 `High`**，见 [`run_driver`]。
    pub confidence: Confidence,
    /// 门禁执行详情（没配门禁时为 `None`）。
    pub evidence: Option<EvidenceResult>,
    /// 本次运行所用的会话 id（**用来给下一轮续接**）。
    ///
    /// 由 driver 从自己的输出里抽出来回传 —— 只有 driver 知道 id 长什么样、
    /// 藏在哪一行。上层把它存进"某个槽位的当前会话"，下一次任务就带上它。
    pub session_id: Option<String>,
}

impl RunOutcome {
    /// 构造一个失败结果。
    pub fn failed(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            status: RunStatus::Failed,
            reason: Some(reason),
            output: String::new(),
            duration_ms: 0,
            confidence: Confidence::Medium,
            evidence: None,
            session_id: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 事件出口
// ---------------------------------------------------------------------------

/// 执行过程中的实时事件出口。
///
/// 用 trait 而不是闭包：闭包无法在 `run_stream` 的 `FnMut` 位置跨 driver 复用，
/// 而且 trait 让"没有接收方"（单测、后台任务）可以显式地用 [`NullSink`]，
/// 而不是到处写 `|_| {}`。
pub trait EventSink: Send {
    /// 一行 stdout（已去换行）。
    fn stdout_line(&mut self, line: &str);
    /// 一行 stderr。
    fn stderr_line(&mut self, line: &str) {
        let _ = line; // 默认丢弃；绝大多数场景只关心 stdout。
    }
}

/// 什么都不做的出口。单测与"不需要进度"的调用点用。
pub struct NullSink;

impl EventSink for NullSink {
    fn stdout_line(&mut self, _line: &str) {}
}

/// 把行收集进 Vec 的出口。
///
/// **只在测试里需要**：生产路径的进度出口是"推给前端事件并节流"（Phase 3 做），
/// 现在挂着它只会多一个没人用的结构。
#[cfg(test)]
#[derive(Default)]
pub struct CollectSink {
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
}

#[cfg(test)]
impl EventSink for CollectSink {
    fn stdout_line(&mut self, line: &str) {
        self.stdout.push(line.to_string());
    }
    fn stderr_line(&mut self, line: &str) {
        self.stderr.push(line.to_string());
    }
}

// ---------------------------------------------------------------------------
// 校验
// ---------------------------------------------------------------------------

/// 一个 driver 的校验结论。
///
/// **`errors` 与 `warnings` 必须分开**，这是"失败不静默"和"别误伤"之间的平衡：
/// - `errors`：结构上就不可能跑起来（缺 model、缺 `{prompt}`），**拒绝注册**。
/// - `warnings`：能跑，但有一条你得知道的风险（可信度中、`HOME` 没设）。
///   这些**不该阻止注册**——阻止了用户就没法用；但**必须展示**，否则风险被静默吞掉。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DriverValidation {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl DriverValidation {
    /// 是否没有硬错误（可以注册）。
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// 环境变量列表里是否声明了 `HOME` / `USERPROFILE`。
///
/// 抽成函数是因为规则本身有点绕（大小写不敏感、两种变量名都认），
/// 而且有两处要用它。
fn declares_home(env: &[String]) -> bool {
    env.iter().any(|e| {
        let key = e.split('=').next().unwrap_or("").trim();
        key.eq_ignore_ascii_case("HOME") || key.eq_ignore_ascii_case("USERPROFILE")
    })
}

/// 校验一个 driver 配置。
///
/// # `errors` 与 `warnings` 的分界：**自相矛盾 vs 还没填完**
///
/// 这条分界是整个校验的骨架，写清楚免得以后被"顺手加一条 error"破坏：
///
/// - **`errors` = 自相矛盾**：字段填全了也跑不起来。例如"Api 形态却要求进程退出码"
///   （根本没有进程）、"选了日志权威串却没给匹配串"（永远判不出完成）、
///   "Cli 参数里没有 `{prompt}`"（任务文本根本传不进去）。
/// - **`warnings` = 还没填完 / 有风险**：缺 key、缺 model、缺 HOME。
///   这些**归 `is_configured()` 表达**，而 `is_schedulable()` 已经要求它成立。
///
/// 为什么"缺 model"不能算 error：预置的 7 个执行器里有几个就是"占位、等你填"
/// （如 `doubao-api`，模型要填自己的接入点 ID）。把它们标成红色错误，
/// 会让界面长期挂着一片红字——**而人对长期存在的红字会免疫**，
/// 那时真正的错误也就看不见了。
///
/// # 关于「不配 `CompletionSignal` 的 driver 拒绝注册」这条铁律
///
/// `CompletionSignal::kind` 在类型上是**非 Option** 的，"一个值都不给"编译期就不可能。
/// 真正危险的是**默认值**：`kind` 默认 `ProcessExitZero`，可信度只有"中"，
/// 如果只是默认就成了，用户永远不会意识到自己的"已完成"其实是"进程退出了"。
///
/// ⇒ 落地方式：对**依赖默认完成信号（`ProcessExitZero` 且无门禁）**的 driver
/// 给一条**必须展示的 warning**，而不是假装"拒绝"。硬拒会让开箱即用变成开箱即坏
/// （预置里有用它的），而 warning 恰好命中真实风险。
pub fn validate_driver(d: &DriverConfig) -> DriverValidation {
    let mut v = DriverValidation::default();

    if d.id.trim().is_empty() {
        v.errors.push("执行器 id 为空".into());
    }
    if d.label.trim().is_empty() {
        v.warnings.push("执行器没有显示名，界面上只能看到 id".into());
    }
    if d.capabilities.is_empty() {
        // 只是"没声明"，不是"配错了"：Exact 绑定完全不受影响。
        v.warnings.push(
            "未声明任何能力——「按能力」绑定与「自动判定」会挑不到它".into(),
        );
    }

    match d.kind {
        DriverKind::Api => {
            if d.api_key.trim().is_empty() {
                v.warnings
                    .push("Api 执行器未填 API Key——探测会失败，任务也无法发起".into());
            }
        }
        DriverKind::Cli => {
            if !d.has_prompt_placeholder() {
                // 空 args 与"写了 args 但忘了占位符"是同一个病因：**任务文本没被传进去**。
                // 不早失败的话，症状是"CLI 正常退出、什么都没做"——最像玄学的那种。
                v.errors.push(
                    "Cli 执行器的参数模板里没有 {prompt} 占位符——任务文本不会被传进去".into(),
                );
            }
            // ★ 这条是实测踩出来的环境坑：本机 `~` 会被 Cadence 污染成 D:\Cadence\SPB_Data，
            // 而这些 CLI 大多把配置放在 ~/.xxx 下。不显式给 HOME，鉴权会莫名失败。
            if !declares_home(&d.env) {
                v.warnings.push(
                    "Cli 执行器未显式设置 HOME/USERPROFILE——本机 `~` 可能被其他软件改写，\
                     导致 CLI 找不到自己的配置而鉴权失败"
                        .into(),
                );
            }
        }
    }

    // ---- 完成信号：结构性检查 + 「形态 × 信号」交叉校验 ----
    //
    // 交叉校验这一段的价值在于：**配错的组合不会报错，只会静默失效**。
    // 给 Api 形态配「进程退出码 0」——它根本没有进程，于是永远判不出完成，
    // 表现为"任务跑完了但槽位一直卡着"。这类故障靠用户自己看日志是查不出来的。
    match d.completion.kind {
        CompletionKind::LogPattern => {
            if d.completion.pattern.trim().is_empty() {
                v.errors.push(
                    "完成信号选了「日志权威串」但没填匹配串——这样永远判不出完成".into(),
                );
            }
        }
        CompletionKind::ProcessExitZero => {
            if d.kind == DriverKind::Api {
                v.errors.push(
                    "Api 执行器不产生子进程，完成信号不能是「进程退出码 0」——\
                     请改成「HTTP 流结束」"
                        .into(),
                );
            } else if d.completion.evidence_cmd.trim().is_empty() {
                // 铁律第二句的直接落地：进程退出码不算完成，必须在 UI 上标注。
                v.warnings.push(
                    "完成信号是「进程退出码 0」且没有世界门禁——可信度只有「未校验」，\
                     界面与播报都会这样标注"
                        .into(),
                );
            }
        }
        CompletionKind::HttpStreamEnd => {
            if d.kind == DriverKind::Cli {
                v.errors.push(
                    "Cli 执行器没有 HTTP 响应流，完成信号不能是「HTTP 流结束」——\
                     请改成「进程退出码 0」或「日志权威串」"
                        .into(),
                );
            }
        }
        CompletionKind::AsyncTaskPoll => {
            // 显式拒绝，而不是留一个"看起来能用"的选项。用户选了它却永远等不到完成，
            // 比界面上直接说明"还没做"糟糕得多。
            v.errors.push(
                "完成信号「异步任务轮询」尚未实现（计划在 Phase 4）——\
                 现在选它会导致任务永远判不出完成"
                    .into(),
            );
        }
    }

    v
}

// ---------------------------------------------------------------------------
// 注册表
// ---------------------------------------------------------------------------

/// 注册表里的一个条目。
#[derive(Clone, Debug)]
pub struct DriverEntry {
    pub config: DriverConfig,
    pub validation: DriverValidation,
}

impl DriverEntry {
    /// 是否可被调度。
    ///
    /// 三个条件缺一不可：**校验无错 + 已启用 + 配置完整**。任缺其一都明确说得出原因，
    /// 不做"看起来能跑就试试"。
    pub fn is_schedulable(&self) -> bool {
        self.validation.is_ok() && self.config.enabled && self.config.is_configured()
    }
}

/// 执行器注册表。
///
/// 从配置构建，**不持有任何连接**——每次执行现建（Api driver 内部一个有自己配置的
/// `LlmClient`；Cli driver 每次 spawn）。这样"设置页改了配置"立刻生效，
/// 不会出现"客户端还拿着旧 key"这类最难查的不一致。
pub struct DriverRegistry {
    entries: Vec<DriverEntry>,
}

impl DriverRegistry {
    /// 从配置列表构建，逐个校验。
    pub fn from_configs(configs: &[DriverConfig]) -> Self {
        let entries = configs
            .iter()
            .map(|c| DriverEntry {
                config: c.clone(),
                validation: validate_driver(c),
            })
            .collect();
        Self { entries }
    }

    /// 全部条目（含不可用的——UI 需要展示"为什么不可用"）。
    pub fn entries(&self) -> &[DriverEntry] {
        &self.entries
    }

    /// 按 id 取条目。
    pub fn get(&self, id: &str) -> Option<&DriverEntry> {
        self.entries.iter().find(|e| e.config.id == id)
    }

    /// 所有可调度的条目。
    pub fn schedulable(&self) -> impl Iterator<Item = &DriverEntry> {
        self.entries.iter().filter(|e| e.is_schedulable())
    }

    /// 可供某能力挑选的 driver（已排序：配置顺序即优先级顺序）。
    ///
    /// 返回空表示**无候选** —— 调用方必须据此报错，不许"随便挑一个"（方案 §2.4.3）。
    pub fn supporting(&self, cap: crate::agent_desk::config::Capability) -> Vec<&DriverEntry> {
        self.entries
            .iter()
            .filter(|e| e.is_schedulable() && e.config.capabilities.contains(&cap))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 执行入口
// ---------------------------------------------------------------------------

/// 执行一次任务：按形态分派 + 统一施加世界门禁。
///
/// # ⛔ 门禁的语义（这一条决定整个系统的诚实度）
///
/// `evidence_cmd` 非空时，必须在 driver 自认为完成之后**再跑一次外部证据**，
/// 只有它退出码 0 才算真完成。这就是课程卡 9 与 `dsh` 的 `world` 层说的同一件事：
/// **进程退出不算完成，完成事件也不代表成功。**
///
/// 失败时**绝不标 `Completed`** —— 那正是"虚假完成"，比报错危险得多。
pub async fn run_driver(
    driver: &DriverConfig,
    req: &RunRequest,
    sink: &mut dyn EventSink,
    cancel: CancelToken,
) -> RunOutcome {
    let validation = validate_driver(driver);
    if !validation.is_ok() {
        return RunOutcome::failed(format!(
            "执行器 {} 配置有误，拒绝执行：{}",
            driver.id,
            validation.errors.join("；")
        ));
    }
    if !driver.enabled {
        return RunOutcome::failed(format!("执行器 {} 未启用", driver.label));
    }

    let base = match driver.kind {
        DriverKind::Api => api_driver::run(driver, req, sink, cancel.clone()).await,
        DriverKind::Cli => cli_driver::run(driver, req, sink, cancel).await,
    };

    apply_evidence_gate(driver, base).await
}

/// 施加世界门禁。
///
/// **只在基础执行成功时才跑**——基础就失败了，再跑门禁没有意义，
/// 而且会掩盖真正的失败原因（用户会看到"门禁没过"，而不是"任务本身崩了"）。
async fn apply_evidence_gate(driver: &DriverConfig, mut base: RunOutcome) -> RunOutcome {
    let cmd = driver.completion.evidence_cmd.trim();
    if cmd.is_empty() || base.status != RunStatus::Completed {
        return base;
    }

    let started = std::time::Instant::now();
    let mut opts = RunnerOptions::new().timeout(std::time::Duration::from_millis(EVIDENCE_TIMEOUT_MS));
    // 门禁跑在**任务同一个工作目录**里。
    //
    // 不传的话它会落在 easyop 自己的进程目录 —— 那个目录随安装方式变化，于是
    // `if exist out.png` 这种相对路径的门禁会表现出"明明生成了却判失败"，
    // 而失败原因里只写"门禁未通过"，排查成本全转给用户。
    if !driver.cwd.trim().is_empty() {
        opts = opts.cwd(driver.cwd.trim());
    }

    // Windows 上走 `cmd /C`：门禁命令是用户写的 shell 片段（`test -f x` 那种
    // 在 Windows 上不存在），交给系统 shell 解释是唯一正确的做法。
    let result = if cfg!(windows) {
        runner::run("cmd", &["/C", cmd], &opts).await
    } else {
        runner::run("sh", &["-c", cmd], &opts).await
    };

    let duration_ms = started.elapsed().as_millis() as u64;

    let evidence = match result {
        Ok(out) => {
            let passed = out.is_success();
            let mut detail = String::new();
            if !out.stdout.trim().is_empty() {
                detail.push_str(out.stdout.trim());
            }
            if !out.stderr.trim().is_empty() {
                if !detail.is_empty() {
                    detail.push('\n');
                }
                detail.push_str(out.stderr.trim());
            }
            EvidenceResult {
                cmd: cmd.to_string(),
                passed,
                exit_code: out.exit_code,
                detail: truncate(&detail, 600),
                duration_ms,
            }
        }
        Err(e) => EvidenceResult {
            cmd: cmd.to_string(),
            passed: false,
            exit_code: None,
            detail: format!("门禁命令未能执行：{e}"),
            duration_ms,
        },
    };

    if evidence.passed {
        // 门禁通过 → 完成可信度升为最高。
        base.confidence = Confidence::High;
        base.evidence = Some(evidence);
        base
    } else {
        // ⛔ 门禁没过 ⇒ 不算完成。原始输出保留，供用户看"到底产出了什么"。
        log_buffer::warn(format!(
            "任务台世界门禁未通过: driver={} cmd={} exit={:?} detail={}",
            driver.id, evidence.cmd, evidence.exit_code, evidence.detail
        ));
        base.status = RunStatus::Failed;
        base.reason = Some(format!(
            "世界门禁未通过（`{}` 退出码 {:?}）——上游自称已完成，但外部证据不成立",
            evidence.cmd, evidence.exit_code
        ));
        base.confidence = Confidence::High; // 这个结论本身是"已校验"的：真的没做成。
        base.evidence = Some(evidence);
        base
    }
}

/// 按字符边界截断（不是字节），避免把 UTF-8 切坏。**保留头部**。
pub(crate) fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push_str("…（已截断）");
    out
}

/// 取**尾部**若干字符（超出时从头部省略）。
///
/// 与 [`truncate`] 的区别不是写法，是**用在哪**：失败诊断要看的是"上游最后说了什么"
/// —— 决定任务终态的事件几乎总在输出末尾（codex 的 `turn.completed` 就是最后一行），
/// 取头部等于把最关键的那句砍掉。
pub(crate) fn tail(s: &str, max_chars: usize) -> String {
    let total = s.chars().count();
    if total <= max_chars {
        return s.to_string();
    }
    let out: String = s.chars().skip(total - max_chars).collect();
    format!("…（已省略前 {} 字）{}", total - max_chars, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_desk::config::{CompletionKind, CompletionSignal, DriverKind};

    fn cli_driver(id: &str) -> DriverConfig {
        DriverConfig {
            id: id.into(),
            label: format!("测试 {id}"),
            kind: DriverKind::Cli,
            enabled: true,
            command: "cmd".into(),
            args: vec!["/C".into(), "echo {prompt}".into()],
            env: vec!["HOME=C:\\Users\\test".into()],
            cwd: String::new(),
            completion: CompletionSignal {
                kind: CompletionKind::ProcessExitZero,
                pattern: String::new(),
                evidence_cmd: String::new(),
            },
            capabilities: vec![crate::agent_desk::config::Capability::Text],
            note: String::new(),
            ..Default::default()
        }
    }

    // -----------------------------------------------------------------------
    // 校验：errors 与 warnings 的分工
    // -----------------------------------------------------------------------

    #[test]
    fn cli_driver_with_prompt_placeholder_passes() {
        let d = cli_driver("ok");
        let v = validate_driver(&d);
        assert!(v.is_ok(), "errors={:?}", v.errors);
    }

    #[test]
    fn cli_driver_without_prompt_placeholder_is_a_hard_error() {
        let mut d = cli_driver("bad");
        d.args = vec!["exec".into(), "hello".into()];
        let v = validate_driver(&d);
        assert!(!v.is_ok());
        assert!(
            v.errors.iter().any(|e| e.contains("{prompt}")),
            "必须明确指出缺占位符，实际 {:?}",
            v.errors
        );
    }

    #[test]
    fn cli_driver_without_home_is_only_a_warning() {
        // 这是"可跑但有风险"——不该阻止注册，但必须被展示。
        let mut d = cli_driver("nohome");
        d.env.clear();
        let v = validate_driver(&d);
        assert!(v.is_ok(), "缺 HOME 不该是 error：{:?}", v.errors);
        assert!(
            v.warnings.iter().any(|w| w.contains("HOME")),
            "必须警告 HOME 风险，实际 {:?}",
            v.warnings
        );
    }

    #[test]
    fn api_driver_without_key_is_warning_not_error() {
        let d = DriverConfig {
            id: "api1".into(),
            label: "百炼".into(),
            kind: DriverKind::Api,
            enabled: true,
            endpoint: "https://example.com/v1/chat/completions".into(),
            model: "qwen-flash".into(),
            api_key: String::new(),
            completion: CompletionSignal {
                kind: CompletionKind::HttpStreamEnd,
                ..Default::default()
            },
            capabilities: vec![crate::agent_desk::config::Capability::Text],
            ..Default::default()
        };
        let v = validate_driver(&d);
        assert!(v.is_ok(), "key 可以后填，不该是 error：{:?}", v.errors);
        assert!(v.warnings.iter().any(|w| w.contains("API Key")));
    }

    #[test]
    fn api_driver_without_model_is_merely_unconfigured() {
        // "还没填"不是"配错了"：归 `is_configured()` 管，不是 error。
        // 否则预置里那些"等你填接入点"的执行器会让界面长期挂着一片红字。
        let d = DriverConfig {
            id: "api2".into(),
            label: "空模型".into(),
            kind: DriverKind::Api,
            enabled: true,
            endpoint: "https://example.com/v1/chat/completions".into(),
            model: String::new(),
            api_key: "sk-x".into(),
            completion: CompletionSignal {
                kind: CompletionKind::HttpStreamEnd,
                ..Default::default()
            },
            capabilities: vec![crate::agent_desk::config::Capability::Text],
            ..Default::default()
        };
        let v = validate_driver(&d);
        assert!(v.is_ok(), "缺 model 不是自相矛盾：{:?}", v.errors);
        // 但它确实跑不起来——这个判断由 is_configured 表达。
        assert!(!d.is_configured());
    }

    #[test]
    fn log_pattern_without_pattern_is_error() {
        // "选了日志权威串却没填串" = 永远判不出完成，必须早失败。
        let mut d = cli_driver("logpat");
        d.completion.kind = CompletionKind::LogPattern;
        d.completion.pattern = "  ".into();
        let v = validate_driver(&d);
        assert!(!v.is_ok());
        assert!(v.errors.iter().any(|e| e.contains("匹配串")));
    }

    #[test]
    fn process_exit_zero_without_gate_is_warned_loudly() {
        // 铁律第二句的落地：进程退出码不算完成，必须在 UI 标注可信度中。
        let d = cli_driver("plain-exit");
        let v = validate_driver(&d);
        assert!(v.is_ok(), "只是警告，不是错误");
        assert!(
            v.warnings.iter().any(|w| w.contains("未校验")),
            "必须警告可信度只有未校验，实际 {:?}",
            v.warnings
        );
    }

    #[test]
    fn process_exit_zero_with_gate_has_no_warning() {
        // 配了门禁 ⇒ 可信度升 High ⇒ 不该再警告。
        let mut d = cli_driver("gated");
        d.completion.evidence_cmd = "echo ok".into();
        let v = validate_driver(&d);
        assert!(v.is_ok());
        assert!(
            !v.warnings.iter().any(|w| w.contains("未校验")),
            "有门禁就不该再警告可信度，实际 {:?}",
            v.warnings
        );
    }

    #[test]
    fn driver_without_capabilities_is_a_warning_for_exact_bindings() {
        // "没声明能力"不等于"配错了"——「指定执行器」绑定完全不受影响。
        let mut d = cli_driver("nocap");
        d.capabilities.clear();
        let v = validate_driver(&d);
        assert!(v.is_ok(), "不该是硬错误：{:?}", v.errors);
        assert!(v.warnings.iter().any(|w| w.contains("能力")));
    }

    // -----------------------------------------------------------------------
    // 形态 × 完成信号 的交叉校验（配错不会报错、只会静默失效的那一类）
    // -----------------------------------------------------------------------

    #[test]
    fn api_driver_with_process_exit_signal_is_error() {
        // Api 形态没有子进程，"进程退出码 0"永远不可能触发 → 任务会永远卡住。
        let d = DriverConfig {
            id: "api-exit".into(),
            label: "错配".into(),
            kind: DriverKind::Api,
            enabled: true,
            endpoint: "https://example.com/v1/chat/completions".into(),
            model: "m".into(),
            api_key: "sk-x".into(),
            completion: CompletionSignal {
                kind: CompletionKind::ProcessExitZero,
                pattern: String::new(),
                evidence_cmd: String::new(),
            },
            capabilities: vec![crate::agent_desk::config::Capability::Text],
            ..Default::default()
        };
        let v = validate_driver(&d);
        assert!(!v.is_ok());
        assert!(
            v.errors.iter().any(|e| e.contains("进程退出码")),
            "必须指出形态与信号不匹配，实际 {:?}",
            v.errors
        );
    }

    #[test]
    fn cli_driver_with_http_stream_end_signal_is_error() {
        let mut d = cli_driver("cli-http");
        d.completion.kind = CompletionKind::HttpStreamEnd;
        let v = validate_driver(&d);
        assert!(!v.is_ok());
        assert!(v.errors.iter().any(|e| e.contains("HTTP")));
    }

    #[test]
    fn async_task_poll_is_explicitly_rejected_not_silently_offered() {
        // 用户选了它却永远等不到完成 —— 必须现在就拒绝，并说清是"还没做"。
        let mut d = cli_driver("async");
        d.completion.kind = CompletionKind::AsyncTaskPoll;
        let v = validate_driver(&d);
        assert!(!v.is_ok());
        assert!(
            v.errors.iter().any(|e| e.contains("尚未实现")),
            "必须说明是未实现而不是配错了，实际 {:?}",
            v.errors
        );
    }

    #[test]
    fn cli_driver_with_empty_args_is_error_not_a_free_pass() {
        // 空 args 与"忘了 {prompt}"是同一个病因：任务文本传不进去。
        let mut d = cli_driver("empty-args");
        d.args.clear();
        let v = validate_driver(&d);
        assert!(!v.is_ok(), "空参数模板不得当成'无所谓'放过");
        assert!(v.errors.iter().any(|e| e.contains("{prompt}")));
    }

    #[test]
    fn all_preset_drivers_pass_hard_validation() {
        // 预置的 7 个 driver 是开箱即用的承诺：任何一个带硬错误都是 bug。
        let cfg = crate::agent_desk::config::DeskConfig::default();
        for d in &cfg.drivers {
            let v = validate_driver(d);
            assert!(
                v.is_ok(),
                "预置执行器 {} 不该有硬错误：{:?}",
                d.id,
                v.errors
            );
        }
    }

    // -----------------------------------------------------------------------
    // 注册表
    // -----------------------------------------------------------------------

    #[test]
    fn registry_separates_schedulable_from_broken() {
        let good = cli_driver("good");
        let mut bad = cli_driver("bad");
        bad.args = vec!["no-placeholder".into()];
        let mut disabled = cli_driver("off");
        disabled.enabled = false;

        let reg = DriverRegistry::from_configs(&[good, bad, disabled]);
        assert_eq!(reg.entries().len(), 3, "不可用的也要留在表里供 UI 说明");

        let schedulable: Vec<&str> = reg.schedulable().map(|e| e.config.id.as_str()).collect();
        assert_eq!(schedulable, vec!["good"]);

        // 不可用的条目必须**说得出原因**。
        assert!(!reg.get("bad").unwrap().validation.is_ok());
        assert!(reg.get("off").unwrap().validation.is_ok());
        assert!(!reg.get("off").unwrap().is_schedulable(), "未启用不可调度");
    }

    #[test]
    fn supporting_returns_empty_for_unavailable_capability() {
        let reg = DriverRegistry::from_configs(&[cli_driver("text-only")]);
        // 没有任何 driver 声明图片能力 → 空候选。调用方必须据此报错，不许随便挑。
        let cands = reg.supporting(crate::agent_desk::config::Capability::Image);
        assert!(cands.is_empty());
    }

    #[test]
    fn supporting_preserves_config_order_as_priority() {
        use crate::agent_desk::config::Capability;
        let mut a = cli_driver("first");
        a.capabilities = vec![Capability::Code];
        let mut b = cli_driver("second");
        b.capabilities = vec![Capability::Code];
        let reg = DriverRegistry::from_configs(&[a, b]);
        let ids: Vec<&str> = reg
            .supporting(Capability::Code)
            .iter()
            .map(|e| e.config.id.as_str())
            .collect();
        assert_eq!(ids, vec!["first", "second"], "配置顺序即优先级顺序");
    }

    // -----------------------------------------------------------------------
    // 世界门禁（用真实子进程离线可测 —— 这是"完成真相源"的核心）
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn evidence_gate_command_is_really_executed() {
        let mut d = cli_driver("gated");
        d.completion.evidence_cmd = "exit 0".into();

        let base = RunOutcome {
            status: RunStatus::Completed,
            reason: None,
            output: "上游说它做完了".into(),
            duration_ms: 10,
            confidence: Confidence::Medium,
            evidence: None,
            session_id: None,
        };
        let out = apply_evidence_gate(&d, base).await;

        assert_eq!(out.status, RunStatus::Completed);
        assert_eq!(out.confidence, Confidence::High, "门禁通过 ⇒ 升为已校验");
        let ev = out.evidence.expect("必须留下门禁执行证据");
        assert!(ev.passed);
        assert_eq!(ev.exit_code, Some(0));
        assert_eq!(ev.cmd, "exit 0");
    }

    #[tokio::test]
    async fn failing_gate_downgrades_completion_to_failure() {
        // ★ 这是整个模块最重要的一条断言：**上游自称完成 + 门禁不成立 ⇒ 不算完成。**
        let mut d = cli_driver("gated-fail");
        d.completion.evidence_cmd = "exit 3".into();

        let base = RunOutcome {
            status: RunStatus::Completed,
            reason: None,
            output: "上游说它做完了".into(),
            duration_ms: 10,
            confidence: Confidence::Medium,
            evidence: None,
            session_id: None,
        };
        let out = apply_evidence_gate(&d, base).await;

        assert_eq!(out.status, RunStatus::Failed, "门禁没过就是没做成");
        assert!(out.reason.as_ref().unwrap().contains("世界门禁未通过"));
        let ev = out.evidence.unwrap();
        assert!(!ev.passed);
        assert_eq!(ev.exit_code, Some(3));
        // 原始输出必须保留——用户要能看"到底产出了什么"。
        assert_eq!(out.output, "上游说它做完了");
    }

    #[tokio::test]
    async fn gate_output_is_captured_for_diagnosis() {
        // 用 ASCII 输出：`cmd` 的代码页是 GBK，中文经 Rust → CreateProcess → cmd
        // 这条链路很容易因编码而丢失（实测输出为空）。门禁命令的内容不是本测试的重点，
        // 重点在"它的输出必须被留下来供排错"。
        let mut d = cli_driver("gated-msg");
        d.completion.evidence_cmd = "echo ARTIFACT_MISSING && exit 1".into();

        let base = RunOutcome {
            status: RunStatus::Completed,
            reason: None,
            output: String::new(),
            duration_ms: 1,
            confidence: Confidence::Medium,
            evidence: None,
            session_id: None,
        };
        let out = apply_evidence_gate(&d, base).await;
        let ev = out.evidence.unwrap();
        assert!(
            ev.detail.contains("ARTIFACT_MISSING"),
            "门禁的输出是排错依据，必须留下，实际 {:?}",
            ev.detail
        );
    }

    #[tokio::test]
    async fn gate_is_skipped_when_there_is_no_evidence_cmd() {
        let d = cli_driver("plain");
        let base = RunOutcome {
            status: RunStatus::Completed,
            reason: None,
            output: "ok".into(),
            duration_ms: 5,
            confidence: Confidence::Medium,
            evidence: None,
            session_id: None,
        };
        let out = apply_evidence_gate(&d, base).await;
        assert_eq!(out.status, RunStatus::Completed);
        assert_eq!(out.confidence, Confidence::Medium, "没门禁就不该升可信度");
        assert!(out.evidence.is_none(), "没配门禁就不该有门禁记录");
    }

    #[tokio::test]
    async fn gate_is_not_run_when_base_already_failed() {
        // 基础执行就失败了，再跑门禁只会掩盖真正的失败原因。
        let mut d = cli_driver("base-failed");
        d.completion.evidence_cmd = "echo SHOULD_NOT_RUN".into();

        let base = RunOutcome {
            status: RunStatus::Failed,
            reason: Some("子进程崩了".into()),
            output: String::new(),
            duration_ms: 5,
            confidence: Confidence::Medium,
            evidence: None,
            session_id: None,
        };
        let out = apply_evidence_gate(&d, base).await;
        assert_eq!(out.status, RunStatus::Failed);
        assert_eq!(out.reason.as_deref(), Some("子进程崩了"), "原失败原因不被覆盖");
        assert!(out.evidence.is_none(), "基础失败时不得执行门禁");
    }

    // -----------------------------------------------------------------------
    // run_driver 的前置拒绝
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn run_driver_refuses_invalid_config_before_spawning_anything() {
        let mut d = cli_driver("invalid");
        d.args = vec!["no-placeholder".into()]; // 缺 {prompt}
        let req = RunRequest::new(0, 1, "任务");
        let mut sink = NullSink;

        let out = run_driver(&d, &req, &mut sink, CancelToken::new()).await;
        assert_eq!(out.status, RunStatus::Failed);
        assert!(out.reason.as_ref().unwrap().contains("配置有误"));
    }

    #[tokio::test]
    async fn run_driver_refuses_disabled_driver() {
        let mut d = cli_driver("disabled");
        d.enabled = false;
        let req = RunRequest::new(0, 1, "任务");
        let mut sink = NullSink;

        let out = run_driver(&d, &req, &mut sink, CancelToken::new()).await;
        assert_eq!(out.status, RunStatus::Failed);
        assert!(out.reason.as_ref().unwrap().contains("未启用"));
    }

    // -----------------------------------------------------------------------
    // 工具
    // -----------------------------------------------------------------------

    #[test]
    fn truncate_respects_char_boundaries() {
        // 按字符截断，不能按字节——否则中文会被切出乱码。
        let s: String = "中".repeat(100);
        let cut = truncate(&s, 10);
        assert!(cut.starts_with(&"中".repeat(10)));
        assert!(cut.chars().count() > 10, "要带截断提示");
        // 短于上限时原样返回。
        assert_eq!(truncate("短", 10), "短");
    }

    #[test]
    fn tail_keeps_the_end_because_terminal_events_are_last() {
        // 失败诊断要看"上游最后说了什么" —— codex 的 turn.completed 就在最后一行，
        // 取头部等于把最关键那句砍掉。
        let long = "A".repeat(100) + "ZZZ_END";
        let t = tail(&long, 20);
        assert!(t.contains("ZZZ_END"), "尾部必须保住：{t}");
        assert!(t.contains("已省略"), "应说明前面被省略了多少：{t}");
        assert_eq!(tail("短", 10), "短");
    }
}
