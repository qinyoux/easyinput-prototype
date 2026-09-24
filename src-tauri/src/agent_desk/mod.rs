//! Agent 任务台（`agent_desk`）—— easyop 内的常驻任务编排器。
//!
//! 把 EasyInput 键盘变成局域网内的「Agent 任务台」：按一下键说话 → 归属槽位 →
//! 排队交执行器（Driver）跑 → 完成后总结转 TTS 下发键盘 → 灯亮 / 按键播放。
//!
//! # 模块边界（当前进度）
//!
//! | 子模块 | 职责 | 状态 |
//! |---|---|---|
//! | `config` | 配置结构 + 持久化 + 预置 Driver | ✅ 已实现 |
//! | `queue` | 每槽 FIFO、同槽串行、跨槽并行、运行计数 | ✅ Phase 2 |
//! | `unread` | 未读代际 + 播放租约（含持久化） | ✅ Phase 2 |
//! | `driver` | 执行器校验 + Api/Cli 两种形态 + 世界门禁 | ✅ Phase 2 |
//! | `state` | 运行时状态合成 + 变更检测 | ✅ Phase 2 |
//! | `device_led` | 下行灯语：`0x16` 逐像素帧 + 心跳 | ✅ Phase 3（固件侧待烧） |
//! | `notice` / `notice_assets` / `audio_out` | 状态播报去抖、语料预合成缓存、PC 喇叭播放 | ✅ Phase 3（**板子喇叭未做**：固件没有"播任意 PCM"的通道） |
//! | `router` | 能力路由（显式优先 → 自动兜底） | ⏳ Phase 4 |
//!
//! **本模块只做纯增量**：不碰固件协议、不碰既有分流。删除 `agent_desk/` 目录
//! + 摘掉命令注册即完成回滚。
//!
//! # 编排器在这里（`mod.rs`）
//!
//! `run_driver` 是"一次执行"，而**编排器**是"把一次执行放进一个槽位的生命周期里"：
//! 入队 → 起执行 → 收终态 → 推下一条。它需要同时碰队列、未读、配置与 Tauri 事件，
//! 所以落在这个唯一对外的模块里，而不是散进 `queue.rs` / `state.rs`。
//!
//! ⛔ **尚未接上的一段（别误以为能用）**：
//!
//! 1. **按键链路**：`handle_reserved_uuid` 已接通 —— 按一下开始录音并记下归属槽，
//!    再按一下结束，识别结果按归属派进任务台（`session.dispatch_to_desk`）。
//!    固件按键只有一条报文、无按下/松开 ⇒ 只能 toggle。**这是硬件限制**；
//!    KEY1 同样是 toggle，但那是产品规定（短按），两边理由不同，
//!    别互相引用 —— 见 `ptt_hook::PttEvent`。
//!
//! ✅ **已接上、但只在 PC 侧**：任务跑完会自动产生播报（`emit_notice` 两条驱动源）
//! 与未读；播放走 `audio_out`（PC 声卡）。**从键盘喇叭出声**需要板子路，尚未做。

pub mod artifact;
pub mod audio_out;
pub mod codex_probe;
pub mod config;
pub mod delivery;
pub mod device_led;
pub mod driver;
pub mod history;
pub mod notice;
pub mod notice_assets;
pub mod queue;
pub mod state;
pub mod unread;

pub use config::{DeskConfig, DeskConfigStore, DriverKind, SlotBinding};

/// 仅本模块（含测试）用到的名字。
///
/// 不做 `pub use`：本模块在 `lib.rs` 里是私有模块，`pub use` 出来的名字在 crate 内
/// 不可达，rustc 会判定为 unused import 并报警告。
#[cfg(test)]
use config::{Capability, DESK_CONFIG_FILE};

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::capability::{ApiEnvelope, ErrorCode};
use crate::log_buffer;
use crate::runner::CancelToken;
use crate::tts::{TtsAudio, TtsClient};

use codex_probe::CodexProbe;
use config::{SummaryConfig, SLOT_COUNT};
use driver::{run_driver, NullSink, RunOutcome, RunRequest, RunStatus};
use notice::{DeskPulse, NoticeGate, NoticeKind, NoticePolicy};
use notice_assets::NoticeAssets;
use history::TaskRecord;
use queue::TaskId;
use state::{
    complete_task, describe_lease_error, describe_queue_error, lease_label, DispatchOutcome,
    DeskRuntime, RuntimeSnapshot, TaskTerminal,
};
use unread::{ConsumeOutcome, Lease, UnreadPersist};

/// 当前时间（unix 毫秒）。
///
/// 单独抽成函数是因为**所有时间戳都必须从同一个来源取**：
/// 测试要注入假时间、生产要用真实时间，若各处自己调 `SystemTime::now()`，
/// 就会得到"入队时间晚于开始时间"这类看起来像 bug 但其实是取时不一致的现象。
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 任务台运行时管理器：配置存储 + TTS 客户端 + 槽位运行时。
///
/// 三者放在一起是因为 **配置真源只有一个**（`DeskConfig`），而运行时状态必须以它为
/// 前提解释（槽位数、每个槽绑了谁）。`sync_tts()` 保证每次调用前客户端与配置一致，
/// 避免"设置页改了但客户端还用旧 key"这种最难查的错。
pub struct DeskManager {
    pub store: DeskConfigStore,
    tts: TtsClient,
    /// 槽位运行时（队列 + 未读）。**一把锁护住两者**——见 `state.rs` 的说明。
    runtime: Mutex<DeskRuntime>,
    /// 每槽的"执行泵"单飞标志。
    ///
    /// 一个槽同时只能有一个泵在跑（`compare_exchange` 决定谁拿到）。这是"同槽严格串行"
    /// 在**调度层**的第二道保险：`TaskQueue::start_next` 已经在数据层挡住了并发启动，
    /// 但如果没有这个标志，第二次派发会 spawn 出第二个泵——它取不到任务就退出，
    /// 看起来无害，却让"谁在驱动这个槽"这件事变得不可追踪。
    pumping: Vec<AtomicBool>,
    /// 每槽正在执行的取消令牌。
    ///
    /// 由泵在启动任务时放入、结束时清空。用户点"取消"时从这里取出并触发——
    /// 否则"取消"就只能取消还没跑的，正在跑的那条只能干等（超时上限 3 分钟）。
    cancel_tokens: Mutex<Vec<Option<CancelToken>>>,
    /// 灯语当前是否在设备上生效（决定"全空闲时该不该补发一帧 clear"）。
    ///
    /// 记住它，是为了让"从有活动变成全空闲"这件事**只下发一次** clear：
    /// 无条件周期性发 clear 既没有信息量，也会持续占用 HID 通道。
    led_active: AtomicBool,
    /// 上一次灯语下发是否成功。**只在翻转时记日志**。
    ///
    /// 设备没插是常态；每 5 秒一条 warn 会把真正有用的日志淹掉。
    /// 初值 `true`（"还没失败过"），这样第一次失败一定会留下一条记录。
    led_ok: AtomicBool,
    /// 状态播报（Notice）的去抖器。
    ///
    /// 放在管理器上而不是散在执行路径里：因为**两条驱动源**
    /// （终态走事件、进度走状态快照）必须共用同一个节奏账本 ——
    /// 各自持一份的话，"刚播完进度又播进度"就拦不住了。
    notice_gate: Mutex<NoticeGate>,
    /// 外部会话监工（C）的增量扫描器。
    ///
    /// **必须长期存活**：它持有每个已见文件的读取偏移。每次新建实例，
    /// 就等于把历史全部当成"刚刚完成"播一遍（`codex_probe` 的首轮说明）。
    probe: Mutex<CodexProbe>,
    /// 外部会话监工**独立**的去抖账本。
    ///
    /// ⚠️ 为什么不能与 `notice_gate` 共用：它们是**不同的信息源**。
    /// 共用一本账的话，"槽位任务刚播过 `Completed`"会把"外部会话也完成了"
    /// 判定成"与上一条相同 ⇒ 沉默"，于是**一条真实的结果被无声吞掉** ——
    /// 而"告诉你它完成了"正是这条链路存在的唯一理由。
    probe_gate: Mutex<NoticeGate>,
    /// 每槽**正在续接的会话**（多轮对话的关键状态）。
    ///
    /// # 为什么持久化，而 `pending_results` 不持久化
    ///
    /// 未读计数是"我欠你一句"，所以必须跨重启；而结果文本是"说了什么"，
    /// 重启后念不出内容也还能如实报错。会话 id 属于前者：它是**上游已经给过的事实**
    /// ——丢了它，用户说的"那刚才那个呢"就彻底答不上来，而这句追问很可能发生在
    /// 关掉软件去干活之后、第二天回来接着问的时候。
    ///
    /// ⚠️ 绑定了 `driver_id`：把 codex 的 thread id 传给另一个 CLI 是灾难，
    /// 换执行器必须自动作废。
    slot_sessions: Mutex<Vec<Option<SlotSession>>>,
    /// 每槽最近一次执行的结果（供「播报未读」用）。
    ///
    /// ⚠️ **刻意不持久化**：重启后未读**计数**还在（那是 `unread.rs` 承诺的），
    /// 但内容没了 —— 进程内存里的东西不可能跨重启。所以播报时若内容缺失，
    /// 必须明确告诉用户"有几条未读但内容已随重启丢失"，**不许**放一段假话。
    pending_results: Mutex<Vec<Option<SlotResult>>>,
    /// 任务历史（索引形态；每条任务的完整输出各自落盘）。
    ///
    /// ⚠️ 与 `pending_results` 正好相反：那个**刻意不持久化**（它只是"最近一条结果的
    /// 内存副本，用来播报"），而这个**必须持久化** ——
    /// "我三天前让 Agent 做的那件事，结论是什么"只有它能回答。
    /// 上限见 [`history::KEEP`]：超了淘汰最老的（连它的日志文件一起删）。
    history: Mutex<Vec<TaskRecord>>,
    /// 当前 PC 喇叭上正在播的内容。
    ///
    /// 这是**抢占规则**的落点：喇叭是物理单通道，所以"正在播什么"必须被显式记录，
    /// 否则无法回答"新来的这条该打断别人、还是该被丢弃"。
    playing: Mutex<Option<PlayingState>>,
}

/// 槽位会话的落盘文件名（在 `agent_desk_out/` 下）。
const SESSIONS_FILE: &str = "slot_sessions.json";

/// 某槽**正在续接**的会话（多轮对话的实现基础）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SlotSession {
    /// 上游给的会话标识（codex 是 `thread_id`）。
    pub id: String,
    /// 这个 id **属于哪个执行器**。跨执行器一律作废，理由见
    /// [`DeskManager::session_for`]。
    pub driver_id: String,
    /// 建立时刻（unix 毫秒），只用于界面展示"这段对话从什么时候开始"。
    pub at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SessionsPersist {
    slots: Vec<Option<SlotSession>>,
}

/// 界面上看的会话摘要（**不给完整 id**）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionView {
    pub driver_id: String,
    /// id 前 8 位：完整 UUID 对用户没有意义，但短 id 足以让他确认
    /// "这两轮是不是同一段对话"（比如自己吐槽前先看一眼是不是断了）。
    pub id_short: String,
    pub at_ms: u64,
}

/// 某槽最近一次执行的结果（供播报）。
#[derive(Clone, Debug)]
pub struct SlotResult {
    pub task_id: TaskId,
    /// 已经压成"可以念出来"的短文本（不是原始 stdout）。
    pub spoken: String,
    pub at_ms: u64,
    /// 是否成功（决定播报的开头措辞与是否要提示查看详情）。
    pub success: bool,
}

/// 喇叭上正在播的东西。
struct PlayingState {
    /// 通道占用编号（单调递增）。播完清场时用它确认"还是不是我占着"。
    id: u64,
    /// `None` = 正在播 Notice（状态播报）；`Some` = 正在播某槽的结果。
    ///
    /// 抢占规则由它区分：**Result 可以打断 Notice，反之不行**（方案 §2.6.1）。
    slot: Option<usize>,
    /// 结果播报持有的未读租约。Notice 播报**永远为 `None`** ——
    /// 它不碰未读账本，这是 §2.6.1 的 Gate。
    lease: Option<Lease>,
    control: audio_out::PlaybackControl,
}

impl Default for DeskManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DeskManager {
    pub fn new() -> Self {
        Self {
            store: DeskConfigStore::new(),
            tts: TtsClient::new(crate::tts::TtsConfig::default()),
            runtime: Mutex::new(DeskRuntime::new(SLOT_COUNT)),
            pumping: (0..SLOT_COUNT).map(|_| AtomicBool::new(false)).collect(),
            cancel_tokens: Mutex::new((0..SLOT_COUNT).map(|_| None).collect()),
            led_active: AtomicBool::new(false),
            led_ok: AtomicBool::new(true),
            notice_gate: Mutex::new(NoticeGate::new()),
            probe: Mutex::new(CodexProbe::new()),
            probe_gate: Mutex::new(NoticeGate::new()),
            slot_sessions: Mutex::new((0..SLOT_COUNT).map(|_| None).collect()),
            pending_results: Mutex::new((0..SLOT_COUNT).map(|_| None).collect()),
            history: Mutex::new(Vec::new()),
            playing: Mutex::new(None),
        }
    }

    /// 初始化：绑定配置目录 + 同步 TTS 配置 + 恢复未读。
    pub fn init(&self, config_dir: &std::path::Path) {
        self.store.init(config_dir);
        self.sync_tts();
        self.sync_notice();
        self.load_unread();
        self.load_sessions();
        self.load_history();
    }

    /// 把配置里的 TTS 段落推给客户端。
    fn sync_tts(&self) {
        self.tts.set_config(self.store.get().tts);
    }

    /// 把配置里的**播报段落**推给去抖器。
    ///
    /// 与 [`Self::sync_tts`] 同形，理由也一样：配置只在**两个时刻**进入运行时 ——
    /// `init`（进程启动）与 `desk_set_config`（用户点保存）。若改成"每条播报现读配置"，
    /// 就得回答"运行中改了配置算不算数"，而那个问题的答案会随调用点散落而漂移。
    ///
    /// 秒 → 毫秒的换算只在这里做一次，`notice.rs` 全程用毫秒。
    fn sync_notice(&self) {
        let cfg = self.store.get().notice;
        let marks_ms: Vec<u64> = cfg
            .progress_marks_secs
            .iter()
            .map(|s| s.saturating_mul(1_000))
            .collect();
        let policy = NoticePolicy::normalize(
            cfg.enabled,
            cfg.on_start,
            cfg.on_progress,
            cfg.on_complete,
            cfg.on_fail,
            cfg.min_interval_secs.saturating_mul(1_000),
            &marks_ms,
        );
        if marks_ms.len() > policy.progress_marks_ms.len() {
            // 不静默：配置里多写的档位没生效，得在日志里说清，否则用户会以为
            // "我配了 5 档怎么只播 3 档"是代码 bug。
            log_buffer::warn(format!(
                "任务台播报：配置了 {} 个进度档位，语料闭集只支持 {} 档 ⇒ 超出的已忽略",
                marks_ms.len(),
                NoticePolicy::MARK_CAPACITY
            ));
        }
        // 两个账本吃同一份策略：它们只是**信息源不同**，用户对"播什么、多频繁"的
        // 意愿是同一套。若只喂一个，外部会话的播报就会用默认策略悄悄跑起来。
        {
            let mut gate = self
                .notice_gate
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            gate.set_policy(policy.clone());
        }
        {
            let mut gate = self.probe_gate.lock().unwrap_or_else(|e| e.into_inner());
            gate.set_policy(policy);
        }
    }

    /// 产物目录：配置为空时回退到配置目录旁的 `agent_desk_out/`。
    pub fn output_dir(&self) -> PathBuf {
        let cfg = self.store.get();
        let raw = cfg.output_dir.trim();
        if !raw.is_empty() {
            return PathBuf::from(raw);
        }
        self.store
            .path()
            .and_then(|p| p.parent().map(|d| d.join("agent_desk_out")))
            .unwrap_or_else(|| std::env::temp_dir().join("easyop_agent_desk_out"))
    }

    // -----------------------------------------------------------------------
    // 运行时访问
    // -----------------------------------------------------------------------

    /// 锁住运行时。**调用方必须让锁的作用域尽量短**（不要跨 `.await` 持有）。
    fn runtime(&self) -> std::sync::MutexGuard<'_, DeskRuntime> {
        self.runtime.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 读一份运行时快照。
    pub fn runtime_snapshot(&self) -> RuntimeSnapshot {
        self.runtime().snapshot()
    }

    // -----------------------------------------------------------------------
    // 未读持久化
    // -----------------------------------------------------------------------

    fn unread_path(&self) -> Option<PathBuf> {
        self.store
            .path()
            .and_then(|p| p.parent().map(|d| d.join(state::UNREAD_FILE)))
    }

