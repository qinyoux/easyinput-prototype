//! Agent 任务台的配置层。
//!
//! # 落点选择：为什么是独立的 `agent_desk.json`
//!
//! easyop 既有配置集中在 `config.json`（`session.rs::PersistConfig`，扁平键值）。
//! 任务台的配置是**嵌套结构 + 数组**（driver 列表、槽位绑定），塞进去要么拍平成
//! 十几个前缀键，要么把 `PersistConfig` 改成递归结构 —— 后者会动到所有既有配置的
//! 读写路径。
//!
//! 多 agent 共用一棵工作树的前提下，**改动面就是风险面**。所以选独立文件：
//! 纯增量、读写自洽、删掉即回滚，且与 `config.json` 的演进互不干扰。
//!
//! # 密钥处置
//!
//! 所有 `api_key` 字段默认空串，由用户在设置页填写。仓库里不留真实密钥，
//! 也不写进 prompt / 截图。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::tts::TtsConfig;

/// 配置 schema 版本。改动**不兼容**的字段结构时 +1，并在 `migrate()` 里补迁移。
pub const DESK_SCHEMA: u32 = 1;

/// 配置文件名（位于 `app_config_dir()`）。
pub const DESK_CONFIG_FILE: &str = "agent_desk.json";

/// 槽位数量。与固件第 1–4 颗灯一一对应。
///
/// **由 `constants::desk_slot_count()` 推导，不要写死数字**：槽位号→保留 UUID 的
/// 那张表才是唯一真源（任务台按键靠 `host_action` 通道回传，UUID 表里有几项，
/// 就有几个可用槽位）。写死数字的后果是"表里加了第 5 个槽位，但配置层只认 4 个"，
/// 表现为 UI 上配了、按键也下发了、按下去被 `normalize()` 截掉——最难查的那种。
pub const SLOT_COUNT: usize = crate::constants::desk_slot_count() as usize;

// ---------------------------------------------------------------------------
// 能力枚举
// ---------------------------------------------------------------------------

/// 任务能力分类。这就是"路由"要判的那个东西。
///
/// 与 `SlotBinding::Capability` 配套：槽位可以绑"能力"而不是绑"某个执行器"，
/// 由 `Router` 在具备该能力的 driver 里挑。**这层抽象是"任意换模型"的落点**——
/// 换供应商只需改 driver 的 `capabilities`，不动槽位绑定。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// 纯文本问答 / 写作 / 总结。
    Text,
    /// 代码生成 / 修改 / 审查。
    Code,
    /// 生图。
    Image,
    /// 生视频。
    Video,
    /// 联网检索。
    Search,
    /// 图像理解（读图）。
    Vision,
}

impl Capability {
    /// 中文标签（UI 用）。
    ///
    /// ⚠️ **已知重复**：前端 `DeskSettings.tsx` 里有一份 `CAPABILITY_LABELS` 副本。
    /// 这是刻意接受的重复——纯展示映射、不参与逻辑，且让设置页不必先拉一次快照
    /// 才能渲染下拉框。**代价：加新能力时两边都要改。**
    /// 若以后能力种类显著变多，应改成由后端下发选项列表（`Capability::all()` 式的接口）。
    pub fn label(&self) -> &'static str {
        match self {
            Capability::Text => "文本",
            Capability::Code => "代码",
            Capability::Image => "图片",
            Capability::Video => "视频",
            Capability::Search => "检索",
            Capability::Vision => "读图",
        }
    }
}

// ---------------------------------------------------------------------------
// 完成信号
// ---------------------------------------------------------------------------

/// 完成信号来源。**课程卡 9 的语义，抽象成显式配置。**
///
/// 铁律：**进程退出 ≠ 任务完成。** 因此 `ProcessExitZero` 是"可信度中"的取值，
/// 必须由用户显式选择，UI 上也要标出来。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionKind {
    /// HTTP 流式响应正常结束。
    HttpStreamEnd,
    /// 日志里出现权威串（如 `task_complete`）。
    LogPattern,
    /// 子进程退出码为 0。
    ProcessExitZero,
    /// 异步任务轮询（生图 / 生视频这种 `queued → running → succeeded`）。
    AsyncTaskPoll,
}

impl CompletionKind {
    /// 中文标签。
    pub fn label(&self) -> &'static str {
        match self {
            CompletionKind::HttpStreamEnd => "HTTP 流结束",
            CompletionKind::LogPattern => "日志权威串",
            CompletionKind::ProcessExitZero => "进程退出码 0",
            CompletionKind::AsyncTaskPoll => "异步任务轮询",
        }
    }

    /// 单独看这个信号时的固有可信度。
    pub fn base_confidence(&self) -> Confidence {
        match self {
            CompletionKind::HttpStreamEnd | CompletionKind::LogPattern => Confidence::High,
            // 进程退出只说明"进程结束了"，不说明"事情做成了"；异步轮询的 succeeded
            // 也只是上游自述。
            CompletionKind::ProcessExitZero | CompletionKind::AsyncTaskPoll => Confidence::Medium,
        }
    }
}

/// 完成可信度。混合路由之后**必须显式区分**——同一个"已完成"在不同槽位含义不同。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// 有外部证据门禁（测试通过 / 产物落地 / diff 存在），或信号本身权威。
    High,
    /// 只有"过程结束"这类弱证据。
    Medium,
}

impl Confidence {
    pub fn label(&self) -> &'static str {
        match self {
            Confidence::High => "已校验",
            Confidence::Medium => "未校验",
        }
    }
}

/// 完成判定配置。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CompletionSignal {
    pub kind: CompletionKind,
    /// `LogPattern` 用的匹配串。其它 kind 忽略。
    pub pattern: String,
    /// **世界门禁**：非空时，命令必须退出码 0 才算真完成。
    ///
    /// 这是"独立门禁"的落地——测试、文件、diff、产物探测决定真正的完成，
    /// 而不是上游自己的说法。填了就升级为 `High`。
    pub evidence_cmd: String,
}

impl Default for CompletionSignal {
    fn default() -> Self {
        Self {
            kind: CompletionKind::ProcessExitZero,
            pattern: String::new(),
            evidence_cmd: String::new(),
        }
    }
}

impl CompletionSignal {
    /// 有效可信度：填了世界门禁就是 `High`，否则取 kind 的固有值。
    pub fn confidence(&self) -> Confidence {
        if self.evidence_cmd.trim().is_empty() {
            self.kind.base_confidence()
        } else {
            Confidence::High
        }
    }
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

/// 执行器形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverKind {
    /// OpenAI 兼容 HTTP（零外部依赖）。
    Api,
    /// 任意 CLI 子进程（headless）。
    Cli,
}

