//! 窗口识别：一个窗口叫什么、现在前台是不是个终端。
//!
//! # 范围收窄（v0.35.0）
//!
//! 本模块原本还承担「出口 A / A″ —— 把命令行投进用户正在用的终端」那一侧的目标解析
//! 与准入策略（`resolve_target` / `TargetPolicy` / `target_allowed` / `current_label`）。
//! 整套出口 A 已在 v0.35.0 移除（按键改走后台队列），因此这里只剩**「启动终端」那条路
//! 仍然需要**的通用能力：
//!
//! - [`WindowLabel`] —— 一个窗口的人类可读标签（类名 + 标题）
//! - [`is_known_terminal`] —— 类名是不是"已实测的终端"
//! - [`wait_for_terminal_foreground`] —— **等**到前台落在某个已知终端上（带超时）
//!
//! # ⛔ "什么算已知终端"只允许有一份判据
//!
//! 判据就是 [`crate::text_delivery::paste_style_for_class`]（那张"终端类名 → 该按哪个
//! 粘贴键"的表）。另立一张表的后果是**两边都看不出错**：等窗口时认得的终端、
//! 该粘贴时不认得（或反之），表现为"明明等到终端了却没打进去"这类无从查起的现象。
//!
//! # 为什么不需要"排除 easyop 自己"
//!
//! 这里的判据是**白名单**（类名必须在终端表里），而 easyop 自己的窗口不在表中 ——
//! 排除是白名单的天然结果，再写一遍只是多一处可能与主判据漂移的地方。

use std::time::{Duration, Instant};

/// 轮询间隔。
///
/// 30ms 是权衡：`GetForegroundWindow` 是极轻的无参查询，但"窗口刚起来还没拿到前台"
/// 的窗口期通常是几十到几百毫秒 —— 间隔大了会让用户感到"按键之后卡了一下"。
const POLL_INTERVAL: Duration = Duration::from_millis(30);

/// 一个窗口的人类可读标签。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowLabel {
    /// 窗口类名（`GetClassNameW` 的返回值）。
    ///
    /// 用类名而不是进程名：同一个终端（Windows Terminal / mintty / conhost）在不同
    /// Windows 版本上进程名可能不同，类名却相对稳定，且它正是粘贴键判据的输入。
    pub class: String,
    /// 窗口标题。读不到就是空串（不是错误 —— 无标题窗口合法存在）。
    pub title: String,
}

impl WindowLabel {
    /// 给人看的一行描述。
    pub fn display(&self) -> String {
        if self.title.trim().is_empty() {
            format!("[{}]", self.class)
        } else {
            format!("{}（{}）", self.title, self.class)
        }
    }
}

/// 类名是不是"已实测的终端"。
///
/// 判据见模块文档：唯一的真源是 `text_delivery` 的粘贴键表。
pub fn is_known_terminal(class: &str) -> bool {
    crate::text_delivery::paste_style_for_class(class).is_some()
}

/// 等到**前台**落在某个已知终端上，返回它的 hwnd 与标签；超时返回 `None`。
///
/// `timeout` 给 [`Duration::ZERO`] 表示**只查一次、不等待** —— 调用方要的是
/// "现在是不是已经在终端里了"这个瞬时判断，而不是"等它变成终端"。
///
/// # 返回 `None` 的三种情形（都不该被当成"终端坏了"）
///
/// 1. 超时了还是没有终端拿到前台（用户切走了、终端起得慢）；
/// 2. 前台窗口存在但不是终端（浏览器、编辑器……）；
/// 3. 连前台窗口都读不到（锁屏、切换瞬间）。
///
/// 调用方必须把 `None` 当成**正常的一种结果**处理 —— 三条路上该做的事都是
/// "别往里打，等用户自己点一下"，而不是重试或报错。
pub fn wait_for_terminal_foreground(timeout: Duration) -> Option<(isize, WindowLabel)> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(hit) = foreground_terminal() {
            return Some(hit);
        }
        // 先查后判超时：`Duration::ZERO` 因此等价于"只查一次"，不必特判。
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// 当前前台窗口，若它是已知终端。
#[cfg(windows)]
fn foreground_terminal() -> Option<(isize, WindowLabel)> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    // SAFETY: `GetForegroundWindow` 无参数、不阻塞；返回可能为 NULL 的 HWND，
    // 由 `is_invalid()` 判定。下面读类名/标题时 hwnd 已确认有效。
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }
    let class = window_class(hwnd)?;
    if !is_known_terminal(&class) {
        return None;
    }
    Some((
        hwnd.0 as isize,
        WindowLabel {
            class,
            title: window_title(hwnd).unwrap_or_default(),
        },
    ))
}

/// 非 Windows：没有 HWND 这回事，永远等不到终端。
#[cfg(not(windows))]
fn foreground_terminal() -> Option<(isize, WindowLabel)> {
    None
}

/// 读窗口类名。
#[cfg(windows)]
fn window_class(hwnd: windows::Win32::Foundation::HWND) -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;

    let mut buf = [0u16; 256];
    // SAFETY: buf 是本地栈数组，长度由字面量给出；hwnd 由调用方确认有效。
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    if n <= 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..n as usize]))
}

/// 读窗口标题。
#[cfg(windows)]
fn window_title(hwnd: windows::Win32::Foundation::HWND) -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;

    let mut buf = [0u16; 512];
    // SAFETY: 同上。标题可能为空（无标题窗口）—— 那是空串，不是错误。
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if n <= 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..n as usize]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_prefers_title_but_falls_back_to_class() {
        let with = WindowLabel {
            class: "CASCADIA_HOSTING_WINDOW_CLASS".into(),
            title: "codex".into(),
        };
        assert!(with.display().contains("codex"));
        assert!(with.display().contains("CASCADIA_HOSTING_WINDOW_CLASS"));

        let without = WindowLabel {
            class: "mintty".into(),
            title: "".into(),
        };
        // 没标题也必须能认出来是哪个类 —— 显示成空行等于没给信息。
        assert!(without.display().contains("mintty"));
    }

    #[test]
    fn known_terminal_agrees_with_the_paste_key_table() {
        // 判据必须是"粘贴键表里有没有它"，不是另立一张表。
        for class in ["CASCADIA_HOSTING_WINDOW_CLASS", "mintty", "ConsoleWindowClass"] {
            assert_eq!(
                is_known_terminal(class),
                crate::text_delivery::paste_style_for_class(class).is_some(),
                "{class} 的判定与粘贴键表不一致"
            );
        }
        assert!(!is_known_terminal("Chrome_WidgetWin_1"));
        assert!(!is_known_terminal(""));
    }

    #[test]
    fn zero_timeout_never_blocks() {
        // 零超时必须立刻返回（不等一轮 poll），否则"现在是不是在终端里"这个
        // 瞬时查询会变成 30ms 的卡顿。
        let t = Instant::now();
        let _ = wait_for_terminal_foreground(Duration::ZERO);
        assert!(t.elapsed() < Duration::from_millis(25), "零超时不该等一轮轮询");
    }
}