    /// 从磁盘恢复未读。
    ///
    /// **文件不存在 = 首次运行**，这不是错误（`Ok` 与"读不到"要分开看：
    /// 前者是正常状态，后者才需要告警）。损坏 JSON 只记日志、**不覆盖原文件**——
    /// 用户手改坏了还有机会恢复，覆盖掉就真没了。
    fn load_unread(&self) {
        let Some(path) = self.unread_path() else {
            return;
        };
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                log_buffer::warn(format!("任务台未读状态读取失败 {}: {e}", path.display()));
                return;
            }
        };
        match serde_json::from_str::<UnreadPersist>(&raw) {
            Ok(parsed) => {
                let total = parsed.slots.iter().map(|(p, _)| *p).sum::<u32>();
                self.runtime().apply_persist(&parsed);
                if total > 0 {
                    log_buffer::info(format!(
                        "任务台未读状态已恢复: {total} 条（跨重启保留）"
                    ));
                }
            }
            Err(e) => log_buffer::warn(format!(
                "任务台未读状态解析失败（已保留原文件，不覆盖）{}: {e}",
                path.display()
            )),
        }
    }

    /// 把未读落盘。失败不静默——它意味着"重启后通知会丢"。
    fn save_unread(&self) {
        let Some(path) = self.unread_path() else {
            return;
        };
        let persist = self.runtime().to_persist();
        match serde_json::to_string_pretty(&persist) {
            Ok(s) => {
                if let Err(e) = std::fs::write(&path, s) {
                    log_buffer::err(format!("任务台未读状态写入失败 {}: {e}", path.display()));
                }
            }
            Err(e) => log_buffer::err(format!("任务台未读状态序列化失败: {e}")),
        }
    }

    // -----------------------------------------------------------------------
    // 槽位会话（多轮对话续接）
    // -----------------------------------------------------------------------

    fn sessions_path(&self) -> PathBuf {
        self.output_dir().join(SESSIONS_FILE)
    }

    /// 从磁盘恢复槽位会话。
    ///
    /// 与未读同一套哲学：**文件不存在 = 首次运行**，不是错误；
    /// 解析失败只记日志、**不覆盖**（用户还有手改挽救的机会）。
    fn load_sessions(&self) {
        let path = self.sessions_path();
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                log_buffer::warn(format!("任务台会话状态读取失败 {}: {e}", path.display()));
                return;
            }
        };
        match serde_json::from_str::<SessionsPersist>(&raw) {
            Ok(p) => {
                let mut slots = self.slot_sessions.lock().unwrap_or_else(|e| e.into_inner());
                let n = p.slots.iter().filter(|s| s.is_some()).count();
                *slots = p.slots;
                slots.resize(SLOT_COUNT, None);
                if n > 0 {
                    log_buffer::info(format!(
                        "任务台{SESSIONS_FILE} 已恢复 {n} 个会话（跨重启可继续多轮对话）"
                    ));
                }
            }
            Err(e) => log_buffer::warn(format!(
                "任务台会话状态解析失败（已保留原文件）{}: {e}",
                path.display()
            )),
        }
    }

    fn save_sessions(&self) {
        let slots = self.slot_sessions.lock().unwrap_or_else(|e| e.into_inner());
        let persist = SessionsPersist {
            slots: slots.clone(),
        };
        let path = self.sessions_path();
        // ⚠️ 目录可能还不存在（用户从没预合成过语料时就是）。
        // `fs::write` 到不存在的目录会直接失败 —— 于是"会话丢了"
        // 表现为一个只有重启后才复现、且日志里只有一行 IO 错误的谜。
        let dir = self.output_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log_buffer::err(format!("任务台会话目录创建失败 {}: {e}", dir.display()));
            return;
        }
        match serde_json::to_string_pretty(&persist) {
            Ok(s) => {
                if let Err(e) = std::fs::write(&path, s) {
                    log_buffer::err(format!("任务台会话状态写入失败 {}: {e}", path.display()));
                }
            }
            Err(e) => log_buffer::err(format!("任务台会话状态序列化失败: {e}")),
        }
    }

    // -----------------------------------------------------------------------
    // 任务历史
    // -----------------------------------------------------------------------

    /// 历史目录：`output_dir()/tasks`。
    fn history_dir(&self) -> PathBuf {
        history::dir_of(&self.output_dir())
    }

    /// 从磁盘恢复任务历史。
    ///
    /// 与未读/会话同一套哲学：**读不到不是错误**（首次运行就是没有），
    /// 单条坏了只丢那一条，且一律**不覆盖**原文件 —— 用户手改坏了还有挽救机会。
    fn load_history(&self) {
        let dir = self.history_dir();
        let loaded = history::load(&dir);
        if let Some(last) = loaded.last() {
            log_buffer::info(format!(
                "任务台历史已恢复 {} 条（最近一条：S{} / {}）",
                loaded.len(),
                last.slot + 1,
                last.driver_id
            ));
        }
        let mut h = self.history.lock().unwrap_or_else(|e| e.into_inner());
        *h = loaded;
    }

    /// 追加一条任务历史：写全量日志 + 写索引 + 裁剪超限的老记录。
    ///
    /// # 为什么**先写日志、后写索引**
    ///
    /// 反过来会留下"索引里有这条、日志文件却不存在"的状态 —— 用户点开详情看到一片空白，
    /// 而那往往正是他最想看清的那一次（失败那次）。反过来（日志在、索引没记）只是
    /// 一个无人引用的多余文件，无害。
    fn record_history(&self, mut rec: TaskRecord, stdout: &str, stderr: &str) {
        let dir = self.history_dir();
        let name = history::log_file_name(rec.started_at_ms, rec.slot, rec.task_id);
        rec.log_file = history::write_log(&dir, &name, stdout, stderr);
        let mut h = self.history.lock().unwrap_or_else(|e| e.into_inner());
        history::append(&dir, &mut h, rec);
    }

    /// 最近 `limit` 条历史（**最新在前**）。
    pub fn recent_history(&self, limit: usize) -> Vec<TaskRecord> {
        let h = self.history.lock().unwrap_or_else(|e| e.into_inner());
        history::recent(&h, limit.clamp(1, history::KEEP))
    }

    /// 某条任务的全量日志（`None` = 没有这条记录，或它的日志文件已不在）。
    pub fn task_log(&self, task_id: u64) -> Option<String> {
        let dir = self.history_dir();
        let h = self.history.lock().unwrap_or_else(|e| e.into_inner());
        let rec = history::find(&h, task_id)?;
        history::read_log(&dir, rec)
    }

    /// 这条任务**该带上的**会话 id。
    ///
    /// # 为什么一定要核对 `driver_id`
    ///
    /// 会话 id 是上游那个工具内部的标识，**不是通用 token**。把 codex 的 thread id
    /// 交给另一个 CLI，最好的情况是"对方不认识这个参数、直接报错"，
    /// 最坏的情况是它把这个字符串当成别的东西用了。所以换执行器 ⇒ 旧会话自动作废。
    pub fn session_for(&self, slot: usize, driver_id: &str) -> Option<String> {
        let slots = self.slot_sessions.lock().unwrap_or_else(|e| e.into_inner());
        slots.get(slot).and_then(|s| s.as_ref()).and_then(|s| {
            if s.driver_id == driver_id {
                Some(s.id.clone())
            } else {
                None
            }
        })
    }

    /// 记下本轮建立的会话。
    pub fn set_session(&self, slot: usize, id: String, driver_id: String) {
        let mut slots = self.slot_sessions.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cell) = slots.get_mut(slot) {
            *cell = Some(SlotSession {
                id,
                driver_id,
                at_ms: now_ms(),
            });
        }
        drop(slots);
        self.save_sessions();
    }

    /// 丢弃某槽的会话 ⇒ **下一次任务会新起一轮对话**。
    ///
    /// 这是用户唯一的"重新开始"入口：没有它，一旦某个槽挂上会话，
    /// 用户再也没办法换个话题（每句话都会被当成上一轮的延续）。
    pub fn clear_session(&self, slot: usize) -> bool {
        let had = {
            let mut slots = self.slot_sessions.lock().unwrap_or_else(|e| e.into_inner());
            slots
                .get_mut(slot)
                .map(|cell| cell.take().is_some())
                .unwrap_or(false)
        };
        if had {
            self.save_sessions();
        }
        had
    }

    /// 给界面看的会话摘要。
    pub fn session_views(&self) -> Vec<Option<SessionView>> {
        let slots = self.slot_sessions.lock().unwrap_or_else(|e| e.into_inner());
        (0..SLOT_COUNT)
            .map(|i| {
                slots.get(i).and_then(|s| s.as_ref()).map(|s| SessionView {
                    driver_id: s.driver_id.clone(),
                    // 完整 id 太长且没意义，但前 8 位足够让人认出"是不是同一段对话"。
                    id_short: s.id.chars().take(8).collect(),
                    at_ms: s.at_ms,
                })
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // 路由（Phase 2 只支持"指定执行器"）
    // -----------------------------------------------------------------------

    /// 决定某个槽位这次用哪个执行器。
    ///
    /// # ⛔ 为什么 `Capability` / `Auto` 这里是错误而不是"尽量挑一个"
    ///
    /// 方案 §2.4.3 第三条硬约束：**失败不静默**。路由判不出、或候选全不可用时必须报错，
    /// 不许回落到"随便挑一个"。Phase 4 的 Router 还没做，所以现在这两个模式就是
    /// "判不出"——明确报错并告诉用户该怎么办，比悄悄用第一个 driver 好得多：
    /// 后者会让用户以为自动路由已经在工作了。
    pub fn resolve_driver(&self, slot: usize) -> Result<String, String> {
        let cfg = self.store.get();
        let Some(binding) = cfg.slots.get(slot) else {
            return Err(format!("槽位号 {} 越界（共 {} 个槽位）", slot, cfg.slots.len()));
        };
        match binding {
            SlotBinding::Empty => Err(format!(
                "槽位 S{} 未绑定执行器——请在「按键重映射 → 类型=任务台」里选一个执行器",
                slot + 1
            )),
            SlotBinding::Exact { driver } => {
                // 用注册表判定，而不是在这里再手写一遍检查：
                // 手写版会和 `validate_driver` 的结论慢慢漂移（改了 A 忘了 B），
                // 最终表现为"面板说可用、按下去报错"。
                let registry = driver::DriverRegistry::from_configs(&cfg.drivers);
                let Some(e) = registry.get(driver) else {
                    return Err(format!(
                        "槽位 S{} 绑定的执行器 {driver} 不存在（可能被删了）",
                        slot + 1
                    ));
                };
                if !e.is_schedulable() {
                    // 必须说出**具体是哪一条**不满足，不能只回一句"不可用"。
                    let why = if !e.validation.is_ok() {
                        e.validation.errors.join("；")
                    } else if !e.config.enabled {
                        "未启用".to_string()
                    } else {
                        "配置不完整（缺 API Key 或命令）".to_string()
                    };
                    return Err(format!(
                        "槽位 S{} 的执行器「{}」不可调度：{why}",
                        slot + 1,
                        e.config.label
                    ));
                }
                Ok(e.config.id.clone())
            }
            SlotBinding::Capability { .. } => Err(format!(
                "槽位 S{} 用的是「按能力」绑定，但能力路由尚未实现（Phase 4）——\
                 请先在设置里改成「指定执行器」",
                slot + 1
            )),
            SlotBinding::Auto => Err(format!(
                "槽位 S{} 用的是「自动判定」，但路由尚未实现（Phase 4）——\
                 请先在设置里改成「指定执行器」",
                slot + 1
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// 视图结构（给前端的投影，不含密钥）
// ---------------------------------------------------------------------------

/// Driver 的 UI 视图。
///
/// **`api_key` 不在这里**——前端的设置页用它自己的输入框管理密钥，
/// 列表视图不需要看到它，少一处暴露面。
#[derive(Clone, Debug, Serialize)]
pub struct DriverView {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub enabled: bool,
    /// 是否已具备发起请求的最小条件。
    pub configured: bool,
    /// **能否被调度**（校验无错 + 已启用 + 配置完整）。
    ///
    /// 与 `configured` 分开：`configured` 只回答"key/命令填了没"，
    /// 而"能不能真的跑"还要看形态与完成信号是否自洽。两个都给出，
    /// 前端才能说清"是缺 key 还是配错了"。
    pub schedulable: bool,
    /// 完成信号的中文标签。
    pub completion_label: String,
    /// 完成可信度（`已校验` / `未校验`）。
    pub confidence: String,
    /// 是否会经过世界门禁。
    pub gated: bool,
    pub capabilities: Vec<String>,
    /// 硬错误：这些会让该执行器被拒绝调度，必须展示。
    pub errors: Vec<String>,
    /// 软警告：能跑但有风险（可信度中 / 缺 HOME），**必须展示而不是静默吞掉**。
    pub warnings: Vec<String>,
    pub note: String,
}

/// 槽位视图。
#[derive(Clone, Debug, Serialize)]
pub struct SlotView {
    pub index: usize,
    pub mode: String,
    pub detail: String,
    /// 需要注意但不阻塞的事（如"能力路由尚未实现"）。
    ///
    /// 与 `detail` 分开而不是拼成一句：`detail` 说的是"这个槽配成什么样"，
    /// `notes` 说的是"它现在还不能用"。前端要给后者不同的视觉分量，
    /// 混在一句话里就淹没了。
    pub notes: Vec<String>,
}

/// 任务台总览（前端的唯一数据源）。
#[derive(Clone, Debug, Serialize)]
pub struct DeskSnapshot {
    pub enabled: bool,
    /// 配置是否已就绪到"能跑一个任务"的程度。
    pub ready: bool,
    /// 未就绪的具体原因（逐条列，不合并成一句"配置有误"）。
    pub blockers: Vec<String>,
    pub tts_configured: bool,
    pub tts_model: String,
    pub output_dir: String,
    pub config_path: String,
    pub drivers: Vec<DriverView>,
    pub slots: Vec<SlotView>,
}

/// TTS 试合成报告。
#[derive(Clone, Debug, Serialize)]
pub struct TtsTestReport {
    pub sample_rate: u32,
    pub samples: usize,
    pub duration_ms: u64,
    pub characters: Option<u64>,
    /// 落盘的 WAV 绝对路径（供试听与人工核验）。
    pub wav_path: String,
}

/// Driver 探测报告。
///
/// `scope` 明确写出本次只做了哪一层检查——**不同层次的探测能证明的事不同**，
/// 混成一句 "可用/不可用" 会让排错时无法定位。
#[derive(Clone, Debug, Serialize)]
pub struct DriverProbe {
    pub id: String,
    pub ready: bool,
    /// 检查范围：`static`（仅配置完整性）| `runtime`（真跑一次）。
    pub scope: String,
    pub detail: String,
}

// ---------------------------------------------------------------------------
// 命令
// ---------------------------------------------------------------------------

/// 读取任务台配置。
#[tauri::command]
pub fn desk_get_config(mgr: State<'_, DeskManager>) -> ApiEnvelope<DeskConfig> {
    ApiEnvelope::ok(mgr.store.get())
}

/// 写入任务台配置（含 TTS / Driver / 槽位）。
#[tauri::command]
pub fn desk_set_config(
    mgr: State<'_, DeskManager>,
    config: DeskConfig,
) -> ApiEnvelope<DeskConfig> {
    match mgr.store.set(config) {
        Ok(cfg) => {
            mgr.sync_tts();
            mgr.sync_notice();
            log_buffer::info("Agent 任务台配置已保存");
            ApiEnvelope::ok(cfg)
        }
        Err(e) => {
            log_buffer::err(format!("Agent 任务台配置保存失败: {e}"));
            ApiEnvelope::err(ErrorCode::Internal, e)
        }
    }
}

/// 单独更新 TTS 配置（设置页的"保存"按钮走这条，不必回传整份配置）。
#[tauri::command]
pub fn desk_set_tts_config(
    mgr: State<'_, DeskManager>,
    tts: crate::tts::TtsConfig,
) -> ApiEnvelope<crate::tts::TtsConfig> {
    let mut cfg = mgr.store.get();
    cfg.tts = tts;
    match mgr.store.set(cfg) {
        Ok(cfg) => {
            mgr.sync_tts();
            log_buffer::info("TTS 配置已保存");
            ApiEnvelope::ok(cfg.tts)
        }
        Err(e) => ApiEnvelope::err(ErrorCode::Internal, e),
    }
}

/// 任务台总览快照。
#[tauri::command]
pub fn desk_snapshot(mgr: State<'_, DeskManager>) -> ApiEnvelope<DeskSnapshot> {
    ApiEnvelope::ok(build_snapshot(&mgr))
}

/// 探测一个 Driver（当前仅静态检查配置完整性）。
///
/// ⚠️ **不做运行时探测**：真跑一次子进程/HTTP 会引入副作用与延迟，
/// 属 Phase 4 的 `probe()`。这里的 `scope` 字段把这件事说明白，
/// 免得用户以为"探测通过 = 一定能跑"。
#[tauri::command]
pub fn desk_probe_driver(mgr: State<'_, DeskManager>, id: String) -> ApiEnvelope<DriverProbe> {
    let cfg = mgr.store.get();
    // ★ 复用注册表的校验结论，而不是在这里再手写一套。
    // 手写版与 `validate_driver` 会各自演化，最后表现为"探测说没问题、派发却报配置有误"。
    let registry = driver::DriverRegistry::from_configs(&cfg.drivers);
    let Some(entry) = registry.get(&id) else {
        return ApiEnvelope::err(ErrorCode::NotFound, format!("没有 id 为 {id} 的执行器"));
    };
    let d = &entry.config;

    // 硬错误：会导致拒绝调度。
    let mut issues = entry.validation.errors.clone();
    if !d.enabled {
        issues.push("未启用（不会被调度）".into());
    }
    // 软提醒：不阻止调度，但用户该知道。
    let mut notes = entry.validation.warnings.clone();
    if d.kind == DriverKind::Cli && d.cwd.trim().is_empty() {
        // 这条不放进 validate_driver：它是一个"某些 CLI 才需要"的提示，
        // 而不是所有 CLI 的通病（bl/qwen 不需要 cwd，dsh 需要）。
        notes.push("未填工作目录——部分 CLI（如 dsh）没有 cwd 会拒绝执行".into());
    }

    let ready = issues.is_empty();
    let detail = if ready {
        let mut s = format!("静态检查通过（{}）。尚未做运行时探测。", d.label);
        if !notes.is_empty() {
            s.push_str(&format!("注意：{}", notes.join("；")));
        }
        s
    } else {
        issues.join("；")
    };

    // 顺带报出"全局还有几个能用的"——排错时这一个数字能立刻区分
    // "就这一个配错了"和"整个执行器体系都没配好"。
    let available = registry.schedulable().count();

    ApiEnvelope::ok(DriverProbe {
        id,
        ready,
        scope: "static".into(),
        detail: format!("{detail}当前可调度执行器 {available} 个。"),
    })
}

/// TTS 试合成：真调一次服务，落在磁盘上，供人工试听。
///
/// 这是 Phase 1 的验收点——"能落地成可听的 wav"。
///
/// 返回 `Result<ApiEnvelope<..>, String>`：async 命令里带引用参数时 Tauri 要求
/// 这样签名（与 `test_llm_connection` 同形）。业务失败走信封的 `code`，
/// 外层 `Err` 只留给"命令本身无法执行"。
#[tauri::command]
pub async fn desk_tts_test(
    mgr: State<'_, DeskManager>,
    text: String,
) -> Result<ApiEnvelope<TtsTestReport>, String> {
    mgr.sync_tts();
    // 先把客户端 clone 出来（内部是 Arc，代价极低），让 `await` 期间不依赖 `State`。
    // 这样 future 一定是 Send 的，也不会和别的命令抢锁。
    let tts = mgr.tts.clone();

    let audio = match tts.synthesize(&text).await {
        Ok(a) => a,
        Err(e) => return Ok(ApiEnvelope::err(e.code(), e.message())),
    };

    match write_preview_wav(&mgr, &audio) {
        Ok(path) => Ok(ApiEnvelope::ok(TtsTestReport {
            sample_rate: audio.sample_rate,
            samples: audio.pcm.len(),
            duration_ms: audio.duration_ms,
            characters: audio.characters,
            wav_path: path,
        })),
        Err(e) => Ok(ApiEnvelope::err(ErrorCode::Internal, e)),
    }
}

/// 把试听 WAV 落到产物目录。
fn write_preview_wav(mgr: &DeskManager, audio: &TtsAudio) -> Result<String, String> {
    let dir = mgr.output_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建产物目录失败：{e}"))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("tts_preview_{ts}.wav"));
    std::fs::write(&path, audio.to_wav_bytes())
        .map_err(|e| format!("写入 {} 失败：{e}", path.display()))?;
    Ok(path.to_string_lossy().to_string())
}

// ---------------------------------------------------------------------------
// 状态播报（Notice）与未读播报（Result）—— PC 侧喇叭
//
// ⛔ 一条贯穿全段的边界：**Notice 不写未读账本**。状态播报既不 push_result
// 也不 grant —— 用户看到"未读"灯亮，按下去必须真的能听到"结果"。
// 少了这条，灯就骗人了，而"灯可信"是整台设备的根。
// ---------------------------------------------------------------------------

/// 一份语料在磁盘上的状态。
#[derive(Serialize)]
pub struct NoticeItemView {
    pub kind: NoticeKind,
    pub code: u8,
    pub text: &'static str,
    /// 是否已有**可用**缓存（存在且长度合理）。
    pub cached: bool,
    pub path: Option<String>,
    pub bytes: Option<u64>,
}

#[derive(Serialize)]
pub struct NoticeStatusReport {
    pub dir: String,
    pub total: usize,
    pub cached: usize,
    /// ⭐ 全部就绪 ⇒ **离线播报可用**：拔网线也能播状态。
    ///
    /// 这是"离线可用"这个承诺的唯一判据 —— 它必须可查询，而不是一句口头保证。
    pub offline_ready: bool,
    /// TTS 是否已配置。没配置就**无法**预合成，必须让用户看到原因。
    pub tts_configured: bool,
    /// 上一条真的播出去的通知。
    ///
    /// 诊断用："为什么刚才没响"的第一步排查就是看这里 ——
    /// 如果是 `None`，说明从没播过（去抖没放行 / 事件没到）。
    pub last_spoken: Option<NoticeKind>,
    pub items: Vec<NoticeItemView>,
}

#[derive(Serialize)]
pub struct NoticePrepareFailure {
    pub kind: NoticeKind,
    pub reason: String,
}

#[derive(Serialize)]
pub struct NoticePrepareReport {
    pub synthesized: Vec<NoticeKind>,
    pub already_cached: Vec<NoticeKind>,
    pub failed: Vec<NoticePrepareFailure>,
    pub offline_ready: bool,
    pub duration_ms: u64,
}

#[derive(Serialize)]
pub struct NoticePlayReport {
    pub kind: NoticeKind,
    pub text: &'static str,
    /// 是否**完整**播完（被抢占/中断则为 false）。
    pub completed: bool,
    pub duration_ms: u64,
    pub played_samples: usize,
    pub total_samples: usize,
}

#[derive(Serialize)]
pub struct RenderResultReport {
    pub slot: usize,
    pub task_id: Option<TaskId>,
    /// 真正送给 TTS 的文本。回传给前端是为了**可核对** ——
    /// "它到底念了什么"不该是个谜。
    pub spoken: String,
    /// 这条结果本身是成功还是失败（与"有没有播完"是两件事）。
    pub success: bool,
    /// 这条结果产生的时刻（unix 毫秒）。
    pub result_at_ms: u64,
    pub completed: bool,
    pub lease_token: u64,
    /// ⭐ 未读是否被消费。**只有完整播完才为 true** —— 被中断时未读必须留着。
    pub consumed: bool,
    /// 消费后该槽剩余的未读条数。
    pub unread_left: u32,
    pub duration_ms: u64,
}

/// 喇叭占用的单调编号，用于"播完清场时确认还是不是自己"。
static CHANNEL_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// 租约错误 → 统一错误码（按"用户该怎么办"分类）。
fn lease_error_code(e: &unread::LeaseError) -> ErrorCode {
    match e {
        unread::LeaseError::NothingToPlay { .. } => ErrorCode::NotFound,
        unread::LeaseError::ChannelBusy { .. } => ErrorCode::Conflict,
        unread::LeaseError::StaleLease { .. } => ErrorCode::Conflict,
        unread::LeaseError::SlotOutOfRange { .. } => ErrorCode::InvalidInput,
    }
}

/// 此刻是否允许 `slot` 开始占用喇叭。`None` = 这是一条 Notice。
///
/// 规则（方案 §2.6.1，两条通道优先级不同）：
///
/// - **结果正在播 ⇒ Notice 一律丢弃**。绝不打断"有结果等你听" ——
///   这是灯的承诺，比"知道它还在跑"重要一个数量级。
/// - **结果可以接管任何东西**（包括另一条结果）：用户新按的键就是最新意图。
/// - **Notice 之间后到者接管**：都是低优先级、可丢弃的信息，排队没有价值。
fn may_start_playing(mgr: &DeskManager, slot: Option<usize>) -> bool {
    let playing = mgr.playing.lock().unwrap_or_else(|e| e.into_inner());
    match playing.as_ref() {
        None => true,
        Some(current) => !(slot.is_none() && current.slot.is_some()),
    }
}

/// 占用喇叭：打断当前播放（若有）并登记新状态。返回 `(通道号, 被抢占者的租约)`。
///
/// 刻意**不在这里释放**被抢占者的租约：释放要拿 `runtime` 锁，而这里正持有
/// `playing` 锁 —— 两把锁交叉获取是死锁的经典配方。租约的处置交给调用方在锁外做。
fn take_channel(
    mgr: &DeskManager,
    slot: Option<usize>,
    lease: Option<Lease>,
    control: audio_out::PlaybackControl,
) -> (u64, Option<Lease>) {
    let id = CHANNEL_ID.fetch_add(1, Ordering::Relaxed);
    let mut playing = mgr.playing.lock().unwrap_or_else(|e| e.into_inner());
    let prev = playing.take();
    let prev_lease = match prev {
        Some(p) => {
            // "抢占"的字面含义：立刻让前一个闭嘴。
            p.control.stop();
            p.lease
        }
        None => None,
    };
    *playing = Some(PlayingState {
        slot,
        lease,
        control,
        id,
    });
    (id, prev_lease)
}

/// 播放结束后清场 —— **只在占位者还是自己时**才清。
///
/// 不做这个判断会出现这种竞态：A 播完 → 清空 → 此刻 B 刚接管并写入了自己的状态
/// → A 把 B 清掉了 ⇒ B 播完后再没人清场，喇叭被永久标记为"占用中"。
///
/// ⚠️ 这里**不调** `control.stop()`：`stop` 会污染 `completed_fully()`，
/// 而"是否完整播完"是消费未读的**唯一凭据**。已经播完的东西也没什么可停的；
/// 真正需要打断的场景（被抢占）在 [`take_channel`] 里已经停过了。
fn clear_channel_if_mine(mgr: &DeskManager, id: u64) {
    let mut playing = mgr.playing.lock().unwrap_or_else(|e| e.into_inner());
    if playing.as_ref().map(|p| p.id) == Some(id) {
        let _ = playing.take();
    }
}

/// 查询固定语料的缓存状态。
#[tauri::command]
pub fn desk_notice_status(mgr: State<'_, DeskManager>) -> ApiEnvelope<NoticeStatusReport> {
    let assets = NoticeAssets::new(&mgr.output_dir());
    let items: Vec<NoticeItemView> = NoticeKind::ALL
        .iter()
        .map(|kind| {
            let cached_path = assets.cached(*kind);
            let bytes = cached_path
                .as_ref()
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| m.len());
            NoticeItemView {
                kind: *kind,
                code: kind.code(),
                text: kind.text(),
                cached: cached_path.is_some(),
                path: cached_path.map(|p| p.to_string_lossy().to_string()),
                bytes,
            }
        })
        .collect();

    let (cached, total) = assets.cached_count();
    let last_spoken = {
        let gate = mgr.notice_gate.lock().unwrap_or_else(|e| e.into_inner());
        gate.last_kind()
    };
    ApiEnvelope::ok(NoticeStatusReport {
        dir: assets.dir().to_string_lossy().to_string(),
        total,
        cached,
        offline_ready: cached == total,
        tts_configured: mgr.tts.config().is_configured(),
        last_spoken,
        items,
    })
}

/// 预合成固定语料 ⇒ **离线播报可用的前提**。
///
/// `force = false` 只补缺失的（日常用法）；`force = true` 全部重合成
/// （换了音色 / 改了语料措辞之后用）。
///
/// **单条失败不中断**：10 句里 9 句成功，那 9 句就应该留下来，
/// 而不是因为第 10 句超时把前面全白做。所以这里是"逐条尽力 + 完整汇报"。
#[tauri::command]
pub async fn desk_prepare_notices(
    mgr: State<'_, DeskManager>,
    force: bool,
) -> Result<ApiEnvelope<NoticePrepareReport>, String> {
    mgr.sync_tts();
    let tts = mgr.tts.clone();
    let assets = NoticeAssets::new(&mgr.output_dir());
    let started = now_ms();

    let targets: Vec<NoticeKind> = if force {
        // 全量重合成意味着语料内容很可能变了（改措辞 / 换音色）⇒
        // 去抖器里"上一条播的是什么"这个记录随之作废。
        // 不复位的话会出现"改了措辞但半天不响，因为新内容与旧记录判为同一句"。
        mgr.notice_gate
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .reset();
        NoticeKind::ALL.to_vec()
    } else {
        assets.missing()
    };
    let already_cached: Vec<NoticeKind> = NoticeKind::ALL
        .iter()
        .copied()
        .filter(|k| !targets.contains(k) && assets.cached(*k).is_some())
        .collect();

    let mut synthesized = Vec::new();
    let mut failed = Vec::new();
    for kind in targets {
        match tts.synthesize(kind.text()).await {
            Ok(audio) => match assets.store(kind, &audio.to_wav_bytes()) {
                Ok(_) => synthesized.push(kind),
                Err(e) => failed.push(NoticePrepareFailure { kind, reason: e }),
            },
            Err(e) => failed.push(NoticePrepareFailure {
                kind,
                reason: e.message(),
            }),
        }
    }

    Ok(ApiEnvelope::ok(NoticePrepareReport {
        offline_ready: assets.all_cached(),
        synthesized,
        already_cached,
        failed,
        duration_ms: now_ms().saturating_sub(started),
    }))
}

/// 出声失败的原因。
///
/// 分成三类而不是一个 `String`，是因为**自动播报**对三类的反应完全不同：
/// `NoAsset` 已经在那行日志里标注过了（不必再刷一遍），`Busy` 是**正常行为**
/// （不打断"有结果等你听"，见 `may_start_playing`），只有 `Failed` 才是真的故障。
/// 混成一串字符串 ⇒ 要么把正常的抢占当成错误刷屏，要么把真故障淹掉。
enum NoticeSpeakError {
    /// 语料没预合成 ⇒ 这一句注定无声。
    NoAsset(String),
    /// 喇叭正被未读结果占用 ⇒ 刻意丢弃，不是故障。
    Busy(String),
    /// 读文件 / 解析 / 播放设备出错。
    Failed(String),
}

impl NoticeSpeakError {
    fn message(&self) -> &str {
        match self {
            Self::NoAsset(m) | Self::Busy(m) | Self::Failed(m) => m,
        }
    }
}

/// 读预合成语料并解成 PCM。
///
/// 两种失败分开报：`NoAsset`（**还没合成过** —— 用户可以去设置里点一下预合成）
/// 与 `Failed`（文件在、但读不动或不是合法 WAV —— 那才是真故障）。
/// 压成一句"播不了"会让这两种情况在排查时没法区分。
fn load_cached_asset(
    mgr: &DeskManager,
    kind: NoticeKind,
) -> Result<(Vec<i16>, u32), NoticeSpeakError> {
    let assets = NoticeAssets::new(&mgr.output_dir());
    let Some(path) = assets.cached(kind) else {
        return Err(NoticeSpeakError::NoAsset(format!(
            "语料「{}」尚未预合成，无法离线播放。请先在任务台设置里点「预合成语料」。",
            kind.text()
        )));
    };
    let wav = std::fs::read(&path)
        .map_err(|e| NoticeSpeakError::Failed(format!("读取语料 {} 失败：{e}", path.display())))?;
    crate::tts::parse_wav_pcm16_mono(&wav).map_err(|e| {
        NoticeSpeakError::Failed(format!("语料 {} 不是合法的 WAV：{e:?}", path.display()))
    })
}

/// 播一条状态语料（**不涉及未读**）—— 命令与自动播报共用的唯一出口。
///
/// # 为什么必须落在宿主这一层
///
/// 喇叭是**设备级的承诺**：用户按了键、听到"任务已开始"，才会放心走开。
/// 若把出声交给前端（收到事件后自己放音频），那么任务台页面没打开、
/// 或正处在切页的那一瞬间，这一句就**静默消失** —— 而丢的偏偏是他正在等的那句。
/// 这正是方案里"播放必须在后端"那条铁律的实际含义：不是实现偏好，是承诺的可靠性。
async fn speak_notice(
    mgr: &DeskManager,
    kind: NoticeKind,
    voice_text: Option<&str>,
) -> Result<NoticePlayReport, NoticeSpeakError> {
    // ---- 抢占检查（在锁内做，只读一眼）----
    if !may_start_playing(mgr, None) {
        return Err(NoticeSpeakError::Busy(
            "结果正在播放 —— 状态播报被丢弃（不打断「有结果等你听」）".to_string(),
        ));
    }

    // ---- 取音频：优先念**动态正文**，失败退回预合成语料 ----
    //
    // # 为什么降级链必须放在这一层
    //
    // 动态合成的失败来源很多（没配 TTS key / 网络不通 / 文本超长 / 服务限流），
    // 而预合成语料是离线的、几乎不会失败。把这条链写在这里，**所有**调用方
    // （终态播报、外部会话监工）都自动获得"念不成正文，至少还有固定语料"的保证；
    // 写到调用方去则会随调用点增加而各写一份、并各自漂移。
    let (pcm, rate) = match voice_text.map(str::trim).filter(|s| !s.is_empty()) {
        Some(text) => match mgr.tts.synthesize(text).await {
            Ok(a) => (a.pcm, a.sample_rate),
            Err(e) => {
                log_buffer::warn(format!(
                    "任务台动态播报合成失败，退回固定语料「{}」：{}",
                    kind.text(),
                    e.message()
                ));
                load_cached_asset(mgr, kind)?
            }
        },
        None => load_cached_asset(mgr, kind)?,
    };

    let started = now_ms();
    let control = audio_out::play_pcm(pcm, rate).map_err(NoticeSpeakError::Failed)?;

    // ---- 占位 + 播完后清场 ----
    let (id, preempted) = take_channel(mgr, None, None, control.clone());
    if preempted.is_some() {
        // Notice 抢占别人的租约是不可能的（Notice 从不持有租约）。
        // 真出现了说明有 bug —— 但不能静默。
        log_buffer::err("任务台播报：Notice 竟持有租约，这是不应发生的状态".to_string());
    }

    let waiter = control.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        // 上限 = 音频时长 + 5 秒，避免设备异常时永远等下去。
        let timeout = std::time::Duration::from_millis(
            waiter.total_samples() as u64 * 1000 / 48_000 + 5_000,
        );
        waiter.wait_finished(timeout)
    })
    .await;

    // ⚠️ 顺序要紧：**先**判定"是否完整播完"，**再**清场 ——
    // 清场会动共享的播放句柄，反过来读它的状态就不可靠了。
    let completed = control.completed_fully();
    clear_channel_if_mine(mgr, id);

    let report = NoticePlayReport {
        kind,
        text: kind.text(),
        completed,
        duration_ms: now_ms().saturating_sub(started),
        played_samples: control.played_samples(),
        total_samples: control.total_samples(),
    };

    // ⚠️ **必须检查"到底播出去几个样本"**，只看 finished 是不够的。
    //
    // 流已经建立、也"播完"了，但一个样本都没送进设备 —— 这是设备端静默失败的典型形状
    // （输出被别的程序独占、系统音量为 0、默认设备是个没人接的虚拟卡片）。
    // 那种情况下每一步返回值都正常，用户却一声没听见。
    // 这条日志把"没声音"从玄学变成一条可以照着查的记录。
    //
    // ⚠️ 但"0 样本"有**两个完全不同的成因**，混成一条告警会把人带偏（实测踩到）：
    //
    //   ① 被后一条播报抢占 —— `take_channel` 会对前一个调 `stop()`。
    //      派发那一刻「任务已开始」紧跟「正在推进」触发，后者必被前者打断。
    //      这是**设计如此**（越新的状态越该先说），不是故障。
    //   ② 设备真的没出声 —— 那才需要用户去查声音设置。
    //
    // 判据就是 `stop_requested()`：它为真说明是我们自己主动掐的。
    // 早先这里一律按 ② 报，于是每次派发都稳定产出一条"去检查默认输出设备"的
    // 假警报 —— 用户照着查会发现设备一切正常，然后学会忽略这类告警
    // （比不报警更糟：真出 ② 的时候也没人看了）。
    if report.played_samples == 0 {
        if control.stop_requested() {
            log_buffer::info(format!(
                "任务台播报「{}」被后一条播报抢占而中断（{} 样本一个没播）——\
                 正常现象：越新的状态越该先出声",
                kind.text(),
                report.total_samples
            ));
        } else {
            log_buffer::warn(format!(
                "任务台播报「{}」一个样本都没播出去（设备共应有 {} 样本）——\
                 去 Windows 声音设置里看一眼系统**默认输出设备**是不是你在用的那只，以及音量为 0 与否",
                kind.text(),
                report.total_samples
            ));
        }
    }

    Ok(report)
}

/// 各槽当前的会话摘要（界面据此显示"现在在续接哪一段对话"）。
#[tauri::command]
pub fn desk_session_info(mgr: State<'_, DeskManager>) -> ApiEnvelope<Vec<Option<SessionView>>> {
    ApiEnvelope::ok(mgr.session_views())
}

/// 丢掉某槽的会话 ⇒ **下一次任务会新起一轮对话**。
///
/// 这是"重新开始"的唯一入口。没有它，一个槽一旦挂上会话就再也下不来 ——
/// 用户想换个话题时，每句话都会被当成上一轮的延续，而系统看起来"工作正常"。
#[tauri::command]
pub fn desk_reset_session(mgr: State<'_, DeskManager>, slot: usize) -> ApiEnvelope<bool> {
    if slot >= SLOT_COUNT {
        return ApiEnvelope::err(
            ErrorCode::InvalidInput,
            format!("槽位号 {slot} 越界（合法范围 0..{SLOT_COUNT}）"),
        );
    }
    ApiEnvelope::ok(mgr.clear_session(slot))
}

/// 播一条状态语料（**不涉及未读**）—— 给设置页「试听」用。
#[tauri::command]
pub async fn desk_play_notice(
    mgr: State<'_, DeskManager>,
    kind: NoticeKind,
) -> Result<ApiEnvelope<NoticePlayReport>, String> {
    match speak_notice(&mgr, kind, None).await {
        Ok(report) => Ok(ApiEnvelope::ok(report)),
        Err(e) => Ok(ApiEnvelope::err(
            match &e {
                NoticeSpeakError::NoAsset(_) => ErrorCode::NotConfigured,
                NoticeSpeakError::Busy(_) => ErrorCode::Conflict,
                NoticeSpeakError::Failed(_) => ErrorCode::Internal,
            },
            e.message().to_string(),
        )),
    }
}

/// 播报某槽的**未读结果**：取租约 → 合成 → 播放 → **只有完整播完才消费未读**。
///
/// # 这就是 Phase 3.5 的语义在 PC 侧的落地
///
/// | 发生什么 | 未读 |
/// |---|---|
/// | 完整播完 | **消费**（灯灭一条） |
/// | 播到一半被抢占 / 中断 | **保留**（灯继续亮） |
/// | 播到一半进程消失（拔电） | **保留**（未读是持久化的，租约刻意不持久化） |
///
/// 最后一行是"漂到一半拔电未读仍在"能自动成立的原因：未读账本落盘，
/// 而"正在播"这个物理状态不落盘 —— 重启后没有租约，也就没有悬空的消费。
#[tauri::command]
pub async fn desk_render_result(
    app: tauri::AppHandle,
    mgr: State<'_, DeskManager>,
    slot: usize,
) -> Result<ApiEnvelope<RenderResultReport>, String> {
    mgr.sync_tts();
    let tts = mgr.tts.clone();

    if slot >= SLOT_COUNT {
        return Ok(ApiEnvelope::err(
            ErrorCode::InvalidInput,
            format!("槽位号 {slot} 越界（合法范围 0..{SLOT_COUNT}）"),
        ));
    }

    // ---- 1. 取租约（喇叭是全局单通道，先到先得）----
    let lease = {
        let mut rt = mgr.runtime();
        rt.unread_mut().grant(slot, now_ms())
    };
    let lease = match lease {
        Ok(l) => l,
        Err(e) => {
            return Ok(ApiEnvelope::err(
                lease_error_code(&e),
                describe_lease_error(&e),
            ))
        }
    };
    let lease_token = lease.token;

    // ---- 2. 取结果文本 ----
    let held = {
        let results = mgr
            .pending_results
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        results.get(slot).and_then(|r| r.clone())
    };
    let Some(held) = held else {
        // 有未读但没有内容 —— 这在重启之后是**正常**的（内容不持久化）。
        // 必须说清楚，否则用户会以为"按了没反应"是个 bug。
        let left = {
            let mut rt = mgr.runtime();
            rt.unread_mut().release(lease).ok();
            rt.unread_mut().pending(slot).unwrap_or(0)
        };
        return Ok(ApiEnvelope::err(
            ErrorCode::NotFound,
            format!(
                "S{} 有 {} 条未读，但内容已随重启丢失（未读计数会保留，正文不会）。\
                 请到执行器输出里查看详情。",
                slot + 1,
                left
            ),
        ));
    };

    // ---- 3. 合成 ----
    let audio = match tts.synthesize(&held.spoken).await {
        Ok(a) => a,
        Err(e) => {
            // 合成失败 ⇒ **不消费**未读，并把租约还回去。
            let mut rt = mgr.runtime();
            rt.unread_mut().release(lease).ok();
            return Ok(ApiEnvelope::err(e.code(), e.message()));
        }
    };

    // ---- 4. 播放 ----
    let started = now_ms();
    let control = match audio_out::play_pcm(audio.pcm.clone(), audio.sample_rate) {
        Ok(c) => c,
        Err(e) => {
            let mut rt = mgr.runtime();
            rt.unread_mut().release(lease).ok();
            return Ok(ApiEnvelope::err(ErrorCode::Internal, e));
        }
    };

    let (id, preempted) = take_channel(&mgr, Some(slot), Some(lease), control.clone());

    // 被抢占者若是"另一次结果播报"，它也没听完 ⇒ 释放其租约（**不消费**）。
    if let Some(old) = preempted {
        if old.slot < SLOT_COUNT && old.token != 0 {
            let mut rt = mgr.runtime();
            rt.unread_mut().release(old).ok();
        }
    }

    let waiter = control.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        let timeout = std::time::Duration::from_millis(
            audio_ms_upper_bound(waiter.total_samples()) + 5_000,
        );
        waiter.wait_finished(timeout)
    })
    .await;

    // ⚠️ 顺序要紧：**先**判定"是否完整播完"，**再**清场。
    let completed = control.completed_fully();
    clear_channel_if_mine(&mgr, id);

    // ---- 5. 只有**完整播完**才消费未读 ----
    let mut consumed = false;
    let unread_left = {
        let mut rt = mgr.runtime();
        if completed {
            match rt.unread_mut().consume(lease, now_ms()) {
                Ok(outcome) => {
                    consumed = true;
                    outcome.remaining
                }
                Err(e) => {
                    // 走到这里意味着租约在播放期间被别人动了。**不能静默**：
                    // 用户会看到灯没灭而不知道为什么。
                    log_buffer::warn(format!(
                        "任务台消费未读失败（槽位 S{}）: {}",
                        slot + 1,
                        describe_lease_error(&e)
                    ));
                    rt.unread_mut().pending(slot).unwrap_or(0)
                }
            }
        } else {
            // 没播完 ⇒ 释放租约、**保留未读**。这一行就是"播到一半打断，未读还在"。
            rt.unread_mut().release(lease).ok();
            rt.unread_mut().pending(slot).unwrap_or(0)
        }
    };

    // ---- 6. 消费成功后把这条结果的文本清掉（避免下次重复念同一条）----
    if consumed {
        let mut results = mgr
            .pending_results
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(cell) = results.get_mut(slot) {
            if cell.as_ref().map(|r| r.task_id) == Some(held.task_id) {
                *cell = None;
            }
        }
    }

    mgr.save_unread();
    emit_state_if_changed(&app);

    Ok(ApiEnvelope::ok(RenderResultReport {
        slot,
        task_id: Some(held.task_id),
        spoken: held.spoken,
        success: held.success,
        result_at_ms: held.at_ms,
        completed,
        lease_token,
        consumed,
        unread_left,
        duration_ms: now_ms().saturating_sub(started),
    }))
}

/// 按 48 kHz 估算播放时长的上限（毫秒）。
fn audio_ms_upper_bound(samples: usize) -> u64 {
    (samples as u64) * 1000 / 48_000
}

/// 组装总览快照。
///
/// 参数是普通引用而不是 Tauri 的 `State`：这样它可以被单测直接调用，
/// 而命令层靠 deref coercion 把 `State<DeskManager>` 传进来。
fn build_snapshot(mgr: &DeskManager) -> DeskSnapshot {
    let cfg = mgr.store.get();
    let mut blockers: Vec<String> = Vec::new();

    if !cfg.enabled {
        blockers.push("任务台总开关未打开".into());
    }

    // 执行器注册表：**校验只做一次**，快照与阻塞项共用同一份结论。
    // 以前这里有一份"手写的、和 validate_driver 不一致的"检查，两处结论会漂移。
    let registry = driver::DriverRegistry::from_configs(&cfg.drivers);

    // 至少有一个"绑定到槽位且可调度"的执行器，才算能跑任务。
    let mut bound_any = false;
    let mut bound_but_unconfigured: Vec<String> = Vec::new();
    // 不阻塞、但值得在槽位说明里提一句的事（比如优先级列表里有个失效项）。
    let mut slot_notes: Vec<(usize, String)> = Vec::new();

    for (i, s) in cfg.slots.iter().enumerate() {
        match s {
            SlotBinding::Exact { driver } => {
                bound_any = true;
                match registry.get(driver) {
                    None => bound_but_unconfigured
                        .push(format!("槽位 S{} 绑定了不存在的执行器 {driver}", i + 1)),
                    Some(e) if !e.config.enabled => bound_but_unconfigured
                        .push(format!("槽位 S{} 的执行器「{}」未启用", i + 1, e.config.label)),
                    Some(e) if !e.config.is_configured() => bound_but_unconfigured.push(format!(
                        "槽位 S{} 的执行器「{}」未配置完整（缺 API Key 或命令）",
                        i + 1,
                        e.config.label
                    )),
                    Some(e) if !e.validation.is_ok() => bound_but_unconfigured.push(format!(
                        "槽位 S{} 的执行器「{}」配置有误：{}",
                        i + 1,
                        e.config.label,
                        e.validation.errors.join("；")
                    )),
                    Some(_) => {}
                }
            }
            SlotBinding::Capability { capability, prefer } => {
                bound_any = true;
                // ★ 这里用**真实的可用候选数**判断，而不是看 `prefer` 列了几项。
                // 只看 prefer.len() 会出现"列了 3 个、但一个都不存在或被禁用"却显示
                // "候选 3 个"的假象——用户一直以为配好了，直到按下键才报错。
                let cands = registry.supporting(*capability);
                if cands.is_empty() {
                    bound_but_unconfigured.push(format!(
                        "槽位 S{} 按能力「{}」挑选，但没有任何可调度执行器声明该能力",
                        i + 1,
                        capability.label()
                    ));
                } else if !prefer.is_empty() {
                    let missing: Vec<&str> = prefer
                        .iter()
                        .filter(|id| !cands.iter().any(|e| &e.config.id == *id))
                        .map(|s| s.as_str())
                        .collect();
                    if !missing.is_empty() {
                        // 优先级列表里有失效项：不阻塞（其余候选仍能用），但必须说出来。
                        slot_notes.push((
                            i,
                            format!("优先级列表里 {} 当前不可调度，会被跳过", missing.join("、")),
                        ));
                    }
                }
                // ⛔ 能力路由本身还没实现（Phase 4）。现在就说清楚，
                // 而不是让 ready 显示为 true、用户按下去才收到错误。
                slot_notes.push((
                    i,
                    "能力路由尚未实现（Phase 4）——当前请改用「指定执行器」".to_string(),
                ));
            }
            SlotBinding::Auto => {
                bound_any = true;
                // 同上：Auto 完全依赖 Router，现在等于"必然失败"。
                slot_notes.push((
                    i,
                    "自动判定尚未实现（Phase 4）——当前请改用「指定执行器」".to_string(),
                ));
            }
            SlotBinding::Empty => {}
        }
    }
    if !bound_any {
        blockers.push("4 个槽位都未绑定执行器".into());
    }
    blockers.extend(bound_but_unconfigured);

    if !cfg.tts.is_configured() {
        blockers.push("TTS 未填 API Key（完成后无法播报总结）".into());
    }

    let drivers = registry
        .entries()
        .iter()
        .map(|e| {
            let d = &e.config;
            DriverView {
                id: d.id.clone(),
                label: d.label.clone(),
                kind: match d.kind {
                    DriverKind::Api => "api".into(),
                    DriverKind::Cli => "cli".into(),
                },
                enabled: d.enabled,
                configured: d.is_configured(),
                schedulable: e.is_schedulable(),
                completion_label: d.completion.kind.label().to_string(),
                confidence: d.completion.confidence().label().to_string(),
                gated: !d.completion.evidence_cmd.trim().is_empty(),
                capabilities: d.capabilities.iter().map(|c| c.label().to_string()).collect(),
                errors: e.validation.errors.clone(),
                warnings: e.validation.warnings.clone(),
                note: d.note.clone(),
            }
        })
        .collect();

    let slots = cfg
        .slots
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let detail = match s {
                SlotBinding::Empty => "按下不派发任务".to_string(),
                SlotBinding::Exact { driver } => {
                    let label = registry
                        .get(driver)
                        .map(|e| e.config.label.clone())
                        .unwrap_or_else(|| format!("{driver}（不存在）"));
                    format!("固定使用 {label}")
                }
                SlotBinding::Capability { capability, prefer } => {
                    // 展示**真实可用候选数**，而不是 prefer 的条目数。
                    let n = registry.supporting(*capability).len();
                    format!(
                        "按能力「{}」挑选：可用候选 {n} 个{}",
                        capability.label(),
                        if prefer.is_empty() {
                            String::new()
                        } else {
                            format!("，优先级 {prefer:?}")
                        }
                    )
                }
                SlotBinding::Auto => "由路由判定能力".to_string(),
            };
            let notes: Vec<String> = slot_notes
                .iter()
                .filter(|(idx, _)| *idx == i)
                .map(|(_, n)| n.clone())
                .collect();
            SlotView {
                index: i,
                mode: s.mode_label().to_string(),
                detail,
                notes,
            }
        })
        .collect();

    DeskSnapshot {
        enabled: cfg.enabled,
        ready: blockers.is_empty(),
        blockers,
        tts_configured: cfg.tts.is_configured(),
        // ⛔ 不要改回 `cfg.tts.model`：`model` 是**百炼专用**字段，豆包 provider 下
        // 被忽略、界面上也不显示，直接读它会让豆包用户看到陈旧的值
        // （实测：界面填 `seed-tts-2.0`，卡片显示 `seed-tts-1.0`）。
        // 按 provider 分流这件事收在 `TtsConfig::active_model()` 一处。
        tts_model: cfg.tts.active_model().to_string(),
        output_dir: mgr.output_dir().to_string_lossy().to_string(),
        config_path: mgr
            .store
            .path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "(未初始化)".into()),
        drivers,
        slots,
    }
}