/// 会话续接配置 —— 让多轮对话**接着上一轮聊**，而不是每轮重新起一个会话。
///
/// # 为什么要做成数据而不是代码
///
/// 不同 CLI 表达"接着这个会话继续"的方式各不相同：codex 是 `exec resume <id>`，
/// 别的工具可能是 `--session <id>`。它们的**形态是一致的三个要素**：
/// ① 会话 id 从哪一行输出里来 ② 那行里哪个键是 id ③ 带上它时要加哪些 argv。
/// 所以这里是三字段配置，而不是一个 codex 专用 driver ——
/// 与"driver 只分 Api / Cli 两种形态"同一条理由。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContinuationSpec {
    /// 会话 id 出现在哪种 stdout 事件里（比对 JSONL 的 `"type"` 字段）。
    ///
    /// 空 ⇒ 不启用续接。
    pub id_event_type: String,
    /// 会话 id 在该事件里的**键名**。
    pub id_field: String,
    /// 续接时**追加在基础 args 之后、prompt 之前**的参数模板。
    /// 含 `{id}` 的元素会被替换成会话 id。
    ///
    /// ⚠️ 顺序是有实测依据的，不是随手选的：
    /// `codex exec --json --skip-git-repo-check resume <id> "<prompt>"`
    /// —— 子命令必须落在 `--json` 这类全局选项**之后**、prompt **之前**，
    /// 写成 `exec resume <id> --json` 会把 `--json` 当成 resume 的参数。
    pub args: Vec<String>,
}

impl ContinuationSpec {
    /// 配置是否完整到能用。**只看两个必填字段**，不看 `args`
    /// —— 有的工具把 id 塞进环境变量或复用已有参数即可，模板可以是空的。
    pub fn usable(&self) -> bool {
        !self.id_event_type.trim().is_empty() && !self.id_field.trim().is_empty()
    }
}

/// 一个可被调度的执行器。
///
/// **加新 Agent = 加一条配置，不写 Rust 代码**（见方案 §2.3）。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DriverConfig {
    /// 唯一 id，也是槽位绑定的引用键。
    pub id: String,
    /// 显示名。
    pub label: String,
    pub kind: DriverKind,
    /// 是否启用。
    pub enabled: bool,

    // ---- Api 形态 ----
    pub endpoint: String,
    pub model: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub api_key: String,

    // ---- Cli 形态 ----
    /// 可执行文件。Windows 上 npm 全局包是 `.cmd` shim，必要时写全路径。
    pub command: String,
    /// 参数模板。`{prompt}` 会被替换为任务文本。
    pub args: Vec<String>,
    /// 环境变量（`KEY=VALUE`）。**CLI driver 必须显式给 `HOME`**。
    pub env: Vec<String>,
    /// 工作目录。**dsh 这类工具没有 cwd 就不接受任务**。
    pub cwd: String,

    /// 单次执行的超时上限（毫秒）。**0 = 用全局默认**（`driver::DEFAULT_TIMEOUT_MS`，180 秒）。
    ///
    /// # 为什么必须可配
    ///
    /// 180 秒这个默认值是照"一次 HTTP 调用"定的。而 CLI Agent 跑一个**真实**任务
    /// 动辄几分钟到几十分钟 —— 拿 180 秒去卡它，结果是每个任务都在 3 分钟处被判超时，
    /// 而且那个时刻恰好落在"它干得好好的"区间里。用户看到的是一串毫无道理的"已超时"。
    ///
    /// # 为什么 0 的含义是"用默认"而不是"不超时"
    ///
    /// **没有超时上限的 driver 不可用。** Agent 卡死时若没有回收点，运行位永远不会
    /// 被清出 ⇒ 槽位被永久占用，后续派发全部变 `Busy`。那正是"一直显示在执行中"
    /// 的另一条成因（第一条见 `state::claim_current` 的注释）。
    pub timeout_ms: u64,

    /// 完成判定。
    pub completion: CompletionSignal,
    /// 会话续接。`None` = 每次都新开会话（多数 CLI 的默认行为）。
    ///
    /// ⚠️ `Option` 是刻意的：**"不支持续接"与"还没配"是不同的事实**，
    /// 用一个空结构体表达会把两者混成一个"能用的空壳"。
    #[serde(default)]
    pub continuation: Option<ContinuationSpec>,
    /// 该执行器声明会做的事。`Capability` 绑定与 `Auto` 路由都读它。
    pub capabilities: Vec<Capability>,

    /// 产物过滤（可选 glob，如 `out/*.png`）。空 = 只按时间窗判定，不做名字过滤。
    ///
    /// 只在 `cwd` 有配时才起作用：没配工作目录就不扫盘（见 `artifact` 模块文档）。
    #[serde(default)]
    pub artifact_glob: String,
    /// 任务成功后是否自动打开产物。
    ///
    /// # ⚠️ 默认值必须用 `default_true`，不能靠 `bool::default()`
    ///
    /// 老配置文件里没有这个字段，若走 `#[serde(default)]` 会拿到 `false` —— 于是
    /// 用户升级后看到的是"功能没生效"，而他在界面上明明开着。`false` 是 Rust 的默认，
    /// 不是这个功能的默认。
    #[serde(default = "default_true")]
    pub artifact_open: bool,

    /// 备注重（UI 展示，提醒可信度/门槛）。
    pub note: String,
}

/// serde 默认值：`true`。
///
/// 必须是**具名函数**（serde 的 `default = "…"` 要的是函数路径，不接任意表达式），
/// 且与 `Default for DriverConfig` 里那处保持同值 —— 两处不一致会让"新建的执行器"
/// 与"老配置升级上来的执行器"行为不同。
fn default_true() -> bool {
    true
}

impl Default for DriverConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            label: String::new(),
            kind: DriverKind::Api,
            enabled: false,
            endpoint: String::new(),
            model: String::new(),
            api_key: String::new(),
            command: String::new(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: String::new(),
            timeout_ms: 0,
            completion: CompletionSignal::default(),
            continuation: None,
            capabilities: vec![Capability::Text],
            artifact_glob: String::new(),
            // ⚠️ 与 serde 的 `default_true` 保持一致（见字段文档）。
            artifact_open: true,
            note: String::new(),
        }
    }
}

impl DriverConfig {
    /// 是否已具备发起请求的最小条件（key / command 有没有填）。
    pub fn is_configured(&self) -> bool {
        match self.kind {
            DriverKind::Api => !self.api_key.trim().is_empty() && !self.model.trim().is_empty(),
            DriverKind::Cli => !self.command.trim().is_empty(),
        }
    }

    /// 参数模板是否含 `{prompt}`。缺了就等于"任务文本传不进去"，必须早失败。
    pub fn has_prompt_placeholder(&self) -> bool {
        self.args.iter().any(|a| a.contains("{prompt}"))
    }
}

// ---------------------------------------------------------------------------
// 槽位绑定
// ---------------------------------------------------------------------------

/// 槽位绑定方式。**这就是"两个方案"合一的落点**——同一个枚举的三种取值。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum SlotBinding {
    /// 空槽：按下不派发任何任务。
    Empty,
    /// **方案 1**：钉死一个 driver。路径确定、完全可复现、可攒路由数据。
    Exact { driver: String },
    /// **方案 2**：绑"能力"，由 Router 在具备该能力的 driver 里挑。
    Capability {
        capability: Capability,
        /// 优先级顺序（driver id）。空的候选 = 报错，不许"随便挑一个"。
        prefer: Vec<String>,
    },
    /// **方案 2 极端**：能力也交给 Router 判。
    Auto,
}

impl Default for SlotBinding {
    fn default() -> Self {
        SlotBinding::Empty
    }
}

impl SlotBinding {
    /// UI 显示用的模式名。
    pub fn mode_label(&self) -> &'static str {
        match self {
            SlotBinding::Empty => "未绑定",
            SlotBinding::Exact { .. } => "指定执行器",
            SlotBinding::Capability { .. } => "按能力",
            SlotBinding::Auto => "自动判定",
        }
    }
}

