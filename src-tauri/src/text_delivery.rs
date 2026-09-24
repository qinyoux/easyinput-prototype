//! 文本注入 —— 复刻 easyop 逆向分析得到的 text_delivery.v1 四阶段流程：
//!
//!   admission（等键盘空闲）→ preflight（备份剪贴板）→ dispatch（写剪贴板
//!   + 模拟粘贴键）→ terminal（等目标应用取走内容后恢复剪贴板）。
//!
//! 为什么用剪贴板而不是 KEYEVENTF_UNICODE 逐字符注入：
//!   - 兼容不处理 Unicode 键盘事件的程序；
//!   - 多行长文本的注入耗时与文本长度无关；
//!   - 行为与真实用户粘贴一致，目标应用零特判。
//!
//! 防递归：模拟的按键序列带 LLKHF_INJECTED 标志，ptt_hook 见标志即放行。
//!
//! # 粘贴键为什么必须可配置（`PasteStyle`）
//!
//! `Ctrl+V` 是**图形界面**的粘贴约定。终端里的 TUI 程序（codex / vim / htop）会把
//! 终端置于 raw mode 直接读按键字节 —— `Ctrl+V` 在那里是**字面量 0x16**，
//! 会被 TUI 当成自己的快捷键吃掉，**粘不进去**。
//!
//! ⚠️ 而"终端该用哪一组键"**没有统一答案**（本机实测，09-20）：
//!
//! | 终端 | 实测可用的粘贴键 |
//! |---|---|
//! | Git Bash / MSYS2 / Cygwin（`mintty`） | **`Ctrl+V`**（程序模拟唯一有效的那一组，见下） |
//! | Windows Terminal（`CASCADIA_HOSTING_WINDOW_CLASS`） | `Ctrl+Shift+V`（或鼠标右键）；`Shift+Insert` 不一定 |
//!
//! ⚠️ 上表的 mintty 一行是**用程序模拟逐个实测出来的**（09-21，测的就是你机器上那个
//! `C:\Program Files\Git\usr\bin\mintty.exe`）：
//!
//! | 发法 | mintty 里发生了什么 |
//! |---|---|
//! | `Shift+Insert`（人**手**按） | ✅ 粘贴（但它只对人手成立，见下） |
//! | `Shift+Insert`（程序模拟，带不带 E0 扩展位都一样） | ❌ **什么都不发生** |
//! | `Ctrl+V`（程序模拟，`wScan = 0`） | ✅ **粘贴** |
//! | `Ctrl+V`（程序模拟，`wScan = 0x2F`） | ❌ 什么都不发生 |
//!
//! # 为什么"人手能按"不等于"程序能模拟"（这是本文件最贵的一条知识）
//!
//! mintty 的粘贴有两套通路，**它们对修饰键的读法不一样**：
//!
//! 1. `Shift+Ins` 走 `wininput.c` 的通用快捷键分支
//!    （`if (cfg.clip_shortcuts && key == VK_INSERT && mods && !alt) { if (shift) win_paste(); }`），
//!    而 `shift` 来自 `get_mods()` → `GetKeyState(VK_SHIFT)` —— **查的是泛用 `VK_SHIFT`(0x10)**。
//!    注入的 shift 到达目标时是 `VK_LSHIFT`(0xA0)（现场实测），**泛用位不会被置上**
//!    ⇒ `shift == false` ⇒ 分支进了、什么也不做。
//!    （Ctrl 那条没事：`get_mods()` 查的是具体的 `VK_LCONTROL`，注入能置上。）
//! 2. `Ctrl+V` 有一条**专门为"注入"写的**分支：
//!    `// Workaround for Windows clipboard history pasting simply injecting Ctrl+V (mintty/wsltty#139)`
//!    `if (key == 'V' && mods == MDK_CTRL && !scancode) { win_paste(); return true; }`
//!    ⇒⛔ **它要求 `scancode == 0`。** 这正是"`wScan` 必须留 0"的理由：补上真实扫描码
//!    会把这条分支**关掉**（已实测：补了之后 Ctrl+V 不再粘贴）。
//!
//! **教训（可复用）**：`wScan = 0` 不是"省事"，它在目标程序眼里是"**这是合成事件**"的
//! 标记 —— 有些程序据此走兼容路径。**测"哪个键能粘贴"必须在目标机上用程序模拟测，
//! 人肉按一下得到的结论可能完全不适用。**
//!
//! 另外记一笔（本次排查得到的、暂时用不上的事实）：`Insert` 的扩展扫描码 `E0 52`
//! 与小键盘 `0` 的 `52` 撞车，`wScan = 0` + 无 `KEYEVENTF_EXTENDEDKEY` 时 mintty 会把
//! `Insert` 认成 `KP0`（源码 `vktab`：`{VK_INSERT, 2}` ⇒ `keypad = !extended`）。
//! 试过补扩展位（并实测过），**对粘贴没有任何帮助**，且补扫描码会关掉上面那条兼容分支
//! ⇒ 已回退，不留无用改动。详见 `scripts/probe_key_events.py` 与 `scripts/e2e_terminal_paste_key.py`。
//!
//! 于是产生一个绕不开的现实：**同一套硬件，一会儿要往微信里打字，一会儿要往
//! 你正开着的 codex TUI 里打字**。让用户在设置里来回切是错误的设计（他一天要切几十次），
//! 所以默认走 [`PasteStyle::Auto`]：**看当前焦点窗口的类名**决定用哪一种。
//!
//! ⚠️ 判定失败的兜底方向是**刻意选的**：拿不到类名 / 类名不在终端名单里 ⇒
//! 一律回落 `Ctrl+V`（= 改造前的既有行为）。也就是说，**最坏情况退化成改造前的样子，
//! 而不是变成一个新的坏行为**。同理，名单里没实测过的终端（conhost / ConEmu）
//! 也保持 `Ctrl+V`，**不猜**。
//!
//! # 为什么默认**不**"粘贴后回车"（`submit_after_paste`）
//!
//! ⚠️ 这条默认值**被显式反转过**（v0.21.0 开 → v0.28.2 关），两边的理由都要留着，
//! 否则下一个人只会看到后半句就把关改回开。
//!
//! **当初为什么开**：TUI 里粘进去只落地到输入框、不会自动提交（实测 09-20），
//! 不回车就等于"说完一段话还得手动敲一次"，语音输入这条链没闭合。
//!
//! **为什么改成关**：回车是一个**不可撤销的提交**，而"这句话该不该立刻提交"
//! 是**用户才知道的事**，不是宿主能替他判断的。钦友的场景尤其如此 ——
//! 语音内容是接在一条**半成品命令后面**的（见 `cli_terminal.rs`：命令前缀先预置、
//! 语音内容后追加），此时回车 = 立即执行一条他还没看完的命令。
//!
//! 关键区分：**"注入"和"提交"是两件事**。注入的失败模式是"没粘上"，看得见、改得动；
//! 提交的失败模式是"一条没审完的命令被执行了"，看不见、不可逆。两害相权，
//! 默认压力应当加在**可撤销**的那一侧 —— 提交权交还用户（KEY2 绑的就是 Return）。
//!
//! ⛔ 因此**不要**在"终端里粘完还得手动敲回车很麻烦"这个论点上改回去：
//! 要减少那一次手动动作，正确做法是让用户自己把这个开关打开（设置项仍在），
//! 而不是替他默认按下去。

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, OpenClipboard,
    SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VK_CONTROL, VK_INSERT, VK_RETURN, VK_SHIFT, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow};