/// 供 `lib.rs` 装配：创建并初始化管理器。
pub fn create_manager(config_dir: &std::path::Path) -> DeskManager {
    let mgr = DeskManager::new();
    mgr.init(config_dir);
    mgr
}

// ---------------------------------------------------------------------------
// 槽位按键拦截
// ---------------------------------------------------------------------------

/// 一次「任务台槽位按键被按下」事件。
#[derive(Clone, Debug, Serialize)]
pub struct DeskSlotKeyEvent {
    /// 槽位号（1–4）。
    pub slot: u8,
    /// 人类可读说明，直接进日志面板与前端提示。
    pub message: String,
    /// 这一按是否真的起了作用（`false` = 明确的失败，前端按红色提示）。
    pub dispatched: bool,
}

/// 拦截任务台槽位按键的保留 UUID。返回 `true` 表示已认领。
///
/// # 调用契约
///
/// 调用方（`host_action::handle_host_action`）必须**在返回 true 时立即 return**，
/// 不要再往下走「按 UUID 去应用库开程序」那条分支——否则会因为应用库里没有这个
/// UUID 而报一句"打开失败"，把一次正常的任务台按键记成错误。
///
/// # 按键在这里只做一件事：toggle 录音
///
/// ⚠️ 结论没变，但这个理由**只适用于槽位键，别推广到所有键**：
///
/// 槽位键走的是 `host_action:<uuid>`（白名单制的自定义语义），固件对它在
/// `InputPhase::Released` 时返回**空事件**，只在按下时回发一次 ⇒ 确实
/// **没有按下/松开之分**，能表达的只有"按一下开、再按一下关"。
///
/// 对比：KEY1 / KEY3 用的 `VoicePttHold` / `EditPttHold` **是**成对发
/// keydown/keyup 的 —— 但它们的**产品语义是短按**（按下开始、再按一次结束），
/// 所以钩子层同样只认按下、把 keyup 丢弃。
///
/// ⇒ **两边都是 toggle，理由却完全相反**：一边是"硬件只能这样"，一边是"产品就这么定"。
/// ⛔ 别用一边的现状去论证另一边 —— 这段注释早先就干过这件事（原话是"这与
/// `SessionManager::ptt_down` 的 toggle 语义正好一致（它本来就是这样：
/// Hold 模式下 `ptt_up` 是空的）"，属于**拿 bug 当论据**），结果是先把 KEY1
/// 改成"按住说话"（v0.29.0）、又按用户要求改回短按（v0.30.0）。
/// 详见 `ptt_hook::PttEvent` 的注释。
///
/// 于是这里的职责边界很清楚：**按键只决定"这段录音派给谁"**（`set_desk_intent`），
/// 录音与识别仍走既有的 PTT/ASR 链路 —— 不去发明第二套录音。
/// 识别结果出来后，由 `session` 按归属分叉：有归属 ⇒ 派进任务台，无归属 ⇒ 照旧上屏。
///
/// # ⛔ 归属必须在 `ptt_down` 之前读
///
/// 结束那一按会一路走到会话收尾，而收尾会把"提示用归属"清掉。若先按键再读，
/// 读到的永远是 `None`，于是"你按的是 S2，但这段是给 S1 的"这句关键提示
/// **永远不会出现** —— 用户会以为自己改派成功了。
///
/// # 为什么"第二次按"不改归属
///
/// 一段录音只能属于一个槽。若第二次按下允许改派，"说到一半改主意"会变成
/// 半句派 S1、半句派 S2 这种谁都解释不清的结果。所以第二次按下的语义固定为
/// **结束并派给最初那个槽**；想换槽就先结束再重新按。
pub fn handle_reserved_uuid(app: &tauri::AppHandle, uuid: &str) -> bool {
    use tauri::{Emitter, Manager as _};

    let Some(slot) = crate::constants::desk_slot_of_uuid(uuid) else {
        return false;
    };

    let session = app.state::<crate::session::SessionManager>();
    let origin = crate::session::PttOrigin::DeskSlot(slot);

    // ⛔ 归属必须在 `ptt_down` **之前**声明：`start_hold` 开头就 `take_desk_intent()`
    // 认领它，晚一步设等于没设（这段录音会退化成普通语音输入、内容直接上屏）。
    // 只在"由这一按开始录音"时才设 —— 结束那一按不该改归属。
    let starting = !session.is_listening();
    if starting {
        session.set_desk_intent(Some(slot));
    }

    // 归属校验发生在 `ptt_down_from` 内部：如果现在录着的是**别的入口**开的那段
    // —— KEY1 的普通语音输入（`voice_ptt_hold`）、KEY3 的编辑键 —— 这一按会被**拒绝**，
    // 既不改归属、也不 toggle。用户按的键和正在录的那段不是一回事时，**没有正确做法能
    // "猜"出他想要什么**：顺手结束它会把这句话按一个他看不见的归属送走（要么任务被
    // 当成语音上屏、要么语音被当成任务执行掉），两个方向都是静默错误。
    if !session.ptt_down_from(app.clone(), origin) {
        // ⛔ 被拒 ⇒ **必须撤回**刚才声明的归属。它会粘住：下一次普通语音输入
        // （KEY1 / 界面按钮 / 语音编辑）会把认领到的这个槽位当成自己的归属，
        // 于是识别结果被当任务**投进终端执行**，而不是照常上屏 ——
        // 用户说的是给编辑器的内容，结果变成一条命令。声明与撤回必须成对。
        session.set_desk_intent(None);
        log_buffer::warn(format!(
            "任务台按键：S{slot} 被拒 —— 当前录着的那段不是任务台起的（拒绝提示已单独发出）"
        ));
        return true;
    }

    // 同一个道理的另一半：这一按**没能真的开始录音**（例如没填 API Key，
    // `start_hold` 开头就挡下并返回）⇒ 刚声明的归属同样成了残留，撤掉。
    // 判据用"现在到底有没有在录"，而不是"我以为它开始了"。
    if !session.is_listening() {
        session.set_desk_intent(None);
        log_buffer::warn(format!(
            "任务台按键：S{slot} 没能开始录音，已撤回本次归属声明"
        ));
    }

    // 走到这里，正在录的那段**一定是本槽**的：要么是这一按刚开的，要么是上一次按本槽键
    // 开的。所以不用再区分"原本派给谁"。要区分的只有**开始**和**结束** —— 这两件事
    // 对用户完全不同（一个刚开始录、一个马上要派任务），合并成一句等于骗他。
    let message = if starting {
        format!("S{slot} 开始录音 —— 说完再按一次同一个键结束，识别结果会派给 S{slot}")
    } else {
        format!("S{slot} 录音结束 —— 识别完成后派给 S{slot}")
    };

    log_buffer::info(format!("任务台按键：{message}"));
    let _ = app.emit(
        "desk-slot-key",
        DeskSlotKeyEvent {
            slot,
            message,
            dispatched: true,
        },
    );
    true
}