/// 默认 4 个空槽。
fn default_slots() -> Vec<SlotBinding> {
    (0..SLOT_COUNT).map(|_| SlotBinding::Empty).collect()
}

// ---------------------------------------------------------------------------
// 摘要 / 播报
// ---------------------------------------------------------------------------

/// 任务完成后的总结。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SummaryConfig {
    /// 用哪个 driver 做总结。**空 = 不总结**。
    ///
    /// # 空为什么是"不总结"，而不是"随便挑一个"
    ///
    /// CLI Agent 交回的**最后一句**本就是它给自己做的总结。再套一层模型不会让它更准，
    /// 只会更慢、更容易跑偏（把一句"微信已启动"扩写成三句客套话）。
    /// 所以默认不总结，播报念 Agent 自己的结论（按 `max_chars` 截断）。
    ///
    /// 非空时才拿那个 driver 跑一次"二次总结" —— 这是给"想听更凝练版本"的人留的口子。
    /// 总结**不占槽位**，且失败时按 `fallback_truncate` 决定退路。
    pub driver_id: String,
    /// 提示词模板。支持的占位符：`{task}` 原始指令、`{output}` Agent 输出、
    /// `{max_chars}` 字数上限。
    pub prompt_template: String,
    /// 摘要字数上限。TTS 上限 512 Token 的反推值，留足余量。
    pub max_chars: usize,
    /// 总结失败时是否退回截断原文。
    ///
    /// 默认 true：**宁可播一句粗糙的原文截断，也不要"灯亮了按下去没有声音"**——
    /// 后者会让整条链路的可信度归零。
    pub fallback_truncate: bool,
}

impl Default for SummaryConfig {
    fn default() -> Self {
        Self {
            driver_id: String::new(),
            prompt_template: "把下面这段 Agent 执行结果，用不超过{max_chars}字的口语化中文总结：\
                              先说做成了什么，再说有没有需要我处理的事。不要客套，不要 Markdown。\
                              \n\n原始指令：{task}\n\n执行结果：{output}"
                .to_string(),
            max_chars: 180,
            fallback_truncate: true,
        }
    }
}

/// 状态播报（Notice）配置。
///
/// **Notice 与 Result 必须分开**：状态播报不产生未读（可丢弃、可被抢占），
/// 只有"终态 + 产物已交付"才产生未读。不分开的话，用户看到未读灯亮、
/// 按下去只听到一句"正在推进"，灯立刻失去可信度。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct NoticeConfig {
    /// 播报总开关。**只管喇叭，不管灯** —— 灯是另一条信道，见 `notice.rs` 的模块头。
    pub enabled: bool,
    /// 任务开始。
    pub on_start: bool,
    /// 进度类（正在推进 / 已运行 N 分钟）。
    ///
    /// 单独一个开关是刻意的：进度是最吵的一类，而"我想安静点"绝不该被迫连
    /// "任务已完成"一起关掉。
    pub on_progress: bool,
    /// 任务完成（终态）。
    pub on_complete: bool,
    /// 任务失败 / 需要确认 / 超时 / 失联。
    pub on_fail: bool,
    /// 同类 Notice 最小间隔（秒）——**去抖**，只在状态跃迁时发。0 = 不限。
    pub min_interval_secs: u64,
    /// 「已运行 N 分钟」的固定档位（秒）。
    ///
    /// ⚠️ **档数上限由语料闭集决定，不由这里决定**（`NoticeKind::PROGRESS_MARKS`，
    /// 当前 3 档）。多写的会被静默截断成"还能表达的最高档"—— 因为每档都要有
    /// 一句固定语料并预合成，报错也变不出第 4 句。
    pub progress_marks_secs: Vec<u64>,
}

impl Default for NoticeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            on_start: true,
            on_progress: true,
            on_complete: true,
            on_fail: true,
            min_interval_secs: 30,
            progress_marks_secs: vec![60, 300, 900],
        }
    }
}

// ---------------------------------------------------------------------------
// 顶层配置
// ---------------------------------------------------------------------------

/// Agent 任务台总配置。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DeskConfig {
    pub schema: u32,
    /// 任务台总开关。
    pub enabled: bool,
    /// 产物交付目录。空 = 用 `app_config_dir/agent_desk_out`。
    pub output_dir: String,
    pub tts: TtsConfig,
    pub summary: SummaryConfig,
    pub notice: NoticeConfig,
    /// 外部会话监工（C：盯 codex 的 rollout 日志）。
    pub probe: ProbeConfig,
    pub drivers: Vec<DriverConfig>,
    pub slots: Vec<SlotBinding>,
    /// **预置清单**的迁移世代标记。
    ///
    /// 含义见 [`DeskConfig::migrate_presets`]：预置清单每演进一轮
    /// （补新能力 / 退役旧条目），`PRESET_FEATURES_VERSION` +1，
    /// 老配置在下一次加载时自动跟上，且**用户主动改过的东西一律不动**。
    ///
    /// ⚠️ 字段名带 `features` 是历史留名，它现在同时管"删条目"，别被名字误导。
    /// **不能改名**：这是磁盘格式的一部分，改名等于所有老配置的标记静默归零、
    /// 迁移被重放一遍。
    #[serde(default)]
    pub preset_features_version: Option<u32>,
}

/// **预置清单的**当前世代。改动预置（补新能力 / 退役旧条目）⇒ 这个数 +1。
///
/// ⚠️ 名字里的 `FEATURES` 是历史留名，**别据此以为它只管"加能力"**：
/// 它守的是"我对预置清单做过第几轮演进"，而演进的方向有两个。
/// 共用一个世代标记是刻意的 —— 拆成两个标记会各自演化、互相不同步，
/// 而它们要防的是同一件事："同一个迁移跑第二遍"。
const PRESET_FEATURES_VERSION: u32 = 2;

/// 已**退役**的预置执行器 id。加进这里 = 下次启动从用户配置里删掉。
///
/// # 为什么必须单独有一张表，不能"从 `default_drivers()` 里删掉就完事"
///
/// `DeskConfig::normalize()` 只会**补**缺失的预置，**不删**多余的条目 ——
/// 那是刻意的（否则用户在设置页里的一切自定义都会被每次启动重置）。
/// 代价是：把一条预置从 `default_drivers()` 里删掉，**老用户的磁盘上那份还在**，
/// 而且没有任何代码路径会去清它。表现就是"新版明明删了这个执行器，
/// 老用户升级后它还杵在那儿" —— 一个只有老用户能复现的 bug。
///
/// # 与 `default_drivers()` 是相反方向的同一件事
///
/// 那边加了 id ⇒ 补进老配置；这边加了 id ⇒ 从老配置删掉。
/// 两边都**不能"顺手"加**：每次都要有明确理由，且删之前先看
/// [`RETIRED_DRIVER_IDS`] 的调用点（`migrate_presets`）有没有同步清槽位绑定。
const RETIRED_DRIVER_IDS: &[&str] = &[
    // 2026-09-22（钦友指定只留三条）：调研期铺的路，从未被真正调度过。
    // ⚠️ 删除会**连带丢掉用户已填的 api_key**（deepseek-api 是填过且实测通过的），
    // 所以迁移会把它记进日志。要加回来在「模型配置」页点「添加」即可。
    "bailian-api",
    "bailian-cli",
    "qwen-code",
    "deepseek-api",
];