const CF_UNICODETEXT: u32 = 13;
const CF_TEXT: u32 = 1;
const CF_OEMTEXT: u32 = 7;
const CF_LOCALE: u32 = 16;
const VK_V: VIRTUAL_KEY = VIRTUAL_KEY(0x56);

// ---------------------------------------------------------------------------
// 粘贴方式
// ---------------------------------------------------------------------------

/// 模拟粘贴时按哪一组键。
///
/// `Auto` 是默认值：由**焦点窗口的类名**决定，见 [`paste_style_for_class`]。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasteStyle {
    /// 看焦点窗口：按终端类名选对应那一组键，认不出来 → `Ctrl+V`。
    #[default]
    Auto,
    /// 恒用 `Ctrl+V`（图形界面的约定，也是改造前的既有行为）。
    CtrlV,
    /// 恒用 `Shift+Insert`（mintty / Git Bash 的约定，TUI 程序也可用）。
    ShiftInsert,
    /// 恒用 `Ctrl+Shift+V`（Windows Terminal 的约定，实测 09-20）。
    CtrlShiftV,
}

impl PasteStyle {
    /// 稳定字符串标识（写进 `config.json`，**改动会让用户已存的配置失效**）。
    pub const fn as_str(self) -> &'static str {
        match self {
            PasteStyle::Auto => "auto",
            PasteStyle::CtrlV => "ctrl_v",
            PasteStyle::ShiftInsert => "shift_insert",
            PasteStyle::CtrlShiftV => "ctrl_shift_v",
        }
    }

    /// 从配置字符串解析。**无法识别时返回 `None`**，由调用方决定如何处理
    /// —— 这里刻意不"看不懂就当 Auto"，那会让配置写错时**毫无痕迹地**改变行为。
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(PasteStyle::Auto),
            "ctrl_v" | "ctrl+v" => Some(PasteStyle::CtrlV),
            "shift_insert" | "shift+insert" | "shift_ins" => Some(PasteStyle::ShiftInsert),
            "ctrl_shift_v" | "ctrl+shift+v" => Some(PasteStyle::CtrlShiftV),
            _ => None,
        }
    }

    const fn code(self) -> u8 {
        match self {
            PasteStyle::Auto => 0,
            PasteStyle::CtrlV => 1,
            PasteStyle::ShiftInsert => 2,
            PasteStyle::CtrlShiftV => 3,
        }
    }

    const fn from_code(c: u8) -> Self {
        match c {
            1 => PasteStyle::CtrlV,
            2 => PasteStyle::ShiftInsert,
            3 => PasteStyle::CtrlShiftV,
            _ => PasteStyle::Auto,
        }
    }
}

/// 进程内的当前粘贴方式。
///
/// # 为什么是全局状态，而不是给 `deliver` 加参数
///
/// [`deliver`] 有 7 个调用点（语音会话 / 语音编辑 / 具名动作 / 工作台 / 插件 API /
/// 命令层），散布在完全不同的功能里。给签名加参数意味着**7 处全改**，而它们
/// 对"用哪种粘贴"这件事**没有任何观点** —— 那是全局环境事实，不是每处调用各自的决策。
///
/// 这与 `DeskManager::sync_tts` 推送 TTS 配置是同一个模式：**配置集中一份，
/// 使用点只读全局**。
static PASTE_STYLE: AtomicU8 = AtomicU8::new(0); // 0 = Auto
/// 粘贴完成后是否再按一次回车。
///
/// **默认关**（v0.28.2 起）：回车是不可撤销的提交，而"该不该现在提交"只有用户知道。
/// 见文件头「为什么默认**不**粘贴后回车」——那里同时留着"当初为什么开"，
/// 以免这个默认值被反复改回来。
pub const DEFAULT_SUBMIT_AFTER_PASTE: bool = false;
static SUBMIT_AFTER_PASTE: AtomicBool = AtomicBool::new(DEFAULT_SUBMIT_AFTER_PASTE);

/// 设置本次进程使用的粘贴方式（由配置加载路径调用）。
pub fn set_paste_style(style: PasteStyle) {
    PASTE_STYLE.store(style.code(), Ordering::Relaxed);
}

/// 读当前的粘贴方式。
pub fn paste_style() -> PasteStyle {
    PasteStyle::from_code(PASTE_STYLE.load(Ordering::Relaxed))
}

/// 设置"粘贴后是否回车"。
pub fn set_submit_after_paste(on: bool) {
    SUBMIT_AFTER_PASTE.store(on, Ordering::Relaxed);
}

/// 读"粘贴后是否回车"。
pub fn submit_after_paste() -> bool {
    SUBMIT_AFTER_PASTE.load(Ordering::Relaxed)
}