// ---------------------------------------------------------------------------
// 编排器：槽位执行泵
// ---------------------------------------------------------------------------

/// 跨层错误：带一个统一的错误码 + 一句说得出病因的中文。
///
/// 为什么不在每处直接返回 `String`：**错误码是给程序看的，消息是给人看的**。
/// 只给字符串，前端就只能靠匹配文字来判断"这是配置问题还是并发冲突"；
/// 只给错误码，用户就只知道"失败了"。两个都要。
pub struct DeskError {
    code: ErrorCode,
    message: String,
}

impl DeskError {
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// 给人看的那句话。
    ///
    /// 字段保持私有（错误码 + 消息必须成对产生，不允许外部随便拼），但**消息本身
    /// 必须能被跨模块读到**：按键链路派发失败时要把病因原样告诉用户，否则他只知道
    /// "没派成"，不知道是该改配置还是该等一等。
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// 队列错误 → 错误码。
///
/// 分类的依据是"**用户该怎么处理**"，不是"错误发生在哪个函数"：
/// 越界/空文本要改输入，队列满/重复要等一等，回执不符要查是不是有并发的旧任务。
fn queue_error_code(e: &queue::QueueError) -> ErrorCode {
    use queue::QueueError as Q;
    match e {
        Q::SlotOutOfRange { .. } | Q::EmptyPrompt { .. } => ErrorCode::InvalidInput,
        Q::QueueFull { .. } | Q::DuplicatePending { .. } => ErrorCode::Conflict,
        Q::NotRunning { .. } | Q::TaskIdMismatch { .. } => ErrorCode::Conflict,
    }
}

/// 派发一次任务：路由 → 入队 → 拉起执行泵。
///
/// **路由在入队之前**：判不出执行器时就别入队。否则队列里会堆一批注定失败的任务，
/// 用户看到"排队 3 条"却永远等不到结果——比当场报错糟糕得多。
pub fn dispatch_task(
    app: &tauri::AppHandle,
    slot: usize,
    prompt: String,
) -> Result<DispatchOutcome, DeskError> {
    use tauri::Manager as _;
    let mgr = app.state::<DeskManager>();

    let driver_id = mgr
        .resolve_driver(slot)
        .map_err(|m| DeskError::new(ErrorCode::NotConfigured, m))?;

    let outcome = {
        let mut rt = mgr.runtime();
        state::dispatch_into(&mut rt, slot, prompt, driver_id, now_ms())
            .map_err(|e| DeskError::new(queue_error_code(&e), describe_queue_error(&e)))?
    };

    // 入队成功 → 拉泵（它自己会判断该槽是否已有泵在跑）。
    ensure_pump(app, slot);
    emit_state_if_changed(app);

    // 起点播报（**事件驱动**，且只在"真的开始跑"时才发）。
    //
    // ⚠️ 排队时**不发**"任务已开始"：那一刻这句话是假的 —— 它还在等前面的任务。
    // 播报假状态比不播更糟：用户会以为可以走了，实际它还没动。
    // 而"还在排队"这件事该由灯来说（灯是状态信道，喇叭是事件信道）。
    if matches!(&outcome, DispatchOutcome::Started { .. }) {
        emit_notice(app, Some(slot), NoticeKind::TaskStarted, now_ms());
    }
    Ok(outcome)
}

/// 确保某个槽位有一个执行泵在跑。
///
/// # 为什么要"单飞"
///
/// 每派发一次就 spawn 一个泵，会得到一堆"取不到任务就退出"的空转任务。
/// 更糟的是它让"谁在驱动这个槽"变成不可追踪的问题——出故障时无法回答
/// "现在到底有几个泵在抢这个槽"。
///
/// `compare_exchange` 保证**同一时刻只有一个泵拿到票据**。票据在泵退出前归还，
/// 并在归还后**再检查一次**：泵运行期间可能有新任务入队（那时 `ensure_pump`
/// 看到票据被占就直接返回了），不补这一次检查，那条任务会永远没人执行。
fn ensure_pump(app: &tauri::AppHandle, slot: usize) {
    use tauri::Manager as _;
    {
        let mgr = app.state::<DeskManager>();
        let Some(flag) = mgr.pumping.get(slot) else {
            log_buffer::err(format!("ensure_pump: 槽位号 {slot} 越界"));
            return;
        };
        if flag
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return; // 已有泵在跑这个槽
        }
    }