/// 外部会话监工（C 方案）。
///
/// 场景：用户**自己开终端跑 codex**，任务台不参与执行，只在任务完成时告诉他一声。
/// 详见 `codex_probe` 模块文档。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ProbeConfig {
    /// 总开关。
    ///
    /// 默认**开**：没装 codex 时根目录不存在 ⇒ `scan` 直接返回空，零副作用；
    /// 装了的用户则开箱就得到"跑完了会告诉你"这件事。
    pub enabled: bool,
    /// rollout 根目录。**留空 = 用 `%USERPROFILE%\.codex\sessions`**。
    ///
    /// 做成可配而不是硬编码，是因为有人会把 `CODEX_HOME` 指到别处；
    /// 但默认值必须能自己工作，不能要求用户先去查路径。
    pub root: String,
}

impl Default for ProbeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            root: String::new(),
        }
    }
}

impl ProbeConfig {
    /// 实际要扫的根目录。
    ///
    /// 留空时回落到 codex 的默认落点。取不到 `USERPROFILE` 就返回 `None` ——
    /// **不猜路径**：猜错会去扫一个不存在的目录，看起来和"没装 codex"一样，
    /// 反而是最难排查的那种失败。
    pub fn resolved_root(&self) -> Option<std::path::PathBuf> {
        if !self.root.trim().is_empty() {
            return Some(std::path::PathBuf::from(self.root.trim()));
        }
        let home = std::env::var("USERPROFILE").ok()?;
        if home.trim().is_empty() {
            return None;
        }
        Some(std::path::PathBuf::from(home).join(".codex").join("sessions"))
    }
}

impl Default for DeskConfig {
    fn default() -> Self {
        Self {
            schema: DESK_SCHEMA,
            enabled: false,
            output_dir: String::new(),
            tts: TtsConfig::default(),
            summary: SummaryConfig::default(),
            notice: NoticeConfig::default(),
            probe: ProbeConfig::default(),
            drivers: default_drivers(),
            slots: default_slots(),
            // 出厂配置天生就是最新世代：新用户的 `default()` 已经带着所有新能力，
            // 再跑一次迁移没有意义（也避免第一天就记一条"已更新配置"的日志）。
            preset_features_version: Some(PRESET_FEATURES_VERSION),
        }
    }
}

impl DeskConfig {
    /// 按 id 找 driver。
    pub fn driver(&self, id: &str) -> Option<&DriverConfig> {
        self.drivers.iter().find(|d| d.id == id)
    }

    /// 补齐结构：槽位数量、schema 版本、缺失的预置 driver。
    ///
    /// 磁盘上的配置可能来自旧版本，或被人手改过。**不在这里补齐，UI 就会
    /// 拿到长度不对的槽位数组，然后按下标越界。**
    pub fn normalize(&mut self) {
        while self.slots.len() < SLOT_COUNT {
            self.slots.push(SlotBinding::Empty);
        }
        self.slots.truncate(SLOT_COUNT);
        if self.schema == 0 {
            self.schema = DESK_SCHEMA;
        }
        // 预置 driver 缺失时补回（用户删掉的不会复活——只按 id 补齐从未出现过的）。
        for preset in default_drivers() {
            if !self.drivers.iter().any(|d| d.id == preset.id) {
                self.drivers.push(preset);
            }
        }
    }

    /// 修正**已经写进用户磁盘的、已知的预置错值**。返回被改动的 driver id。
    ///
    /// # 为什么必须有这一层
    ///
    /// `normalize()` 只补"从来没有过的预置 driver"，**已存在的条目刻意不动** ——
    /// 否则用户自己在设置页里改的东西会被每次启动覆盖。代价是：预置里一旦写错，
    /// 那个错值就跟着用户的 `agent_desk.json` **一直错下去**，且用户根本不知道它错
    /// （09-20 实测：每个 codex 任务都报"失败"，而它们其实全都做完了）。
    ///
    /// # 为什么是"纠错表"而不是"重写预置"
    ///
    /// 只有磁盘值**正好等于表里记录的那个错值**时才改。用户自己设的任何值都不动。
    /// 表里的每一条都必须有**实测结论**支撑，不是"顺手对齐一下"。
    /// 迁移：**把新加的预置能力补进老配置**。返回写回作废 Tracker 的描述。
    ///
    /// # 与 [`Self::apply_preset_patches`] 的分工
    ///
    /// | | 纠错补丁 | 本函数 |
    /// |---|---|---|
    /// | 处理对象 | 已知**错值** | 老配置里**还没有**的新能力 / **已经不该有**的旧条目 |
    /// | 判据 | 磁盘值 == 错值 | 世代标记 < 当前版本 |
    /// | 用户已设置 | 绝不动 | 绝不动（只补 `None`、只删退役 id）|
    ///
    /// # 为什么必须有世代标记
    ///
    /// 新能力用 `Option` 表达（"不支持" vs "未配置"在内存里都是 `None`），
    /// 而 `#[serde(default)]` 让"字段不存在"也反序列化成 `None` ⇒
    /// **无法区分"用户主动关掉"与"他就没见过这个功能"**。
    /// 没有世代标记的话，每次启动都会把它补回去，
    /// 用户在设置页里取消勾选 ⇒ 重启后又被勾上 —— 那是"改了没用"的另一种形态。
    ///
    /// 退役同理：没有标记的话，用户只要手工把退役条目加回来（同名 id），
    /// 每次启动都会再被删一次 —— 那同样是"改了没用"。
    fn migrate_presets(&mut self) -> Vec<String> {
        if self.preset_features_version >= Some(PRESET_FEATURES_VERSION) {
            return Vec::new();
        }
        let mut changed = Vec::new();

        // ---- 方向一：删除已退役的预置（含清掉指向它的槽位绑定）----
        changed.extend(self.retire_drivers());

        // ---- 方向二：把老配置还没有的新能力补上 ----
        for preset in default_drivers() {
            let Some(preset_cont) = preset.continuation else {
                continue;
            };
            let Some(d) = self.drivers.iter_mut().find(|d| d.id == preset.id) else {
                continue;
            };
            // 只补"没配过"的。用户主动设成 `None`（在设置页取消勾选）与他手写的
            // 任何值都不碰 —— 这一版已经跑过迁移之后，那个标记会拦住后续所有运行。
            if d.continuation.is_none() {
                d.continuation = Some(preset_cont);
                changed.push(format!("{}（会话续接）", preset.id));
            }
        }

        self.preset_features_version = Some(PRESET_FEATURES_VERSION);
        changed
    }

