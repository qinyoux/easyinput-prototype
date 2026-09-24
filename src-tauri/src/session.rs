//! 录音识别会话 —— Hold / Call 两种触发模式的状态机。
//!
//! - Hold（按住说）：PTT 按下启动一次录音识别，松手发负包收尾，最终文本注入。
//! - Call（打电话）：PTT 按一下进入通话态，持续录音，服务端 VAD 自动断句，
//!   每句定稿即注入上屏、自动继续下一句，再按一下挂断收尾。
//!
//! 会话以代数（generation）标识：任务收尾时只有「仍是当前会话」才清空活动槽，
//! 防止快速连按 PTT 时上一会话的文本注入到新会话中间。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

use crate::{asr, audio, keyboard_audio, text_delivery};

/// 显示后台字幕浮层：先定位到鼠标光标下方居中，再显示。弹出后不再跟随移动。
///
/// `pub(crate)` 的理由：显隐权整体收归后端之后，`set_overlay_enabled`（在 `lib.rs`）
/// 也需要在中途把开关打开时补一次显示 —— 它必须走**同一个**定位逻辑，
/// 不能自己再写一份 `set_position + show`，否则两处的定位迟早会不一致。
pub(crate) fn show_overlay(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("overlay") {
        if let Some((x, y)) = crate::cursor_subtitle_pos(&win, crate::OVERLAY_W, crate::OVERLAY_H) {
            let _ = win.set_position(tauri::PhysicalPosition::new(
                x.round() as i32,
                y.round() as i32,
            ));
        }
        let _ = win.show();
    }
}

/// 隐藏后台字幕浮层（若存在）。
fn hide_overlay(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("overlay") {
        let _ = win.hide();
    }
}

/// 显示编辑浮层（语音编辑专用）。
fn show_edit_overlay(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("edit_overlay") {
        if let Some((x, y)) = crate::cursor_subtitle_pos(&win, 400.0, 120.0) {
            let _ = win.set_position(tauri::PhysicalPosition::new(
                x.round() as i32,
                y.round() as i32,
            ));
        }
        let _ = win.show();
    }
}

/// 隐藏编辑浮层（若存在）。
fn hide_edit_overlay(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("edit_overlay") {
        let _ = win.hide();
    }
}

/// 录音源的显示名。
///
/// 必须出现在"没收到音频"这类提示里：用户看到的第一反应就是"你用的是哪个麦克风"，
/// 答不出来这句提示等于白写（他会去查系统设置，而问题可能在板子上）。
fn mic_label(use_keyboard: bool) -> &'static str {
    if use_keyboard {
        "开发板麦克风"
    } else {
        "电脑麦克风"
    }
}

/// 把一句"语音链路的告警"同时发到字幕浮层**和主窗口**。
///
/// ⛔ 只发浮层是不够的：浮层只有录音那几秒才显示，而这条链路上的故障大多发生在
/// **按下之前**（会话卡死、归属冲突）或**按下之后**（没收到音频、上屏失败）——
/// 恰恰是浮层不在的时候。用户看到的就是"按了没反应"，然后只能猜。
/// 多开一条 `voice-alert` 通道，主窗口会把它显示成提示条 ⇒ **失败一定有地方看得见**。
fn alert_voice(app: &tauri::AppHandle, message: &str) {
    crate::log_buffer::warn(format!("语音链路：{message}"));
    let _ = app.emit("asr-state", message);
    let _ = app.emit(
        "voice-alert",
        serde_json::json!({ "message": message, "source": "session" }),
    );
}

/// 单次会话最长录音时长（对齐原版 max_recording_duration_secs=180）。
const MAX_DURATION: Duration = Duration::from_secs(180);
/// Call 模式最长通话时长（30 分钟兜底，正常由「再按一次」结束）。
const MAX_CALL_DURATION: Duration = Duration::from_secs(1800);

/// 触发模式（持久化在 `config.json` 的 `mode` 字段，取值 `"hold"` / `"call"`）。
///
/// ⚠️ **两个变体的名字都是历史遗留，与它们现在的语义对不上 —— 别按名字改行为。**
///
/// | 名字 | 现在的语义 | 名字的问题 |
/// |---|---|---|
/// | `Hold` | **短按**：按下开始，**再按一次**结束并交付 | 名字暗示"按住" |
/// | `Call` | 按一下开始、再按一下挂断，中间逐句上屏 | 名字是准的 |
///
/// 不改名是为了不动 `config.json` 的格式（改它就是破坏性变更，而用户可见的
/// 行为一个字节都不会变）。取而代之的是把话说透 —— 这次踩的坑正是
/// "看到固件动作名里有 `Hold` / 看到 `clamp_to_budget` 形的注释，就反推出
/// 产品是按住说话"。**语义以用户看到的那句话为准**（`constants.ts` 的
/// 「语音 · 按下说话」），不以标识符为准。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriggerMode {
    /// **短按单句**：按下开始录音 → 再按一次结束并交付。
    ///
    /// 交付去向由"结束的那一下"决定（见 [`delivery_for`]）：
    /// 再按语音键 = 只注入（不发送）；录音期间按**回车** = 注入并发送。
    ///
    /// ⛔ **它不是"按住说话"。** 固件对 `VoicePttHold` 确实发 keydown/keyup 一对，
    /// 但产品语义是短按 —— 钩子层只认按下（`PttEvent` 的注释里记着那次误改的经过）。
    Hold,
    /// **连续通话**：按一下开始、再按一下挂断，中间自动断句逐句上屏。
    /// ⚠️ 这个模式里**没有**"回车提交"（`enter_grabs` 会拒绝它）：通话是连续多句，
    /// 把回车折叠成"结束并发送"会让用户没法在通话中间正常敲回车。
    Call,
}

/// 麦克风来源（用户在通信配置面板选择）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MicSource {
    /// 自动：优先键盘 Wi-Fi 音频，回退电脑麦克风。
    Auto,
    /// 强制使用电脑本机麦克风。
    Pc,
    /// 强制使用键盘 Wi-Fi 音频（离线时回退电脑）。
    Wifi,
    /// 蓝牙键盘音频 —— **固件没有这个能力**，选中即回退电脑麦克风。
    ///
    /// 保留这个枚举值是为了让老配置（`"mic_source":"bluetooth"`）仍能正确反序列化，
    /// 而不是让整份配置读取失败。行为上它等价于 [`MicSource::Pc`]，但**理由要说清**
    /// （见 `resolve_mic`）—— 否则用户会把"板子没有蓝牙音频"误判成"蓝牙没连上"，
    /// 然后一直去查配对。
    Bluetooth,
}

/// 把磁盘上的 `mic_source` 字符串解析成**这台设备上真的能用的**来源；
/// 第二个返回值表示"是否发生了纠错迁移"（调用方据此写一条日志）。
///
/// # 为什么单独抽出来
///
/// 这段的失效方式是静默的：如果它把 `"bluetooth"` 直接放行，`resolve_mic` 会
/// 一直回退电脑麦克风，界面不报错，用户只会觉得"麦克风怎么老是电脑的"，
/// 然后去查蓝牙配对 —— 方向从一开始就是错的。所以判据必须能被单测钉死，
/// 而单测没法跑 `load_from_disk`（它要读磁盘）。
///
/// # 迁移的判据是"值等于已知错值"，不是"用户没选过"
///
/// `"bluetooth"` 是一个**已知不生效**的值（对固件源码核实：`AudioTransport` 只有
/// `Unavailable` / `WifiUdp`，板子只有 Wi-Fi UDP 一条音频通路），选它和选电脑麦克风
/// 完全等价。因此落到 `Auto` 不覆盖任何用户可能想要的语义 —— 反而把两条真实可用的
/// 路（板子 Wi-Fi / 电脑麦克风）都留着。`pc` / `wifi` 这两个**真能生效**的选择一律保留。
fn parse_mic_source(raw: &str) -> (MicSource, bool) {
    match raw {
        "pc" => (MicSource::Pc, false),
        "wifi" => (MicSource::Wifi, false),
        "bluetooth" => (MicSource::Auto, true),
        _ => (MicSource::Auto, false),
    }
}

/// 语音编辑阶段（toggle 模式状态机）。
#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EditPhase {
    /// 等待按下编辑键。
    Idle,
    /// 语音识别中，字幕实时显示。
    Listening,
    /// LLM 处理中。
    Processing,
}

#[derive(Clone)]
pub struct SessionManager {
    inner: Arc<SessionInner>,
}

/// 落盘持久化的全部配置，保存在 `app_config_dir/config.json`。
#[derive(Serialize, Deserialize, Default)]
struct PersistConfig {
    // ASR
    #[serde(default)]
    asr_url: Option<String>,
    #[serde(default)]
    resource_id: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    // LLM
    #[serde(default)]
    llm_backend: Option<String>,
    #[serde(default)]
    llm_endpoint: Option<String>,
    #[serde(default)]
    llm_model: Option<String>,
    #[serde(default)]
    llm_api_key: Option<String>,
    #[serde(default)]
    llm_timeout_secs: Option<u32>,
    // PTT
    #[serde(default)]
    ptt_spec: Option<String>,
    #[serde(default)]
    edit_ptt_spec: Option<String>,
    #[serde(default)]
    trigger_mode: Option<String>,
    // 通信
    #[serde(default)]
    mic_source: Option<String>,
    #[serde(default)]
    wifi_ssid: Option<String>,
    #[serde(default)]
    wifi_password: Option<String>,
    #[serde(default)]
    ble_enabled: Option<bool>,
    // UI
    #[serde(default)]
    overlay_enabled: Option<bool>,
    // Everything 搜索
    #[serde(default)]
    everything_path: Option<String>,
    // ⌘SS 面板
    #[serde(default)]
    compact_context_menu: Option<bool>,
    // 文本注入（text_delivery）
    //
    // ⚠️ 这两项**不在 `SessionInner` 里另存一份**。真源是 `text_delivery` 的进程内全局
    // —— 存两份就必然出现"读的是 A、写的是 B"的漂移，而这类漂移不会报错，
    // 只会表现为"设置里改了没反应"。
    #[serde(default)]
    paste_style: Option<String>,
    #[serde(default)]
    submit_after_paste: Option<bool>,
    /// 文本注入默认值的**世代标记**（见 `load_from_disk` 里的迁移说明）。
    ///
    /// 默认值是会变的（v0.21.0 把"粘贴后回车"默认改成了开）。旧配置里写着的
    /// `false` **不是用户的选择**，只是当初默认值写盘留下的痕迹 —— 没有这个标记
    /// 就无法区分"用户主动关掉"和"默认值变了"，升级后用户会看到"明明改了默认
    /// 却还是关"。
    #[serde(default)]
    injection_defaults_version: Option<u32>,
}

/// 文本注入默认值的当前世代。**改动默认值时必须 +1**，否则老用户拿不到新默认
/// （他们的磁盘上写着旧默认值，看起来"明明改了默认值却没生效"）。
///
/// 世代沿革：
/// - v1：`submit_after_paste` 默认 **关**（出厂行为）
/// - v2：v0.21.0 起默认 **开**（TUI 里粘完不自动提交，不回车这条链没闭合）
/// - v3：v0.28.2 起默认 **关** —— 回车是不可撤销的提交，该不该提交只有用户知道；
///       语音内容是接在半成品命令后面的，此时回车等于执行一条没审完的命令。
///   ⇒ **这是把 v2 的结论反过来了**，所以必须 +1：磁盘上那些 `true` 是 v2 默认值
///     写盘留下的痕迹，不 +1 的话这份配置会被当成"用户选过"而永远停在"开"，
///     用户改了默认却看不到变化。
const INJECTION_DEFAULTS_VERSION: u32 = 3;

struct SessionInner {
    current: Mutex<Option<Arc<Active>>>,
    generation: AtomicU64,
    api_key: Mutex<String>,
    asr_url: Mutex<String>,
    resource_id: Mutex<String>,
    mode: Mutex<TriggerMode>,
    mic_source: Mutex<MicSource>,
    config_path: Mutex<Option<PathBuf>>,
    overlay_enabled: AtomicBool,
    keyboard_audio: Mutex<Option<keyboard_audio::KeyboardAudioManager>>,
    voice_edit: Mutex<Option<crate::voice_edit::VoiceEditManager>>,
    edit_phase: Mutex<EditPhase>,
    wifi_ssid: Mutex<String>,
    wifi_password: Mutex<String>,
    ble_enabled: AtomicBool,
    everything_path: Mutex<String>,
    compact_context_menu: AtomicBool,
    /// **待认领**的归属槽位（1-based）："下一次录音是派给谁的"。
    ///
    /// 生命周期只有一瞬 —— `start_hold` / `start_call` 开头 `take()` 走，之后即 `None`。
    /// 这样设计是为了杜绝**跨会话串味**：归属若一直躺在全局里，上一次录音超时结束时
    /// 没清掉，下一次普通语音输入就会被错派进任务台（用户明明没按键，话却被派走了）。
    /// "认领即消失"从机制上让"残留"不可能发生，不需要任何一处记得去清。
    desk_intent: Mutex<Option<u8>>,
    /// 当前这段录音实际派给哪个槽 —— **只用于给用户提示**（第二次按键时要说清
    /// "这段是给 S1 的，你按的是 S2"）。真正的派发依据不是它，是 `start_*` 里
    /// 认领后沿调用链传下去的那个局部值。
    ///
    /// 分成两个字段是因为它们的**失效时机不同**：意图必须"认领即消失"（否则残留），
    /// 而提示必须在整段录音期间都还在（否则第二次按键时已经读不到了）。
    recording_desk_slot: Mutex<Option<u8>>,
    /// **注入失败**的那一句话：识别成功、注入却没成功，暂存在这里等用户重试。
    ///
    /// # 它与"待确认"的区别（v0.29.0 有过一个 `pending_inject`，已删）
    ///
    /// - **待确认**（已删）：注入**还没发生**，是流程里的一个正常站点 ——
    ///   于是"按一下说话"变成要按三次，与用户要的短按语义冲突。
    /// - **注入失败**（本字段）：注入**已经试过并且失败了**。它不在正常流程上，
    ///   只在异常路径上出现，所以不会给正常操作多加任何一次按键。
    ///
    /// # 为什么不能干脆不留
    ///
    /// 不留的话，`take()` 走内容之后注入失败 ⇒ **用户刚说的那句话凭空消失**，
    /// 而这句话可能是一整段口述。项目铁律是「失败不静默」，而这里比"不静默"
    /// 更进一步：**已经识别出来的话，不能因为收尾出错而作废**（同 `asr.rs` 那条）。
    /// 所以宁可多一个字段、多三个命令，也不留这条静默丢字的路。
    failed_inject: Mutex<Option<String>>,
    /// **本次交付要不要回车**（"注入并发送"）。
    ///
    /// 由**结束录音的那一下**决定，在交付时被读取：
    ///
    /// | 结束录音的方式 | 值 | 结果 |
    /// |---|---|---|
    /// | 再按一次语音键 | `false` | 只注入，用户自己决定要不要发 |
    /// | 录音期间按**回车** | `true` | 注入并回车，一次按键完成 |
    ///
    /// ⚠️ 它必须在 `finish()` **之后**还活着（识别在途时用户仍可按回车改主意），
    /// 所以放这里而不是 `Active` 上 —— 与 `stop_flag` 的生命周期不同，
    /// 别因为"看起来像会话属性"就塞进 `Active`。
    /// ⛔ 正因为如此，它**必须在每段会话开头重置**（见 `register`）：漏掉的话，
    /// 上一次"要发送"的意图会跟着下一次录音走，表现是"我明明只按了语音键，
    /// 它却自己发出去了" —— 那是不可撤销的动作，代价最大的一类脏状态。
    submit_on_delivery: AtomicBool,
    /// **回车要不要被接管**：从"开始录这一段"到"这一段交付完毕"之间为 `true`。
    ///
    /// 钩子层的回车判据直接读它（见 `ptt_hook::set_enter_gate`）—— 钩子不存副本，
    /// 就是为了避免这个标志在两处各有一份、然后其中一份老化。
    ///
    /// ⚠️ 只有"结果会进输入框"的会话才置位：任务台派发、编辑改写、连续通话
    /// 都不置位。判据 = 纯函数 [`enter_grabs`]。
    /// ⛔ 收尾必须清掉（`start_hold` 的识别任务末尾、以及 `reclaim_stale_session`
    /// 这条强制释放的兜底路）。清不掉就把用户**所有**的回车都吞了。
    voice_delivery_pending: AtomicBool,
}