    let app_cloned = app.clone();
    tauri::async_runtime::spawn(async move {
        pump_slot(app_cloned.clone(), slot).await;
        {
            let mgr = app_cloned.state::<DeskManager>();
            if let Some(flag) = mgr.pumping.get(slot) {
                flag.store(false, Ordering::Release);
            }
        }
        // 归还票据后再检查一次（见函数文档）。
        ensure_pump(&app_cloned, slot);
    });
}

/// 某个槽位的执行循环：取任务 → 执行 → 收终态 → 再取。
///
/// 循环到队列空为止。**同槽串行就是这个循环的形状**——它天然不会同时跑两条。
async fn pump_slot(app: tauri::AppHandle, slot: usize) {
    use tauri::Manager as _;

    // 上一次已经执行完（并已尝试收终态）的任务号。
    //
    // 只为一个防御：万一 `complete_task` 没能清出运行位，下一次循环会**再次认领同一条**
    // —— 那就是"同一条任务被无限重复执行"的热循环，会真的反复拉起 Agent（既烧钱又刷屏）。
    // 认领到已处理过的任务号 ⇒ 停机并如实报错，绝不默默重跑。
    let mut last_done: Option<TaskId> = None;

    loop {
        // ---- 1. 认领该槽当前应当执行的任务 ----
        //
        // ⚠️ **这一步的正确语义是"认领"，不是"从队首取一条"。**
        // 派发走的是 `state::dispatch_into`（"入队 + 启动"的复合操作），
        // 它**在派发那一刻就已经把任务放进了运行位**。所以这里 `start_next` 会返回
        // `Busy` —— 而 `Busy` 的含义是"该执行的就是这一条"，**绝不是**"没事可做"。
        //
        // 把这两件事混为一谈的后果是**完全静默**的：任务会永久停在"正在执行"，
        // 没有子进程、没有终态、没有未读、没有播报，**连超时都不会触发**
        // （超时在 runner 内部，而 runner 从未被调用）。
        // 这正是 `state::claim_current` 存在的唯一理由 —— 它把这段判断放进
        // **纯数据层**，从而可以被单测钉死，不必依赖 `AppHandle`。
        let claimed = {
            let mgr = app.state::<DeskManager>();
            let mut rt = mgr.runtime();
            state::claim_current(&mut rt, slot, now_ms())
        };
        let task = match claimed {
            Ok(Some(t)) => t,
            Ok(None) => break,
            Err(e) => {
                log_buffer::err(format!(
                    "任务台认领任务失败（槽位 S{}）: {}",
                    slot + 1,
                    describe_queue_error(&e)
                ));
                break;
            }
        };

        if last_done == Some(task.id) {
            log_buffer::err(format!(
                "任务台 S{} 任务 #{} 的运行位没能清出（收终态失败）——停止该槽的执行泵，\
                 以免同一条任务被无限重复执行",
                slot + 1,
                task.id
            ));
            break;
        }

        emit_state_if_changed(&app);

        // ---- 2. 取执行器配置 + 登记取消令牌 ----
        let driver_cfg = {
            let mgr = app.state::<DeskManager>();
            mgr.store.get().driver(&task.driver_id).cloned()
        };

        // ---- 2.5 取出本槽正在续接的会话（多轮对话）----
        //
        // 取不到就当"没有"，任务仍照跑 —— 续接是**增强**，不是前提。
        let prior_session = {
            let mgr = app.state::<DeskManager>();
            mgr.session_for(slot, &task.driver_id)
        };
        if let Some(id) = prior_session.as_deref() {
            log_buffer::info(format!(
                "任务台 S{} 续接会话 {id}（执行器 {}）",
                slot + 1,
                task.driver_id
            ));
        }

        let cancel = CancelToken::new();
        {
            let mgr = app.state::<DeskManager>();
            let mut toks = mgr.cancel_tokens.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(cell) = toks.get_mut(slot) {
                *cell = Some(cancel.clone());
            }
        }

        log_buffer::info(format!(
            "任务台开始执行: 槽位 S{} 任务 {} 执行器 {} 文本 {} 字",
            slot + 1,
            task.id,
            task.driver_id,
            task.prompt.chars().count()
        ));

        // ---- 2.9 记下开始时刻（产物发现的时间窗起点）----
        //
        // ⚠️ 必须取在 `run_driver` **之前**。产物判据只有一条：文件 mtime 落在
        // "这次任务跑过的那段时间"内。取在之后就等于把整段执行时间排除在外，
        // 结果是一个产物都找不到 —— 而且失败得完全静默（"没产物"和"没扫到"长得一样）。
        let started_ms = now_ms();

        // ---- 3. 执行 ----
        //
        // 用 `as_ref()` 借出来而不是把 `driver_cfg` move 进 match：下面第 5.7 步
        // 还要用它拿 `cwd` / `artifact_glob` / `artifact_open` 去扫产物。
        let outcome = match driver_cfg.as_ref() {
            Some(d) => {
                let mut req = RunRequest::new(slot, task.id, task.prompt.clone());
                req.session_id = prior_session;
                // 超时来自**驱动配置**（0 = 用全局默认 180 秒）。
                //
                // 这一步不是可选的打磨：全局默认是照"一次 HTTP 调用"定的，
                // 拿它去卡 CLI Agent ⇒ **每个真实任务都会在 3 分钟处被判超时**，
                // 而那个时刻恰好落在"它干得好好的"区间里。
                req.timeout_ms = d.timeout_ms;
                // ⚠️ 进度出口暂用 NullSink：把每一行推给前端需要一套节流与 UI 设计，
                // 那是 Phase 3 与状态灯一起做的工作。现在用一个假的进度条，
                // 比什么都不显示更危险（用户会以为它真的在动）。
                let mut sink = NullSink;
                run_driver(d, &req, &mut sink, cancel.clone()).await
            }
            None => RunOutcome::failed(format!(
                "找不到执行器「{}」——配置可能刚被改动，本条任务未执行",
                task.driver_id
            )),
        };

        // ---- 4. 注销取消令牌（无论成败都要，否则"取消"会打到下一条任务上）----
        {
            let mgr = app.state::<DeskManager>();
            let mut toks = mgr.cancel_tokens.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(cell) = toks.get_mut(slot) {
                *cell = None;
            }
        }

        // ---- 5. 收终态（清运行位 + 按规则产生未读）----
        let terminal = match outcome.status {
            RunStatus::Completed => TaskTerminal::Succeeded,
            RunStatus::Failed => TaskTerminal::Failed,
            RunStatus::TimedOut => TaskTerminal::TimedOut,
            RunStatus::Cancelled => TaskTerminal::Cancelled,
        };
        {
            let mgr = app.state::<DeskManager>();
            let mut rt = mgr.runtime();
            match complete_task(&mut rt, slot, task.id, terminal) {
                Ok(eff) => {
                    let line = format!(
                        "任务台 S{} 任务 {} {}：{}{}{}",
                        slot + 1,
                        task.id,
                        outcome.status.label(),
                        summarize_outcome(&outcome),
                        if eff.produced_unread {
                            format!("，未读累计 {}", eff.unread)
                        } else {
                            "，未产生未读（用户主动取消）".to_string()
                        },
                        outcome
                            .reason
                            .as_ref()
                            .map(|r| format!("\n    原因：{}", driver::truncate(r, 300)))
                            .unwrap_or_default()
                    );
                    match outcome.status {
                        RunStatus::Completed => log_buffer::info(line),
                        // 失败/超时/取消都不是"正常情况"，用 warn 让它们在日志面板里显眼。
                        _ => log_buffer::warn(line),
                    }
                }
                Err(e) => log_buffer::err(format!(
                    "任务台收终态失败（槽位 S{}，任务 {}）: {}",
                    slot + 1,
                    task.id,
                    e.message()
                )),
            }
        }

        // ---- 5.5 状态播报：终态（**事件驱动**）----
        // 为什么放在这里而不是"扫快照"里：终态是**瞬时事件**。"刚刚完成了"这个
        // 事实在状态快照里根本看不到——快照只描述"现在是什么样"，而后台永远不会
        // 记录"上一拍它曾经从 running 变成 idle"。所以终态必须由产生它的那一行代码
        // 直接播报，不能指望事后从状态里反推。
        //
        // 结果文本在这里算**一次本地版本**，下面几处共用 —— 分两处算必然漂移
        // （一处念 JSON、一处念结论，用户会以为系统自相矛盾）。
        //
        // ⚠️ `SummaryConfig.max_chars` 在这里才真正生效。它此前**全仓没有任何读取点**
        // （实际用的是硬编码 150），用户改它不会有任何变化。现在：本地正文按它截，
        // 二次总结也按它压。
        let summ = {
            let mgr = app.state::<DeskManager>();
            mgr.store.get().summary
        };
        let head = spoken_head(outcome.status);
        let body = spoken_body(&outcome, summ.max_chars);
        // 本地正文 = 前缀 + 结论截断。它同时是三样东西：
        //   ① 屏幕字幕（`detail`）；② 「播报未读」留存的结果；③ 二次总结失败时的兜底。
        // 之所以三处共用一份，是因为**它们必须是同一句话** ——
        // 耳朵听到的、屏幕上看到的、回头按未读重听到的，三者不一致最伤信任。
        let local = compose_spoken(head, &body);

        // 要不要第二次总结，由 `SummaryConfig.driver_id` 说了算。
        //
        // **空 = 不总结**：CLI Agent 交回的最后一句本就是它给自己做的总结，
        // 再套一层模型只会更慢、更容易跑偏（把一句"微信已启动"扩写成三句客套话）。
        // 非空 = 用户明确要求"我想听更凝练的版本"，那就按他配的执行器再跑一次。
        let voice = if summ.driver_id.trim().is_empty() {
            VoiceSpec::Text(local.clone())
        } else {
            VoiceSpec::Summarize {
                head: head.to_string(),
                fallback_body: body,
                task: task.prompt.clone(),
                // ⚠️ 只把**尾部**喂给总结器：Agent 输出动辄几十 KB 的事件流，
                // 全文进去既贵又淹没重点，而结论一定在最后。
                output: tail_chars(&outcome.output, SUMMARY_INPUT_CHARS),
                cfg: summ,
            }
        };

        // `detail` 只进字幕与日志，**不进语音**（理由见 `NoticeEvent::detail`）：
        // 语音已经念过一遍结论了，再念一遍是噪音；但界面上必须能看见它 ——
        // 用户判断"到底做没做成"，靠的就是这一句。
        if let Some(kind) = terminal_notice(outcome.status) {
            emit_notice_with(
                &app,
                GateId::Slot,
                Some(slot),
                kind,
                now_ms(),
                Some(&local),
                voice,
            );
        }

        // ---- 5.6 留存结果文本（供「播报未读」用）----
        // 未读账本只记"有几条"，不记"是什么"。想按未读听到内容，就必须在
        // **产生未读的同一处**把文本留下来 —— 分两处写必然漂移。
        // 取消不留（取消不产生未读，与上面同一条规则）。
        //
        // ⚠️ 存的是**本地正文**而不是二次总结的产物：按未读重听时用户在等一个
        // **立刻**能出声的答复，绝不能在这里再等一次 LLM。而且此刻总结可能还没跑完。
        if !matches!(outcome.status, RunStatus::Cancelled) {
            let mgr = app.state::<DeskManager>();
            let mut results = mgr
                .pending_results
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(cell) = results.get_mut(slot) {
                *cell = Some(SlotResult {
                    task_id: task.id,
                    spoken: local.clone(),
                    at_ms: now_ms(),
                    success: matches!(outcome.status, RunStatus::Completed),
                });
            }
        }

        // ---- 5.7 任务历史 + 产物（"贾维斯"体验的后半段）----
        //
        // 刻意排在播报**之后**：历史是"这次留下了什么"，与喇叭出不出声无关。
        // 播报被去抖挡掉、或 TTS 没配时，历史照样必须记上 ——
        // 否则用户最需要追溯的那几次（静默失败）恰好没有记录。
        //
        // ⚠️ 产物发现与自动打开都**不能改变任务结论**：任务跑完了是既成事实，
        // 扫不到产物、或窗口抢不到焦点，只该留一条日志。
        let artifacts = discover_and_open(driver_cfg.as_ref(), &outcome, started_ms);

        {
            let mgr = app.state::<DeskManager>();
            let finished_ms = now_ms();
            mgr.record_history(
                TaskRecord {
                    task_id: task.id,
                    slot,
                    driver_id: task.driver_id.clone(),
                    prompt: task.prompt.clone(),
                    status: outcome.status.label().to_string(),
                    reason: outcome.reason.clone(),
                    confidence: match outcome.confidence {
                        config::Confidence::High => "high",
                        config::Confidence::Medium => "medium",
                    }
                    .to_string(),
                    started_at_ms: started_ms,
                    finished_at_ms: finished_ms,
                    duration_ms: outcome.duration_ms,
                    session_id: outcome.session_id.clone(),
                    log_file: None, // 由 `record_history` 写入后回填
                    artifacts,
                    output_tail: history::tail_for_list(&outcome.output),
                },
                &outcome.output,
                // ⚠️ stderr 目前没有从 driver 里带出来（`RunOutcome` 没有 `stderr` 字段），
                // 所以失败原因只在 `reason` 里、且已被 driver 截断过。
                // 这是已知缺口，写进了 CHANGELOG 的「未完成」—— 不要误以为日志里
                // 「----- stderr -----」那段总会有内容。
                "",
            );
        }

        // ---- 6. 未读落盘 + 状态事件 ----
        {
            let mgr = app.state::<DeskManager>();
            mgr.save_unread();
        }
        emit_state_if_changed(&app);
        last_done = Some(task.id);

        // ---- 6.5 记下本轮会话，供下一轮续接 ----
        //
        // 规则刻意简单：**只有 driver 明确交回 id 时才更新**。
        // 没交回（工具不支持续接 / 这轮被降级重跑 / 抽取失败）⇒ 保持原样，
        // 由 cli_driver 的降级逻辑决定要不要清 —— 上层不去猜。
        if let Some(id) = outcome.session_id.clone() {
            let mgr = app.state::<DeskManager>();
            mgr.set_session(slot, id, task.driver_id.clone());
        }
        // ---- 7. 回到循环顶部认领下一条 ----
    }
}