/// 终端窗口类名 → **该终端实测可用的粘贴键**。
///
/// 这些名字是**窗口类名**（`GetClassNameW` 的返回值），不是进程名 —— 它比进程名稳：
/// 进程名随版本/安装方式变化，而类名是窗口注册时写死的。
///
/// ⚠️ 键位**不是猜的，是逐个实测的**。但**"怎么测的"分两档，必须写清楚**：
///
/// - **人手动按键**能用的键（09-20 那一批）—— 这只说明"这个终端认这组键"；
/// - **程序模拟**能生效的键（09-21 起）—— 这才是我们要的，条件更严：
///   程序没法"真的按住"修饰键，目标程序读修饰键的方式（泛用 VK vs 具体 VK）会决定成败。
///
/// mintty 就是活教材：人手 `Shift+Ins` ✓，程序模拟 `Shift+Ins` ✘，程序模拟 `Ctrl+V` ✓。
/// ⇒ **表里每条都该用程序模拟复核过再填**；目前复核过的只有 mintty。
///
/// 两者不一致，所以这里是一张**映射表**而不是一个"终端名单"：
/// 若将来退化成名单（所有终端同一组键），Windows Terminal 会重新粘不进去。
///
/// 未实测的终端（conhost / ConEmu）**不写进表里** —— 拿不到实测结论就保持
/// `Ctrl+V`（改造前的既有行为），不猜。
const TERMINAL_PASTE_KEYS: &[(&str, PasteStyle)] = &[
    // Windows Terminal：Ctrl+Shift+V（或右键）。Shift+Insert 实测不保证可用。
    // ⚠️ 这一条是**人肉按键**测出来的，还没用程序模拟复核过；而 `Ctrl+Shift+V` 依赖
    //    SHIFT 修饰键 —— 注入的 shift 在目标进程里可能读不到（mintty 就是这样，见下）。
    //    在**本机**用程序模拟实测过一次之前，这里不改（不猜）。
    ("CASCADIA_HOSTING_WINDOW_CLASS", PasteStyle::CtrlShiftV),
    // Git Bash / MSYS2 / Cygwin（mintty）—— ⭐ 09-21 用**程序模拟**逐个实测的结论：
    //
    //   | 发法 | 结果 |
    //   |---|---|
    //   | 人手按 `Shift+Ins` | ✓ 粘贴（但这只对人手成立） |
    //   | 程序模拟 `Shift+Ins`（补不补 E0 扩展位都一样） | ✘ 什么都不发生 |
    //   | 程序模拟 `Ctrl+V`（`wScan = 0`） | ✓ **粘贴** |
    //
    // 为什么 `Shift+Ins` 程序模拟无效：它走 `wininput.c` 的通用分支
    // `if (clip_shortcuts && key == VK_INSERT && mods && !alt) { if (shift) win_paste(); }`，
    // 而 `shift` 来自 `get_mods()` → **`GetKeyState(VK_SHIFT)`（泛用 VK）**；
    // 注入的 shift 到达目标时是 `VK_LSHIFT`，泛用位不会被置上 ⇒ 分支进了、什么也不做。
    // （Ctrl 那条没事：`get_mods()` 查的是**具体**的 `VK_LCONTROL`。）
    //
    // 为什么 `Ctrl+V` 程序模拟有效：mintty 有一条**专门为注入写的**分支
    // `if (key == 'V' && mods == MDK_CTRL && !scancode) { win_paste(); }`
    // —— 前提 `!scancode` 就是 `key_input` 里 `wScan` 必须留 0 的原因。
    //
    // ⇒ 必须写 `CtrlV`，不能写人手能用的 `ShiftInsert`。见文件头与 `scripts/e2e_terminal_paste_key.py`。
    ("mintty", PasteStyle::CtrlV),
];

/// 表里**还没用"程序模拟"复核过**的类名（v0.30.4）。
///
/// 为什么必须单独列出来：表里每一项都是"这个终端吃哪组键"的判断，
/// 而**人手能按 ≠ 程序模拟能生效**（mintty 就是活教材 —— 人手 `Shift+Ins` 好好的，
/// 程序模拟一个键都送不进去）。没复核过的项一旦失效，症状是
/// **日志写"已投递成功"、终端里一个字都没有** —— 什么都不说就等于帮它静默。
/// ⇒ 命中它时必须在日志里留一句话，并把退路（显式设 `ctrl_v`）一并告诉用户。
const UNVERIFIED_PASTE_KEYS: &[&str] = &["CASCADIA_HOSTING_WINDOW_CLASS"];

/// 这个类名的粘贴键**有没有用程序模拟实测过**（纯函数，便于单测钉死）。
///
/// 不在表里的类名返回 `true`：它们根本不走表（`paste_style_for_class` 返回 `None`
/// ⇒ 回落 `Ctrl+V`），"没复核"这件事对它们不成立，不该报警。
pub fn paste_key_verified(class: &str) -> bool {
    !UNVERIFIED_PASTE_KEYS
        .iter()
        .any(|c| class.eq_ignore_ascii_case(c))
}

/// 类名对应的粘贴键；不是已知终端则返回 `None`。
///
/// 比较用**大小写不敏感**：`GetClassNameW` 一般返回注册时的原样，
/// 但不同 Windows 版本 / 不同终端实现的写法不完全一致，赌它的一致性没有收益。
/// 这个函数是纯的，专门抽出来是为了能被单测钉死 —— 它是 `Auto` 模式唯一的判断依据。
pub fn paste_style_for_class(class: &str) -> Option<PasteStyle> {
    TERMINAL_PASTE_KEYS
        .iter()
        .find(|(c, _)| class.eq_ignore_ascii_case(c))
        .map(|(_, s)| *s)
}


/// 当前焦点窗口**该用哪种粘贴键**；判不出来则返回 `None`。
///
/// 判不出来（没有焦点窗口 / 类名读失败 / 类名不在表里）⇒ `None` ⇒ 走 `Ctrl+V`，
/// 即改造前的既有行为。**这是刻意的兜底方向**：宁可"没帮上忙"，也不要"帮倒忙"。
fn foreground_paste_style() -> Option<PasteStyle> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut buf = [0u16; 256];
        let n = GetClassNameW(hwnd, &mut buf);
        if n <= 0 {
            return None;
        }
        let class = String::from_utf16_lossy(&buf[..n as usize]);
        let style = paste_style_for_class(&class);
        if style.is_some() && !paste_key_verified(&class) {
            // 不是"可能出问题"，是"这条**从没被程序模拟验证过**"。
            // 用户看到的现象会是"日志说投递成功、终端里没反应" —— 如果不留这句话，
            // 他会先怀疑板子、怀疑识别、怀疑自己，而不会怀疑这张表。
            eprintln!(
                "[text_delivery] paste_key: 焦点窗口类是「{}」，这一条**还没用程序模拟实测过** \
                 （人手能按 ≠ 程序模拟能生效，mintty 就栽在这里）。\
                 若终端里没反应，请把设置里的「粘贴方式」显式改成 ctrl_v 再试。",
                class
            );
        }
        style
    }
}