/// 一段录音是**谁**发起的。
///
/// # 为什么必须有这个东西（这不是可选的锦上添花）
///
/// 录音会话是**互斥资源**：一个麦克风、一路 ASR、一个注入出口。但驱动它的入口有
/// 四个 —— 全局 PTT 热键（KEY1 的 `voice_ptt_hold`）、编辑键（KEY3 的
/// `edit_ptt_hold`）、任务台槽位键（任意键的 `task_desk`）、界面上的「开始录音」按钮。
///
/// 四个入口都只会 toggle（"有录音就停、没有就开"），而这句判断**不认得当前这段录音
/// 是谁开的**。于是任何入口的按下都能"结束"别人正在录的那段，并按一个它自己都没在看的
/// 归属把文本送走 —— 用户看到的现象是「按了这个键，结果是另一个键的事」：
///
/// - 按 KEY6（S1）开始录音 → 习惯性按 KEY1 想语音输入 → **KEY1 把 S1 那段结束了，
///   识别结果当作任务派给 S1 并执行了命令**。用户以为自己只是开了个语音输入。
/// - 按 KEY1 开始语音输入 → 按 KEY6 以为要派给 S1 → 其实是"结束 KEY1 那段"，
///   识别结果**上屏到了编辑器里**。用户以为任务已经派出去了。
///
/// 两次都是**静默**的：没有任何一处报错，用户只能靠"结果不对"自己发现。
///
/// 修法不是"在哪一步多加个 if"，而是让录音会话**自己记住发起者**：任何入口按下时先
/// 比对自己和当前会话的发起者，不一致就**明确拒绝**（不改归属、不 toggle、说清原因）。
/// 这与"归属是会话的属性、不是全局状态"是同一条原则 —— 见 `desk_intent` 的说明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PttOrigin {
    /// 全局 PTT 热键（`voice_ptt_hold`）或界面上的「开始录音」按钮。
    /// 识别结果**上屏**到当前焦点窗口。
    Voice,
    /// 编辑键（`edit_ptt_hold`）。识别结果交给语音编辑链路，不回填原文。
    Edit,
    /// 任务台槽位键（`task_desk`，1-based）。识别结果**派给该槽**，绝不退回上屏。
    DeskSlot(u8),
}

impl PttOrigin {
    /// 给用户看的名字。拒绝提示里要说清"现在这段是谁开的"，否则用户不知道该按哪个键。
    pub fn label(self) -> String {
        match self {
            PttOrigin::Voice => "语音输入键".to_string(),
            PttOrigin::Edit => "编辑键".to_string(),
            PttOrigin::DeskSlot(slot) => format!("槽位 S{slot} 的键"),
        }
    }
}

/// 一次按下是否该被拒绝；`Some(owner)` = 冲突，`owner` 是**正在录**的那个入口。
///
/// # 为什么单独做成纯函数
///
/// 拒绝动作本身要 `AppHandle`（得 emit 一句提示出去）⇒ 在测试里构造不出来。
/// 而"该不该拒绝"恰恰是整条防线里**唯一会出错、且出错后完全静默**的一步：
/// 判错一次，用户按 KEY1 就会把任务台正在录的那段派出去执行命令。
///
/// 按项目铁律：**要 AppHandle 的逻辑测不了，纯判据就必须抽出来单独钉死**。
/// 否则这条防线只剩"看代码好像是对的"。
pub fn press_conflict(pressed: PttOrigin, active: Option<PttOrigin>) -> Option<PttOrigin> {
    match active {
        // 没有录音在跑 ⇒ 谁都能开始。
        None => None,
        // 同一个入口再按一次 ⇒ 这就是"结束"，放行。
        Some(owner) if owner == pressed => None,
        // 别的入口正录着 ⇒ 拒绝，并把"是谁"带回去给提示用。
        Some(owner) => Some(owner),
    }
}

/// 一句话识别完之后**去哪儿**。
///
/// # 为什么这个判断值得单独抽出来
///
/// 它是整条语音链路的**最后一个岔路口**，而两个方向之间没有中间态：判成甲就是
/// 把用户的话打进输入框，判成乙就是把它当任务派出去执行命令。判错的代价是
/// "我说的话进了错误的地方"，而且用户多半要过一会儿才发现。
///
/// 抽成纯函数的另一个理由同 `press_conflict`：真正的执行要 `AppHandle`
/// （要 emit 事件、要注入），测试里构造不出来；而"该往哪儿去"恰恰是这段
/// 唯一会静默判错的一步。按项目铁律：**要 `AppHandle` 的逻辑测不了，
/// 纯判据就必须抽出来单独钉死**。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Delivery {
    /// 派给任务台的某个槽位（1-based）。**不注入、不进输入框。**
    Desk(u8),
    /// 注入当前焦点窗口。`submit == true` 时额外按一次回车。
    Inject { submit: bool },
}

/// 这一轮的识别结果该走哪条路。
///
/// - `desk_slot`：这段录音在"按下开始"那一刻就认领好的归属（`take_desk_intent`）。
/// - `submit`：结束录音的方式是不是"注入并发送"（用户在录音期间按了回车）。
///
/// # 两条规则，各有各的理由
///
/// 1. **派发优先**：有槽位就派给槽位。用户按的是任务台的键，结果就该落进那个槽。
/// 2. **派发时 `submit` 被吃掉**：任务台那一侧靠"未读"让用户来听结果，没有
///    "发送"这个概念。若把回车也体现在派发上，就会出现"按回车让任务台提交"
///    这种谁也不认得的动作 —— 所以这里**不把 `submit` 传下去**，而不是传给下游
///    再让下游忽略它。
pub fn delivery_for(desk_slot: Option<u8>, submit: bool) -> Delivery {
    match desk_slot {
        Some(slot) => Delivery::Desk(slot),
        None => Delivery::Inject { submit },
    }
}