    /// 按 [`RETIRED_DRIVER_IDS`] 从配置里删除执行器，返回人类可读的变更说明。
    ///
    /// # 为什么必须**连槽位绑定一起清**
    ///
    /// 槽位绑定存的是 driver **id**。删了执行器却留下绑定，下次派发会在
    /// `desk_dispatch_task` 里撞上"槽位 S1 绑定的执行器 xxx 不存在"——
    /// 而且**错误发生在用户按下硬件键那一刻**，离"我只是删了个没用的执行器"
    /// 这个动作已经很远了。删的时候就清，比事后解释便宜得多。
    ///
    /// 槽位被清成 `Empty`（而不是"顺手挑一个剩下的执行器"）：
    /// 猜错就是**把任务发给了错误的 Agent**，代价远高于"这个键暂时没反应"。
    fn retire_drivers(&mut self) -> Vec<String> {
        if RETIRED_DRIVER_IDS.is_empty() {
            return Vec::new();
        }
        let mut changed = Vec::new();
        self.drivers.retain(|d| {
            let retired = RETIRED_DRIVER_IDS.contains(&d.id.as_str());
            if retired {
                // 只记"它填过密钥"，**不记密钥本身** —— 日志会被贴进 issue。
                let had_key = !d.api_key.trim().is_empty();
                changed.push(format!(
                    "{}（已退役的预置执行器{}）",
                    d.id,
                    if had_key { "，连同已填的密钥" } else { "" }
                ));
            }
            !retired
        });
        for (i, slot) in self.slots.iter_mut().enumerate() {
            match slot {
                SlotBinding::Exact { driver } => {
                    if RETIRED_DRIVER_IDS.contains(&driver.as_str()) {
                        changed.push(format!("槽位 S{} 的绑定（原指向 {driver}）", i + 1));
                        *slot = SlotBinding::Empty;
                    }
                }
                SlotBinding::Capability { prefer, .. } => {
                    // `prefer` 是"优先级列表"，删掉的 id 会以**原始 id** 出现在
                    // 槽位描述里（`，优先级 ["qwen-code"]`）—— 一条永远挑不中的幽灵
                    // 优先级，比没有更糟：它让人以为配置生效了。
                    let before = prefer.len();
                    prefer.retain(|id| !RETIRED_DRIVER_IDS.contains(&id.as_str()));
                    if prefer.len() != before {
                        changed.push(format!("槽位 S{} 的优先级列表", i + 1));
                    }
                }
                SlotBinding::Empty | SlotBinding::Auto => {}
            }
        }
        changed
    }

    fn apply_preset_patches(&mut self) -> Vec<String> {
        let mut patched = Vec::new();
        for (id, wrong, right) in PRESET_COMPLETION_PATCHES {
            let Some(d) = self.drivers.iter_mut().find(|d| d.id == *id) else {
                continue;
            };
            if d.completion.kind == CompletionKind::LogPattern && d.completion.pattern == *wrong {
                d.completion.pattern = (*right).to_string();
                patched.push((*id).to_string());
            }
        }
        patched
    }
}

/// 已知**写错过**的预置完成串 → 正确值（`(driver id, 错值, 正确值)`）。
///
/// ⚠️ 只有**实测证明它错**才加进来。每加一条都要在注释里写清错在哪、依据是什么。
const PRESET_COMPLETION_PATCHES: &[(&str, &str, &str)] = &[(
    "codex-cli",
    "task_complete",
    // 依据见 `default_drivers()` 里 codex-cli 的 completion 注释：
    // stdout 上出现的是 `turn.completed`，`task_complete` 只在 rollout 文件里。
    "turn.completed;task_complete",
)];

/// 预置 driver 清单——**只保留钦友实际在用的三条**。
///
/// # 为什么只剩三条（2026-09-22 指定）
///
/// 上一版铺了 7 条（3 个 API + 4 个 CLI），其中 4 条是调研期留下的路：
/// `bailian-api` / `bailian-cli`（百炼）、`qwen-code`、`deepseek-api`。
/// 它们从未被真正调度过，却每天都在设置页占位置、在槽位绑定的候选里制造噪声。
/// **一个从不使用的选项和一个坏掉的选项，成本是一样的**：每次选择都得先排除它。
///
/// 退役清单见 [`RETIRED_DRIVER_IDS`]，老配置由 [`DeskConfig::migrate_presets`]
/// 在升级时一并清掉（含指向它们的槽位绑定），不会留下坏引用。
///
/// **要加回任意模型不必改代码**：在「模型配置」页点「添加」即可——
/// `DriverConfig` 的字段一个没动，schema 兼容。
///
/// 留下的三条各自代表一种**真实存在**的调度形态，不是"多留几个备选"：
/// - `doubao-api`：Api 形态（HTTP 一次往返），也是语音编辑的默认后端；
/// - `codex-cli`：Cli 形态 + 高置信度完成信号（`--json` 的 `turn.completed`）+ 会话续接；
/// - `dsh`：Cli 形态 + 只有"进程退出码"这一个弱信号（未校验），作对照。
fn default_drivers() -> Vec<DriverConfig> {
    vec![
        DriverConfig {
            id: "doubao-api".into(),
            label: "豆包 API（火山方舟）".into(),
            kind: DriverKind::Api,
            enabled: true,
            endpoint: "https://ark.cn-beijing.volces.com/api/v3/chat/completions".into(),
            model: String::new(),
            api_key: String::new(),
            completion: CompletionSignal {
                kind: CompletionKind::HttpStreamEnd,
                ..Default::default()
            },
            capabilities: vec![Capability::Text, Capability::Code],
            note: "模型填方舟的接入点 ID（ep-...）或模型名。这也是 easyop 语音编辑的默认后端。".into(),
            ..Default::default()
        },
        DriverConfig {
            id: "dsh".into(),
            label: "DSH（DeepSeek Harness）".into(),
            kind: DriverKind::Cli,
            enabled: false,
            command: "dsh".into(),
            args: vec!["--profile".into(), "headless".into(), "{prompt}".into()],
            env: vec!["DSH_HOME=C:\\Users\\86136\\.dsh".into()],
            cwd: String::new(),
            // CLI Agent 跑真实任务动辄几分钟 ⇒ 出厂 180 秒会把每个任务都判超时。
            timeout_ms: 900_000,
            completion: CompletionSignal {
                kind: CompletionKind::ProcessExitZero,
                pattern: String::new(),
                evidence_cmd: String::new(),
            },
            capabilities: vec![Capability::Text, Capability::Code],
            note: "国产 CLI 里唯一把「进程退出≠完成」写进文档的一个（分开读 stdout/exit/stderr/world 四个信号）。\
                   必须钉死版本号、必须给 cwd、headless 不支持跨调用续跑；\
                   指令含糊时它会主动提问而无人应答，会 fail closed。"
                .into(),
            ..Default::default()
        },
        DriverConfig {
            id: "codex-cli".into(),
            label: "Codex CLI".into(),
            kind: DriverKind::Cli,
            enabled: false,
            // ⚠️ **绝不要写裸名 `codex`。** Windows 上 CreateProcess 在程序名没有扩展名时
            // **只会自动补 `.exe`**，而 npm 全局安装留下的是 `codex.cmd` / `codex.ps1`
            // ⇒ 裸名**必定启动失败**（本机实测：`codex` 找不到，`codex.cmd` 正常返回版本号）。
            // 这与 PATHEXT 无关 —— 那是 cmd.exe 的行为，不是 CreateProcess 的。
            // PATH 上没有 npm 全局目录时，这里改写成全路径。
            command: "codex.cmd".into(),
            // ---- 参数为什么要带 `--json` ----
            //
            // `codex exec` 是**非交互单次执行**（交互式 TUI 是 `codex` 裸命令，见
            // https://www.runoob.com/codex/codex-cli.html —— headless 调度不能用它，
            // 因为 TUI 没有可读的完成信号，也没有产物出口）。
            //
            // `--json` 把事件以 **JSONL 打到 stdout**。
            // `--skip-git-repo-check`：工作目录未必是 git 仓库，不给它 codex 会直接拒跑。
            args: vec![
                "exec".into(),
                "--json".into(),
                "--skip-git-repo-check".into(),
                "{prompt}".into(),
            ],
            env: vec!["HOME=C:\\Users\\86136".into()],
            cwd: String::new(),
            completion: CompletionSignal {
                // ---- 取 stdout 的哪一行当"做完了"（09-20 实测纠正）----
                //
                // 本机实测 `codex.cmd exec --json --skip-git-repo-check "<prompt>"`
                // **stdout 全文**就这 4 行（exec 自己的 JSONL 格式）：
                //   {"type":"thread.started","thread_id":"…"}
                //   {"type":"turn.started"}
                //   {"type":"item.completed","item":{…,"type":"agent_message","text":"OK"}}
                //   {"type":"turn.completed","usage":{…}}
                // ⇒ 代表"这一轮真的跑完了"的是 **`turn.completed`**（最后一行，带 usage）。
                //
                // ⚠️ 上一版写的是 `task_complete`，**那是错的**：`task_complete` 属于
                // app-server 事件格式，只落在 rollout 文件里（C 方案扫的那种），
                // stdout 上从来没有 ⇒ 每个任务都被判"失败"，而 Agent 其实做完了
                // （09-20 实测：exit=0、stdout 2456 字、rollout 里有 task_complete）。
                //
                // 写成 `;` 分隔的**多候选串，任一命中即完成**：将来上游换回
                // app-server 事件格式时不必改代码，也不必改用户的配置。
                kind: CompletionKind::LogPattern,
                pattern: "turn.completed;task_complete".into(),
                evidence_cmd: String::new(),
            },
            // 出厂默认 180 秒是照 HTTP 调用定的，拿它卡 CLI Agent ⇒ 每个真实任务
            // 都会在 3 分钟处被判超时。给 15 分钟。
            timeout_ms: 900_000,
            capabilities: vec![Capability::Text, Capability::Code],
            // ---- 会话续接：让多轮对话接着上一轮 ----
            //
            // 不配这项时，每一次任务都是 `codex exec "<prompt>"` ⇒ **每次新起一个会话**，
            // 于是第二轮问"那刚才那个呢"它就答不上来 —— 它根本没见过上一轮。
            //
            // 实测依据（09-20 本机，两步验证）：
            //   ① `codex exec --json --skip-git-repo-check "记住 7391，只回记住"`
            //      → thread.started 给出 thread_id=01a0bf1e-…，答案"记住"
            //   ② `codex exec --json --skip-git-repo-check resume 01a0bf1e-… "那个数字是多少？"`
            //      → 同一个 thread_id，答案"7391"（没有重开会话）
            // ⇒ 会话 id 的来源是 `thread.started` 事件的 `thread_id`，
            //   带上它的方式是把 `resume <id>` 插在**全局选项之后、prompt 之前**。
            continuation: Some(ContinuationSpec {
                id_event_type: "thread.started".into(),
                id_field: "thread_id".into(),
                args: vec!["resume".into(), "{id}".into()],
            }),
            note: "课程模板的原生 Harness。命令必须写 codex.cmd（裸名 codex 在 Windows 上\
                   起不来，已实测）。完成判据取 stdout 最后一行的 turn.completed\
                   （这是 exec --json 的 JSONL 格式；注意 stdout 上**没有** task_complete，\
                   那是 rollout 文件里的事件 —— 09-20 实测）。\
                   已配会话续接：同一槽位的下一次任务会用 resume 接着上次会话跑。"
                .into(),
            ..Default::default()
        },
    ]
}

