//! 文本注入 —— 复刻 easyop 逆向分析得到的 text_delivery.v1 四阶段流程：
//!
//!   admission（等键盘空闲）→ preflight（备份剪贴板）→ dispatch（写剪贴板
//!   + 模拟 Ctrl+V）→ terminal（等目标应用取走内容后恢复剪贴板）。
//!
//! 为什么用剪贴板而不是 KEYEVENTF_UNICODE 逐字符注入：
//!   - 兼容不处理 Unicode 键盘事件的程序；
//!   - 多行长文本的注入耗时与文本长度无关；
//!   - 行为与真实用户粘贴一致，目标应用零特判。
//!
//! 防递归：模拟的 Ctrl+V 带 LLKHF_INJECTED 标志，ptt_hook 见标志即放行。

use std::thread::sleep;
use std::time::{Duration, Instant};

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
    KEYEVENTF_KEYUP, VK_CONTROL, VIRTUAL_KEY,
};

const CF_UNICODETEXT: u32 = 13;
const CF_TEXT: u32 = 1;
const CF_OEMTEXT: u32 = 7;
const CF_LOCALE: u32 = 16;
const VK_V: VIRTUAL_KEY = VIRTUAL_KEY(0x56);

/// 四阶段完整注入。阻塞约 1.2~1.8s（含键盘空闲等待与消费等待）。
///
/// 消费等待（terminal sleep）取 1200ms：Ctrl+V 是异步按键，目标应用需时间处理粘贴。
/// 若等待不足即 restore 剪贴板，目标应用可能粘贴到恢复后的内容或下一次注入的内容，
/// 导致残留/重复粘贴。1200ms 同时保证连续注入间隔 ≥ 1s，避免竞态。
pub fn deliver(text: &str) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("空文本".into());
    }

    // stage=admission：等所有修饰键物理松开。
    // 用户刚松开 PTT（如 Ctrl+Shift+Space）时手指常还压着 Ctrl/Shift，
    // 此时注入会把 Ctrl+V 变成 Ctrl+Shift+V（触发应用的其他快捷键）。
    let _ = wait_keyboard_neutral(Duration::from_millis(600));

    // stage=preflight：备份用户剪贴板。
    let backup = backup_clipboard();

    // stage=dispatch：写剪贴板 + 模拟 Ctrl+V。
    set_clipboard_text(text)?;
    send_ctrl_v();

    // stage=terminal：给目标应用充足消费时间，然后恢复原剪贴板。
    // 必须在目标应用消费完之后再 restore，否则未处理的 Ctrl+V 会粘贴到错误内容。
    sleep(Duration::from_millis(1200));
    if let Some(b) = backup {
        restore_clipboard(b)?;
    }
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

/// 模拟 Ctrl+V 四步按键序列。间隔 15ms 保证焦点窗口完整读取。
fn send_ctrl_v() {
    unsafe {
        SendInput(&[key_input(VK_CONTROL, false)], std::mem::size_of::<INPUT>() as i32);
        sleep(Duration::from_millis(15));
        SendInput(&[key_input(VK_V, false)], std::mem::size_of::<INPUT>() as i32);
        sleep(Duration::from_millis(15));
        SendInput(&[key_input(VK_V, true)], std::mem::size_of::<INPUT>() as i32);
        sleep(Duration::from_millis(15));
        SendInput(&[key_input(VK_CONTROL, true)], std::mem::size_of::<INPUT>() as i32);
    }
}