/// 派发到任务台时，字幕浮层要收到的**事件序列**（事件名 + 正文）。
///
/// # 为什么这件事值得单独抽出来
///
/// 它承载的是一条**用户可见的承诺**：「我说的话，字幕上一定看得见。」
/// 这个承诺在任务台路径上曾经是断的，而且断得完全静默 ——
///
/// - 本机 ASR 走的是 `.../plan/sauc/bigmodel_**nostream**` 端点：识别过程中服务端
///   回的每一帧文本**都是空的**（实测 #1~#22 全部 `文本=""`），
///   真正的内容只在收尾那一下给出来。所以浮层在工作期间的字幕本来就是空的。
/// - 任务台路径过去在收尾时**只发 `asr-state`（状态行）**，从不发正文 ——
///   而注入路径的第一件事就是 `emit("asr-final", ...)`。
///
/// 两件事叠起来的结果：用户按住任务台槽位键说完一句话，日志里
/// 「语音：识别成功 N 字」一切正常，屏幕上却**一个字都没有**。
/// 这类"成功了但没留痕"的故障比报错更难查 —— 它没有任何错误可看。
///
/// ⛔ 所以 `asr-final` 这一条**不是可选的**。改这里之前先读上面这段。
pub fn desk_subtitle_events(slot: u8, text: &str) -> [(&'static str, String); 2] {
    [
        // ① 正文。与注入路径同一理由（见 `inject` 开头）：先让用户看见
        //    "我这句话被识别成了什么"，再把它交给下游。
        ("asr-final", text.to_string()),
        // ② 状态行。**必须排在正文之后**：浮层收到 `asr-final` 会把状态置成
        //    "识别完成"，紧接着这一条把它改写成"正在派给 S{slot}" —— 顺序反了
        //    用户就只看到"识别完成"，不知道东西去哪儿了。
        ("asr-state", format!("识别完成，正在派给 S{slot}")),
    ]
}

/// 任务台派发后，字幕浮层的**停留时长**。
///
/// 为什么注入路径不需要它、任务台路径需要：
///
/// - **注入路径**：识别结果紧接着就出现在输入框里。浮层马上收掉不影响用户
///   "看见自己说了什么"—— 输入框里就写着。
/// - **任务台路径**：结果进的是后台队列，**屏幕上不留任何痕迹**。
///   浮层是唯一能看见这句话的地方，立刻收掉等于没显示。
///
/// 取值理由：实测按住键说完一句话约 5 秒，松键后识别在 0.5 秒内返回；
/// 一行十几字的句子读一遍需要 1~2 秒。1.8 秒够读完，又不至于挡着下一次操作。
const DESK_SUBTITLE_LINGER: Duration = Duration::from_millis(1800);

/// 此刻**回车**该不该被当作「注入并发送」（而不是一个普通回车）。
///
/// # 为什么必须窄
///
/// 这个判据用错的后果不是"功能不灵"，而是**把用户正常的回车吃掉**：他在终端里
/// 敲回车执行命令、在编辑器里敲回车换行，结果什么都没发生 —— 现象与"键盘坏了"
/// 一模一样，而真正的原因藏在一个跟他毫无关系的状态位里。这也是它被抽成纯函数
/// 的原因（见 `press_conflict` 的同类理由）。
///
/// 所以只有一种会话认回车：**这一轮的话会进输入框，而且不是任务台的**。
///
/// | 会话 | 认回车？ | 为什么 |
/// |---|---|---|
/// | 语音输入（KEY1 / 面板 ● 按钮） | ✅ | 结果进输入框，"说完就发"正是他要的 |
/// | 任务台槽位键 | ❌ | 结果派给槽位，不进输入框，"发送"无从谈起 |
/// | 编辑键 | ❌ | 结果是"改写选中文本"这个替换动作，没有提交这一步 |
/// | 连续通话（Call） | ❌ | 走的是 `start_call`，压根不经过武装点；且通话期中间还要能正常敲回车 |
pub fn enter_grabs(origin: PttOrigin, desk_slot: Option<u8>) -> bool {
    matches!(origin, PttOrigin::Voice) && desk_slot.is_none()
}

struct Active {
    stop_flag: Arc<AtomicBool>,
    gen: u64,
    /// 这一段录音的发起者。**写一次、只读**，存在 `Active` 里而不是 `SessionInner` 上，
    /// 是为了让它和 `current` **同生共死**：单独放一个字段就必须回答"谁负责清它，
    /// 早一步晚一步都错"（v0.25.0 在归属上踩过一模一样的坑）。
    origin: PttOrigin,
    /// 会话开始时刻。**用途只有一个：判断它是不是已经卡死了**（见 `reclaim_stale_session`）。
    ///
    /// 会话槽的清空点在识别任务**返回之后**，所以只要那条异步链路里有一处永久等待，
    /// 槽位就永远是"有人"，所有语音入口一起失效。这里是最后一道兜底：
    /// 超过硬上限还没收尾的会话一律强行释放，宁可错杀也不能让键盘变砖。
    started_at: Instant,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(SessionInner {
                current: Mutex::new(None),
                generation: AtomicU64::new(0),
                api_key: Mutex::new(String::new()),
                asr_url: Mutex::new(
                    "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async".to_string(),
                ),
                resource_id: Mutex::new("volc.seedasr.sauc.duration".to_string()),
                mode: Mutex::new(TriggerMode::Hold),
                mic_source: Mutex::new(MicSource::Auto),
                config_path: Mutex::new(None),
                overlay_enabled: AtomicBool::new(true),
                keyboard_audio: Mutex::new(None),
                voice_edit: Mutex::new(None),
                edit_phase: Mutex::new(EditPhase::Idle),
                wifi_ssid: Mutex::new(String::new()),
                wifi_password: Mutex::new(String::new()),
                ble_enabled: AtomicBool::new(false),
                everything_path: Mutex::new(String::new()),
                compact_context_menu: AtomicBool::new(false),
                desk_intent: Mutex::new(None),
                recording_desk_slot: Mutex::new(None),
                failed_inject: Mutex::new(None),
                submit_on_delivery: AtomicBool::new(false),
                voice_delivery_pending: AtomicBool::new(false),
            }),
        }
    }

    /// 设定配置文件的落盘路径，并立即加载已保存的配置。
    pub fn init_config_file(&self, path: PathBuf) {
        *self.inner.config_path.lock().unwrap() = Some(path.clone());
        self.load_from_disk();
    }

    /// 从磁盘加载配置到内存（仅覆盖「文件里实际存在」的字段）。
    fn load_from_disk(&self) {
        let path = match self.inner.config_path.lock().unwrap().as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        let Ok(data) = std::fs::read_to_string(&path) else {
            return;
        };
        // 兼容带 UTF-8 BOM 的文本编辑器写入结果（serde_json 不接受前置 BOM）。
        let data = data.trim_start_matches('\u{feff}');
        let Ok(cfg) = serde_json::from_str::<PersistConfig>(data) else {
            return;
        };
        if let Some(v) = cfg.asr_url.filter(|v| !v.is_empty()) {
            *self.inner.asr_url.lock().unwrap() = v;
        }
        if let Some(v) = cfg.resource_id.filter(|v| !v.is_empty()) {
            *self.inner.resource_id.lock().unwrap() = v;
        }
        if let Some(v) = cfg.api_key {
            *self.inner.api_key.lock().unwrap() = v.trim().to_string();
        }
        if let Some(v) = cfg.trigger_mode.as_deref() {
            match v {
                "call" => *self.inner.mode.lock().unwrap() = TriggerMode::Call,
                _ => *self.inner.mode.lock().unwrap() = TriggerMode::Hold,
            }
        }
        if let Some(v) = cfg.mic_source.as_deref() {
            let (source, migrated) = parse_mic_source(v);
            *self.inner.mic_source.lock().unwrap() = source;
            if migrated {
                crate::log_buffer::warn(
                    "配置迁移：mic_source=\"bluetooth\" → \"auto\"（开发板无蓝牙音频通路，\
                     该设置不生效；已改为自动：优先板子 Wi-Fi 音频，否则用电脑麦克风）",
                );
            }
        }
        if let Some(v) = cfg.overlay_enabled {
            self.inner.overlay_enabled.store(v, Ordering::Relaxed);
        }
        // LLM 配置分发到 VoiceEditManager
        if let Some(ve) = self.inner.voice_edit.lock().unwrap().as_ref() {
            let llm_cfg = ve.llm_client().config();
            let updated = crate::llm_client::LlmConfig {
                backend: cfg.llm_backend.as_deref().and_then(|s| serde_json::from_value(serde_json::Value::String(s.to_string())).ok()).unwrap_or(llm_cfg.backend),
                endpoint: cfg.llm_endpoint.clone().unwrap_or(llm_cfg.endpoint),
                model: cfg.llm_model.clone().unwrap_or(llm_cfg.model),
                api_key: cfg.llm_api_key.clone().unwrap_or(llm_cfg.api_key),
                timeout_secs: cfg.llm_timeout_secs.unwrap_or(llm_cfg.timeout_secs),
            };
            crate::log_buffer::info(format!(
                "从磁盘加载 LLM 配置: backend={:?} model={} endpoint={} key_len={} timeout={}s",
                updated.backend, updated.model, updated.endpoint, updated.api_key.len(), updated.timeout_secs
            ));
            ve.llm_client().set_config(updated);
        } else {
            crate::log_buffer::warn("load_from_disk: voice_edit 未设置，LLM 配置未加载");
        }
        // PTT 键分发到 ptt_hook
        if let Some(spec) = cfg.ptt_spec.as_deref() {
            if let Ok(key) = crate::ptt_hook::PttKey::parse(spec) {
                let _ = crate::ptt_hook::set_ptt_key(key);
            }
        }
        if let Some(spec) = cfg.edit_ptt_spec.as_deref() {
            if let Ok(key) = crate::ptt_hook::PttKey::parse(spec) {
                let _ = crate::ptt_hook::set_edit_ptt_key(Some(key));
            }
        }
        // Wi-Fi 配置
        if let Some(v) = cfg.wifi_ssid {
            *self.inner.wifi_ssid.lock().unwrap() = v;
        }
        if let Some(v) = cfg.wifi_password {
            *self.inner.wifi_password.lock().unwrap() = v;
        }
        if let Some(v) = cfg.ble_enabled {
            self.inner.ble_enabled.store(v, Ordering::Relaxed);
        }
        // Everything 路径
        if let Some(v) = cfg.everything_path {
            *self.inner.everything_path.lock().unwrap() = v;
        }
        // 文本注入：推送到 text_delivery 的进程内全局（那里是真源，见 PersistConfig 说明）
        if let Some(v) = cfg.paste_style.as_deref() {
            match crate::text_delivery::PasteStyle::parse(v) {
                Some(s) => crate::text_delivery::set_paste_style(s),
                // 无法识别就**保留默认并留一条日志**。静默忽略会让"我在设置里改了没反应"
                // 变成一个无从查起的谜，而这类配置写错的概率并不低。
                None => crate::log_buffer::warn(format!(
                    "config.json 的 paste_style 无法识别：{v:?}（保留默认值）。有效值：auto / ctrl_v / shift_insert / ctrl_shift_v"
                )),
            }
        }
        //
        // ⚠️ 默认值迁移：默认值是会变的（v0.21.0 起"粘贴后回车"默认开，v0.28.2 起又改回关）。
        // 旧配置里的 `true`/`false` **不一定是用户的选择** —— 老版本的 persist 会把默认
        // 值原样写盘，所以"磁盘里有值"不等于"用户选过"。区分依据是世代标记：
        // 标记不是当前世代 ⇒ 磁盘值按旧默认处理，直接套用新默认。
        // ⛔ 代价要认：这一迁移会**覆盖**用户在 v2 世代里手动打开过的开关。这是有意的
        // —— 世代标记的意义正是"这份配置没有留下用户的显式痕迹"，而要区分
        // "用户在 v2 里主动打开"和"v2 的默认值写盘"，现有字段里没有这个信息。
        // 代价可接受：用户去设置里再打开一次即可，那次操作会带上新的世代标记。
        if cfg.injection_defaults_version == Some(INJECTION_DEFAULTS_VERSION) {
            if let Some(v) = cfg.submit_after_paste {
                crate::text_delivery::set_submit_after_paste(v);
            }
        } else {
            crate::text_delivery::set_submit_after_paste(
                crate::text_delivery::DEFAULT_SUBMIT_AFTER_PASTE,
            );
            crate::log_buffer::info(format!(
                "文本注入默认值已更新：粘贴后回车 = {}（旧配置里的值视为旧默认，已重置；手动改一次后即按你的选择持久化）",
                crate::text_delivery::DEFAULT_SUBMIT_AFTER_PASTE
            ));
        }
        // ⌘SS 精简右键菜单
        if let Some(v) = cfg.compact_context_menu {
            self.inner.compact_context_menu.store(v, Ordering::Relaxed);
        }
    }

    /// 将当前所有配置写入磁盘（仅在已设置路径时生效）。
    fn persist(&self) {
        let path = match self.inner.config_path.lock().unwrap().as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        let mut cfg = PersistConfig {
            asr_url: Some(self.asr_url()),
            resource_id: Some(self.resource_id()),
            api_key: Some(self.api_key()),
            trigger_mode: Some(match self.mode() {
                TriggerMode::Hold => "hold".to_string(),
                TriggerMode::Call => "call".to_string(),
            }),
            mic_source: Some(match self.mic_source() {
                MicSource::Auto => "auto".to_string(),
                MicSource::Pc => "pc".to_string(),
                MicSource::Wifi => "wifi".to_string(),
                MicSource::Bluetooth => "bluetooth".to_string(),
            }),
            overlay_enabled: Some(self.overlay_enabled()),
            ptt_spec: crate::ptt_hook::ptt_key().map(|k| k.label),
            edit_ptt_spec: crate::ptt_hook::edit_ptt_key().map(|k| k.label),
            wifi_ssid: Some(self.inner.wifi_ssid.lock().unwrap().clone()),
            wifi_password: Some(self.inner.wifi_password.lock().unwrap().clone()),
            ble_enabled: Some(self.inner.ble_enabled.load(Ordering::Relaxed)),
            everything_path: Some(self.inner.everything_path.lock().unwrap().clone()),
            compact_context_menu: Some(self.inner.compact_context_menu.load(Ordering::Relaxed)),
            // 直接从真源读回，不另存一份（见 `PersistConfig` 里的说明）。
            paste_style: Some(crate::text_delivery::paste_style().as_str().to_string()),
            submit_after_paste: Some(crate::text_delivery::submit_after_paste()),
            // 写盘即意味着"这份配置已经按当前世代的默认值对齐过"，
            // 下次启动就不再迁移（用户的选择从此优先于默认值）。
            injection_defaults_version: Some(INJECTION_DEFAULTS_VERSION),
            ..Default::default()
        };
        // LLM 配置
        if let Some(ve) = self.inner.voice_edit.lock().unwrap().as_ref() {
            let llm = ve.llm_client().config();
            crate::log_buffer::info(format!(
                "持久化 LLM 配置: backend={:?} model={} key_len={} timeout={}s",
                llm.backend, llm.model, llm.api_key.len(), llm.timeout_secs
            ));
            cfg.llm_backend = Some(serde_json::to_string(&llm.backend).unwrap_or_default().trim_matches('"').to_string());
            cfg.llm_endpoint = Some(llm.endpoint);
            cfg.llm_model = Some(llm.model);
            cfg.llm_api_key = Some(llm.api_key);
            cfg.llm_timeout_secs = Some(llm.timeout_secs);
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&cfg) {
            let _ = std::fs::write(&path, json);
        }
    }

    /// 持久化全部配置（公开接口，供 lib.rs 命令调用）。
    pub fn persist_all(&self) {
        self.persist();
    }

    /// 重新从磁盘加载配置并分发到各模块（供 lib.rs 在所有模块初始化后调用）。
    pub fn reload_config(&self) {
        self.load_from_disk();
    }

    /// 设置 Wi-Fi 配置并持久化。
    pub fn set_wifi_config(&self, ssid: &str, password: &str) {
        *self.inner.wifi_ssid.lock().unwrap() = ssid.to_string();
        *self.inner.wifi_password.lock().unwrap() = password.to_string();
        self.persist();
    }

    /// 获取 Wi-Fi 配置 (ssid, password)。
    pub fn get_wifi_config(&self) -> (String, String) {
        (
            self.inner.wifi_ssid.lock().unwrap().clone(),
            self.inner.wifi_password.lock().unwrap().clone(),
        )
    }

    /// 设置蓝牙启用状态并持久化。
    pub fn set_ble_enabled(&self, enabled: bool) {
        self.inner.ble_enabled.store(enabled, Ordering::Relaxed);
        self.persist();
    }

    /// 获取蓝牙启用状态。
    pub fn ble_enabled(&self) -> bool {
        self.inner.ble_enabled.load(Ordering::Relaxed)
    }

    pub fn set_api_key(&self, key: &str) {
        *self.inner.api_key.lock().unwrap() = key.to_string();
        self.persist();
    }

    pub fn set_asr_url(&self, url: &str) {
        *self.inner.asr_url.lock().unwrap() = url.to_string();
        self.persist();
    }

    pub fn set_resource_id(&self, id: &str) {
        *self.inner.resource_id.lock().unwrap() = id.to_string();
        self.persist();
    }

    pub fn set_mode(&self, mode: TriggerMode) {
        *self.inner.mode.lock().unwrap() = mode;
        self.persist();
    }

    pub fn mode(&self) -> TriggerMode {
        *self.inner.mode.lock().unwrap()
    }

    pub fn set_mic_source(&self, source: MicSource) {
        *self.inner.mic_source.lock().unwrap() = source;
        self.persist();
    }

    pub fn mic_source(&self) -> MicSource {
        *self.inner.mic_source.lock().unwrap()
    }

    pub fn set_overlay_enabled(&self, enabled: bool) {
        self.inner.overlay_enabled.store(enabled, Ordering::Relaxed);
        self.persist();
    }

    pub fn overlay_enabled(&self) -> bool {
        self.inner.overlay_enabled.load(Ordering::Relaxed)
    }

    pub fn set_everything_path(&self, path: String) {
        *self.inner.everything_path.lock().unwrap() = path;
        self.persist();
    }

    pub fn everything_path(&self) -> String {
        self.inner.everything_path.lock().unwrap().clone()
    }

    pub fn set_compact_context_menu(&self, enabled: bool) {
        self.inner.compact_context_menu.store(enabled, Ordering::Relaxed);
        self.persist();
    }

    pub fn compact_context_menu(&self) -> bool {
        self.inner.compact_context_menu.load(Ordering::Relaxed)
    }

    /// 设置粘贴方式。**无法识别的字符串一律拒绝**（不静默回落默认值）。
    pub fn set_paste_style(&self, style: &str) -> Result<crate::text_delivery::PasteStyle, String> {
        let Some(s) = crate::text_delivery::PasteStyle::parse(style) else {
            return Err(format!(
                "无法识别的粘贴方式：{style}。有效值：auto / ctrl_v / shift_insert / ctrl_shift_v"
            ));
        };
        crate::text_delivery::set_paste_style(s);
        self.persist();
        Ok(s)
    }

    pub fn set_submit_after_paste(&self, on: bool) {
        crate::text_delivery::set_submit_after_paste(on);
        self.persist();
    }

    /// 设置键盘音频管理器（APP 启动时调用，UDP 绑定成功后传入）。
    pub fn set_keyboard_audio(&self, ka: keyboard_audio::KeyboardAudioManager) {
        *self.inner.keyboard_audio.lock().unwrap() = Some(ka);
    }

    /// 开发板 Wi-Fi 音频是否在线（最近 5 秒内收到过心跳）。
    pub fn keyboard_audio_online(&self) -> bool {
        self.inner
            .keyboard_audio
            .lock()
            .unwrap()
            .as_ref()
            .map(|k| k.is_online())
            .unwrap_or(false)
    }

    /// 停掉板子音频监听线程（**进程退出时显式调用**）。
    ///
    /// ⛔ 必须显式调用：监听线程刻意**不挂 `Drop`** —— 那个类型是 `Clone` 的，
    /// 而录音路径拿到的只是局部克隆；挂在 `Drop` 上必然导致"第一次录音结束
    /// 就把全局监听线程带走"（2026-09-21 的真实故障，见
    /// `keyboard_audio::KeyboardAudioManager::shutdown` 的注释）。
    pub fn shutdown_keyboard_audio(&self) {
        if let Some(ka) = self.inner.keyboard_audio.lock().unwrap().as_ref() {
            ka.shutdown();
        }
    }

    /// 尝试获取键盘音频管理器克隆（供监听线程使用）。
    fn keyboard_audio_clone(&self) -> Option<keyboard_audio::KeyboardAudioManager> {
        self.inner.keyboard_audio.lock().unwrap().clone()
    }

    /// 取键盘音频管理器的克隆（`pub` 版本）。
    ///
    /// Wi-Fi 链路看护线程用它走"配置 JSON 直投"这条快通道（不用插 USB）。
    /// ⚠️ 拿到的是**克隆**：它内部是 `Arc<Inner>`，克隆不会多出一个监听线程，
    /// 也不能在离开作用域时把线程带走（详见
    /// `KeyboardAudioManager::shutdown` 那段注释里记着的真实故障）。
    pub fn keyboard_audio(&self) -> Option<keyboard_audio::KeyboardAudioManager> {
        self.keyboard_audio_clone()
    }

    /// 决定这一轮录音用哪个麦克风，并**说清理由**。
    ///
    /// 返回 `(是否用板子音频, 理由)`。理由要进日志与"空结果"提示 ——
    /// 用户按着板子说话却被告知"没识别到"时，第一件要确认的事就是
    /// "声音到底是从哪个设备进来的"，而这个判断依赖心跳状态。
    ///
    /// ⛔ 别把它简化成一个 `bool`：**"从没连上过"和"刚刚掉线"是两种病**，
    /// 处置方向相反（前者查板子 Wi-Fi 与固件推流，后者查信号与供电）。
    ///
    /// ⚠️ 这里**必须**接受一个外部传入的 `ka`，不能自己去锁里取：
    /// 调用方已经取过一次克隆，两次取值之间通道状态可能变化，
    /// 那会导致"判定用 A、结论写 B"的错位。
    fn resolve_mic(
        &self,
        ka: Option<&keyboard_audio::KeyboardAudioManager>,
    ) -> (bool, String) {
        match self.mic_source() {
            MicSource::Auto => match ka {
                Some(k) if k.is_online() => (true, "板子音频在线（自动选择）".to_string()),
                Some(k) => (false, format!("板子音频离线 —— {}", k.status_note())),
                None => (false, "板子音频通道未初始化（UDP 绑定失败）".to_string()),
            },
            MicSource::Pc => (false, "配置指定：电脑麦克风".to_string()),
            MicSource::Wifi => match ka {
                Some(k) if k.is_online() => (true, "配置指定：板子音频（在线）".to_string()),
                Some(k) => (
                    false,
                    format!("配置指定板子音频，但它离线 —— {}", k.status_note()),
                ),
                None => (false, "配置指定板子音频，但通道未初始化".to_string()),
            },
            // 蓝牙音频：**固件里不存在这条路**（不是"没连上"）。
            //
            // 对固件源码核实（09-21）：`AudioTransport` 枚举只有
            // `Unavailable` 与 `WifiUdp` 两个取值，音频通路的常量
            // `kAudioTransport = "wifi_udp"`；`config_state.h` 还明确写着
            // "旧 audio_enabled/audio_source/microphone_source 只做协议校验，
            // 不进入运行态"。也就是说：**板子只有 Wi-Fi UDP 一条音频通路**，
            // 蓝牙在这台设备上只承担按键事件（HID），从不传音频。
            //
            // ⛔ 所以这里说"暂不支持"都是不够准确的 —— 准确地说是"没有这个能力"，
            // 用户不该期待"等它连上就好了"。同时**必须给出可行的替代**（电脑麦克风），
            // 否则这句话只完成了告知、没完成解决。
            MicSource::Bluetooth => (
                false,
                "开发板没有蓝牙音频通路（音频只能走 Wi-Fi UDP，蓝牙只传按键），本轮用电脑麦克风；\
                 要脱离电脑麦克风请走 Wi-Fi：把板子接到同一网络"
                    .to_string(),
            ),
        }
    }

    /// 设置语音编辑管理器（APP 启动时调用）。
    pub fn set_voice_edit(&self, ve: crate::voice_edit::VoiceEditManager) {
        *self.inner.voice_edit.lock().unwrap() = Some(ve);
    }

    /// 当前编辑阶段。
    pub fn edit_phase(&self) -> EditPhase {
        *self.inner.edit_phase.lock().unwrap()
    }

    /// 编辑 PTT 键**按下**（KEY3 的 `edit_ptt_hold` 发 HidKeyDown）。
    ///
    /// **三态、按一下推一格**，与 KEY1 是同一套"短按"规则（松开不参与任何判断）：
    ///
    /// | 当前 | 这一按做什么 |
    /// |---|---|
    /// | `Idle` | 开始聆听（字幕实时上屏） |
    /// | `Listening` | 结束聆听 → 识别 → LLM 改写 → 替换选中文本 |
    /// | `Processing` | 什么都不做（改写已经在跑，重复触发会识别两次） |
    ///
    /// ⛔ 编辑链路**不认回车**：它的结果是"把选中文本换掉"这个替换动作，
    /// 没有"发送"这一步（判据见 `enter_grabs`）。
    pub fn edit_ptt_down(&self, app: tauri::AppHandle) {
        // 编辑键同样不能"抢"别人正在录的那段。校验放在拿 `edit_phase` 锁**之前**：
        // 拒绝路径会 emit 事件，持着锁往里走是死锁的经典来源。
        if let Some(owner) = press_conflict(PttOrigin::Edit, self.recording_origin()) {
            self.reject_foreign_press(&app, PttOrigin::Edit, owner);
            return;
        }
        let mut phase = self.inner.edit_phase.lock().unwrap();
        match *phase {
            EditPhase::Idle => {
                *phase = EditPhase::Listening;
                drop(phase);
                self.start_edit_listening(app, PttOrigin::Edit);
            }
            EditPhase::Listening => {
                *phase = EditPhase::Processing;
                // 先放锁再调下游：持锁往里走是死锁的经典来源（同上面的 `drop`）。
                drop(phase);
                self.stop_edit_listening(app);
            }
            EditPhase::Processing => {}
        }
    }

    pub fn api_key(&self) -> String {
        self.inner.api_key.lock().unwrap().clone()
    }

    pub fn asr_url(&self) -> String {
        self.inner.asr_url.lock().unwrap().clone()
    }

    pub fn resource_id(&self) -> String {
        self.inner.resource_id.lock().unwrap().clone()
    }

    fn has_active(&self) -> bool {
        self.inner.current.lock().unwrap().is_some()
    }

    /// 当前是否正在录音。
    ///
    /// 公开是因为**两个调用方都要靠它决定"这一按是开始还是结束"**：任务台槽位键
    /// （固件只在按下时回发一条报文，没有松开，所以"按一下开、再按一下关"必须由
    /// 宿主自己记状态），以及识别收尾期间对外报状态。
    /// （⚠️ KEY1 / KEY3 也都是"按下决定"了，但它们的判断就在 `ptt_press` /
    /// `edit_ptt_down` 内部，不经过这里。）
    pub fn is_listening(&self) -> bool {
        self.has_active()
    }

    /// 标记"接下来这段录音是派给槽位 `slot` 的"（1-based）。
    pub fn set_desk_intent(&self, slot: Option<u8>) {
        *self.inner.desk_intent.lock().unwrap() = slot;
    }

    /// 认领待归属槽位（**取走即消失**，见 `desk_intent` 的说明）。
    fn take_desk_intent(&self) -> Option<u8> {
        self.inner.desk_intent.lock().unwrap().take()
    }

    /// 记下"当前这段录音是派给谁的"，仅供提示。
    fn set_recording_desk_slot(&self, slot: Option<u8>) {
        *self.inner.recording_desk_slot.lock().unwrap() = slot;
    }

    /// 当前这段录音派给哪个槽（给界面说清"你按的键和正在录的那段不是一回事"）。
    pub fn current_desk_slot(&self) -> Option<u8> {
        *self.inner.recording_desk_slot.lock().unwrap()
    }

    /// 当前是否存在"注入失败、等重试"的一句话。
    ///
    /// 界面挂载时用它对齐（事件是瞬时的，面板挂载前发出的那条收不到）。
    pub fn failed_inject_text(&self) -> Option<String> {
        self.inner.failed_inject.lock().unwrap().clone()
    }

    /// 取走失败的那一句（**取走即消失**，与 `desk_intent` 同理）。
    fn take_failed_inject(&self) -> Option<String> {
        self.inner.failed_inject.lock().unwrap().take()
    }

    /// 显式丢弃失败的那一句，返回被丢弃的内容（供日志与提示）。
    pub fn discard_failed_inject(&self) -> Option<String> {
        self.inner.failed_inject.lock().unwrap().take()
    }

    /// 暂存一句注入失败的内容。
    ///
    /// ⛔ 只在**注入失败**时调用。正常路径碰它一次，就说明"注入"这件事正在
    /// 被当成一个需要人工确认的站点 —— 那正是 v0.29.0 那个 `pending_inject`
    /// 的毛病（多按一次）。它的返回值是被顶掉的上一条，调用方负责记日志。
    fn stage_failed_inject(&self, text: String) -> Option<String> {
        self.inner.failed_inject.lock().unwrap().replace(text)
    }

    /// 回车此刻算不算「注入并发送」。
    ///
    /// ⛔ 这是**钩子层判据的真相源**（见 `ptt_hook::set_enter_gate`）：钩子不存副本，
    /// 就是为了不让这个状态在两处各有一份、然后其中一份老化成"把所有回车都吞掉"。
    ///
    /// ⛔ 必须**无锁、无阻塞**：它在键盘钩子回调里被调用。
    /// 这里只读一个 `AtomicBool`，正是为了满足这一点。
    pub fn grabs_enter(&self) -> bool {
        self.inner.voice_delivery_pending.load(Ordering::SeqCst)
    }

    /// 记下"这次交付要回车"（回车键按下的那一刻置位）。
    fn request_submit(&self) {
        self.inner.submit_on_delivery.store(true, Ordering::SeqCst);
    }

    /// 本次交付要不要回车。**每段会话开始时被 `register` 清零**
    /// —— 那条清零不是可选项：漏掉它，"要发送"会跟着下一段录音走。
    fn submit_wanted(&self) -> bool {
        self.inner.submit_on_delivery.load(Ordering::SeqCst)
    }

    /// 把一句话注入焦点窗口（`submit == true` 时额外按一次回车）。
    ///
    /// # 这是语音链路的**唯一注入出口**
    ///
    /// 两条路（"再按一次语音键"和"录音期间按回车"）在这里汇合，差别只有 `submit`
    /// 一个布尔值 —— 出口只有一个，是为了让"注入失败怎么办"这个问题只需回答一次。
    ///
    /// ⛔ **必须是 async，且阻塞部分必须走 `spawn_blocking`**：一次注入要
    /// 1.2~1.8s（剪贴板 + 等按键落地），而调用它的那段跑在 Tauri 的异步运行时上。
    /// 在里面直接同步阻塞，钉住的是整个后端运行时的线程（同类坑见
    /// `cli_terminal.rs` 里对 `launch()` 必须 spawn 的说明）。
    ///
    /// # 失败必须留下内容
    ///
    /// 识别已经成功了，只是"送进输入框"这一步没成 —— 此时把整句话扔掉，等于让用户
    /// 重新说一遍。所以失败一律留底，并在界面上给一个「重新注入」。
    /// **已识别的话不能因为收尾出错而作废**（同 `asr.rs` 那条）。
    async fn inject(&self, app: tauri::AppHandle, text: String, submit: bool) -> Result<(), String> {
        // 先让用户看到识别结果（浮层与面板都认这个事件），再做注入。
        // 顺序反过来的话，注入失败时用户连"识别成了什么"都看不到。
        let _ = app.emit("asr-final", &text);
        crate::log_buffer::info(format!(
            "语音：注入 {} 字（{}）",
            text.chars().count(),
            if submit { "并回车" } else { "不回车" }
        ));
        let payload = text.clone();
        let done = tauri::async_runtime::spawn_blocking(move || {
            if submit {
                text_delivery::deliver_and_submit(&payload)
            } else {
                text_delivery::deliver_no_submit(&payload)
            }
        })
        .await;
        match done {
            Ok(Ok(())) => {
                // ★ `voice-injected` = **"这句话真的进了输入框"**，一条独立的事实。
                //
                // ⛔ 不能拿 `asr-final` 当这个证据：它在**注入之前**就发了（见上），
                //    那一刻这句话还没进任何输入框。界面若据此显示"最近上屏：xxx"，
                //    就是在替用户宣布一件还没发生的事 —— 而"进没进输入框"恰恰是
                //    用户最需要看准的一件事。宁可多发一个事件，也不要靠推断。
                let _ = app.emit("voice-injected", &text);
                let _ = app.emit(
                    "asr-state",
                    if submit {
                        "已注入输入框并发送"
                    } else {
                        "已注入输入框（未发送；录音时按回车可直接发送）"
                    },
                );
                crate::usage_log::record("asr", &text, "keyboard", None);
                hide_overlay(&app);
                Ok(())
            }
            Ok(Err(e)) => {
                // ⛔ 注入报错（粘贴键没送到 / 剪贴板拿不到等）。
                crate::log_buffer::err(format!("语音：注入失败 {e}（内容已留底，可重新注入）"));
                self.keep_failed(&app, &text, &e, "注入失败");
                Err(e)
            }
            Err(e) => {
                // ⛔ 这一支比上面那支更隐蔽：它不是"注入报错"，而是**注入任务根本没跑完**
                //    （执行线程 panic / 运行时提前关闭）。对用户而言两者没有区别
                //    （"我说的那句话没进去"），所以必须做完全一样的事：留底 + 出声。
                //    漏掉它，就等于在异常路径上留了一条静默丢字的小路 ——
                //    而异常路径从来没人测。
                crate::log_buffer::err(format!("语音：注入任务异常终止 {e}（内容已留底）"));
                self.keep_failed(&app, &text, &e.to_string(), "注入没能完成");
                Err(format!("注入任务失败: {e}"))
            }
        }
    }

    /// 把一句注入失败的内容留底，并告诉用户下一步该怎么办（两条失败路径共用）。
    ///
    /// 分开写是为了让"失败"这件事在代码里只出现一次：两条分支各写一遍的话，
    /// 迟早有一条会被改成只留底不出声、或者只出声不留底。
    fn keep_failed(&self, app: &tauri::AppHandle, text: &str, reason: &str, what: &str) {
        if let Some(prev) = self.stage_failed_inject(text.to_string()) {
            // 连上一次失败的都还没重试成 —— 两条都得说。静默覆盖就是净丢一句话。
            crate::log_buffer::warn(format!(
                "语音：上一条注入失败的内容（{} 字）被本次的覆盖",
                prev.chars().count()
            ));
        }
        // 事件带上**内容**：面板拿到它就能直接列出"重新注入"那一行，
        // 不用再回头读一次命令（少一次往返，也少一处"事件到了但状态还没写好"的窗口）。
        let _ = app.emit("voice-failed-inject", serde_json::json!({ "text": text }));
        let _ = app.emit(
            "voice-alert",
            serde_json::json!({
                "message": format!(
                    "{what}：{reason}。内容已保留（{} 字），可在语音面板点「重新注入」。",
                    text.chars().count()
                ),
                "source": "session",
            }),
        );
    }

    /// 重新注入上一次失败的内容（界面上的「重新注入」按钮）。
    ///
    /// 与正常交付的**唯一**区别：内容来自失败留底，而不是刚识别出来的结果。
    /// 成功才真正清掉留底 —— 失败时 `inject` 会自己再放回去，
    /// 所以"重试又失败 ⇒ 内容消失"这条路是堵死的。
    pub async fn retry_failed_inject(&self, app: tauri::AppHandle) -> Result<String, String> {
        let Some(text) = self.take_failed_inject() else {
            return Err("没有待重新注入的内容".to_string());
        };
        crate::log_buffer::info(format!(
            "语音：重新注入上次失败的内容 {} 字",
            text.chars().count()
        ));
        // ⛔ 重试**不回车**：用户点的是"重新注入"，不是"重新发送"。
        // 要发送他自己会按回车 —— "提交权归用户"不因为一次重试而放宽。
        self.inject(app, text.clone(), false).await?;
        Ok(text)
    }

    fn register(&self, stop_flag: Arc<AtomicBool>, origin: PttOrigin) -> u64 {
        let gen = self.inner.generation.fetch_add(1, Ordering::SeqCst) + 1;
        // ★ 每段会话开始时把"交付意图"复位。**这一步不是可选的**：
        //   漏掉它，上一次"回车 ⇒ 要发送"的意图会跟着下一段录音走，
        //   表现是"我明明只按了语音键，它却自己发出去了" —— 不可撤销的脏状态。
        self.inner.submit_on_delivery.store(false, Ordering::SeqCst);
        self.inner.voice_delivery_pending.store(false, Ordering::SeqCst);
        *self.inner.current.lock().unwrap() = Some(Arc::new(Active {
            stop_flag,
            gen,
            origin,
            started_at: Instant::now(),
        }));
        gen
    }

    /// 当前这段录音是谁发起的；`None` = 没有录音在跑。
    ///
    /// 与 `is_listening()` 读的是**同一个锁、同一个 `Option`**，所以两者不可能漂移：
    /// "正在录音"和"知道是谁开的"在结构上就是同一件事。
    pub fn recording_origin(&self) -> Option<PttOrigin> {
        self.inner
            .current
            .lock()
            .unwrap()
            .as_ref()
            .map(|a| a.origin)
    }

    /// 会话**硬上限**：超过它还没收尾，一律认为卡死。
    ///
    /// 取"正常上限 + 余量"而不是写死一个数：Hold 的正常上限就是 `MAX_DURATION`，
    /// 通话是 `MAX_CALL_DURATION`，两者差一个数量级，用同一个常数必然误杀长通话。
    fn session_hard_limit(&self) -> Duration {
        match self.mode() {
            TriggerMode::Hold => MAX_DURATION + Duration::from_secs(120),
            TriggerMode::Call => MAX_CALL_DURATION + Duration::from_secs(180),
        }
    }

    /// 若当前会话已经卡死（超过硬上限），**强行释放**并返回它的发起者；否则返回 `None`。
    ///
    /// # 为什么需要这道兜底
    ///
    /// 会话槽的清空点在识别任务**返回之后**（`finish`）。所以只要那条异步链路里
    /// 有一处永久等待（网络 IO 全都有可能），槽位就永远被认为是"有人在录"：
    /// 同一个入口再按只是去"结束"一段永远不会结束的会话（**按了没反应**），
    /// 别的入口会被归属校验拒绝 —— 整台键盘的语音功能一起死，而且一个字都不报。
    ///
    /// 上游当然也要加超时（`asr.rs` 的「防砖超时」），但那是"让正常路径不卡"；
    /// 这里是**最后一道**：即便将来又冒出一个没想到的等待点，用户最多等几分钟，
    /// 而不是重启应用。宁可错杀一段超长录音，也不能让键盘变砖。
    ///
    /// 不碰 `AppHandle`（emit 由调用方做）、上限也由调用方给 ⇒ 这段判据可以被单测
    /// 直接钉死，不必去和真实时钟较劲（测试里传 `Duration::ZERO` 就能造出"已卡死"）。
    fn reclaim_stale_session(&self, limit: Duration) -> Option<PttOrigin> {
        let owner = {
            let mut cur = self.inner.current.lock().unwrap();
            let stale = matches!(cur.as_ref(), Some(a) if a.started_at.elapsed() >= limit);
            if !stale {
                return None;
            }
            let owner = cur.as_ref().map(|a| a.origin);
            *cur = None;
            owner
        };
        // 锁不嵌套：与 `finish` 保持同样的加锁顺序（先 current、再 recording_desk_slot），
        // 两处顺序相反就是死锁的经典来源。
        self.set_recording_desk_slot(None);
        // ⛔ 强制释放时必须**同时解除回车接管**。这一段会话永远不会交付了，
        // 所以没有任何一处会在"交付完成"时把它清掉 —— 留着它，用户后面每一个
        // 回车都会被吞进一个不存在的会话里（表现为"回车键失灵"）。
        self.inner.voice_delivery_pending.store(false, Ordering::SeqCst);
        owner
    }

    fn finish(&self, gen: u64) {
        let mut cur = self.inner.current.lock().unwrap();
        if cur.as_ref().map(|a| a.gen) == Some(gen) {
            *cur = None;
            // 会话结束 ⇒ 提示用的归属作废。派发依据不在这里（是 start_* 认领后
            // 传下去的局部值），所以这里清空**不会影响**本次识别结果的去向。
            self.set_recording_desk_slot(None);
        }
    }

    /// 置位当前会话的停止标志（若有）。
    fn stop_active(&self) {
        if let Some(a) = self.inner.current.lock().unwrap().as_ref() {
            a.stop_flag.store(true, Ordering::Relaxed);
        }
        // ⛔ 这里**刻意不清归属**。早期版本在这里清空，结果 Hold 模式第二次按键时
        // 归属先被清掉，识别结果回来只能走上屏 —— 任务台永远收不到一句话。
        // 归属的正确失效点是"会话结束"（`finish`），不是"用户按下停止"。
    }

    /// 这一段录音是否**已经收到停止指令**（`stop_flag` 已置位、识别在途）。
    ///
    /// 用途只有一个：把"还在录"和"录制已结束、结果还没回来"分开。
    /// 两者对同一次按键的反应不同（前者是"我来结束"，后者是"请稍候"），
    /// 分不清就只能静默 —— 而"按了没反应"正是最该避免的那种体感。
    fn stop_requested(&self) -> bool {
        self.inner
            .current
            .lock()
            .unwrap()
            .as_ref()
            .map(|a| a.stop_flag.load(Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// 主 PTT 键**按下**（KEY1 发 HidKeyDown；面板 ● 按钮也走这里）。
    ///
    /// **按下即动作，两种语义由当前状态决定：**
    ///
    /// - 手上没有正在录的会话 ⇒ **开始录音**（Hold / Call 都是）。
    /// - 正在录 ⇒ **结束录音，并把这一句交出去**。
    ///
    /// ⛔ **没有第三种语义。** v0.29.0 曾在这里插过一个"确认注入"的分支（要按第三下
    /// 才把内容送进输入框），结果同一件事要按三下，用户明确否掉了。
    /// "结束"那一按就该把话交出去 —— 但只**注入、不发送**，发送权留给用户
    /// （录音期间按回车，或自己在输入框里按回车）。
    ///
    /// # 面板 ● 按钮与 KEY1 完全等价
    ///
    /// v0.29.0 曾给两者分叉（按钮不消费"待确认"），理由是"按钮旁边摆着显式的注入按钮，
    /// 它不该替用户确认"。那套理由随两段式一起消失了：现在两者都是
    /// "按一下开始、再按一下结束并注入"，不存在任何分歧。
    /// ⛔ **不要**再为它们造第二条路径 —— 两条路径本身就是下一起
    /// "两个入口行为不一致"的来源。
    pub fn ptt_down(&self, app: tauri::AppHandle) {
        let _ = self.ptt_down_from(app, PttOrigin::Voice);
    }

    /// 带**发起者**的 PTT 按下。所有入口都必须走这里，不许直接操作录音状态。
    ///
    /// 进入时先做归属校验（见 `PttOrigin` 的长注释）：当前有录音在跑、而它不是自己开的
    /// ⇒ **拒绝**。这一步就是"按键冲突"这个病的药 —— 不拒绝的话，按 KEY1 语音输入会把
    /// 任务台正在录的那段结束掉并派出去执行命令，而用户根本不知道。
    ///
    /// 返回**这一按是否真的起了作用**：调用方（如任务台按键）要靠它决定"该不该接着
    /// 播报『开始录音 / 录音结束』"—— 被拒了还说"已开始"，用户会以为自己在录音。
    pub fn ptt_down_from(&self, app: tauri::AppHandle, origin: PttOrigin) -> bool {
        self.ptt_press(app, origin)
    }

    /// PTT 按下的真正实现。这一层只管**执行**。
    ///
    /// "该不该拒绝"的判据在纯函数 `press_conflict` 里 —— 本函数要 `AppHandle`，
    /// 测试里构造不出来；按项目铁律，要 `AppHandle` 的逻辑测不了，
    /// **纯判据就必须抽出去单独钉死**。
    fn ptt_press(&self, app: tauri::AppHandle, origin: PttOrigin) -> bool {
        // 先清障：上一段如果已经卡死，它会永久占着会话槽 —— 于是**所有**入口一起失效
        // （自己那个入口"按了没反应"，别的入口被归属校验拒绝）。这里强行释放，
        // 让这一按按"开始"正常走；同时把话说清楚，因为用户此刻一定会以为是自己按错了。
        if let Some(stuck) = self.reclaim_stale_session(self.session_hard_limit()) {
            alert_voice(
                &app,
                &format!(
                    "上一段录音（{}开的）卡住了超过 {} 秒没有收尾，已强制释放；\
                     本次按键按「开始」处理。若反复出现，请把日志发给开发者。",
                    stuck.label(),
                    self.session_hard_limit().as_secs()
                ),
            );
        }
        if let Some(owner) = press_conflict(origin, self.recording_origin()) {
            self.reject_foreign_press(&app, origin, owner);
            return false;
        }

        // 每一次"入口按下"都记一笔：这是整条语音链路里**唯一的状态机入口**，
        // 事后复盘"是不是按错了键""谁把谁的录音截断了"全靠它。
        let starting = !self.has_active();
        crate::log_buffer::info(format!(
            "语音：{} 按下 → {}｜模式={}",
            origin.label(),
            if starting { "开始录音" } else { "结束录音" },
            match self.mode() {
                TriggerMode::Hold => "短按单句(hold)",
                TriggerMode::Call => "通话(call)",
            }
        ));
        match self.mode() {
            TriggerMode::Hold => {
                if starting {
                    self.start_hold(app, origin);
                } else if self.stop_requested() {
                    // ⛔ 别静默：录制已经结束、识别结果还在路上（约 1~2 秒的真空）。
                    //    用户这一按**确实**不改变任何状态（`stop_flag` 早就是 true），
                    //    不说一声他不会知道"该等"，只会觉得按键失灵。
                    crate::log_buffer::info("语音：识别仍在收尾，本次按下不改变任何状态");
                    let _ = app.emit("asr-state", "上一句还在识别中，请稍候…");
                } else {
                    self.stop_active();
                }
            }
            TriggerMode::Call => {
                // 通话：按一下开、再按一下挂断。
                if starting {
                    self.start_call(app, origin);
                } else {
                    self.stop_active();
                }
            }
        }
        true
    }

    /// **回车键：一次按键完成「结束录音 + 注入 + 发送」。**
    ///
    /// 它是"再按一次语音键"的**快捷替代**，不是流程里的第三个阶段 ——
    /// 两条路的差别只有一个布尔值（要不要回车），别在这里再加状态。
    ///
    /// # 到达这里时未必还在录音
    ///
    /// 钩子把回车报上来时，会话可能处于两种状态：
    ///
    /// 1. **还在录**：用户说到一半直接按回车 —— 结束 + 注入 + 发送。
    /// 2. **录制已结束、识别结果在路上**：他先按了语音键，又顺手补一下回车。
    ///
    /// 两种都要认，否则第 2 种情况下按回车等于按了个寂寞。所以这里统一处理成
    /// "记下要发送 + 结束（若还在录）"—— 两件事都是幂等的。
    ///
    /// # 判据竞态的兜底
    ///
    /// 钩子先按 [`SessionManager::grabs_enter`] 决定吞不吞这个键，会话再判一次。
    /// 两者之间隔着一次线程投递，判据**可能在这期间变化**（恰好那几十毫秒里交付完了）。
    /// 那时键已经被吞掉，直接返回就等于**净吃用户一次回车**，现象是"按了没反应"。
    /// 所以这里补发一次真实回车，把这个键还回它本来的语义。
    pub fn enter_submit(&self, app: tauri::AppHandle) -> bool {
        if !self.grabs_enter() {
            crate::log_buffer::warn(
                "回车：会话在键被吞掉之后才收尾，改为补发一次普通回车（判据竞态，非故障）",
            );
            tauri::async_runtime::spawn_blocking(text_delivery::press_enter);
            return false;
        }
        self.request_submit();
        if self.has_active() && !self.stop_requested() {
            crate::log_buffer::info("回车 → 结束录音，本次交付：注入并发送");
            self.stop_active();
            let _ = app.emit("asr-state", "正在识别，稍后自动注入并发送…");
        } else {
            // 录制已结束、结果还在路上：把这次交付升级成"发送"。
            crate::log_buffer::info("回车 → 识别在途，本次交付改为：注入并发送");
            let _ = app.emit("asr-state", "已确认发送：识别完成即注入并回车");
        }
        true
    }

    /// 拒绝一次"不是自己发起的"按下。
    ///
    /// ⛔ 拒绝是**静默失败的反面**：既不改归属、也不 toggle（否则等于替用户做了他没下的
    /// 决定），同时把话说清楚 —— 谁在录、该按哪个键收尾。
    /// 只发字幕浮层不够：任务台槽位键被拒时，用户多半正盯着任务台页面等结果，
    /// 所以还要把提示送到那里。
    fn reject_foreign_press(&self, app: &tauri::AppHandle, pressed: PttOrigin, owner: PttOrigin) {
        use tauri::Emitter;
        let msg = format!(
            "{}按下无效：现在录着的这段是{}开的，还没结束。先按一下{}收尾，再按{}。",
            pressed.label(),
            owner.label(),
            owner.label(),
            pressed.label()
        );
        // 走统一告警出口 ⇒ 主窗口也会看到（过去只发浮层，而用户被拒时浮层恰好不在，
        // 于是"按了没反应"没有任何解释）。
        alert_voice(app, &msg);
        if let PttOrigin::DeskSlot(slot) = pressed {
            let _ = app.emit(
                "desk-slot-key",
                serde_json::json!({
                    "slot": slot,
                    "message": msg,
                    "dispatched": false,
                }),
            );
        }
    }

    /// Hold：**短按单句** —— 按一下开始录音，再按一次结束并把这一句交出去。
    ///
    /// 触发与结束都是"按下"（`ptt_down`）；松开不参与任何判断（见 `PttEvent`）。
    /// 交付方式在"结束那一下"决定：语音键 = 只注入；回车 = 注入并发送。
    fn start_hold(&self, app: tauri::AppHandle, origin: PttOrigin) {
        let api_key = self.api_key();
        let asr_url = self.asr_url();
        let resource_id = self.resource_id();
        if api_key.trim().is_empty() {
            let _ = app.emit("asr-state", "请先在设置里填写 API Key");
            return;
        }

        let stop_flag = Arc::new(AtomicBool::new(false));
        let gen = self.register(stop_flag.clone(), origin);

        // 认领归属：**这一次录音**是派给哪个槽的。认领后它就是这段会话的局部值，
        // 一路传到 `deliver_hold_result`，不再回头读全局 —— 所以无论用户什么时候按停止、
        // 什么时候超时，这句话的去向在按下"开始"那一刻就已经定了。
        let desk_slot = self.take_desk_intent();
        self.set_recording_desk_slot(desk_slot);

        // ★ 武装「回车 = 注入并发送」。
        //
        // 条件 = 这一轮的话**会进输入框**（不是派给任务台、也不是编辑改写）。
        // 判据是纯函数 `enter_grabs` —— 它判错一次的后果是"把用户正常的回车全吞掉"
        // （现象像键盘坏了），所以必须能被单测钉死，而不是揉进这行 if 里。
        //
        // ⛔ 解除点只有两个：识别任务末尾（交付意图冻结那一刻），以及
        //    `reclaim_stale_session` 那条强制释放的兜底路。**别处不许清** ——
        //    清早了"录音中按回车"就失效，清漏了回车被永久吞掉。
        self.inner
            .voice_delivery_pending
            .store(enter_grabs(origin, desk_slot), Ordering::SeqCst);

        let (audio_tx, audio_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);

        let ka = self.keyboard_audio_clone();
        let (use_keyboard, mic_reason) = self.resolve_mic(ka.as_ref());

        if use_keyboard {
            let ka = ka.unwrap();
            ka.start_streaming(gen as u64, audio_tx);
            let flag_watch = stop_flag.clone();
            std::thread::spawn(move || {
                let started = Instant::now();
                while !flag_watch.load(Ordering::Relaxed) && started.elapsed() < MAX_DURATION {
                    std::thread::sleep(Duration::from_millis(50));
                }
                ka.stop_streaming();
            });
        } else {
            let flag_rec = stop_flag.clone();
            let app_rec = app.clone();
            std::thread::spawn(move || {
                let capture = match audio::AudioCapture::start(audio_tx) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = app_rec.emit("asr-state", format!("录音失败: {e}"));
                        return;
                    }
                };
                let started = Instant::now();
                while !flag_rec.load(Ordering::Relaxed) && started.elapsed() < MAX_DURATION {
                    std::thread::sleep(Duration::from_millis(50));
                }
                drop(capture);
            });
        }

        // 录音源只在这一个地方定下来，且**记进日志**：事后排查"没收到音频"时，
        // 第一个要回答的问题就是"这一轮到底用的哪个麦克风"。
        // ⛔ 光记"用了哪个"还不够 —— **为什么没用另一个**才是关键：
        // 板子没连 Wi-Fi？刚掉线？还是用户自己配了电脑麦克风？三者处置完全不同。
        let source = mic_label(use_keyboard);
        crate::log_buffer::info(format!("录音源：{source}｜{mic_reason}"));

        let app_asr = app.clone();
        let session = self.clone();
        tauri::async_runtime::spawn(async move {
            let _ = app_asr.emit("asr-state", "正在听，请说话…");
            if session.overlay_enabled() {
                show_overlay(&app_asr);
            }
            let result = asr::run_recognition(
                app_asr.clone(),
                api_key,
                asr_url,
                resource_id,
                audio_rx,
                stop_flag.clone(),
            )
            .await;
            stop_flag.store(true, Ordering::Relaxed);
            session.finish(gen);

            // ★ 交付意图在此**冻结**，同时**解除回车接管**。
            //
            //   - 冻结：这一刻之后再按回车也改不了这一句的去向（识别结果马上就交付了）。
            //   - 解除：从此那一下回车**透传给前台窗口** —— 内容紧接着就会出现在输入框里，
            //     回车正好把它发出去，语义反而是对的；继续吞掉它才是错的（吞了没人接）。
            //
            // ⛔ 必须放在**所有分支之前**：下面任何一条提前走掉的分支（空结果、识别失败）
            //    若漏掉解除动作，回车就被永久接管 —— 用户此后每个回车都被吞掉，
            //    现象与"键盘坏了"一模一样。
            let submit = session.submit_wanted();
            session.inner.voice_delivery_pending.store(false, Ordering::SeqCst);

            // 字幕浮层的收尾权按路径分流：任务台那条路要**多留一会儿**（用户只有
            // 这一个地方能看见自己说了什么），所以由 `deliver_hold_result` 在延时后
            // 自己收，并用返回值告诉这里"别再收一次"。其余几条路走原来的兜底。
            let mut subtitle_handled = false;

            match result {
                // ⛔ 空结果**必须说清是哪一环断的**。过去一律显示"未识别到内容"，
                // 于是"麦克风一个字节都没采到""音频上去了服务端一声不吭""回了但没文字"
                // 三种完全不同、排查方向也完全不同的故障长得一模一样 —— 判断成本全转给用户。
                Ok(out) if out.text.trim().is_empty() => {
                    // 提示里带上"为什么选这个麦克风"：用户按着板子说话却收不到声音时，
                    // 这一句就是他唯一能自己判断的线索。
                    let msg = asr::explain_empty(&out, &format!("{source}；{mic_reason}"));
                    crate::log_buffer::warn(format!("语音：空结果 · {msg}"));
                    alert_voice(&app_asr, &msg);
                    // 任务台那边也要说：用户按的是槽位键，他正盯着任务台等结果。
                    if let Some(slot) = desk_slot {
                        let _ = app_asr.emit(
                            "desk-slot-key",
                            serde_json::json!({
                                "slot": slot,
                                "message": format!("S{slot} 没识别到内容：{msg}"),
                                "dispatched": false,
                            }),
                        );
                    }
                }
                Ok(out) => {
                    // 收尾阶段的服务端错误：文本照常交付，但**写进日志不瞒着**。
                    // 这里刻意不弹窗 —— 用户的话已经成功上屏/派发了，再弹一个他自己
                    // 无法处理的告警只会制造噪音；而日志（现在已落盘）才是该看的地方。
                    if let Some(e) = out.server_error.as_deref() {
                        crate::log_buffer::warn(format!(
                            "语音：文本已交付，但服务端在收尾时报过错 —— {e}"
                        ));
                    }
                    crate::log_buffer::info(format!(
                        "语音：识别成功 {} 字｜音源 {}｜上行 {} 帧 / {} KB｜回包 {} 个",
                        out.text.chars().count(),
                        source,
                        out.audio_frames,
                        out.audio_bytes / 1024,
                        out.server_messages
                    ));
                    subtitle_handled = session
                        .deliver_hold_result(app_asr.clone(), out.text, desk_slot, submit)
                        .await;
                }
                Err(e) => {
                    crate::log_buffer::err(format!("语音：识别失败 {e}"));
                    alert_voice(&app_asr, &format!("识别失败: {e}"));
                }
            }
            // 交付完成（或说清了为什么没有东西可交付）⇒ 浮层可以收了。
            // 注入成功时 `inject` 里已经收过一次，这里是给空结果 / 识别失败那几条路兜底。
            // ⛔ 任务台那条路**自己收**（`subtitle_handled`）—— 在这里再收一次，
            //    等于把"让用户看清识别结果"这个停留当场抹掉，字幕又变回一闪而过。
            if !subtitle_handled {
                hide_overlay(&app_asr);
            }
        });
    }

    /// Call：持续通话，每句注入，再按一次挂断。
    fn start_call(&self, app: tauri::AppHandle, origin: PttOrigin) {
        let api_key = self.api_key();
        let asr_url = self.asr_url();
        let resource_id = self.resource_id();
        if api_key.trim().is_empty() {
            let _ = app.emit("asr-state", "请先在设置里填写 API Key");
            return;
        }

        let stop_flag = Arc::new(AtomicBool::new(false));
        let gen = self.register(stop_flag.clone(), origin);

        let (audio_tx, audio_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
        // 定稿句队列：识别任务产出 → 注入 worker 串行上屏。
        let (inj_tx, inj_rx) = tokio::sync::mpsc::channel::<String>(16);

        // 同 `start_hold`：归属在会话开始时认领一次，之后作为局部值传给注入 worker。
        // Call 模式一段通话会产出**多句**，全部派给同一个槽 —— 这个局部值保证了
        // 不会出现"前半句进任务台、后半句粘进当前窗口"。
        let desk_slot = self.take_desk_intent();
        self.set_recording_desk_slot(desk_slot);

        let ka = self.keyboard_audio_clone();
        let (use_keyboard, mic_reason) = self.resolve_mic(ka.as_ref());
        crate::log_buffer::info(format!(
            "录音源（通话）：{}｜{mic_reason}",
            mic_label(use_keyboard)
        ));

        if use_keyboard {
            let ka = ka.unwrap();
            ka.start_streaming(gen as u64, audio_tx);
            let flag_watch = stop_flag.clone();
            std::thread::spawn(move || {
                let started = Instant::now();
                while !flag_watch.load(Ordering::Relaxed) && started.elapsed() < MAX_CALL_DURATION {
                    std::thread::sleep(Duration::from_millis(50));
                }
                ka.stop_streaming();
            });
        } else {
            let flag_rec = stop_flag.clone();
            let app_rec = app.clone();
            std::thread::spawn(move || {
                let capture = match audio::AudioCapture::start(audio_tx) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = app_rec.emit("asr-state", format!("录音失败: {e}"));
                        return;
                    }
                };
                let started = Instant::now();
                while !flag_rec.load(Ordering::Relaxed) && started.elapsed() < MAX_CALL_DURATION {
                    std::thread::sleep(Duration::from_millis(50));
                }
                drop(capture);
            });
        }

        // 注入 worker：串行消费定稿句，逐句上屏。
        let app_inj = app.clone();
        let session_inj = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut rx = inj_rx;
            while let Some(text) = rx.recv().await {
                // 这段通话的归属是**开始时定好的局部值**：每一句都派给它，不读全局，
                // 所以"用户按下停止"不会让后半截句子改走上屏。
                if let Some(slot) = desk_slot {
                    let t = text.trim().to_string();
                    if !t.is_empty() {
                        // 与 Hold 路径同一个坑（见 `desk_subtitle_events`）：通话里派给
                        // 任务台的每一句，过去也只发状态、不发正文，字幕因此一直是空的。
                        // 通话期间浮层本来就一直开着（只有挂断才收），所以不需要停留，
                        // 但**状态行要回到"通话中"** —— 否则它会停在"识别完成"上，
                        // 用户以为通话已经结束了。
                        for (name, payload) in desk_subtitle_events(slot, &t) {
                            let _ = app_inj.emit(name, payload);
                        }
                        session_inj.dispatch_to_desk(&app_inj, slot, &t).await;
                        let _ = app_inj.emit("asr-state", format!("通话中：已派给 S{slot}，继续说…"));
                        continue;
                    }
                }
                let app2 = app_inj.clone();
                let text2 = text.clone();
                let injected =
                    tauri::async_runtime::spawn_blocking(move || text_delivery::deliver(&text2))
                        .await;
                match injected {
                    Ok(Ok(())) => {
                        let _ = app2.emit("asr-final", &text);
                        let _ = app2.emit("asr-state", "通话中：自动断句上屏…");
                    }
                    Ok(Err(e)) => {
                        let _ = app2.emit("asr-state", format!("上屏失败: {e}"));
                    }
                    Err(e) => {
                        let _ = app2.emit("asr-state", format!("上屏任务失败: {e}"));
                    }
                }
            }
        });

        // 识别任务：持续流式识别，定稿句送注入队列。
        let app_asr = app.clone();
        let session = self.clone();
        tauri::async_runtime::spawn(async move {
            let _ = app_asr.emit("asr-state", "通话中：说完一句自动上屏，再按一次挂断");
            if session.overlay_enabled() {
                show_overlay(&app_asr);
            }
            let result = asr::run_call(
                app_asr.clone(),
                api_key,
                asr_url,
                resource_id,
                audio_rx,
                stop_flag.clone(),
                inj_tx,
            )
            .await;
            stop_flag.store(true, Ordering::Relaxed);
            session.finish(gen);

            if let Err(e) = result {
                let _ = app_asr.emit("asr-state", format!("识别失败: {e}"));
            } else {
                let _ = app_asr.emit("asr-state", "通话结束");
            }
            hide_overlay(&app_asr);
        });
    }

    /// 把一句识别结果**派给 S{slot}** —— 送进后台队列执行。
    ///
    /// # 为什么改成后台（v0.35.0 起，取代原「出口 A」）
    ///
    /// 老实现把拼好的命令行投进用户眼前的终端。那个设计唯一的卖点是**过程可见**，
    /// 代价是"你必须守着它" —— 与任务台"说完就走、结果回来找你"的价值正好相反。
    /// 更要紧的是它把任务台其余机制**全部绕开**了：队列、同槽串行、四档完成判定、
    /// 世界门禁、未读、播报、会话续接 —— 于是按键与页面按钮跑的不是一回事，
    /// 而用户有理由相信它们一致。
    ///
    /// 现在两条入口合流：都调 [`crate::agent_desk::dispatch_task`]。
    /// 投终端那条链路（`slot_command_line` / `deliver_command_line` /
    /// `desk_deliver_foreground` 命令 / `text_delivery::deliver_to_window*`）已整体移除。
    ///
    /// ⚠️ `agent_desk::delivery.rs` **没有**被删 —— 它被收窄成"等外部终端到前台"这一件事，
    /// 仍然被 `cli_terminal.rs`（「启动终端」那条路）使用。别把它当成出口 A 的遗留物删掉。
    ///
    /// # ⛔ 失败也绝不退回上屏
    ///
    /// 派发失败时**绝不**把手里的文本粘进用户当前窗口：那会把一句说给 Agent 的任务
    /// 送进聊天框或编辑器，而他以为任务在后台跑 —— 文本去了错的地方还报成功。
    async fn dispatch_to_desk(&self, app: &tauri::AppHandle, slot: u8, text: &str) {
        use crate::agent_desk::state::DispatchOutcome;
        use tauri::Emitter;

        let index = slot.saturating_sub(1) as usize;
        let (dispatched, msg) =
            match crate::agent_desk::dispatch_task(app, index, text.to_string()) {
                Ok(DispatchOutcome::Started { task_id, depth }) => (
                    true,
                    format!(
                        "S{slot} 已开始执行（任务 #{task_id}{}）—— 过程在后台跑，跑完会播报结果",
                        if depth > 1 {
                            format!("，该槽还有 {} 条在排队", depth - 1)
                        } else {
                            String::new()
                        }
                    ),
                ),
                Ok(DispatchOutcome::Queued { task_id, ahead }) => (
                    true,
                    format!(
                        "S{slot} 已排队（任务 #{task_id}，前面还有 {ahead} 条）—— 轮到它时自动开始"
                    ),
                ),
                Err(e) => (
                    false,
                    format!("S{slot} 没能派发：{}（你的原话：{}）", e.message(), text),
                ),
            };

        if dispatched {
            crate::log_buffer::info(format!("按键链路：{msg}"));
        } else {
            crate::log_buffer::warn(format!("按键链路：{msg}"));
        }
        let _ = app.emit(
            "desk-slot-key",
            serde_json::json!({ "slot": slot, "message": msg, "dispatched": dispatched }),
        );
        if !dispatched {
            let _ = app.emit("asr-state", "任务台派发失败 —— 你的原话已留在提示里");
        }
    }

    /// 单句识别结果的落点（Hold 模式专用）。
    ///
    /// 两条出口，互斥，判据 = 纯函数 [`delivery_for`]：
    /// 1. `desk_slot` 有值 ⇒ **派进任务台**（不注入、不进输入框）；
    /// 2. 否则 ⇒ **注入焦点窗口**；`submit == true` 时额外按一次回车。
    ///
    /// 文本为空 ⇒ 什么都不做（空结果在调用方那一层已经说清原因了，这里是最后一道）。
    ///
    /// # 返回值：字幕浮层是否**已由本函数收尾**
    ///
    /// 任务台路径要让浮层多留一会儿（用户只有这一个地方能看见自己说了什么），
    /// 所以由它自己在延时之后收，并返回 `true` 让调用方**不要再收一次**。
    /// 注入路径返回 `false` —— 它按原来的方式交给调用方兜底。
    ///
    /// # 派发时 `submit` 被吃掉，而不是"传下去让下游忽略"
    ///
    /// 任务台那一侧靠"未读"通知用户回来听结果，根本没有"发送"这个概念。
    /// 让回车在派发上生效，就会出现"按回车让任务台提交"这种谁也不认得的动作。
    ///
    /// # 任务台为什么不参与"回车发送"
    ///
    /// 任务台的派发带着**"未读"这个承诺** —— 它的设计前提正是"结果落在用户看不见的
    /// 地方，所以留一颗灯、留一条未读等他回来听"。那个承诺与"用户正盯着屏幕"是互斥的，
    /// 所以它不能依赖任何交互式动作（v0.29.0 那版两段式也曾把它排除在外，理由同源）。
    async fn deliver_hold_result(
        &self,
        app: tauri::AppHandle,
        raw: String,
        desk_slot: Option<u8>,
        submit: bool,
    ) -> bool {
        let text = raw.trim().to_string();
        if text.is_empty() {
            let _ = app.emit("asr-state", "未识别到内容");
            return false;
        }
        match delivery_for(desk_slot, submit) {
            Delivery::Desk(slot) => {
                // 正文 + 状态一次发全（见 `desk_subtitle_events` 的说明：
                // 任务台路径过去只发状态行，字幕因此从头到尾是空的）。
                for (name, payload) in desk_subtitle_events(slot, &text) {
                    let _ = app.emit(name, payload);
                }
                self.dispatch_to_desk(&app, slot, &text).await;
                // ⛔ 不能在这儿立刻把浮层收了。任务台**不注入输入框** —— 派发完，
                //    屏幕上没有任何地方写着这句话，浮层是唯一的凭据（理由见
                //    `DESK_SUBTITLE_LINGER`）。
                //
                //    收之前再看一眼"现在还在录音吗"：用户完全可能在这 1.8 秒里
                //    又按了一次槽位键开始下一句，那一轮已经 `show_overlay` 接管了浮层，
                //    这时再关就是把新一轮的字幕一起关掉。
                tokio::time::sleep(DESK_SUBTITLE_LINGER).await;
                if !self.is_listening() {
                    hide_overlay(&app);
                }
                // 已经由本函数收尾 ⇒ 调用方不要再收一次（那会把刚才的停留当场抹掉）。
                true
            }
            Delivery::Inject { submit } => {
                // 刻意忽略返回值：`inject` 内部已经把**失败的全部后果**处理完了
                // （日志 + 出声 + 内容留底），这里没有第二件该做的事。
                // ⛔ 别改成"失败就自动重试"：那要回答"几次、间隔多久"，而用户就在旁边，
                //    一次点击（「重新注入」）比他等一个自动重试更快，也更让他确认结果。
                let _ = self.inject(app, text, submit).await;
                false
            }
        }
    }

    /// 启动编辑模式语音识别（**第一次按编辑键**；结束在第二次按下 → `stop_edit_listening`）。
    fn start_edit_listening(&self, app: tauri::AppHandle, origin: PttOrigin) {
        let api_key = self.api_key();
        let asr_url = self.asr_url();
        let resource_id = self.resource_id();
        if api_key.trim().is_empty() {
            let _ = app.emit("edit-state", "请先在设置里填写 API Key");
            *self.inner.edit_phase.lock().unwrap() = EditPhase::Idle;
            return;
        }

        let stop_flag = Arc::new(AtomicBool::new(false));
        let gen = self.register(stop_flag.clone(), origin);

        let (audio_tx, audio_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);

        let ka = self.keyboard_audio_clone();
        let (use_keyboard, mic_reason) = self.resolve_mic(ka.as_ref());

        // 录音源记进日志：排查"没收到音频"时，第一个要回答的就是这一问。
        let source = mic_label(use_keyboard);
        crate::log_buffer::info(format!("录音源（编辑）：{source}｜{mic_reason}"));

        if use_keyboard {
            let ka = ka.unwrap();
            ka.start_streaming(gen as u64, audio_tx);
            let flag_watch = stop_flag.clone();
            std::thread::spawn(move || {
                let started = Instant::now();
                while !flag_watch.load(Ordering::Relaxed) && started.elapsed() < MAX_DURATION {
                    std::thread::sleep(Duration::from_millis(50));
                }
                ka.stop_streaming();
            });
        } else {
            let flag_rec = stop_flag.clone();
            let app_rec = app.clone();
            std::thread::spawn(move || {
                let capture = match audio::AudioCapture::start(audio_tx) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = app_rec.emit("edit-state", format!("录音失败: {e}"));
                        return;
                    }
                };
                let started = Instant::now();
                while !flag_rec.load(Ordering::Relaxed) && started.elapsed() < MAX_DURATION {
                    std::thread::sleep(Duration::from_millis(50));
                }
                drop(capture);
            });
        }

        let app_asr = app.clone();
        let session = self.clone();
        tauri::async_runtime::spawn(async move {
            let _ = app_asr.emit("edit-state", "编辑聆听中，请说出修改要求…");
            hide_overlay(&app_asr);
            show_edit_overlay(&app_asr);

            let result = asr::run_recognition(
                app_asr.clone(),
                api_key,
                asr_url,
                resource_id,
                audio_rx,
                stop_flag.clone(),
            )
            .await;

            stop_flag.store(true, Ordering::Relaxed);
            session.finish(gen);

            match result {
                Ok(out) => {
                    let text = out.text.trim().to_string();
                    if text.is_empty() {
                        // 与主链路同理：说清是哪一环断的，别只说"未识别到内容"。
                        // 同样带上麦克风决策理由 —— 编辑时用错麦克风同样收不到声音。
                        let msg = asr::explain_empty(&out, &format!("{source}；{mic_reason}"));
                        alert_voice(&app_asr, &msg);
                        *session.inner.edit_phase.lock().unwrap() = EditPhase::Idle;
                        hide_edit_overlay(&app_asr);
                        return;
                    }

                    let _ = app_asr.emit("edit-state", format!("识别完成: {text}"));

                    let ve = session.inner.voice_edit.lock().unwrap().clone();
                    if let Some(ve) = ve {
                        // execute_edit 负责浮层的显示/隐藏（含错误 2 秒延迟关闭）。
                        let _ = ve.execute_edit(text, app_asr.clone()).await;
                    } else {
                        let _ = app_asr.emit("edit-state", "编辑管理器未初始化");
                        hide_edit_overlay(&app_asr);
                    }

                    *session.inner.edit_phase.lock().unwrap() = EditPhase::Idle;
                }
                Err(e) => {
                    let _ = app_asr.emit("edit-state", format!("识别失败: {e}"));
                    *session.inner.edit_phase.lock().unwrap() = EditPhase::Idle;
                    hide_edit_overlay(&app_asr);
                }
            }
        });
    }

    /// 停止编辑模式语音识别（第二次按编辑键，确认提交）。
    fn stop_edit_listening(&self, _app: tauri::AppHandle) {
        self.stop_active();
    }
}