/// 取字符串**末尾**的至多 `max` 个字符（按**字符**边界切，不按字节）。
///
/// 为什么取末尾而不是开头：CLI / API 驱动的 Agent，真正的结论通常落在**最后**，
/// 开头多是"正在读取…／正在分析…"这类过程日志。播报要念的是结论，不是过程。
pub(crate) fn tail_chars(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    s.chars().skip(count - max).collect()
}

/// 拼出**可以念出来的**本地正文（前缀 + 结论）。
///
/// ⚠️ 它只管"拼"，不管"截"也不管"定性"，那两件事分别在 [`spoken_body`] 与
/// [`spoken_head`] 里。拆成三块不是洁癖：**二次总结只需要正文** ——
/// 它既不需要前缀（"成功/失败"是本地已知的事实，不该交给另一个模型去复述），
/// 也不该重复一次 JSONL 抽取。合成一个函数的话，那两处就只能各自复制片段，
/// 而复制出来的措辞迟早会漂（一处念"任务已完成"、一处念"完成啦"）。
///
/// 两条硬约束（由 `spoken_body` 保证）：
///
/// 1. **要短。** TTS 念 180 字约 30 秒 —— 再长用户就等不下去了（他还想按别的键）。
/// 2. **不撒谎。** 失败就必须说失败。绝不能因为"有输出"就念成"已完成"。
fn compose_spoken(head: &str, body: &str) -> String {
    if body.is_empty() {
        head.to_string()
    } else {
        format!("{head}{body}")
    }
}

/// 终态 → 语音前缀。
///
/// 单独抽出来的理由：它同时被三条路用到 —— 本地正文（`compose_spoken`）、
/// 二次总结（`VoiceSpec::Summarize`）的前缀、以及"二次总结失败且用户不许退回原文"
/// 时**唯一还留在嘴边的那句话**。三处各写一份的话，措辞迟早会漂成三种。
fn spoken_head(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Completed => "任务已完成。",
        RunStatus::Failed => "任务失败。",
        RunStatus::TimedOut => "任务超时。",
        RunStatus::Cancelled => "任务已取消。",
    }
}

/// 从 CLI 的 **JSONL 事件流**里取出最后一句"说给用户听的话"。
///
/// # 为什么非有它不可
///
/// `codex exec --json` 的 stdout 不是回答，是事件流，而且**结尾一定是**
/// `{"type":"turn.completed","usage":{…}}`。照"取输出尾部"的老办法，
/// TTS 念出来的就是一串 JSON 噪音 —— 用户等了 40 秒，听到的是
/// `type turn completed usage input tokens 31386`。他真正该听到的
/// 「微信已启动」恰恰**不在结尾**，而在 `item.completed` 的 `agent_message` 里。
///
/// 这是纯函数（不碰 IO、不碰 AppHandle），所以能被单测直接钉死。
fn final_agent_text(stdout: &str) -> Option<String> {
    let mut last: Option<String> = None;
    for line in stdout.lines() {
        let line = line.trim();
        // 半行 / 进度噪音直接跳过：**不能**因为一行解析失败就放弃整份输出
        // （最后一行常常是不完整的，而结论在它前面）。
        if !line.starts_with('{') || !line.ends_with('}') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("item.completed") {
            continue;
        }
        let Some(item) = v.get("item") else { continue };
        if item.get("type").and_then(|t| t.as_str()) != Some("agent_message") {
            continue;
        }
        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
            let t = text.trim();
            if !t.is_empty() {
                last = Some(t.to_string());
            }
        }
    }
    last
}

/// 结果正文（不含"任务已完成。"这类前缀）。
///
/// 优先念 Agent 真正说给用户听的那句话；取不到（比如执行器输出的就是纯文本）
/// 才退回"取输出尾部"。这个次序是刻意的：退回路径是**兜底**，不是主路径。
///
/// `max_chars` 来自 `SummaryConfig.max_chars`（默认 180）。它此前是硬编码的 150，
/// 而配置里那个字段**全仓没有任何读取点** —— 用户改它不会有任何变化。
/// v0.35.0 把它真正接上：这个数字现在是配置说了算。
fn spoken_body(o: &RunOutcome, max_chars: usize) -> String {
    // 下限 20：再短会切出半个词，比不念更难懂。
    let max = max_chars.max(20);

    let body = final_agent_text(&o.output).unwrap_or_else(|| tail_chars(o.output.trim(), max));
    let body = tail_chars(body.trim(), max);

    if body.is_empty() {
        // 没有输出时要说明**为什么** —— 只念一句"任务失败"，
        // 用户还得自己去翻日志才知道发生了什么。
        return tail_chars(o.reason.as_deref().unwrap_or_default(), 80);
    }
    body
}

/// 终态 → 播报语料。
///
/// **取消刻意返回 `None`**：那是用户自己按的。播一句"已取消"既没有信息量，
/// 还会打断他已经在做的事 —— 与"取消不产生未读"（`TaskTerminal::produces_unread`）
/// 是同一条规则的两面。
///
/// ⚠️ **措辞纪律**（方案 §2.6.3）：播报文本必须由"产物已交付且可回读"驱动，
/// 而不是由任务 API 的状态驱动。所以这里的 `Completed` 严格表示**执行成功**；
/// 而"成功但产物交付失败"有专用的 `CompletedArtifactFailed` 语料 ——
/// 它要等 Phase 4.8 的产物交付通道落地后才会有调用点。
/// 现在仍把它列在闭集里，是为了让预合成层**在上线那天之前就把音频备好**，
/// 而不是等到需要它的那一刻才发现少一句。
fn terminal_notice(status: RunStatus) -> Option<NoticeKind> {
    match status {
        RunStatus::Completed => Some(NoticeKind::Completed),
        RunStatus::Failed => Some(NoticeKind::Failed),
        RunStatus::TimedOut => Some(NoticeKind::TimedOut),
        RunStatus::Cancelled => None,
    }
}

/// 喂给"总结执行器"的原始输出**上限**（字符数）。
///
/// # 为什么必须截
///
/// CLI Agent 的 stdout 是**事件流**，动辄几十 KB。整份塞进总结提示词有两个后果：
/// 一是 token 费陡增，二是真正要总结的结论被淹没在过程日志里（模型会把注意力
/// 平均分给"正在读取文件 X"这类噪音）。而结论一定落在**最后**，所以取尾部。
const SUMMARY_INPUT_CHARS: usize = 8_000;

/// 终态播报要念的正文 —— **可能还没算出来**。
///
/// # 为什么不让调用方先把正文算好（上一版这里是个 `Option<&str>`）
///
/// 二次总结（`SummaryConfig.driver_id` 非空时）是**一次额外的执行器调用**，
/// 快则几秒、慢则几十秒。若在 `pump_slot` 里 `await` 它，就是让**整个槽位**陪着等：
/// 排在后面的任务明明可以立刻开始，却为了这几十秒的"润色"被卡住。
/// 所以这里只把**原料**交给后台的播报任务，让它去算、算完就念 ——
/// 那一刻槽位早已清空，下一条任务已经在跑了。
enum VoiceSpec {
    /// 不念正文，只用 `kind` 的固定语料（预合成、断网也能播）。
    Corpus,
    /// 念这段现成的文本（现调 TTS 合成）。
    Text(String),
    /// 先按 `SummaryConfig` 二次总结，再念。
    Summarize {
        /// 前缀（"任务已完成。" 等）。**总结器只负责正文，不负责定性** ——
        /// 「成功/失败」是本地已知的事实，不该交给另一个模型去复述。
        head: String,
        /// 本地兜底正文（Agent 自己结论的截断）。
        fallback_body: String,
        /// 用户原话，填 `{task}`。
        task: String,
        /// Agent 输出**尾部**，填 `{output}`（上限 `SUMMARY_INPUT_CHARS`）。
        output: String,
        cfg: SummaryConfig,
    },
}

impl VoiceSpec {
    /// 最终会不会出声。
    ///
    /// ⚠️ 判的是"**值得为它开一次合成**"，不是"一定能合成成功"。
    /// 合成失败由 `speak_notice` 的降级链兜（退回预合成语料）。
    fn will_speak(&self) -> bool {
        match self {
            VoiceSpec::Corpus => false,
            VoiceSpec::Text(t) => !t.trim().is_empty(),
            // 结论未知，但**意图明确** ⇒ 必须给合成一次机会。若在这里返回 false，
            // "配了总结器 + 语料没预合成"就会变成一声不响 —— 用户以为播报坏了。
            VoiceSpec::Summarize { .. } => true,
        }
    }

    /// 定稿成要念的文本。`None` = 只能用固定语料。
    ///
    /// `async` 是因为 `Summarize` 分支会真的去跑一次执行器。
    async fn resolve(self, mgr: &DeskManager) -> Option<String> {
        match self {
            VoiceSpec::Corpus => None,
            VoiceSpec::Text(t) => Some(t).filter(|s| !s.trim().is_empty()),
            VoiceSpec::Summarize {
                head,
                fallback_body,
                task,
                output,
                cfg,
            } => {
                let max = cfg.max_chars.max(20);
                let body = match summarize_via_driver(mgr, &task, &output, &cfg).await {
                    Some(s) => tail_chars(s.trim(), max),
                    None if cfg.fallback_truncate => fallback_body,
                    // 用户明确要求"总结不出来就别念原文" ⇒ 退回固定语料。
                    None => String::new(),
                };
                if body.trim().is_empty() {
                    return None;
                }
                Some(format!("{head}{body}"))
            }
        }
    }
}

/// 拿配置里的"总结执行器"再跑一次，把 Agent 的输出压成一句话。
///
/// 返回 `None` = 没总结出来，由调用方按 `fallback_truncate` 决定退路。
///
/// # 为什么用 `SLOT_COUNT` 当槽位号
///
/// 总结**不占任何用户的槽位** —— 槽位是用户按 S1–S4 划分的注意力信道，往里塞一条
/// 系统自己发起的调用，会让状态快照、未读账本和灯语跟着乱。
/// 用越界的 `SLOT_COUNT` 当哨兵，日志里一眼能看出"这不是 S1–S4 的任务"；
/// 而 driver 只把 `slot` 写进日志行（`cli_driver.rs` / `api_driver.rs` 各一处），
/// 不做索引，所以越界值是安全的。
///
/// # 为什么不复用调用方的取消令牌
///
/// 总结是**尽力而为**的增强：用户取消任务，他要取消的是"那个任务"，
/// 不是"顺便别总结了"。给它一个独立令牌，意味着任务取消后总结仍会跑完 ——
/// 这恰恰是对的：用户按了取消，喇叭更应该告诉他"已取消"。
async fn summarize_via_driver(
    mgr: &DeskManager,
    task_prompt: &str,
    output_tail: &str,
    cfg: &SummaryConfig,
) -> Option<String> {
    let id = cfg.driver_id.trim();
    let d = mgr.store.get().driver(id).cloned()?;
    if !d.enabled {
        log_buffer::warn(format!(
            "任务台总结器「{id}」已停用，本次不总结（按配置退回原文截断）"
        ));
        return None;
    }

    let prompt = cfg
        .prompt_template
        .replace("{task}", task_prompt)
        .replace("{output}", output_tail)
        .replace("{max_chars}", &cfg.max_chars.to_string());

    log_buffer::info(format!(
        "任务台总结器：用执行器「{id}」压缩结果（不占槽位，提示词 {} 字）",
        prompt.chars().count()
    ));

    let mut req = RunRequest::new(SLOT_COUNT, 0, prompt);
    req.timeout_ms = d.timeout_ms;
    let mut sink = NullSink;
    let r = run_driver(&d, &req, &mut sink, CancelToken::new()).await;

    match r.status {
        RunStatus::Completed => {
            // 总结器本身也可能是个 CLI（stdout 是 JSONL 事件流）⇒ 同样要抽最后一句人话，
            // 否则喇叭会念出一串 `{"type":"turn.completed"…}`。
            let t = final_agent_text(&r.output).unwrap_or_else(|| r.output.trim().to_string());
            if t.trim().is_empty() {
                log_buffer::warn("任务台总结器返回空文本，本次不总结");
                None
            } else {
                Some(t)
            }
        }
        other => {
            log_buffer::warn(format!(
                "任务台总结器执行未成功（{}）{}，本次不总结",
                other.label(),
                r.reason
                    .as_ref()
                    .map(|x| format!("：{}", driver::truncate(x, 200)))
                    .unwrap_or_default()
            ));
            None
        }
    }
}

/// 扫出这次任务的产物，并在条件齐备时替用户打开。
///
/// 返回产物绝对路径列表（写进任务历史）。
///
/// # 为什么"打开"要三条硬条件同时成立
///
/// 自动打开会**抢走用户的焦点**。抢错了（任务其实失败了、或 cwd 里恰好有别的
/// 新文件）比不打开烦人得多。所以：任务**成功** ∧ 用户**开着**这个开关
/// （`artifact_open`）∧ **确实扫到了**候选，缺一不开。
///
/// # 为什么不给超时/取消的任务扫产物
///
/// 那两种情况下目录里留下的是**半成品**（写到一半的导出、没封口的容器）。
/// 把它们当"产物"打开是主动误导；用户真要看，历史页里照样有这次的完整日志。
fn discover_and_open(
    cfg: Option<&config::DriverConfig>,
    outcome: &RunOutcome,
    started_ms: u64,
) -> Vec<String> {
    if matches!(outcome.status, RunStatus::Cancelled | RunStatus::TimedOut) {
        return Vec::new();
    }
    let Some(cfg) = cfg else { return Vec::new() };

    // ⚠️ 没配工作目录就**不扫** —— 绝不能拿 easyop 自己的进程 cwd 顶替：
    // 那是安装目录，扫出来的"新文件"与用户的 Agent 毫无关系，打开它更是纯骚扰。
    let cwd = cfg.cwd.trim();
    if cwd.is_empty() {
        return Vec::new();
    }
    let root = std::path::Path::new(cwd);
    let glob = cfg.artifact_glob.trim();
    let found = artifact::discover(root, started_ms, glob);
    if found.files.is_empty() {
        return Vec::new();
    }

    let files: Vec<String> = found
        .files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    log_buffer::info(format!(
        "任务台产物：在 {} 扫到 {} 个新文件（mtime ≥ {}{}；共访问 {} 个条目，跳过 {} 个目录{}）",
        root.display(),
        files.len(),
        started_ms,
        if glob.is_empty() {
            String::new()
        } else {
            format!("，glob「{glob}」")
        },
        found.scanned,
        // ⚠️ "跳过了几个目录"必须进日志。没有它，"扫不到产物"有两种可能
        // （真没有 / 全被跳过），而用户无从区分 —— 他会以为功能坏了。
        found.skipped,
        if found.truncated {
            "，已达条目上限被截断"
        } else {
            ""
        }
    ));

    if cfg.artifact_open && matches!(outcome.status, RunStatus::Completed) {
        match artifact::open_artifacts(&found.files) {
            Ok(artifact::OpenOutcome::File { path }) => {
                log_buffer::info(format!("任务台已打开产物文件 {path}"))
            }
            Ok(artifact::OpenOutcome::Folder { path, count }) => log_buffer::info(format!(
                "任务台已打开产物目录 {path}（{count} 个文件，已定位最新的那个）"
            )),
            // ⚠️ 打不开**不改变任务结论**：任务已经跑完了，抢焦点失败是另一回事。
            Err(e) => log_buffer::warn(format!(
                "任务台产物自动打开失败（不影响任务结果）: {e}"
            )),
        }
    } else if !files.is_empty() {
        log_buffer::info("任务台产物：按配置未自动打开（任务未成功，或开关已关）");
    }

    files
}