/// 把 [`PasteStyle`] 解析成"这一次实际要按哪组键"。
///
/// `focus` 是焦点窗口查表的结果（`None` = 不是已知终端 / 读不到类名）。
///
/// 抽成独立函数是为了让"Auto 的判定结果"可被单独断言，不必真的去按键。
fn resolve_style(style: PasteStyle, focus: Option<PasteStyle>) -> PasteStyle {
    match style {
        PasteStyle::Auto => focus.unwrap_or(PasteStyle::CtrlV),
        other => other,
    }
}

/// 四阶段完整注入。阻塞约 1.2~1.8s（含键盘空闲等待与消费等待）。
///
/// 消费等待（terminal sleep）取 1200ms：粘贴键是异步按键，目标应用需时间处理。
/// 若等待不足即 restore 剪贴板，目标应用可能粘贴到恢复后的内容或下一次注入的内容，
/// 导致残留/重复粘贴。1200ms 同时保证连续注入间隔 ≥ 1s，避免竞态。
///
/// 用哪种粘贴键由进程内全局设置决定（[`PasteStyle`]，由 `config.json` 驱动），
/// **本函数的签名与行为对 7 个调用点保持不变**。
pub fn deliver(text: &str) -> Result<(), String> {
    deliver_inner(text, None, None, None, false)
}

/// 注入文本，但**这次不回车**（忽略全局「粘贴后回车」设置）。
///
/// # 与全局设置的关系
///
/// 全局开关（设置里可改）服务的是"说完一句话就把它交出去"这类默认预期；
/// 而"注入"与"提交"是两件不同的事 —— 提交是不可撤销的，该不该提交只有用户知道。
///
/// 语音链路的**默认出口**就是它：按一下语音键结束录音 ⇒ 只注入、不发送
/// （v0.30.0 起）。要提交的话，用户在录音期间按一下回车即可（见
/// [`deliver_and_submit`]），或者自己在输入框里按回车。
///
/// ⛔ 别把它实现成"临时改全局再改回来"：那是跨线程共享状态，一次并发就会
/// 把一个开关的临时值泄漏成永久值（同类问题见 `cli_terminal.rs` 的注入串行化）。
/// 按次覆盖、绝不碰全局。
pub fn deliver_no_submit(text: &str) -> Result<(), String> {
    deliver_inner(text, None, None, Some(false), false)
}

/// 注入文本**并回车**（本次强制提交，忽略全局「粘贴后回车」设置）。
///
/// # 为什么必须与 [`deliver_no_submit`] 成对存在
///
/// 语音链路的两个出口，都在**用户明确按下某个键**的那一刻决定：
///
/// | 用户按下的 | 出口 | 他要的是 |
/// |---|---|---|
/// | 语音键（第二下） | [`deliver_no_submit`] | "放进去，我还要改 / 还要看一眼" |
/// | **回车** | 本函数 | "放进去，并且现在就发出去" |
///
/// 两者都是按次覆盖，**都不碰**全局设置 —— 全局设置只服务那些没有明确用户动作
/// 的默认路径（例如命令注入）。
pub fn deliver_and_submit(text: &str) -> Result<(), String> {
    deliver_inner(text, None, None, Some(true), false)
}

/// 注入到**指定窗口**，并（可选）指定这次用哪组粘贴键。
///
/// # 为什么要在按下粘贴键之前重新激活并回读校验
///
/// 普通 [`deliver`] 假设"焦点此刻已经在目标上"。而一次注入从"等修饰键松开"
/// （最长 600ms）到真正按下粘贴键要一秒多 —— 这段时间里 Windows 完全可能把前台
/// 改回去（点击收尾的延迟激活、系统挑"下一个前台窗口"）。于是按下 Ctrl+V 的那一刻
/// 前台已经不是目标：内容粘进了别的窗口，日志却仍然一路写着"投递成功"。
///
/// 所以这里把 **激活 → 判定 → 按键** 压到零间隔：等修饰键松开之后、写剪贴板之后，
/// **在按下去之前**再激活一次并回读校验；校验不过就直接失败
/// （宁可什么都不做，也不要粘到错误的地方）。
///
/// # 为什么粘贴键必须能按次覆盖（而不是读全局设置）
///
/// codex / claude 这类 TUI 把 **Ctrl+V 绑成了"粘贴图片"**：键事件一到它们手里，
/// 先用图片格式读剪贴板，读不到就报
/// `Failed to paste image: no image on clipboard`，**并且不回落到文本粘贴**
/// （openai/codex #17103："Ctrl+V in TUI is image-only when it reaches Codex as a
/// key event"）。也就是说：**Ctrl+V 只有在被终端自己吃掉时才是文本粘贴**。
///
/// 而"终端吃不吃这个键"是按终端来的（Windows Terminal 吃 Ctrl+Shift+V、mintty 吃
/// Shift+Insert），不是按目标程序来的 —— 程序名在这里派不上用场。所以这个选择
/// **只能由调用方按次给**：全局设置服务的是所有语音注入，这一次不该跟着改。
///
/// 注入一条**命令前缀**到指定窗口：不回车、**不 trim**（两端空白原样保留）。
///
/// # 为什么这里必须不 trim
///
/// "先预置 `codex exec --skip-git-repo-check`、再用语音把内容接上去"这个用法里，
/// 分隔两个词的**唯一**那个空格就在命令末尾。而：
/// - 默认命令本身末尾没有空格（也不该有 —— 存配置时被 `trim` 掉了）；
/// - 语音识别出来的中文也不会自带前导空格。
///
/// 两边一夹，粘出来就是 `--skip-git-repo-check打开计算器` —— 被 shell 当成
/// **一个**参数，命令直接报错。而报错信息看起来像"codex 参数写错了"，
/// 完全不会让人想到是这里的空格没了。所以末尾那个空格**必须**由调用方补上、
/// 且必须活着穿过这一层。
///
/// 与 [`deliver_to_window_submit`] 的分工：那个管"要不要回车"，这个管"折不折空白"，
/// 两件事正交，所以是两个函数，而不是一个函数的一个开关 ——
/// 若合成一个 "mode: enum"，将来加第三种组合又要改签名。
pub fn deliver_to_window_prefix(
    text: &str,
    hwnd: isize,
    style: Option<PasteStyle>,
) -> Result<(), String> {
    deliver_inner(text, Some(hwnd), style, Some(false), true)
}