#[cfg(test)]
mod tests {
    //! 键盘按键派任务的归属模型。
    //!
    //! 这块逻辑**必须**有测试：它的失效方式是静默的 —— 归属被清早了，话就上屏了
    //! （用户以为派给了 Agent）；归属残留了，下一次普通语音输入就被错派走。
    //! 两种都不会报错，只会"看起来有点怪"，而这类故障没人会去查日志。
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn manager() -> SessionManager {
        SessionManager::new()
    }

    /// 归属是**一次性的**：认领一次之后必须消失。
    ///
    /// 这是"上一次录音的残留不会串味到下一次"的唯一保障 —— 不靠谁记得去清，
    /// 而是认领即消失，残留从机制上不可能存在。
    #[test]
    fn desk_intent_is_consumed_once() {
        let s = manager();
        s.set_desk_intent(Some(1));
        assert_eq!(s.take_desk_intent(), Some(1));
        assert_eq!(s.take_desk_intent(), None, "认领后必须归零，否则会串味到下一次录音");
    }

    /// ⛔ 回归：按下"停止"**不能**清掉归属。
    ///
    /// 早期版本在 `stop_active` 里清空，结果 Hold 模式第二次按键时归属先没了，
    /// 识别结果回来只能走上屏 —— 任务台永远收不到一句话，而用户看到的是"已上屏"。
    #[test]
    fn stopping_does_not_clear_recording_owner() {
        let s = manager();
        let flag = Arc::new(AtomicBool::new(false));
        let gen = s.register(flag.clone(), PttOrigin::Voice);
        s.set_recording_desk_slot(Some(1));

        s.stop_active();

        assert!(flag.load(Ordering::Relaxed), "按下停止必须真的置停止标志");
        assert_eq!(
            s.current_desk_slot(),
            Some(1),
            "停止键不能清归属：识别结果还没回来，清了它就没地方可派了"
        );

        // 会话真正结束（ASR 返回）后才作废 —— 此时结果已经派完了。
        s.finish(gen);
        assert_eq!(s.current_desk_slot(), None);
    }