/// 把一次执行结果压成一行日志摘要。
fn summarize_outcome(o: &RunOutcome) -> String {
    let mut s = format!(
        "耗时 {}ms，输出 {} 字",
        o.duration_ms,
        o.output.chars().count()
    );
    if let Some(ev) = o.evidence.as_ref() {
        s.push_str(&format!(
            "，世界门禁{}（`{}`）",
            if ev.passed { "通过" } else { "未通过" },
            ev.cmd
        ));
    }
    s
}

/// 若运行时状态与上次发出不同，发一条 `agent-desk-state`。
///
/// **只有真变化才发**：前端因此不需要轮询，也不会被"什么都没变"的事件刷屏。
/// 发送失败只记日志——它是观测通道，出问题不该影响任务执行本身。
fn emit_state_if_changed(app: &tauri::AppHandle) {
    use tauri::{Emitter, Manager as _};
    let snap = {
        let mgr = app.state::<DeskManager>();
        let mut rt = mgr.runtime();
        rt.snapshot_if_changed()
    };
    if let Some(s) = snap {
        let now = now_ms();
        // 先下发灯语（借用 `s`），再发事件（move `s`）—— 省掉一次 Clone。
        // 灯语是"事件驱动立刻反应"，心跳（`start_led_pump`）负责之后每 5 秒续期。
        push_led_if_needed(app, &s);
        // 进度类播报由**状态**驱动（"进度"没有独立的瞬时事件可言，它是快照的函数）；
        // 终态与起点类由**事件**驱动（见 `pump_slot` / `dispatch_task`）——
        // 因为"刚刚完成了"这个事实在快照里根本看不到：快照只描述"现在是什么样"。
        emit_progress_notice(app, &s, now);
        if let Err(e) = app.emit("agent-desk-state", s) {
            log_buffer::warn(format!("任务台状态事件发送失败: {e}"));
        }
    }
}

/// 一条状态播报（Notice）事件。
///
/// ⚠️ **它不携带任何"未读"语义** —— 收到它不代表有结果等你听。
/// 那是 `agent-desk-state` 里 `unread` 字段的事。两条通道各说各的：
/// 喇叭说"发生了什么"，灯说"有几件事等你处理"。
#[derive(Clone, Serialize)]
pub struct NoticeEvent {
    /// 归属槽位（0 基）；`None` 表示这是全局进度播报。
    ///
    /// 进度播报刻意不带槽位号：语料是固定闭集，带上槽位就得把 10 句 × 4 槽
    /// 全预合成一遍。而**"是哪一槽"本就该由灯来说**（灯是多路信道，喇叭是单路）。
    pub slot: Option<usize>,
    pub kind: NoticeKind,
    /// 固定语料文本（供前端做字幕 / 无障碍）。
    pub text: &'static str,
    /// 是否终态。
    ///
    /// 给前端一句"这条值得打断吗"的判据：终态（完成 / 失败 / 失联）值得，
    /// 进度类（"已运行 1 分钟"）不值得 —— 它是可以随时丢弃的。
    pub terminal: bool,
    /// 预合成音频的本地路径；`None` = 尚未合成，**前端不得静默跳过**。
    pub audio_path: Option<String>,
    /// 可选的补充文本（C 方案：外部会话的最终回复摘要）。
    ///
    /// ⚠️ **它不参与语音**。语音播的仍是 `text`（固定语料）—— 动态文本必须现调 TTS，
    /// 那就毁掉了"预合成一次、断网也能播"这个前提。它只用于屏幕上的字幕与日志，
    /// 让人一眼看出"刚完成的是哪个任务"。
    pub detail: Option<String>,
}

/// 状态播报的统一出口：去抖 → 发事件。返回是否真的播了出去。
///
/// # ⛔ 这里有一条不可越过的边界（方案 §2.6.1 的 Gate）
///
/// **Notice 绝不写未读账本。** 本函数体内没有任何 `push_result` / `grant` ——
/// 状态播报既不产生未读，也不点亮未读灯。用户的"未读"只能由任务终态产生
/// （`state::complete_task`）。
///
/// 少了这条边界就会出现最伤信任的失败模式：**用户看到"未读"灯亮，按下去只听到
/// 一句"正在推进"**。灯一旦骗人一次，"灯 = 有东西等你听"这条契约就作废了。
///
/// # 两条驱动源共用这一个出口
///
/// 终态（事件驱动）与进度（状态驱动）都调它，于是去抖账本只有一份 ——
/// 各自持一份的话，"刚播完进度又播进度"就拦不住了。
fn emit_notice(
    app: &tauri::AppHandle,
    slot: Option<usize>,
    kind: NoticeKind,
    now: u64,
) -> bool {
    emit_notice_with(app, GateId::Slot, slot, kind, now, None, VoiceSpec::Corpus)
}

/// 用哪一本去抖账本。
///
/// 分立的理由见 `DeskManager::probe_gate`：任务台派发与外部会话是**两个信息源**，
/// 让它们共用一本账会让"刚播过一次完成"把另一条真实的完成吞掉。
#[derive(Clone, Copy, PartialEq, Eq)]
enum GateId {
    /// 任务台派发的任务（槽位 / 进度）。
    Slot,
    /// 外部会话监工（C）。
    Probe,
}

/// 播报的实际出口：`emit_notice` 与 C 方案共用。
///
/// `voice` 决定耳朵听到什么：
///
/// - `Corpus` ⇒ 只念 `kind` 的固定语料（预合成、断网可用）。
/// - `Text` / `Summarize` ⇒ 念"这次任务的结论"。终态播报走这两条，喇叭念的就是
///   结果本身，而不是一句干巴巴的"任务已完成"。合成为空/失败时自动退回固定语料 ——
///   降级链在 `speak_notice` 里，**只有一份**（任何调用方都自动获得这个保证）。
///
/// ⚠️ 它**不是** `Option<&str>`：`Summarize` 是一次真实的执行器调用，必须在后台算
/// （理由见 [`VoiceSpec`] 的文档）。把它写成"调用方先算好再传文本"，就等于让
/// `pump_slot` 拿着整个槽位去等一次 LLM。
fn emit_notice_with(
    app: &tauri::AppHandle,
    gate_id: GateId,
    slot: Option<usize>,
    kind: NoticeKind,
    now: u64,
    detail: Option<&str>,
    voice: VoiceSpec,
) -> bool {
    use tauri::{Emitter, Manager as _};

    let (pass, cached) = {
        let mgr = app.state::<DeskManager>();
        let mut gate = match gate_id {
            GateId::Slot => mgr.notice_gate.lock().unwrap_or_else(|e| e.into_inner()),
            GateId::Probe => mgr.probe_gate.lock().unwrap_or_else(|e| e.into_inner()),
        };
        if !gate.should_emit(kind, now) {
            (false, false)
        } else {
            let assets = NoticeAssets::new(&mgr.output_dir());
            (true, assets.cached(kind).is_some())
        }
    };
    if !pass {
        return false;
    }

    // 动态正文：现合成，不依赖预合成语料（见下方出声条件）。
    //
    // 只判"**值不值得开一次合成**"，不算正文 —— 正文（尤其二次总结）在
    // 下面那个后台任务里算。
    let want_voice = voice.will_speak();

    let payload = NoticeEvent {
        slot,
        kind,
        text: kind.text(),
        terminal: kind.is_terminal(),
        audio_path: {
            let mgr = app.state::<DeskManager>();
            NoticeAssets::new(&mgr.output_dir())
                .cached(kind)
                .map(|p| p.to_string_lossy().to_string())
        },
        detail: detail.map(|s| s.to_string()),
    };

    // 有缓存没缓存都发事件：前端需要知道"该播什么"，而"能不能出声"是另一个问题。
    // 但**必须在日志里说清**，否则"听不到声"会被误诊成前端 bug。
    log_buffer::info(format!(
        "任务台播报: {}{}{}「{}」{}",
        match slot {
            Some(s) => format!("S{} ", s + 1),
            None => String::new(),
        },
        match gate_id {
            GateId::Slot => String::new(),
            GateId::Probe => "外部会话 ".to_string(),
        },
        if cached || want_voice {
            ""
        } else {
            "（语料未预合成且无正文 ⇒ 此行无声）"
        },
        kind.text(),
        // 摘要只进日志，**不进语音**（理由见 `NoticeEvent::detail`）。
        match detail {
            Some(d) if !d.trim().is_empty() => format!(" — {}", crate::agent_desk::codex_probe::tail_chars(d, 120)),
            _ => String::new(),
        }
    ));

    if let Err(e) = app.emit("agent-desk-notice", payload) {
        log_buffer::warn(format!("任务台播报事件发送失败: {e}"));
    }

    // ---- 出声（**必须在宿主里播，不能交给前端**）----
    //
    // 少了这一步，所谓"播报"就只是**日志里的一行 + 界面上的一行字幕**：任务台页面
    // 没打开、或者正处在切页的那一瞬间，用户什么都听不到，而系统日志仍然写着
    // "任务台播报: 「任务已开始」"。这是最伤人的一类失败 —— **看起来在工作，
    // 实际没兑现**，且排查时会一路查到前端去。
    //
    // 喇叭是设备级的承诺：用户听到那句"已开始"才会放心走开。承诺必须在宿主兑现。
    // ⚠️ 出声条件不能只是 `cached`：动态正文（`VoiceSpec::Text` / `Summarize`）是
    // **现合成**的，不依赖预合成语料。只看 `cached` 的话，"语料没预合成"会连正文一起
    // 静音 —— 而用户明明配了 TTS，听起来却像播报整个坏掉了。
    if cached || want_voice {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            use tauri::Manager as _;
            let mgr = app2.state::<DeskManager>();
            // ⚠️ 正文在**这里**才定稿：`Summarize` 会真的跑一次执行器（可能几十秒）。
            // 放在这一层而不是调用方，是为了让那次调用发生在**槽位已经清空之后** ——
            // 排在后面的任务不必陪着等一句"润色"。
            let text = voice.resolve(&mgr).await;
            match speak_notice(&mgr, kind, text.as_deref()).await {
                Ok(r) => log_buffer::info(format!(
                    "任务台出声: 「{}」播放 {}/{} 样本，{}ms",
                    kind.text(),
                    r.played_samples,
                    r.total_samples,
                    r.duration_ms
                )),
                Err(e) => match &e {
                    // 语料缺失已在上面那行日志标注过；被未读播放挡下是**刻意的行为**。
                    // 只有真故障才用 warn —— 否则日志会被正常的抢占刷满，
                    // warn 也就不再是"值得看一眼"的信号了。
                    NoticeSpeakError::NoAsset(_) | NoticeSpeakError::Busy(_) => {
                        log_buffer::info(format!("任务台播报未出声：{}", e.message()))
                    }
                    NoticeSpeakError::Failed(_) => {
                        log_buffer::warn(format!("任务台播报出声失败：{}", e.message()))
                    }
                },
            }
        });
    }

    true
}

/// 进度类播报：由运行时快照推导，没有任何独立的"进度事件"。
fn emit_progress_notice(app: &tauri::AppHandle, snap: &RuntimeSnapshot, now: u64) {
    use tauri::Manager as _;
    let pulse = DeskPulse::from_snapshot(snap);
    // 档位表来自配置（`NoticeConfig.progress_marks_secs`），所以先把策略读出来。
    //
    // ⚠️ 这里**只读**：绝不在此记账。记账统一由 `emit_notice` 里的去抖器做 ——
    // 若这里先判一次"该不该播"，`emit_notice` 再看账本就会判成"重复"，
    // 于是整条播报静默丢失（这正是"两次判定共用一本账"必须避免的形态）。
    let marks_ms = {
        let mgr = app.state::<DeskManager>();
        let gate = mgr.notice_gate.lock().unwrap_or_else(|e| e.into_inner());
        gate.policy().progress_marks_ms.clone()
    };
    if let Some(kind) = pulse.progress_notice(now, &marks_ms) {
        emit_notice(app, None, kind, now);
    }
}

/// 把快照算成灯语并下发设备。**任何失败都只记日志，绝不影响调用方**。
///
/// 灯语是"锦上添花"的下行：它失败（没插设备 / 固件还没有 `0x16`）**不能**导致
/// 队列、未读、事件这些核心链路出错——那些是任务的骨架，灯只是它的指示器。
///
/// # 什么时候发、什么时候不发
///
/// - **有未读 或 有任务在跑** ⇒ 发 active 帧（之后由 [`start_led_pump`] 每 5 秒续期）；
/// - **全空闲** ⇒ 只发**一次** clear，把灯让回固件既有的 idle 灯效，之后不再发。
///
/// 「全空闲也常亮第 5 颗绿」这个文档原案被**刻意放弃**：那会永久占用 5 颗灯去表达
/// "什么都没发生"，而且覆盖掉用户已经习惯的既有 idle 灯效。第 5 颗灯的"0 → 绿"
/// 仍然会在**有未读但没在跑**时出现（那才是它真正有信息量的时刻）。
fn push_led_if_needed(app: &tauri::AppHandle, snap: &RuntimeSnapshot) {
    use tauri::Manager as _;
    let want_active = snap.any_unread || snap.queue.running_count > 0;

    let outcome = {
        let mgr = app.state::<DeskManager>();
        let was_active = mgr.led_active.swap(want_active, Ordering::Relaxed);
        if want_active {
            device_led::push_snapshot(snap)
        } else if was_active {
            device_led::push_clear()
        } else {
            // 本来就没点亮过，现在也没什么要清的 —— 无事可做。
            // 这里**必须直接返回**：否则每 5 秒会白跑一次 HID 枚举。
            return;
        }
    };

    let mgr = app.state::<DeskManager>();
    match outcome {
        Ok(_) => {
            if !mgr.led_ok.swap(true, Ordering::Relaxed) {
                log_buffer::info("任务台灯语已恢复下发".to_string());
            }
        }
        Err(e) => {
            if mgr.led_ok.swap(false, Ordering::Relaxed) {
                log_buffer::warn(format!(
                    "任务台灯语下发失败（同类失败后续静默，避免刷屏）：{e}"
                ));
            }
        }
    }
}

/// 启动灯语心跳泵。
///
/// # 为什么必须有心跳
///
/// 每一帧都自带 ttl（[`device_led::DEFAULT_TTL_MS`]），到期固件自动回落到既有状态机
/// —— 这正是我们要的"上位机死了灯就自己灭"。代价是**活着的时候必须续期**：
/// 没有心跳，灯会每 15 秒闪回一次 idle。
///
/// # 为什么用独立线程而不是 async 任务
///
/// 它只做一件事：每 5 秒醒一次、算个快照、下发。用 `std::thread` 就不必给 tokio
/// 打开 `time` feature，也不必把一个与 Tauri 生命周期无关的循环挂进运行时。
/// 醒来后走的是与事件路径**完全相同**的 [`push_led_if_needed`]，两条路径的行为
/// 从结构上无法漂移。
pub fn start_led_pump(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let period = std::time::Duration::from_millis(device_led::HEARTBEAT_MS);
        loop {
            std::thread::sleep(period);
            let snap = {
                use tauri::Manager as _;
                app.state::<DeskManager>().runtime_snapshot()
            };
            push_led_if_needed(&app, &snap);
            run_probe(&app);
        }
    });
}

/// 外部会话监工（C）：扫一遍 codex 的 rollout 日志，见到"完成"就播一句。
///
/// # 为什么挂在灯语心跳上，而不是另起一个线程
///
/// 它需要的正是心跳已经具备的性质：**每 5 秒醒一次、不阻塞任何东西**。
/// 另起一个线程就多一份生命周期要管，而两个循环做同一件事时，
/// "只改了其中一个的间隔"是最容易发生的漂移。
///
/// # 为什么不产生未读
///
/// 这里的结果**就在用户正开着的终端里**，永久驻留。未读是为"结果在一个
/// 你看不见的地方"设计的；再点一颗"有结果没听"的灯属于重复记账，
/// 也会稀释"灯亮 = 真有东西等你听"这条契约（详见 `codex_probe` 模块文档）。
fn run_probe(app: &tauri::AppHandle) {
    use tauri::Manager as _;

    let now = now_ms();
    // 扫描持 `probe` 锁，但它只做文件 IO，且**在播报之前就释放** ——
    // 播报要拿 `probe_gate` 与 `app.state()`，嵌套持锁是死锁的温床。
    let hits = {
        let mgr = app.state::<DeskManager>();
        let cfg = mgr.store.get();
        if !cfg.probe.enabled {
            return;
        }
        let Some(root) = cfg.probe.resolved_root() else {
            probe_root_missing();
            return;
        };
        let mut probe = mgr.probe.lock().unwrap_or_else(|e| e.into_inner());
        probe.scan(&root, now)
    };

    if hits.is_empty() {
        return;
    }
    if hits.len() > 1 {
        // 不静默：去抖会让第二次及以后**不出声**。若不写日志，
        // "我明明跑了 3 个怎么只播了一次"会变成无从查起的谜。
        log_buffer::info(format!(
            "外部会话：本次扫到 {} 个完成 ⇒ 播报只出声一次（去抖：同类通知不重复播）",
            hits.len()
        ));
    }
    if let Some(hit) = hits.first() {
        // ⚠️ 外部会话（C 方案）目前只念**固定语料**，不念 `hit.summary`。
        // 那不是遗漏：C 方案自己有一套"扫到完成即播报"的去抖规则，
        // 而把动态正文接上去要先想清楚"两个信息源共用一本去抖账"的后果。
        // 现在不动它 —— 也正因为此处是唯一没接动态正文的终态播报点，
        // 它才成为"固定语料仍然可用"这条退路的持续验证点。
        emit_notice_with(
            app,
            GateId::Probe,
            None,
            NoticeKind::Completed,
            hit.at_ms,
            Some(&hit.summary),
            VoiceSpec::Corpus,
        );
    }
}

/// "取不到 rollout 根目录"这条提示只记**一次**。
///
/// 它每 5 秒就会被触发一次；无条件记会把真正有用的日志淹掉 ——
/// 与 `led_ok` 那套"只在翻转时记日志"是同一个理由。
static PROBE_ROOT_WARNED: AtomicBool = AtomicBool::new(false);

