//! 剪贴板工具：选中文本提取与替换。
//!
//! 语音编辑流程的核心辅助模块：
//! - `get_selected_text()`：模拟 Ctrl+C → 读剪贴板 → 恢复原剪贴板，获取用户选中的文本。
//! - `replace_selected_text()`：设置新文本到剪贴板 → 模拟 Ctrl+V 替换选区。
//!
//! 所有操作都保存/恢复原剪贴板内容，对用户透明。

use std::thread::sleep;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VK_CONTROL, VIRTUAL_KEY,
};

const CF_UNICODETEXT: u32 = 13;
const VK_V: VIRTUAL_KEY = VIRTUAL_KEY(0x56);

/// 提取用户当前选中的文本：模拟 Ctrl+C → 读剪贴板 → 恢复原剪贴板。
///
/// 返回选中文本；如果无选中文本（剪贴板未变化或为空）返回 Err。
pub fn get_selected_text() -> Result<String, String> {
    let backup = backup_clipboard_text();

    set_clipboard_text("")?;

    send_ctrl_c();

    let deadline = Instant::now() + Duration::from_millis(800);
    let mut selected = String::new();
    while Instant::now() < deadline {
        sleep(Duration::from_millis(20));
        if open_clipboard(3).is_ok() {
            let text = unsafe { read_clipboard_text() };
            let _ = unsafe { CloseClipboard() };
            if let Some(text) = text {
                if !text.is_empty() {
                    selected = text;
                    break;
                }
            }
        }
    }

    if let Some(b) = backup {
        let _ = set_clipboard_text(&b);
    }

    if selected.is_empty() {
        Err("未检测到选中文本，请先在目标应用中选中要编辑的文本".to_string())
    } else {
        Ok(selected)
    }
}

/// 替换当前选中文本：设置新文本到剪贴板 → 模拟 Ctrl+V。
///
/// 调用前需确保目标应用中仍有选区（用户在 LLM 处理期间不应移动光标）。
pub fn replace_selected_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Err("替换文本为空".to_string());
    }

    let backup = backup_clipboard_text();

    set_clipboard_text(text)?;

    send_ctrl_v();

    sleep(Duration::from_millis(800));

    if let Some(b) = backup {
        let _ = set_clipboard_text(&b);
    }

    Ok(())
}

fn backup_clipboard_text() -> Option<String> {
    unsafe {
        if open_clipboard(10).is_err() {
            return None;
        }
        let text = read_clipboard_text();
        let _ = CloseClipboard();
        text
    }
}

unsafe fn read_clipboard_text() -> Option<String> {
    use windows::Win32::System::DataExchange::GetClipboardData;
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize};

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

fn open_clipboard(retries: u32) -> Result<(), String> {
    for _ in 0..retries {
        if unsafe { OpenClipboard(None) }.is_ok() {
            return Ok(());
        }
        sleep(Duration::from_millis(20));
    }
    Err("剪贴板被占用".to_string())
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
            return Err("锁定剪贴板内存失败".to_string());
        }
        std::ptr::copy_nonoverlapping(units.as_ptr(), dst, units.len());
        let _ = GlobalUnlock(global);

        if let Err(e) = SetClipboardData(CF_UNICODETEXT, HANDLE(global.0)) {
            let _ = CloseClipboard();
            return Err(format!("写入剪贴板失败: {e}"));
        }
        let _ = CloseClipboard();
        Ok(())
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

fn send_ctrl_c() {
    unsafe {
        let vk_c = VIRTUAL_KEY(0x43);
        SendInput(&[key_input(VK_CONTROL, false)], std::mem::size_of::<INPUT>() as i32);
        sleep(Duration::from_millis(20));
        SendInput(&[key_input(vk_c, false)], std::mem::size_of::<INPUT>() as i32);
        sleep(Duration::from_millis(20));
        SendInput(&[key_input(vk_c, true)], std::mem::size_of::<INPUT>() as i32);
        sleep(Duration::from_millis(20));
        SendInput(&[key_input(VK_CONTROL, true)], std::mem::size_of::<INPUT>() as i32);
    }
    sleep(Duration::from_millis(100));
}

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