    /// 会话结束即作废：下一次录音不会捡到上一段的归属。
    #[test]
    fn finish_clears_recording_owner() {
        let s = manager();
        let gen = s.register(Arc::new(AtomicBool::new(false)), PttOrigin::Voice);
        s.set_recording_desk_slot(Some(3));
        s.finish(gen);
        assert_eq!(s.current_desk_slot(), None);
    }

    /// 只有**当前**会话结束才作废：迟到的旧会话不能把新会话的归属清掉。
    ///
    /// 用户按停止后马上开始下一段录音是很常见的操作，而旧会话的 ASR 还在收尾。
    #[test]
    fn stale_finish_does_not_clear_newer_owner() {
        let s = manager();
        let old_gen = s.register(Arc::new(AtomicBool::new(false)), PttOrigin::Voice);
        s.set_recording_desk_slot(Some(1));

        // 新会话抢先登记（旧的还没 finish）
        let new_gen = s.register(Arc::new(AtomicBool::new(false)), PttOrigin::Voice);
        assert!(new_gen > old_gen);
        s.set_recording_desk_slot(Some(2));

        s.finish(old_gen); // 旧会话迟到收尾
        assert_eq!(
            s.current_desk_slot(),
            Some(2),
            "旧会话的 finish 不能动新会话的归属"
        );
    }