fn probe_root_missing() {
    if PROBE_ROOT_WARNED.swap(true, Ordering::Relaxed) {
        return;
    }
    log_buffer::warn(
        "外部会话监工：取不到 rollout 根目录（USERPROFILE 未设置）⇒ 监工已停用。\
         可在任务台设置里显式填 root。"
            .to_string(),
    );
}

// ---------------------------------------------------------------------------
// 命令：运行时状态与槽位操作
// ---------------------------------------------------------------------------

/// 读一份运行时快照（队列 + 未读）。
#[tauri::command]
pub fn desk_runtime_state(mgr: State<'_, DeskManager>) -> ApiEnvelope<RuntimeSnapshot> {
    ApiEnvelope::ok(mgr.runtime_snapshot())
}

/// 最近的任务历史（**最新在前**）。
///
/// # 为什么给 `limit` 而不是给"全部"
///
/// 磁盘上最多留 `history::KEEP`（200）条。历史面板一屏看不了 200 条，而每条带
/// 600 字的输出尾部 ⇒ 全量下发就是几百 KB 的 JSON。让调用方说清楚要几条，
/// 比在前后端之间来回改"默认值"便宜。
#[tauri::command]
pub fn desk_task_history(
    mgr: State<'_, DeskManager>,
    limit: usize,
) -> ApiEnvelope<Vec<TaskRecord>> {
    ApiEnvelope::ok(mgr.recent_history(limit))
}

/// 某条任务的**全量**日志（`None` = 没有这条，或它的日志文件已不在）。
///
/// 与列表命令分开是刻意的，不是"顺手拆的"：列表只读内存里的索引（快，一屏扫过），
/// 详情要读磁盘上那个 `.log`（慢，可能几 MB）。合成一条命令的后果是
/// **打开历史页要读几十个 MB** —— 用户点开一个列表却要等三秒。
#[tauri::command]
pub fn desk_task_log(mgr: State<'_, DeskManager>, task: TaskId) -> ApiEnvelope<Option<String>> {
    ApiEnvelope::ok(mgr.task_log(task))
}

/// 手动派发一条任务到指定槽位。
///
/// 这条命令现在有两个用途：① 调试面板验证编排链路（Phase 2 的验收手段）；
/// ② Phase 3 接上按键录音后，它仍然是"没有硬件也能复现问题"的入口。
#[tauri::command]
pub fn desk_dispatch_task(
    app: tauri::AppHandle,
    slot: u8,
    prompt: String,
) -> ApiEnvelope<DispatchOutcome> {
    match dispatch_task(&app, slot as usize, prompt) {
        Ok(o) => ApiEnvelope::ok(o),
        Err(e) => ApiEnvelope::err(e.code, e.message),
    }
}

/// 撤掉某槽排队中的一条任务（**不影响正在执行的那条**）。
///
/// 参数名用 `task` 而不是 `task_id`：Tauri 命令参数在 JS 侧是 camelCase
/// （`task_id` 会变成 `taskId`），单字命名就没有这层转换，两边读起来一致，也不用记规则。
#[tauri::command]
pub fn desk_cancel_pending(
    app: tauri::AppHandle,
    mgr: State<'_, DeskManager>,
    slot: u8,
    task: TaskId,
) -> ApiEnvelope<u64> {
    let r = {
        let mut rt = mgr.runtime();
        rt.queue_mut()
            .cancel_pending(slot as usize, task)
            .map_err(|e| (queue_error_code(&e), describe_queue_error(&e)))
    };
    match r {
        Ok(t) => {
            emit_state_if_changed(&app);
            ApiEnvelope::ok(t.id)
        }
        Err((code, msg)) => ApiEnvelope::err(code, msg),
    }
}

/// 取消某槽**正在执行**的任务（通过取消令牌杀掉子进程 / 中断请求）。
///
/// 返回 `false` 表示该槽当前没有在跑的任务——这不是错误，但要如实说出来，
/// 免得用户以为"点了取消它一定停了"。
///
/// 这里**不主动发状态事件**：取消只是置了一个内部标志，队列状态要等执行泵收尾时才变
/// （那时它自己会发）。现在发一次只会让前端看到一个与之前一模一样的快照。
#[tauri::command]
pub fn desk_cancel_running(mgr: State<'_, DeskManager>, slot: u8) -> ApiEnvelope<bool> {
    let token = {
        let toks = mgr.cancel_tokens.lock().unwrap_or_else(|e| e.into_inner());
        toks.get(slot as usize).and_then(|c| c.clone())
    };
    match token {
        Some(t) => {
            t.cancel();
            log_buffer::info(format!("任务台 S{} 正在执行的任务已被取消", slot + 1));
            ApiEnvelope::ok(true)
        }
        None => ApiEnvelope::ok(false),
    }
}

/// 清空所有槽位的待执行任务（不动正在执行的）。返回清掉的条数。
#[tauri::command]
pub fn desk_clear_pending(app: tauri::AppHandle, mgr: State<'_, DeskManager>) -> ApiEnvelope<usize> {
    let n = {
        let mut rt = mgr.runtime();
        rt.queue_mut().clear_pending()
    };
    if n > 0 {
        log_buffer::info(format!("任务台清空待执行任务 {n} 条"));
    }
    emit_state_if_changed(&app);
    ApiEnvelope::ok(n)
}

/// 索取播放租约（模拟"用户按下了播放键"）。
///
/// **不消费未读**：只有 [`desk_ack_playback`] 才消费。这是"播到一半拔电 → 未读保留"
/// 得以成立的原因——播放开始和播放完成是两件事。
#[tauri::command]
pub fn desk_grant_playback(
    app: tauri::AppHandle,
    mgr: State<'_, DeskManager>,
    slot: u8,
) -> ApiEnvelope<Lease> {
    let r = {
        let mut rt = mgr.runtime();
        let now = now_ms();
        // 先清超时租约：一个卡死的播放流程不该把喇叭永久锁住。
        let expired = rt.unread_mut().expire(now);
        for s in &expired {
            log_buffer::warn(format!(
                "任务台 S{} 的播放租约已超时释放（播放流程可能已卡死）",
                s + 1
            ));
        }
        rt.unread_mut().grant(slot as usize, now)
    };
    match r {
        Ok(l) => {
            log_buffer::info(format!("任务台播放租约授予：{}", lease_label(Some(l))));
            emit_state_if_changed(&app);
            ApiEnvelope::ok(l)
        }
        Err(e) => ApiEnvelope::err(ErrorCode::Conflict, describe_lease_error(&e)),
    }
}

/// 回执：这段内容**完整播完了**，消费一条未读。
///
/// 迟到回执会被拒绝（token 不匹配），**且不消费任何未读**——这是"灯灭 = 真的听过了"
/// 这条契约的核心保障。
#[tauri::command]
pub fn desk_ack_playback(
    app: tauri::AppHandle,
    mgr: State<'_, DeskManager>,
    slot: u8,
    token: u64,
) -> ApiEnvelope<ConsumeOutcome> {
    use tauri::Manager as _;
    let now = now_ms();
    let r = {
        let mut rt = mgr.runtime();
        let lease = rt
            .unread()
            .active_lease()
            .filter(|l| l.slot == slot as usize && l.token == token);
        match lease {
            Some(l) => rt.unread_mut().consume(l, now),
            None => {
                // 构造一个"必然不匹配"的租约，让 consume 给出统一的迟到/抢占说明。
                rt.unread_mut().consume(
                    Lease {
                        slot: slot as usize,
                        token,
                        granted_at_ms: 0,
                        generation: 0,
                        granted_pending: 0,
                    },
                    now,
                )
            }
        }
    };
    match r {
        Ok(o) => {
            let mgr2 = app.state::<DeskManager>();
            mgr2.save_unread();
            emit_state_if_changed(&app);
            ApiEnvelope::ok(o)
        }
        Err(e) => ApiEnvelope::err(ErrorCode::Conflict, describe_lease_error(&e)),
    }
}

/// 归还租约，**不消费未读**（用户抢占 / 播放流程放弃）。
#[tauri::command]
pub fn desk_release_playback(
    app: tauri::AppHandle,
    mgr: State<'_, DeskManager>,
    slot: u8,
    token: u64,
) -> ApiEnvelope<()> {
    let r = {
        let mut rt = mgr.runtime();
        let lease = Lease {
            slot: slot as usize,
            token,
            granted_at_ms: 0,
            generation: 0,
            granted_pending: 0,
        };
        rt.unread_mut().release(lease)
    };
    match r {
        Ok(()) => {
            emit_state_if_changed(&app);
            ApiEnvelope::ok_empty()
        }
        Err(e) => ApiEnvelope::err(ErrorCode::Conflict, describe_lease_error(&e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::ErrorCode;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "easyop_desk_mod_{tag}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn manager_creates_default_config_on_first_run() {
        let dir = temp_dir("first");
        let mgr = create_manager(&dir);
        let cfg = mgr.store.get();
        assert_eq!(cfg.slots.len(), SLOT_COUNT);
        assert!(dir.join(DESK_CONFIG_FILE).exists(), "首次运行应落盘默认配置");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn output_dir_falls_back_next_to_config() {
        let dir = temp_dir("outdir");
        let mgr = create_manager(&dir);
        let out = mgr.output_dir();
        assert_eq!(out.parent().unwrap(), dir.as_path());
        assert!(out.to_string_lossy().contains("agent_desk_out"));
    }

    #[test]
    fn output_dir_honours_explicit_setting() {
        let dir = temp_dir("outdir2");
        let mgr = create_manager(&dir);
        let mut cfg = mgr.store.get();
        cfg.output_dir = "D:\\tmp\\my_out".into();
        mgr.store.set(cfg).unwrap();
        assert_eq!(mgr.output_dir().to_string_lossy(), "D:\\tmp\\my_out");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_reports_blockers_not_silent() {
        let dir = temp_dir("snap");
        let mgr = create_manager(&dir);
        let snap = build_snapshot(&mgr);
        // 默认配置：开关关着 + 槽位全空 + TTS 无 key → 三条 blocker 都要在。
        assert!(!snap.ready);
        assert!(snap.blockers.iter().any(|b| b.contains("总开关")));
        assert!(snap.blockers.iter().any(|b| b.contains("都未绑定")));
        assert!(snap.blockers.iter().any(|b| b.contains("TTS")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_marks_bound_but_unconfigured_driver() {
        let dir = temp_dir("snap2");
        let mgr = create_manager(&dir);
        let mut cfg = mgr.store.get();
        cfg.enabled = true;
        cfg.slots[0] = SlotBinding::Exact {
            driver: "doubao-api".into(),
        };
        mgr.store.set(cfg).unwrap();

        let snap = build_snapshot(&mgr);
        // doubao-api 预置 enabled=true 但没有 key → 必须点名"未配置完整"。
        assert!(snap
            .blockers
            .iter()
            .any(|b| b.contains("未配置完整")), "blockers={:?}", snap.blockers);
        assert!(!snap.ready);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_flags_empty_capability_candidate_pool() {
        let dir = temp_dir("snap3");
        let mgr = create_manager(&dir);
        let mut cfg = mgr.store.get();
        cfg.enabled = true;
        cfg.tts.api_key = "sk-fake".into();
        cfg.slots[0] = SlotBinding::Capability {
            capability: Capability::Code,
            prefer: vec![],
        };
        mgr.store.set(cfg).unwrap();

        let snap = build_snapshot(&mgr);
        // 空候选池必须显式报错——这是"失败不静默"那条铁律在配置层的体现。
        // 注意这里用的是**真实可调度候选数**（而不是 `prefer` 列了几项），
        // 所以"一个都没配"和"配了但都不可用"都会落进空池。
        assert!(
            snap.blockers
                .iter()
                .any(|b| b.contains("没有任何可调度执行器声明该能力")),
            "实际 blockers: {:?}",
            snap.blockers
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn probe_reports_missing_key_statically() {
        let dir = temp_dir("probe");
        let mgr = create_manager(&dir);
        let cfg = mgr.store.get();
        let d = cfg.driver("doubao-api").unwrap();
        assert!(!d.is_configured());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn driver_view_never_exposes_api_key() {
        let dir = temp_dir("leak");
        let mgr = create_manager(&dir);
        let mut cfg = mgr.store.get();
        cfg.drivers[0].api_key = "sk-SUPER-SECRET".into();
        mgr.store.set(cfg).unwrap();

        let snap = build_snapshot(&mgr);
        let json = serde_json::to_string(&snap).unwrap();
        // 密钥绝不能随快照流到前端列表里。
        assert!(!json.contains("SUPER-SECRET"), "快照泄漏了 API Key");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `build_snapshot` 直接接受 `&DeskManager`，测试里可以自由构造。
    #[test]
    fn error_code_for_not_found_driver() {
        // 直接验证码表选择：找不到 driver 必须报 NotFound 而不是 Internal。
        let e = ApiEnvelope::<()>::err(ErrorCode::NotFound, "没有该执行器");
        assert_eq!(e.code, ErrorCode::NotFound);
    }

    // ---- 结果正文抽取（09-20 实测：codex 的 stdout 是事件流，结论不在结尾）----

    /// 本机实跑 `codex.cmd exec --json --skip-git-repo-check "请帮我打开微信这个应用"`
    /// 抓到的 stdout（有删减，结构未改）。**把真实样本钉在这里**，
    /// 于是"该从哪种事件里取结论"这件事不会再被猜错第二次。
    const CODEX_REAL_STDOUT: &str = concat!(
        r#"{"type":"thread.started","thread_id":"01a0bef0"}"#, "\n",
        r#"{"type":"turn.started"}"#, "\n",
        r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"我现在尝试在这台电脑上启动微信。"}}"#, "\n",
        r#"{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"powershell.exe ...","aggregated_output":"started-uri\r\n","exit_code":0,"status":"completed"}}"#, "\n",
        r#"{"type":"item.completed","item":{"id":"item_2","type":"agent_message","text":"微信已启动。"}}"#, "\n",
        r#"{"type":"turn.completed","usage":{"input_tokens":31386,"output_tokens":248}}"#,
    );

    #[test]
    fn final_text_is_the_last_agent_message_not_the_last_line() {
        // 关键：结论在**倒数第二行**，最后一行是 usage 统计。
        assert_eq!(final_agent_text(CODEX_REAL_STDOUT).as_deref(), Some("微信已启动。"));
    }

    #[test]
    fn spoken_text_never_recites_json() {
        // 用户等了 40 秒，绝不该听到 "turn.completed … input_tokens 31386"。
        let o = RunOutcome {
            status: RunStatus::Completed,
            reason: None,
            output: CODEX_REAL_STDOUT.to_string(),
            duration_ms: 43_700,
            confidence: crate::agent_desk::config::Confidence::High,
            evidence: None,
            session_id: None,
        };
        let spoken = compose_spoken(spoken_head(o.status), &spoken_body(&o, 180));
        assert!(spoken.starts_with("任务已完成。"), "{spoken}");
        assert!(spoken.contains("微信已启动。"), "结论必须念出来：{spoken}");
        assert!(!spoken.contains("turn.completed"), "不许念 JSON：{spoken}");
        assert!(!spoken.contains("31386"), "不许念 token 统计：{spoken}");
    }

    #[test]
    fn half_written_last_line_does_not_discard_the_whole_output() {
        // 抓输出时最后一行常常不完整。若"一行解析失败就放弃整份输出"，
        // 结论会随最后一次抓取的运气一起消失 —— 这类偶发最难查。
        let broken = format!("{}\n{{\"type\":\"turn.comp", CODEX_REAL_STDOUT);
        assert_eq!(final_agent_text(&broken).as_deref(), Some("微信已启动。"));
    }

    #[test]
    fn plain_text_output_falls_back_to_the_tail() {
        // 不是所有执行器都吐 JSONL。兜底路径必须还在（退回"取输出尾部"）。
        assert_eq!(final_agent_text("hello world"), None);
        let o = RunOutcome {
            status: RunStatus::Completed,
            reason: None,
            output: "  ".to_string(),
            duration_ms: 1,
            confidence: crate::agent_desk::config::Confidence::Medium,
            evidence: None,
            session_id: None,
        };
        // 连输出都没有时，要念失败原因而不是干念一句"任务已完成"。
        let o2 = RunOutcome {
            status: RunStatus::Failed,
            reason: Some("退出码 1：command not found".to_string()),
            ..o
        };
        assert!(compose_spoken(spoken_head(o2.status), &spoken_body(&o2, 180))
            .contains("command not found"));
    }

    // ---- 槽位会话（多轮对话）----

    #[test]
    fn session_is_invalidated_when_the_driver_changes() {
        let dir = temp_dir("session_driver");
        let mgr = create_manager(&dir);
        mgr.set_session(0, "thread-abc".into(), "codex-cli".into());
        assert_eq!(mgr.session_for(0, "codex-cli").as_deref(), Some("thread-abc"));
        // 换了执行器 ⇒ 作废。codex 的 thread id 交给别的工具毫无意义，
        // 最好情况是被拒，最坏情况是它被当成别的东西用。
        assert_eq!(mgr.session_for(0, "dsh"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_survives_a_restart_and_reset_clears_it() {
        // 会话 id 是**上游已经给过的事实**：丢了它，用户那句"那刚才那个呢"
        // 就彻底答不上来 —— 而这句话很可能发生在重启之后。
        let dir = temp_dir("session_persist");
        {
            let mgr = create_manager(&dir);
            mgr.set_session(1, "persisted-id".into(), "codex-cli".into());
        }
        let mgr2 = create_manager(&dir);
        assert_eq!(
            mgr2.session_for(1, "codex-cli").as_deref(),
            Some("persisted-id"),
            "会话必须跨重启活着，否则多轮对话只能在一次进程里成立"
        );

        // 手动重置：这是"换个话题"的唯一出口，没有它槽位一旦挂上就下不来。
        assert!(mgr2.clear_session(1));
        assert_eq!(mgr2.session_for(1, "codex-cli"), None);
        assert!(!mgr2.clear_session(1), "已经没有了，重置不该报成功");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn out_of_range_slot_session_never_panics() {
        // 槽位越界必须走"没有会话"这条路，不能 panic —— 它会带着整个执行泵一起死。
        let dir = temp_dir("session_range");
        let mgr = create_manager(&dir);
        assert_eq!(mgr.session_for(99, "codex-cli"), None);
        assert!(!mgr.clear_session(99));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