/// 注入一条**完整**命令到指定窗口并**强制回车**（`submit_after_paste` 全局设置在此被忽略）。
///
/// 用于"自动运行"模式：既然用户明确打开了 auto_run，回车就不该再由另一个开关决定 ——
/// 否则"开了自动运行却停在输入框里"会表现为一个说不清的功能故障。
pub fn deliver_to_window_submit(
    text: &str,
    hwnd: isize,
    style: Option<PasteStyle>,
) -> Result<(), String> {
    deliver_inner(text, Some(hwnd), style, Some(true), false)
}

/// 这一次真正要写进剪贴板的内容。
///
/// 抽成纯函数是为了能被单测钉死：「命令前缀末尾那个空格有没有活下来」这件事
/// 出错时的表现是 *shell 报"参数不对"*，跟注入链路（剪贴板/按键/焦点）八竿子打不着，
/// 但排查的人第一步一定去查注入链路 —— 所以它必须有自己的测试。
fn clipboard_content(text: &str, keep_edges: bool) -> Result<&str, String> {
    let text = if keep_edges { text } else { text.trim() };
    if text.trim().is_empty() {
        return Err("空文本".into());
    }
    Ok(text)
}

/// 四阶段注入的**唯一实现**。所有 `deliver*` 都是这一份的薄封装 ——
/// 拆成多份的代价见 [`send_combo`] 的说明（同一份逻辑分家 ⇒ 只有一处被改）。
///
/// 参数分工：
/// - `style_override`：本次用哪组粘贴键（`None` = 用全局设置）；
/// - `submit_override`：本次要不要回车（`None` = 用全局设置）；
/// - `keep_edges`：`true` = 原样保留首尾空白（命令前缀）；`false` = 先 trim（默认，也是既有行为）。
fn deliver_inner(
    text: &str,
    target: Option<isize>,
    style_override: Option<PasteStyle>,
    submit_override: Option<bool>,
    keep_edges: bool,
) -> Result<(), String> {
    let text = clipboard_content(text, keep_edges)?;

    // stage=admission：等所有修饰键物理松开。
    // 用户刚松开 PTT（如 Ctrl+Shift+Space）时手指常还压着 Ctrl/Shift，
    // 此时注入会把 Ctrl+V 变成 Ctrl+Shift+V（触发应用的其他快捷键）。
    if !wait_keyboard_neutral(Duration::from_millis(600)) {
        // 不静默：接着发出的组合键**可能被目标解释成别的快捷键**。
        // 这类失败偶发、只在某些时候发生、看起来像目标程序的 bug —— 日志里必须先留下
        // 这一行，否则排查会从目标程序开始，方向从一开始就是错的。
        crate::log_buffer::warn(
            "文本注入：等修饰键松开超时（600ms）—— 这次组合键可能被目标解释成别的快捷键"
                .to_string(),
        );
    }

    // ★ 目标窗口模式：按键之前再激活一次。**放在等键之后**——等待期间前台最易易主，
    //   放前面等于白做。
    if let Some(h) = target {
        focus_target_before_paste(h)?;
    }

    // stage=preflight：备份用户剪贴板。
    let backup = backup_clipboard();

    // stage=dispatch：写剪贴板 + 模拟粘贴键。
    set_clipboard_text(text)?;
    let configured = style_override.unwrap_or_else(paste_style);
    // 判定与按键必须紧挨着：中间若被别的窗口抢走焦点，就会把内容粘到错误的地方。
    let effective = resolve_style(configured, foreground_paste_style());
    crate::log_buffer::info(format!(
        "文本注入：粘贴方式 = {}（配置 {}{}）",
        effective.as_str(),
        configured.as_str(),
        match style_override {
            Some(_) => "，本次由投递指定",
            None => "",
        }
    ));
    send_paste(effective)?;

    // stage=terminal：给目标应用充足消费时间，然后恢复原剪贴板。
    // 必须在目标应用消费完之后再 restore，否则未处理的粘贴键会粘到错误内容。
    sleep(Duration::from_millis(1200));
    if let Some(b) = backup {
        // ⚠️ 还原剪贴板失败**不算这次注入失败**：内容已经进目标窗口了。
        // 这里若用 `?`，上层会报"投递失败"，而用户眼前明明是文本进去了 ——
        // 一句假失败比一句警告更贵（它会让人去修一个没坏的东西）。
        if let Err(e) = restore_clipboard(b) {
            crate::log_buffer::warn(format!(
                "文本注入：原剪贴板未能还原（{e}）—— 内容已注入成功，只是剪贴板停在识别文本上"
            ));
        }
    }

    // 可选：粘贴完再按一次回车。
    //
    // 放在 restore 之后：回车与剪贴板无关，放最后就不会和"消费等待"抢时间；
    // 也保证顺序是"内容先落地，再提交"，反过来会提交一个空输入框。
    let submit = submit_override.unwrap_or_else(submit_after_paste);
    if submit {
        sleep(Duration::from_millis(80));
        // 同理：回车没送出去**不等于**这次注入失败（内容已经在输入框里，用户自己按一下即可）。
        if let Err(e) = send_enter() {
            crate::log_buffer::warn(format!(
                "文本注入：粘贴后的回车没有送出去（{e}）—— 内容已在输入框，需要自己按一下回车"
            ));
        }
    }
    Ok(())
}