    // -----------------------------------------------------------------------
    // 入口互斥：一段录音的"发起者"
    //
    // 这一组钉的是**按键冲突**那个 bug：四个入口（KEY1 语音 `voice_ptt_hold` /
    // KEY3 编辑 `edit_ptt_hold` / 任务台槽位键 `task_desk` / 界面按钮）共享同一个
    // 录音会话，而每个入口都只会 toggle（"有录音就停、没有就开"）—— 它不认得当前
    // 这段录音是谁开的。于是按 KEY1 会把任务台正在录的那段**结束并派出去执行命令**，
    // 按槽位键会把普通语音输入那段**结束并上屏到编辑器**。两次都是静默的。
    //
    // 修法：会话记住发起者，不匹配即拒绝（`press_conflict`）。
    // -----------------------------------------------------------------------

    /// ⛔ 核心回归：入口互斥的**判据**必须是纯函数、必须被钉死。
    ///
    /// 拒绝动作要 `AppHandle`（得 emit 提示）⇒ 测试里构造不出来。所以判据被抽成
    /// `press_conflict`；不抽的话，这条防线只剩"看代码好像是对的"。
    #[test]
    fn press_conflict_matrix() {
        // 没有录音在跑 ⇒ 任何入口都能开始。
        assert_eq!(press_conflict(PttOrigin::Voice, None), None);
        assert_eq!(press_conflict(PttOrigin::DeskSlot(1), None), None);
        assert_eq!(press_conflict(PttOrigin::Edit, None), None);

        // 同源再按 ⇒ 这就是"结束"，必须放行（否则第二次按 S1 永远结束不了）。
        assert_eq!(
            press_conflict(PttOrigin::DeskSlot(1), Some(PttOrigin::DeskSlot(1))),
            None
        );
        assert_eq!(press_conflict(PttOrigin::Voice, Some(PttOrigin::Voice)), None);

        // 跨入口 ⇒ 拒绝，并把"现在是谁在录"带回去（提示要用它说清该按哪个键）。
        assert_eq!(
            press_conflict(PttOrigin::Voice, Some(PttOrigin::DeskSlot(1))),
            Some(PttOrigin::DeskSlot(1)),
            "按 KEY1 不能把任务台正在录的那段结束掉 —— 那会把它派出去执行命令"
        );
        assert_eq!(
            press_conflict(PttOrigin::DeskSlot(1), Some(PttOrigin::Voice)),
            Some(PttOrigin::Voice),
            "按 S1 不能把普通语音输入那段结束掉 —— 那会把话粘进当前编辑器"
        );
        assert_eq!(
            press_conflict(PttOrigin::Edit, Some(PttOrigin::Voice)),
            Some(PttOrigin::Voice)
        );
        // 不同槽位也算冲突：按 S2 的意图是"给 S2 录"，不该替 S1 收尾。
        assert_eq!(
            press_conflict(PttOrigin::DeskSlot(2), Some(PttOrigin::DeskSlot(1))),
            Some(PttOrigin::DeskSlot(1))
        );
    }