// ---------------------------------------------------------------------------
// 持久化
// ---------------------------------------------------------------------------

/// 配置管理器（线程安全，可克隆）。
///
/// **状态与持久化同处一地**：`set()` 写完内存立刻落盘，不存在"改了内存忘了存"的窗口。
#[derive(Clone)]
pub struct DeskConfigStore {
    inner: Arc<StoreInner>,
}

struct StoreInner {
    config: Mutex<DeskConfig>,
    path: Mutex<Option<PathBuf>>,
}

impl Default for DeskConfigStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DeskConfigStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(StoreInner {
                config: Mutex::new(DeskConfig::default()),
                path: Mutex::new(None),
            }),
        }
    }

    /// 绑定配置文件路径并从磁盘加载。
    pub fn init(&self, config_dir: &Path) {
        let path = config_dir.join(DESK_CONFIG_FILE);
        *self.inner.path.lock().unwrap() = Some(path.clone());

        match std::fs::read_to_string(&path) {
            Ok(text) => {
                // 兼容带 BOM 的编辑器写入（serde_json 不接受前置 BOM）。
                let text = text.trim_start_matches('\u{feff}');
                match serde_json::from_str::<DeskConfig>(text) {
                    Ok(mut cfg) => {
                        cfg.normalize();
                        // 旧配置里可能带着**已被实测证明是错的**预置值（见
                        // `apply_preset_patches` 的注释）。不动它 ⇒ 用户会一直撞在同一个
                        // 误判上，且完全想不到是这个字段。改完立刻写回，下次不必再改。
                        let patched = cfg.apply_preset_patches();
                        // 新能力（多轮对话续接等）也要补进老配置 ——
                        // 磁盘上根本没有那个字段，`#[serde(default)]` 会让它静默变成
                        // "不支持"，用户表现为"升级了却没变化"。
                        let migrated = cfg.migrate_presets();
                        *self.inner.config.lock().unwrap() = cfg.clone();
                        let changed = [patched, migrated].concat();
                        if !changed.is_empty() {
                            crate::log_buffer::info(format!(
                                "已更新预置执行器的配置（依据实测 / 新能力）：{}",
                                changed.join("、")
                            ));
                            if let Err(e) = self.write(&cfg) {
                                crate::log_buffer::warn(format!(
                                    "修正后的任务台配置写盘失败（本次内存生效）: {e}"
                                ));
                            }
                        }
                        crate::log_buffer::info("Agent 任务台配置已加载");
                    }
                    Err(e) => {
                        // 解析失败**不覆盖**原文件：先保住现场，再退回默认值运行。
                        crate::log_buffer::err(format!(
                            "Agent 任务台配置解析失败（已保留原文件，本次用默认值运行）: {e}"
                        ));
                    }
                }
            }
            Err(_) => {
                // 首次运行：写一份默认配置，让用户打开设置页就有东西可改。
                let cfg = DeskConfig::default();
                *self.inner.config.lock().unwrap() = cfg.clone();
                if let Err(e) = self.write(&cfg) {
                    crate::log_buffer::warn(format!("Agent 任务台默认配置写入失败: {e}"));
                }
            }
        }
    }

    /// 读快照。
    pub fn get(&self) -> DeskConfig {
        self.inner.config.lock().unwrap().clone()
    }

    /// 写入并落盘。
    pub fn set(&self, mut cfg: DeskConfig) -> Result<DeskConfig, String> {
        cfg.normalize();
        // 用户手抄旧文档时也可能把错值填回来 ⇒ 同样打一次补丁。
        // 判定是"值等于已知错值才改"，不是"无脑回填预置" ⇒ 用户自定义的值一律保留。
        cfg.apply_preset_patches();
        // 迁移在这里也得跑：用户可能是从"没有这个字段"的旧版 UI 提交上来的配置。
        cfg.migrate_presets();
        self.write(&cfg)?;
        *self.inner.config.lock().unwrap() = cfg.clone();
        Ok(cfg)
    }

    /// 磁盘路径（未初始化时返回 None）。
    pub fn path(&self) -> Option<PathBuf> {
        self.inner.path.lock().unwrap().clone()
    }

    fn write(&self, cfg: &DeskConfig) -> Result<(), String> {
        let path = match self.inner.path.lock().unwrap().clone() {
            Some(p) => p,
            None => return Err("配置路径未初始化".to_string()),
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败：{e}"))?;
        }
        let text = serde_json::to_string_pretty(cfg)
            .map_err(|e| format!("序列化 Agent 任务台配置失败：{e}"))?;
        std::fs::write(&path, text).map_err(|e| format!("写入 {} 失败：{e}", path.display()))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_usable_out_of_box() {
        let cfg = DeskConfig::default();
        assert_eq!(cfg.slots.len(), SLOT_COUNT);
        assert!(cfg.slots.iter().all(|s| matches!(s, SlotBinding::Empty)));
        // **精确**断言而不是 `>= 3`：预置清单缩水过一次（7 → 3，2026-09-22），
        // 用下界断言的话，"顺手又删掉一条"不会有任何测试拦下来。
        let ids: Vec<&str> = cfg.drivers.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["doubao-api", "dsh", "codex-cli"],
            "预置执行器清单变了——若是有意为之，请同步这里与 RETIRED_DRIVER_IDS"
        );
        // 每个预置 driver 都必须有 id / label，否则 UI 渲染会出空白行。
        for d in &cfg.drivers {
            assert!(!d.id.is_empty(), "driver 缺 id");
            assert!(!d.label.is_empty(), "driver {} 缺 label", d.id);
        }
    }

    /// 退役迁移：老配置里的旧条目要删掉，**且指向它的槽位绑定必须一并清空**。
    ///
    /// 这条测试守的是一个"只有老用户能复现"的故障：执行器删了、绑定还在 ⇒
    /// 用户按下硬件键的那一刻才报"槽位绑定的执行器不存在"。
    #[test]
    fn migration_retires_old_presets_and_clears_their_slot_bindings() {
        let mut cfg = DeskConfig::default();
        // 模拟老用户的磁盘配置：多出 4 条退役执行器，其中一条还填过密钥。
        let mut old = cfg.drivers[0].clone();
        old.id = "deepseek-api".into();
        old.label = "DeepSeek API".into();
        old.api_key = "sk-OLD".into();
        cfg.drivers.push(old);
        let mut old_cli = cfg.drivers[1].clone();
        old_cli.id = "qwen-code".into();
        old_cli.label = "Qwen Code".into();
        cfg.drivers.push(old_cli);
        // 其中一个槽位正绑着将要退役的执行器，另一个绑着要保留的。
        cfg.slots[0] = SlotBinding::Exact {
            driver: "deepseek-api".into(),
        };
        cfg.slots[1] = SlotBinding::Exact {
            driver: "codex-cli".into(),
        };
        cfg.preset_features_version = None;

        let changed = cfg.migrate_presets();

        assert!(cfg.driver("deepseek-api").is_none(), "退役执行器没被删掉");
        assert!(cfg.driver("qwen-code").is_none(), "退役执行器没被删掉");
        assert!(matches!(cfg.slots[0], SlotBinding::Empty), "退役项被清空");
        // 保留项的绑定**绝不能**被顺手清掉。
        assert!(
            matches!(&cfg.slots[1], SlotBinding::Exact { driver } if driver == "codex-cli"),
            "误伤了保留执行器的槽位绑定"
        );
        // 变更说明里必须点名"密钥也一起没了"——这是有损迁移，不能静默。
        assert!(
            changed.iter().any(|c| c.contains("deepseek-api") && c.contains("密钥")),
            "有损迁移必须说明白：{changed:?}"
        );
        // 且日志里**不能**出现密钥内容。
        assert!(
            !changed.iter().any(|c| c.contains("sk-OLD")),
            "变更说明泄漏了密钥内容：{changed:?}"
        );

        // 幂等：标记已推进 ⇒ 第二次什么都不做（用户手工加回同名条目也不会被再删）。
        assert!(cfg.migrate_presets().is_empty());
    }

    #[test]
    fn no_api_key_is_preset() {
        let cfg = DeskConfig::default();
        // 密钥绝不进仓库——默认必须全空。
        assert!(cfg.drivers.iter().all(|d| d.api_key.is_empty()));
        assert!(cfg.tts.api_key.is_empty());
    }

    #[test]
    fn cli_drivers_have_home_and_prompt_placeholder() {
        let cfg = DeskConfig::default();
        for d in cfg.drivers.iter().filter(|d| d.kind == DriverKind::Cli) {
            assert!(
                d.env.iter().any(|e| e.starts_with("HOME=") || e.starts_with("DSH_HOME=")),
                "CLI driver {} 没显式给 HOME —— 会被 Cadence 污染的 ~ 咬",
                d.id
            );
            assert!(
                d.has_prompt_placeholder(),
                "CLI driver {} 的 args 缺 {{prompt}}，任务文本传不进去",
                d.id
            );
        }
    }

    /// 09-20 实测：codex `exec --json` 的 stdout **没有** `task_complete`
    /// （那是 rollout 文件里的 app-server 事件）。判据取的是 `turn.completed`。
    #[test]
    fn codex_completion_pattern_matches_its_real_stdout() {
        let codex = DeskConfig::default()
            .driver("codex-cli")
            .expect("预置必须有 codex-cli")
            .clone();
        let pat = codex.completion.pattern;
        assert!(pat.contains("turn.completed"), "实际则为每个任务误判失败：{pat:?}");
        // 两种格式都认：上游换回 app-server 事件格式时不必动代码、也不必动用户配置。
        assert!(
            pat.contains("task_complete"),
            "候选串里应保留 rollout 侧的事件名：{pat:?}"
        );
    }

    #[test]
    fn preset_patches_fix_known_wrong_values_but_never_user_values() {
        let mut cfg = DeskConfig::default();
        // 模拟"老用户的磁盘配置"：预置里那条被写成了实测证明是错的值。
        cfg.drivers
            .iter_mut()
            .find(|d| d.id == "codex-cli")
            .unwrap()
            .completion
            .pattern = "task_complete".to_string();

        let patched = cfg.apply_preset_patches();
        assert_eq!(patched, vec!["codex-cli".to_string()]);
        assert!(cfg.driver("codex-cli").unwrap().completion.pattern.contains("turn.completed"));

        // 再打一次必须**没有**副作用（否则每次启动都在改用户的东西）。
        assert!(cfg.apply_preset_patches().is_empty());

        // 用户自己设的值一律保留 —— 这一层是"纠错"，不是"重写预置"。
        let mut cfg2 = DeskConfig::default();
        cfg2.drivers
            .iter_mut()
            .find(|d| d.id == "codex-cli")
            .unwrap()
            .completion
            .pattern = "MY_OWN_MARKER".to_string();
        assert!(cfg2.apply_preset_patches().is_empty());
        assert_eq!(
            cfg2.driver("codex-cli").unwrap().completion.pattern,
            "MY_OWN_MARKER"
        );
    }

    #[test]
    fn old_config_gains_continuation_once_and_only_once() {
        // 模拟"磁盘上的旧配置"：codex 条目在，但没有 continuation 字段
        //（`#[serde(default)]` 把它变成 `None` ⇒ 表现为"这个工具不支持续接"）。
        let mut cfg = DeskConfig::default();
        cfg.drivers
            .iter_mut()
            .find(|d| d.id == "codex-cli")
            .unwrap()
            .continuation = None;
        cfg.preset_features_version = None;

        let added = cfg.migrate_presets();
        assert!(
            added.iter().any(|a| a.contains("codex-cli")),
            "必须给老配置补上续接能力：{added:?}"
        );
        assert!(cfg.driver("codex-cli").unwrap().continuation.is_some());

        // 第二次：世代标记已推进 ⇒ 不再动。这一条同时是"用户主动关掉不会被补回"的保证。
        assert!(cfg.migrate_presets().is_empty());
    }

    #[test]
    fn user_disabled_continuation_is_never_restored() {
        // 用户在设置页取消了勾选（`None`），重启后不该又被勾上 ——
        // 那是"改了没用"最烦人的一种形态。
        let mut cfg = DeskConfig::default();
        cfg.drivers
            .iter_mut()
            .find(|d| d.id == "codex-cli")
            .unwrap()
            .continuation = None;
        cfg.preset_features_version = Some(PRESET_FEATURES_VERSION);

        assert!(cfg.migrate_presets().is_empty());
        assert!(cfg.driver("codex-cli").unwrap().continuation.is_none());
    }

    #[test]
    fn dsh_preset_never_uses_latest() {
        let cfg = DeskConfig::default();
        let dsh = cfg.driver("dsh").unwrap();
        // 用 latest 会在上游 rc 迭代时静默换版本。这条断言守着"钉版本"的纪律。
        assert!(!dsh.args.iter().any(|a| a.contains("latest")));
    }

    #[test]
    fn normalize_pads_and_truncates_slots() {
        let mut cfg = DeskConfig::default();
        cfg.slots.clear();
        cfg.normalize();
        assert_eq!(cfg.slots.len(), SLOT_COUNT);

        cfg.slots.push(SlotBinding::Auto);
        cfg.normalize();
        assert_eq!(cfg.slots.len(), SLOT_COUNT);
    }

    #[test]
    fn normalize_restores_missing_preset_driver() {
        let mut cfg = DeskConfig::default();
        cfg.drivers.retain(|d| d.id != "dsh");
        cfg.normalize();
        assert!(cfg.driver("dsh").is_some(), "预置 driver 应被补回");
    }

    #[test]
    fn evidence_cmd_upgrades_confidence() {
        let mut sig = CompletionSignal {
            kind: CompletionKind::ProcessExitZero,
            pattern: String::new(),
            evidence_cmd: String::new(),
        };
        assert_eq!(sig.confidence(), Confidence::Medium);

        sig.evidence_cmd = "test -f out.png".into();
        // 世界门禁把"进程退出"从"未校验"升到"已校验"——这就是 DSH 文档里
        // "world 是独立门禁"那条在配置层的表达。
        assert_eq!(sig.confidence(), Confidence::High);
    }

    #[test]
    fn process_exit_alone_is_never_high() {
        let sig = CompletionSignal {
            kind: CompletionKind::ProcessExitZero,
            pattern: String::new(),
            evidence_cmd: String::new(),
        };
        // 课程卡 9 铁律：进程退出 ≠ 完成。
        assert_ne!(sig.confidence(), Confidence::High);
    }

    #[test]
    fn http_stream_end_is_high_by_itself() {
        let sig = CompletionSignal {
            kind: CompletionKind::HttpStreamEnd,
            pattern: String::new(),
            evidence_cmd: String::new(),
        };
        assert_eq!(sig.confidence(), Confidence::High);
    }

    #[test]
    fn is_configured_requires_key_or_command() {
        let api = DriverConfig {
            kind: DriverKind::Api,
            model: "m".into(),
            ..Default::default()
        };
        assert!(!api.is_configured(), "只有 model 没有 key 不算配置好");

        let cli = DriverConfig {
            kind: DriverKind::Cli,
            ..Default::default()
        };
        assert!(!cli.is_configured(), "空 command 不算配置好");

        let cli_ok = DriverConfig {
            kind: DriverKind::Cli,
            command: "dsh".into(),
            ..Default::default()
        };
        assert!(cli_ok.is_configured());
    }

    #[test]
    fn config_roundtrip_json() {
        let cfg = DeskConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let back: DeskConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.slots.len(), SLOT_COUNT);
        assert_eq!(back.drivers.len(), cfg.drivers.len());
        assert_eq!(back.tts.target_sample_rate, crate::tts::FIRMWARE_SAMPLE_RATE);
    }

    #[test]
    fn partial_json_falls_back_to_defaults_per_field() {
        // 模拟旧版本配置：只写了 enabled，其它字段全缺。
        let json = r#"{"enabled": true}"#;
        let cfg: DeskConfig = serde_json::from_str(json).unwrap();
        assert!(cfg.enabled);
        // 缺字段必须有默认值，不能反序列化失败。
        assert_eq!(cfg.slots.len(), SLOT_COUNT);
        assert!(!cfg.tts.voice.is_empty());
    }

    #[test]
    fn slot_binding_serde_is_tagged() {
        let b = SlotBinding::Capability {
            capability: Capability::Code,
            prefer: vec!["dsh".into()],
        };
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("\"mode\":\"capability\""));
        let back: SlotBinding = serde_json::from_str(&json).unwrap();
        match back {
            SlotBinding::Capability { capability, prefer } => {
                assert_eq!(capability, Capability::Code);
                assert_eq!(prefer, vec!["dsh".to_string()]);
            }
            other => panic!("反序列化成了 {:?}", other.mode_label()),
        }
    }

    #[test]
    fn store_writes_and_reads_back() {
        let dir = std::env::temp_dir().join(format!("easyop_desk_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let store = DeskConfigStore::new();
        store.init(&dir);

        let mut cfg = store.get();
        cfg.enabled = true;
        cfg.slots[0] = SlotBinding::Exact {
            driver: "doubao-api".into(),
        };
        store.set(cfg).unwrap();

        // 新实例读同一个目录，应该拿到刚写的内容。
        let store2 = DeskConfigStore::new();
        store2.init(&dir);
        let back = store2.get();
        assert!(back.enabled);
        assert!(matches!(back.slots[0], SlotBinding::Exact { .. }));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_survives_corrupt_file() {
        let dir = std::env::temp_dir().join(format!("easyop_desk_bad_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(DESK_CONFIG_FILE);
        std::fs::write(&path, "{ 这不是 JSON").unwrap();

        let store = DeskConfigStore::new();
        store.init(&dir);
        // 配置坏了也要能起来：退回默认值，且**不覆盖**原文件（保住现场）。
        assert_eq!(store.get().slots.len(), SLOT_COUNT);
        let raw = std::fs::read_to_string(&path).unwrap();
        assert_eq!(raw, "{ 这不是 JSON", "解析失败时不得覆盖原文件");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