/// 按下粘贴键之前把焦点钉到 `hwnd`，钉不住就返回错误（调用方据此放弃本次注入）。
#[cfg(windows)]
fn focus_target_before_paste(hwnd: isize) -> Result<(), String> {
    crate::workbench::ensure_foreground(hwnd, Duration::from_millis(300));
    if crate::workbench::foreground_hwnd() != Some(hwnd) {
        return Err(format!(
            "粘贴前焦点没能回到目标窗口 0x{hwnd:x} —— 本次未发送按键（免得粘进别的窗口）"
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn focus_target_before_paste(_hwnd: isize) -> Result<(), String> {
    // 非 Windows 没有前台窗口这套机制，保持"不干预"的既有行为。
    Ok(())
}

/// 等待所有修饰键松开（Ctrl/Shift/Alt/Win），超时返回 false。
/// GetAsyncKeyState 在 0.58 里收 i32，返回 i16，高位为 1 表示按下。
fn wait_keyboard_neutral(timeout: Duration) -> bool {
    const MODIFIER_VKS: [i32; 4] = [0x10, 0x11, 0x12, 0x5B]; // Shift, Ctrl, Alt, LWin
    let deadline = Instant::now() + timeout;
    loop {
        let neutral = MODIFIER_VKS
            .iter()
            .all(|vk| unsafe { GetAsyncKeyState(*vk) } >= 0);
        if neutral {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(10));
    }
}

struct ClipboardBackup {
    text: Option<String>,
    /// 原剪贴板含非文本格式（图像/文件等）——原型阶段无法还原，仅记录。
    had_other_formats: bool,
}

/// 打开剪贴板（带重试，剪贴板管理器等程序常短暂占用）。
fn open_clipboard(retries: u32) -> Result<(), String> {
    for _ in 0..retries {
        if unsafe { OpenClipboard(None) }.is_ok() {
            return Ok(());
        }
        sleep(Duration::from_millis(20));
    }
    Err("剪贴板被占用（重试后仍无法打开）".into())
}

fn backup_clipboard() -> Option<ClipboardBackup> {
    unsafe {
        if open_clipboard(10).is_err() {
            eprintln!("[text_delivery] preflight: 剪贴板被占用，跳过备份");
            return None;
        }
        let text = read_clipboard_text();

        // 枚举格式：CF_TEXT/OEMTEXT/LOCALE 是 UNICODETEXT 的系统合成伴随格式，
        // 出现它们不代表剪贴板有独立于文本的其他内容。
        let mut had_other = false;
        let mut fmt = EnumClipboardFormats(0);
        while fmt != 0 {
            if ![CF_UNICODETEXT, CF_TEXT, CF_OEMTEXT, CF_LOCALE].contains(&fmt) {
                had_other = true;
                break;
            }
            fmt = EnumClipboardFormats(fmt);
        }
        let _ = CloseClipboard();
        Some(ClipboardBackup {
            text,
            had_other_formats: had_other,
        })
    }
}

unsafe fn read_clipboard_text() -> Option<String> {
    let handle = GetClipboardData(CF_UNICODETEXT).ok()?;
    let global = HGLOBAL(handle.0);
    let size = GlobalSize(global);
    let ptr = GlobalLock(global) as *const u16;
    if ptr.is_null() {
        return None;
    }
    let max = size / 2;
    let mut len = 0usize;
    while len < max && *ptr.add(len) != 0 {
        len += 1;
    }
    let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
    let _ = GlobalUnlock(global);
    Some(text)
}

fn set_clipboard_text(text: &str) -> Result<(), String> {
    unsafe {
        open_clipboard(10)?;
        let _ = EmptyClipboard();

        let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let global = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2)
            .map_err(|e| format!("分配剪贴板内存失败: {e}"))?;
        let dst = GlobalLock(global) as *mut u16;
        if dst.is_null() {
            return Err("锁定剪贴板内存失败".into());
        }
        std::ptr::copy_nonoverlapping(units.as_ptr(), dst, units.len());
        let _ = GlobalUnlock(global);

        // 写入成功后内存所有权移交系统，不可再释放。
        if let Err(e) = SetClipboardData(CF_UNICODETEXT, HANDLE(global.0)) {
            let _ = CloseClipboard();
            return Err(format!("写入剪贴板失败: {e}"));
        }
        let _ = CloseClipboard();
        Ok(())
    }
}

fn restore_clipboard(b: ClipboardBackup) -> Result<(), String> {
    if let Some(t) = b.text {
        set_clipboard_text(&t)
    } else if b.had_other_formats {
        // 用户剪贴板原是图片/文件等内容：还原文本会伪造内容，不还原则丢失。
        // 原型取舍：保留识别文本（用户可感知、可撤销），生产版应快照全部格式。
        eprintln!("[text_delivery] terminal: 原剪贴板为非文本内容，未还原");
        Ok(())
    } else {
        unsafe {
            open_clipboard(10)?;
            let _ = EmptyClipboard();
            let _ = CloseClipboard();
            Ok(())
        }
    }
}

/// 一个按键事件。
///
/// # ⛔ `wScan` 必须留 `0` —— 这不是"省事"，是目标程序眼里的一个信号
///
/// `SendInput` 的 `wScan = 0` 到达程序时就是"扫描码 = 0"，而**有些程序用它标记
/// "这条是合成事件"**，并据此走兼容路径。最典型的是 mintty：
///
/// ```c
/// // Workaround for Windows clipboard history pasting simply injecting Ctrl+V
/// // (mintty/wsltty#139)
/// if (key == 'V' && mods == MDK_CTRL && !scancode) { win_paste(); return true; }
/// ```
///
/// 也就是说 **mintty 会为了"被注入的 Ctrl+V"去粘贴，前提正是 `!scancode`**。
/// 09-21 实测（`scripts/e2e_terminal_paste_key.py`，目标就是本机的 mintty）：补上真实扫描码之后
/// 这条分支不再命中，Ctrl+V **立刻从"能粘贴"变成"什么都不做"**。
/// ⇒ 想"发得和真实键盘一样"是好直觉，但在这里会把唯一有效的通路关掉。**已回退。**
///
/// 事件里的 `vk` / `scan` / `extended` 三件事在 mintty 上的实测结论：
/// - 只读 `wVk` 的程序（绝大多数）不受影响；
/// - 补不补 `E0` 扩展位，对 `Shift+Insert` 的成败**没有任何影响**（试过、测过）；
/// - 补上扫描码会**破坏**上面那条注入专用的 Ctrl+V 分支。
///
/// 详见文件头的表与 REFERENCE §11。
fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// 发一个按键事件，并**核对系统真的收下了**。
///
/// # 为什么返回值必须查（而不是"发了就当成功"）
///
/// `SendInput` 返回**实际插入到输入流的事件数**。少于请求数就是失败，最常见的两种原因：
///
/// 1. **目标窗口属于更高完整性级别的进程**（以管理员身份运行的终端 / 编辑器）——
///    UIPI 会静默拦下这次注入。这种情况下一个键都不会到达。
/// 2. 输入桌面被切换（UAC 提示、锁屏、另一个用户的会话）。
///
/// 这两种情况的共同点是：**调用方完全看不出区别**，于是日志写着"投递成功"、
/// 界面写着"已把命令粘到…"，而用户眼前什么都没有。所有"看起来成功的失败"里最贵的一类。
/// 宁可报一句"系统没接受这次注入"，也不要让他去查终端为什么没反应。
fn send_one(input: &INPUT) -> Result<(), String> {
    let n = unsafe { SendInput(&[*input], std::mem::size_of::<INPUT>() as i32) };
    if n == 1 {
        return Ok(());
    }
    Err(format!(
        "系统没有接受这次按键注入（SendInput 只收下 {n}/1 个事件）—— \
         目标窗口可能以管理员身份运行（权限更高的窗口会拦下模拟按键），或输入桌面被切走了"
    ))
}

/// 模拟「修饰键组 + 目标键」的按键序列：依次按下修饰键 → 按键 → 抬键 →
/// **逆序**抬起修饰键。间隔 15ms 保证焦点窗口完整读取。
///
/// **所有组合（Ctrl+V / Shift+Insert / Ctrl+Shift+V / 裸回车）共用这一份序列逻辑**。
/// 分成几份写的代价是：将来只有一处被改动（比如调间隔、加去抖）时，会变成
/// "Ctrl+V 正常、Ctrl+Shift+V 偶发丢键"这种只在特定窗口复现、且看起来像
/// "终端 bug" 的问题 —— 与 `audio_out` 里"三种采样格式共用一份游标逻辑"是同一个理由。
///
/// 抬修饰键用逆序：与按下顺序对称，避免系统在"Shift 已松、Ctrl 还压着"的中间态
/// 把后续输入解释成别的意思。
///
/// 返回 `Err` = **这一串里至少有一个键没送出去**。调用方必须把它当成"操作没生效"，
/// 不能吞掉 —— 理由见 [`send_one`]。
fn send_combo(modifiers: &[VIRTUAL_KEY], key: VIRTUAL_KEY) -> Result<(), String> {
    for m in modifiers {
        send_one(&key_input(*m, false))?;
        sleep(Duration::from_millis(15));
    }
    send_one(&key_input(key, false))?;
    sleep(Duration::from_millis(15));
    send_one(&key_input(key, true))?;
    sleep(Duration::from_millis(15));
    for m in modifiers.iter().rev() {
        send_one(&key_input(*m, true))?;
        sleep(Duration::from_millis(15));
    }
    Ok(())
}

/// 按指定方式发出粘贴键。
fn send_paste(style: PasteStyle) -> Result<(), String> {
    match style {
        PasteStyle::ShiftInsert => send_combo(&[VK_SHIFT], VK_INSERT),
        PasteStyle::CtrlShiftV => send_combo(&[VK_CONTROL, VK_SHIFT], VK_V),
        // `Auto` 不该走到这里（[`resolve_style`] 已经把它解开了）。
        // 真走到这里说明解析有 bug —— 但那也只会表现为"用了 Ctrl+V"，
        // 即改造前的既有行为，不会更糟。刻意不给它一个 panic 分支。
        PasteStyle::CtrlV | PasteStyle::Auto => send_combo(&[VK_CONTROL], VK_V),
    }
}

/// 按一次回车（用于"粘贴后提交"）。
fn send_enter() -> Result<(), String> {
    send_combo(&[], VK_RETURN)
}

/// 单独按一次回车（不带任何修饰键）。
///
/// # 它存在的唯一理由：把一次已经被吞掉、却没人接管的回车还回去
///
/// 键盘钩子的做法是"先按判据决定吞不吞，再报给会话层"，两者之间隔着一次线程投递。
/// 判据**有可能在这期间变化**（恰好那几十毫秒里语音交付完成了），于是出现
/// "键已经被吞、会话层却说这事不归我管"的状态。此时若什么都不做，用户就白按了一下
/// 回车 —— 现象是"按了没反应"，最难查的那类故障。
///
/// 宁可多发一个回车（语义与用户本意一致），也不要静默吃键。
pub fn press_enter() {
    // 这里只记日志、不往外抛：调用方（回车接管判据竞态）已经在赶时间了，
    // 没有第二件该做的事。但**绝不能静默** —— 那一下回车是欠用户的。
    if let Err(e) = send_enter() {
        crate::log_buffer::warn(format!(
            "补发回车失败：{e} —— 用户那一下回车没能还给他，请手动按一次"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ⚠️ 这里**只测纯函数**，不测 `set_paste_style` / `paste_style` 那对全局读写。
    // 全局是进程级共享状态，而 cargo test 默认**并发**跑用例 ——
    // 两个用例同时读改写会互相踩，表现为"单独跑绿、一起跑偶发红"。
    // 这类测试要么加互斥、要么别写；这里的判定逻辑全在纯函数里，不必付出那个代价。

    /// ⚠️ **这张表记的是"程序模拟能生效"的键**，测的目标是本机的终端可执行文件。
    ///
    /// mintty 那一条是血泪教训：人手按 `Shift+Ins` 能粘贴，程序模拟却什么都不会发生
    /// （`get_mods()` 查泛用 `VK_SHIFT`，注入的 shift 到不了那个位）；
    /// 而 `Ctrl+V` 程序模拟能粘贴（mintty 有专门的注入兼容分支，要求 `scancode == 0`）。
    /// **改这张表等于改实测结论，必须同步实测。**
    #[test]
    fn measured_terminals_map_to_their_own_paste_key() {
        assert_eq!(
            paste_style_for_class("CASCADIA_HOSTING_WINDOW_CLASS"), // Windows Terminal
            Some(PasteStyle::CtrlShiftV),
            "Windows Terminal：人手实测是 Ctrl+Shift+V（Shift+Insert 不保证可用）"
        );
        assert_eq!(
            paste_style_for_class("mintty"), // Git Bash / MSYS2
            Some(PasteStyle::CtrlV),
            "⛔ 别改回 ShiftInsert：那是**人手**能用的键，程序模拟一个键都到不了目标。\
             能生效的是 Ctrl+V（前提 wScan = 0，见 key_input 的注释）"
        );
    }

    #[test]
    fn class_matching_ignores_case() {
        // 不同实现/不同 Windows 版本的写法不完全一致，赌它的一致性没有收益。
        assert!(paste_style_for_class("MINTTY").is_some());
        assert!(paste_style_for_class("cascadia_hosting_window_class").is_some());
    }

    #[test]
    fn gui_classes_are_not_mistaken_for_terminals() {
        for c in ["Chrome_WidgetWin_1", "Notepad", "WeChatMainWndForPC", ""] {
            assert!(
                paste_style_for_class(c).is_none(),
                "{c} 不是已知终端，不该命中"
            );
        }
    }

    #[test]
    fn unmeasured_terminals_fall_back_to_ctrl_v() {
        // 没实测过的终端（conhost / ConEmu）不进表 ⇒ 走 Ctrl+V（改造前的既有行为）。
        // 关键：**不猜**。猜一个"终端惯例"上去，错的时候表现为"换了个终端就粘不进去"。
        assert_eq!(paste_style_for_class("ConsoleWindowClass"), None);
        assert_eq!(paste_style_for_class("VirtualConsoleClass"), None);
        assert_eq!(
            resolve_style(PasteStyle::Auto, paste_style_for_class("ConsoleWindowClass")),
            PasteStyle::CtrlV
        );
    }

    /// ⛔ 护的是"表里没复核过的项，必须在日志里被点名"。
    /// 反过来的错（把已实测的项标成未复核）同样有害：每次投递都刷一句假警告，
    /// 警告一旦变成常态就没人看了 —— 那和没有警告是一样的。
    #[test]
    fn unverified_terminals_are_flagged_and_the_measured_one_is_not() {
        assert!(
            !paste_key_verified("CASCADIA_HOSTING_WINDOW_CLASS"),
            "Windows Terminal 那条是人肉测的，必须被标成未复核"
        );
        assert!(
            !paste_key_verified("cascadia_hosting_window_class"),
            "比较必须大小写不敏感，否则同一类名换个写法就不报警了"
        );
        assert!(
            paste_key_verified("mintty"),
            "mintty 是 09-21 用程序模拟逐个实测过的，标成未复核等于刷假警告"
        );
        // 不在表里的类根本不走表 ⇒ 谈不上复核 ⇒ 不该报警
        assert!(paste_key_verified("ConsoleWindowClass"));
        assert!(paste_key_verified(""));
    }

    #[test]
    fn auto_uses_the_key_the_focus_window_was_measured_with() {
        // Auto 不是"终端就换一组键"，而是"用这个窗口实测可用的那一组"。
        assert_eq!(
            resolve_style(PasteStyle::Auto, Some(PasteStyle::CtrlShiftV)),
            PasteStyle::CtrlShiftV
        );
        assert_eq!(
            resolve_style(PasteStyle::Auto, Some(PasteStyle::ShiftInsert)),
            PasteStyle::ShiftInsert
        );
        // 认不出来 ⇒ Ctrl+V（既有行为，不是新的坏行为）。
        assert_eq!(resolve_style(PasteStyle::Auto, None), PasteStyle::CtrlV);
    }

    #[test]
    fn explicit_styles_override_the_focus_window() {
        // 自动判定的价值是"省配置"，不是"覆盖用户意愿"——显式选了就照办。
        assert_eq!(
            resolve_style(PasteStyle::CtrlV, Some(PasteStyle::ShiftInsert)),
            PasteStyle::CtrlV
        );
        assert_eq!(resolve_style(PasteStyle::ShiftInsert, None), PasteStyle::ShiftInsert);
    }

    #[test]
    fn style_string_and_code_round_trip() {
        for s in [
            PasteStyle::Auto,
            PasteStyle::CtrlV,
            PasteStyle::ShiftInsert,
            PasteStyle::CtrlShiftV,
        ] {
            assert_eq!(PasteStyle::parse(s.as_str()), Some(s), "{s:?} 应能解析回自己");
            assert_eq!(PasteStyle::from_code(s.code()), s, "{s:?} 编码应能还原");
        }
    }

    #[test]
    fn parse_accepts_the_symbols_users_actually_type() {
        // 用户手写配置会写 Ctrl+V / Shift+Ins 这种形式，不该因为分隔符被判成无效。
        assert_eq!(PasteStyle::parse("Ctrl+V"), Some(PasteStyle::CtrlV));
        assert_eq!(PasteStyle::parse("shift+insert"), Some(PasteStyle::ShiftInsert));
        assert_eq!(PasteStyle::parse("shift_ins"), Some(PasteStyle::ShiftInsert));
        assert_eq!(PasteStyle::parse("Ctrl+Shift+V"), Some(PasteStyle::CtrlShiftV));
    }

    #[test]
    fn unrecognised_style_is_rejected_not_silently_defaulted() {
        // 关键：看不懂必须返回 None。若"看不懂就当 Auto"，用户把配置写错时
        // 行为会**毫无痕迹地**改变，排查时根本想不到去查这一行。
        assert_eq!(PasteStyle::parse("shift-insert"), None);
        assert_eq!(PasteStyle::parse(""), None);
        assert_eq!(PasteStyle::parse("ctrlV"), None);
    }

    #[test]
    fn default_is_auto() {
        // 默认值必须是 Auto：让用户在"打微信"和"打终端"之间手动切是错误的方向。
        assert_eq!(PasteStyle::default(), PasteStyle::Auto);
        assert_eq!(PasteStyle::from_code(0), PasteStyle::Auto);
    }

    #[test]
    fn submit_after_paste_defaults_to_off() {
        // ⛔ 这条断言护的是"提交权归用户"。它曾经是 `assert!(...)`（默认开），
        // 改成关是有意的（见文件头）—— 若将来有人要把它翻回去，必须同时改这里，
        // 而这一行就是让他停下来读那段理由的路障。
        assert!(!DEFAULT_SUBMIT_AFTER_PASTE);
    }

    /// ⛔ 这一条护的是"命令与语音之间的那个空格"。
    ///
    /// 它没了的时候，shell 会报一个看起来像"codex 参数写错"的错，
    /// 而代码里唯一可疑的地方却是注入链路 —— 于是排查方向从一开始就是错的。
    #[test]
    fn prefix_mode_keeps_the_separating_space() {
        assert_eq!(
            clipboard_content("codex exec --skip-git-repo-check ", true).unwrap(),
            "codex exec --skip-git-repo-check ",
            "前缀模式必须保住末尾空格，否则语音内容会与命令黏成一个参数"
        );
        assert_eq!(
            clipboard_content("  codex exec  ", true).unwrap(),
            "  codex exec  ",
            "前缀模式下两端空白一律原样保留"
        );
    }

    #[test]
    fn default_mode_still_trims() {
        assert_eq!(clipboard_content("  hi  ", false).unwrap(), "hi");
    }

    #[test]
    fn blank_content_is_rejected_in_both_modes() {
        // 纯空白在前缀模式下也不能放行：粘一个空格进去会让用户以为"按键坏了"。
        assert!(clipboard_content("   ", true).is_err());
        assert!(clipboard_content("   ", false).is_err());
    }

    // -----------------------------------------------------------------------
    // 发键方式：必须和真实键盘一样（v0.30.3）
    // -----------------------------------------------------------------------

}