    /// **回车判据**：只有"这一轮的话会进输入框"的会话才认回车。
    ///
    /// 这一条判错不是"功能不灵"，而是**把用户正常的回车吃掉** —— 他在终端里敲回车
    /// 执行命令、在编辑器里敲回车换行，结果什么都没发生；现象与"键盘坏了"一模一样，
    /// 而原因藏在一个跟他毫无关系的状态位里。所以逐条钉死。
    #[test]
    fn enter_grab_matrix() {
        // ★ 唯一该认的一路：普通语音输入，结果进输入框。
        assert!(enter_grabs(PttOrigin::Voice, None));

        // ⛔ 任务台的键：结果派给槽位、不进输入框，"发送"无从谈起。
        assert!(!enter_grabs(PttOrigin::Voice, Some(1)));
        // ⛔ 编辑键：结果是"替换选中文本"这个动作，没有提交可言。
        assert!(!enter_grabs(PttOrigin::Edit, None));
        assert!(!enter_grabs(PttOrigin::Edit, Some(2)));
        // ⛔ 槽位键自己开的会话同样不认。
        assert!(!enter_grabs(PttOrigin::DeskSlot(3), None));
    }

    /// **交付去向**：有槽位就派发（且 `submit` 被吃掉），否则注入（可带回车）。
    ///
    /// 判错的代价是"我说的话进了错误的地方"：本该派给任务的命令被注入进编辑器、
    /// 还按了一下回车 —— 而回车是不可撤销的。
    #[test]
    fn delivery_routing_matrix() {
        // ★ 派发优先，且**回车被吃掉**：任务台没有"发送"这个概念。
        //   这一条防的是"用户在任务台按了回车，结果被当成提交"。
        assert_eq!(delivery_for(Some(1), true), Delivery::Desk(1));
        assert_eq!(delivery_for(Some(1), false), Delivery::Desk(1));

        // 没有槽位 ⇒ 注入；`submit` 如实传下去。
        assert_eq!(delivery_for(None, false), Delivery::Inject { submit: false });
        assert_eq!(delivery_for(None, true), Delivery::Inject { submit: true });
    }

    /// **字幕正文**：任务台派发路径必须把识别出来的话发到浮层。
    ///
    /// 钉的是"成功却静默"那一类故障 —— 它比报错更难查，因为**没有任何错误可看**：
    ///
    /// - 本机 ASR 走 `.../plan/sauc/bigmodel_nostream`，识别过程中服务端回的每一帧
    ///   文本都是空的（实测 `#1`~`#22` 全部 `文本=""`），浮层在工作期间本来就是空的；
    /// - 任务台路径收尾时过去**只发 `asr-state`（状态行）**，从不发正文。
    ///
    /// 两者叠加 ⇒ 用户按住槽位键说完一整句、日志里"识别成功 N 字"一切正常，
    /// 屏幕上却一个字都没有。这条测试把"必须有 asr-final"变成不可回退的约定。
    #[test]
    fn desk_dispatch_puts_the_text_on_the_subtitle() {
        let evs = desk_subtitle_events(1, "把今天的日志整理一下");
        assert_eq!(
            evs[0].0, "asr-final",
            "任务台路径必须发 asr-final（字幕正文），否则屏幕上留不下这句话"
        );
        assert_eq!(evs[0].1, "把今天的日志整理一下");
        // 状态行要带槽位号，且必须排在正文**之后**：浮层收到 asr-final 会把状态
        // 置成"识别完成"，顺序反了这句改写就先把状态定死，用户看不到"去了哪儿"。
        assert_eq!(evs[1].0, "asr-state");
        assert!(evs[1].1.contains("S1"), "状态行要说清派给谁：{}", evs[1].1);
    }

    /// 注入失败的留底：**读不消费**，`take` 之后立刻为空。
    ///
    /// 它防的是"重试一次失败就把内容清没了"—— 那等于把"保留内容"这个承诺又收回去，
    /// 而用户已经没法"再说一遍"（他刚说的可能是一整段口述）。
    #[test]
    fn failed_inject_holds_until_retry() {
        let s = manager();
        assert_eq!(s.failed_inject_text(), None, "初始不该有留底");

        // 只有异常路径会碰它 —— 这里直接调就是在模拟"注入失败"。
        assert_eq!(s.stage_failed_inject("注入失败的一句".into()), None);
        assert_eq!(s.failed_inject_text().as_deref(), Some("注入失败的一句"));

        // 读不消费：界面挂载时读一次，不能因此就把内容弄丢。
        assert_eq!(s.failed_inject_text().as_deref(), Some("注入失败的一句"));

        // 取走即消失。
        assert_eq!(s.take_failed_inject().as_deref(), Some("注入失败的一句"));
        assert_eq!(s.failed_inject_text(), None);
    }

    /// 再次注入失败时，被顶掉的那一条要能报出来（调用方据此写日志、出声）——
    /// 静默覆盖就是净丢一句话，正是本文件反复在防的那类失败。
    #[test]
    fn staging_failed_inject_reports_the_displaced_one() {
        let s = manager();
        assert_eq!(s.stage_failed_inject("第一条".into()), None);
        assert_eq!(
            s.stage_failed_inject("第二条".into()).as_deref(),
            Some("第一条")
        );
        assert_eq!(s.discard_failed_inject().as_deref(), Some("第二条"));
        assert_eq!(s.failed_inject_text(), None);
    }

    /// **交付意图在会话之间复位**：`register` 必须把 `submit_on_delivery` 清零。
    ///
    /// 漏掉它，上一次"回车 ⇒ 要发送"的意图会跟着下一段录音走 —— 用户只按了语音键，
    /// 内容却自己发出去了。那是不可撤销的动作，属于代价最大的一类脏状态。
    #[test]
    fn submit_intent_does_not_leak_between_sessions() {
        let s = manager();
        s.request_submit();
        assert!(s.submit_wanted());

        s.register(Arc::new(AtomicBool::new(false)), PttOrigin::Voice);
        assert!(!s.submit_wanted(), "上一段的发送意图不能漏进这一段");
    }

    /// 回车接管标志与会话同生命周期：`register` 清零、卡死强制释放时也要清零。
    ///
    /// 后一条是兜底 —— 卡死的那段会话永远不会交付，所以没有任何一处会在"交付完成"
    /// 时把它清掉；漏掉它，用户此后每个回车都会被吞进一个不存在的会话里。
    #[test]
    fn enter_grab_flag_is_reset_with_the_session() {
        let s = manager();
        s.inner.voice_delivery_pending.store(true, Ordering::SeqCst);
        assert!(s.grabs_enter());

        s.register(Arc::new(AtomicBool::new(false)), PttOrigin::Voice);
        assert!(!s.grabs_enter(), "新会话开始时先归零（随后按判据重新武装）");

        s.inner.voice_delivery_pending.store(true, Ordering::SeqCst);
        assert!(s.reclaim_stale_session(Duration::ZERO).is_some());
        assert!(
            !s.grabs_enter(),
            "卡死强制释放后必须解除回车接管，否则用户每个回车都被吞掉"
        );
    }

    /// 发起者跟会话**同生共死**：`recording_origin()` 与 `is_listening()` 读的是同一个
    /// 锁、同一个 `Option` ⇒ "正在录音"和"知道是谁开的"在结构上不可能漂移。
    ///
    /// 这也是不把 origin 单独放一个 `SessionInner` 字段的原因：单独放就必须回答
    /// "谁负责清它"，而清的时机必然夹在异步回调之间，早一步晚一步都错。
    #[test]
    fn origin_lives_and_dies_with_the_session() {
        let s = manager();
        assert_eq!(s.recording_origin(), None, "没录音时不该有发起者");
        assert!(!s.is_listening());

        let gen = s.register(Arc::new(AtomicBool::new(false)), PttOrigin::DeskSlot(2));
        assert_eq!(s.recording_origin(), Some(PttOrigin::DeskSlot(2)));
        assert!(s.is_listening());

        s.finish(gen);
        assert_eq!(s.recording_origin(), None, "会话结束必须一起消失");
        assert!(!s.is_listening());
    }

    /// 被拒时**不碰停止标志**：碰了就等于替用户结束了这段录音。
    #[test]
    fn rejection_does_not_touch_the_stop_flag() {
        let s = manager();
        let flag = Arc::new(AtomicBool::new(false));
        let _gen = s.register(flag.clone(), PttOrigin::DeskSlot(1));

        // 判据说该拒（KEY1 在 S1 录音期间被按下）。
        assert!(press_conflict(PttOrigin::Voice, s.recording_origin()).is_some());
        // 判据只做判断，不产生副作用。
        assert!(
            !flag.load(Ordering::Relaxed),
            "被拒不能置停止标志：那等于替用户按了一下「结束」"
        );
        assert_eq!(
            s.recording_origin(),
            Some(PttOrigin::DeskSlot(1)),
            "被拒不能改发起者"
        );
    }

    /// 给用户看的名字要能区分入口 —— 拒绝提示靠它说清"该按哪个键收尾"。
    #[test]
    fn origin_labels_are_distinguishable() {
        assert_eq!(PttOrigin::DeskSlot(3).label(), "槽位 S3 的键");
        assert_eq!(PttOrigin::Voice.label(), "语音输入键");
        assert_ne!(PttOrigin::Voice.label(), PttOrigin::Edit.label());
        // 槽位号必须体现出来，否则提示会说成"按一下槽位 S1 的键"而用户按的是 S3。
        assert_ne!(
            PttOrigin::DeskSlot(1).label(),
            PttOrigin::DeskSlot(2).label()
        );
    }

    // -----------------------------------------------------------------------
    // 卡死自愈：会话槽的兜底释放
    //
    // 这一组钉的是**"整个语音功能变砖"**那个 bug。会话槽的清空点在识别任务返回之后，
    // 所以只要那条异步链路里有一处永久等待（网络 IO 全都有可能），槽位就永远是"有人在录"：
    //   · 同一个入口再按 ⇒ 只是去"结束"一段永远不会结束的会话 ⇒ **按了没反应**；
    //   · 别的入口再按 ⇒ 被归属校验拒绝。
    // 四个入口（KEY1 / 槽位键 / 编辑键 / 界面按钮）就这样一起死，且没有任何提示。
    // 上游在 `asr.rs` 里给每个 await 都补了超时；这里是**最后一道**兜底：
    // 即便将来又冒出没想到的等待点，用户最多等几分钟，而不是重启应用。
    // -----------------------------------------------------------------------

    /// ⛔ 核心回归：卡死的会话**必须**能被强行释放，否则整台键盘的语音入口一起失效。
    #[test]
    fn stale_session_is_reclaimed_so_voice_recovers() {
        let s = manager();
        let _gen = s.register(Arc::new(AtomicBool::new(false)), PttOrigin::DeskSlot(1));
        s.set_recording_desk_slot(Some(1));

        // 上限取 0 ⇒ 任何已存在的会话都算"已卡死"（不必和真实时钟较劲）。
        assert_eq!(
            s.reclaim_stale_session(Duration::ZERO),
            Some(PttOrigin::DeskSlot(1)),
            "必须报出是谁卡住了，提示里要说得出这句话"
        );
        // 释放后：槽位空、提示归属也作废。
        assert!(!s.is_listening(), "释放后必须回到'没有录音'");
        assert_eq!(s.recording_origin(), None);
        assert_eq!(s.current_desk_slot(), None, "残留的提示归属会让下一段录音被错派");
        // 关键：原来"按 KEY1 会被拒"的判据现在放行了 —— 这就是恢复的判据。
        assert_eq!(press_conflict(PttOrigin::Voice, s.recording_origin()), None);
    }

    /// 正常录音期间**绝不能**被误释放：否则用户说着话就被"结束"了。
    #[test]
    fn fresh_session_is_not_reclaimed() {
        let s = manager();
        let _gen = s.register(Arc::new(AtomicBool::new(false)), PttOrigin::Voice);

        assert_eq!(
            s.reclaim_stale_session(Duration::from_secs(600)),
            None,
            "刚开始的会话不该被判成卡死"
        );
        assert!(s.is_listening(), "没到上限就必须原样留着");
        assert_eq!(s.recording_origin(), Some(PttOrigin::Voice));
    }

    /// 没有会话时调用是空操作 —— 不能顺手把 `desk_intent` 之类别的状态清掉。
    #[test]
    fn reclaim_is_a_no_op_when_idle() {
        let s = manager();
        s.set_desk_intent(Some(2));
        assert_eq!(s.reclaim_stale_session(Duration::ZERO), None);
        assert_eq!(
            s.take_desk_intent(),
            Some(2),
            "空转时不该动别的状态（归属意图是下一段录音的，还没被认领）"
        );
    }

    /// 硬上限必须跟着模式走：Hold 三分钟、Call 半小时，差一个数量级。
    ///
    /// 用同一个常数会**误杀长通话**（正常通话能到 30 分钟），
    /// 或者让 Hold 的卡死要等半小时才自愈。
    #[test]
    fn hard_limit_follows_the_trigger_mode() {
        let s = manager();
        *s.inner.mode.lock().unwrap() = TriggerMode::Hold;
        let hold = s.session_hard_limit();
        *s.inner.mode.lock().unwrap() = TriggerMode::Call;
        let call = s.session_hard_limit();

        assert!(hold > MAX_DURATION, "必须给正常录音留出余量");
        assert!(call > MAX_CALL_DURATION, "必须给正常通话留出余量");
        assert!(
            call > hold * 2,
            "通话上限必须显著长于单次录音，否则长通话会被腰斩：{call:?} vs {hold:?}"
        );
    }

    /// `mic_source = "bluetooth"` 是**已知不生效**的值，必须迁移掉，且要如实报告"迁移过"。
    ///
    /// 判据刻意用"值等于已知错值"，而不是"用户没选过" ——
    /// 所以 `pc` / `wifi` 这类**真能生效**的选择一律保留，一个字都不改。
    #[test]
    fn bluetooth_mic_source_is_migrated_to_auto() {
        // 已知不生效的值：落到 Auto（两条真实可用的路都留着），并**报告迁移过**
        // —— 调用方靠这个标志写日志，否则这种静默改写将来又是一桩无头案。
        assert_eq!(parse_mic_source("bluetooth"), (MicSource::Auto, true));

        // 真能生效的选择一律不动，且不谎报迁移。
        assert_eq!(parse_mic_source("pc"), (MicSource::Pc, false));
        assert_eq!(parse_mic_source("wifi"), (MicSource::Wifi, false));
        assert_eq!(parse_mic_source("auto"), (MicSource::Auto, false));

        // 认不出的值（老版本写的、手改的垃圾）→ Auto，但**不算迁移**
        //（那不是我们知道的一个具体错值，说成"迁移"会掩盖真正的配置损坏）。
        assert_eq!(parse_mic_source(""), (MicSource::Auto, false));
        assert_eq!(parse_mic_source("蓝牙"), (MicSource::Auto, false));
    }
}
