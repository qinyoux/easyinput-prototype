//! 打开本机目标：可执行程序 / 文件 / URL / 文件夹。
//!
//! Host Action v1 在固件端只是一个 UUID；真正「打开什么」由本机应用在收到
//! 0x11（kind 0x05）后，依据应用库按 UUID 解析并调用本模块打开。

use crate::log_buffer;
use crate::model::{AppEntry, AppTarget};
use std::process::Command;

/// 打开一个应用库条目；返回 `Ok(启动方式说明)` 或错误。
pub fn open_entry(entry: &AppEntry) -> Result<String, String> {
    log_buffer::info(format!("launcher: 打开「{}」", entry.title));
    match &entry.target {
        AppTarget::App { path, args, work_dir } => launch_app(path, args, work_dir),
        AppTarget::File { path } => shell_open(&path),
        AppTarget::Url { url } => shell_open(url),
        AppTarget::Folder { path } => shell_open(path),
    }
}

/// 按 UUID 在应用库中查找并打开。
pub fn open_by_uuid(apps: &[AppEntry], uuid: &str) -> Result<String, String> {
    let entry = apps
        .iter()
        .find(|e| e.uuid.eq_ignore_ascii_case(uuid))
        .ok_or_else(|| format!("未找到 UUID「{uuid}」对应的应用，请先在应用库中添加"))?;
    open_entry(entry)
}

/// 启动一个可执行程序，可带参数与工作目录。
///
/// 若路径以 `.lnk` / `.url` 结尾（Windows 快捷方式），交给系统默认方式打开：
/// 快捷方式内嵌了它自身的目标与参数，无需额外解析，直接启动即可。
/// 若路径以 `.bat` / `.cmd` 结尾（批处理），通过 `cmd.exe /C` 启动，
/// 因为 `CreateProcessW` 不支持直接执行批处理文件。
fn launch_app(path: &str, args: &str, work_dir: &str) -> Result<String, String> {
    let cleaned = clean_path(path);
    if cleaned.is_empty() {
        return Err("程序路径不能为空".into());
    }
    if is_shortcut_path(&cleaned) {
        // .lnk / .url：交给 ShellExecuteW 以系统默认方式打开，
        // 复用快捷方式内嵌的目标与参数，无需额外解析。
        log_buffer::info(format!("launcher: 快捷方式 → ShellExecuteW path={cleaned:?}"));
        return shell_open(&cleaned);
    }
    let prog = std::path::Path::new(&cleaned);
    if !prog.exists() {
        // Path::exists() 可能因不可见字符、长路径（>260）等原因返回 false。
        // 记录详细诊断信息，然后回退到 ShellExecuteW（更健壮，不受 MAX_PATH 限制）。
        log_buffer::warn(format!(
            "launcher: Path::exists()=false path={cleaned:?} len={} bytes={:02X?}",
            cleaned.len(),
            cleaned.as_bytes()
        ));
        if !is_batch_path(&cleaned) {
            // 非 .bat/.cmd：尝试 ShellExecuteW 回退打开。
            return shell_open(&cleaned).map_err(|e| {
                log_buffer::err(format!("launcher: 回退 shell_open 也失败 path={cleaned:?} err={e}"));
                format!("程序路径不存在：{cleaned}（ShellExecuteW 也失败：{e}）")
            });
        }
        // .bat/.cmd：cmd.exe /C 自身会报文件不存在，无需前置拦截。
        log_buffer::warn(format!("launcher: batch 文件跳过 exists 检查，直接 cmd.exe /C path={cleaned:?}"));
    }
    let cleaned_work_dir = clean_path(work_dir);
    log_buffer::info(format!(
        "launcher: 启动 path={cleaned:?} args={args:?} work_dir={cleaned_work_dir:?}"
    ));
    let result = if is_batch_path(&cleaned) {
        // .bat / .cmd：CreateProcessW 无法直接执行，通过 cmd.exe /C 启动。
        let mut cmd = Command::new("cmd.exe");
        cmd.arg("/C").arg(&cleaned).args(split_args(args));
        if !cleaned_work_dir.is_empty() && std::path::Path::new(&cleaned_work_dir).is_dir() {
            cmd.current_dir(&cleaned_work_dir);
        }
        cmd.spawn()
    } else {
        let mut cmd = Command::new(prog);
        if !cleaned_work_dir.is_empty() && std::path::Path::new(&cleaned_work_dir).is_dir() {
            cmd.current_dir(&cleaned_work_dir);
        }
        cmd.args(split_args(args));
        cmd.spawn()
    };
    match result {
        Ok(child) => {
            log_buffer::info(format!("launcher: spawn 成功 pid={}", child.id()));
            Ok(format!("已启动应用：{cleaned}"))
        }
        Err(e) => {
            log_buffer::err(format!("launcher: spawn 失败 path={cleaned:?} err={e}"));
            Err(format!("无法启动「{cleaned}」：{e}"))
        }
    }
}

/// 清理路径：去首尾空白，去首尾配对引号（`"` 或 `'`），过滤不可见 Unicode 字符。
///
/// 不可见字符（BOM、零宽空格、双向控制符等）常从复制粘贴混入路径，
/// 导致 `Path::exists()` 和 `CreateProcessW` 静默失败。
fn clean_path(p: &str) -> String {
    let stripped: String = p.chars().filter(|&c| !is_invisible_char(c)).collect();
    let t = stripped.trim();
    let bytes = t.as_bytes();
    if bytes.len() >= 2 && (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"'
        || bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
    {
        t[1..t.len() - 1].trim().to_string()
    } else {
        t.to_string()
    }
}

/// 判断一个字符是否为不可见 Unicode 字符（不应出现在文件路径中）。
fn is_invisible_char(c: char) -> bool {
    matches!(c,
        '\u{FEFF}'      // BOM / Zero Width No-Break Space
        | '\u{200B}'    // Zero Width Space
        | '\u{200C}'    // Zero Width Non-Joiner
        | '\u{200D}'    // Zero Width Joiner
        | '\u{200E}'    // Left-to-Right Mark
        | '\u{200F}'    // Right-to-Left Mark
        | '\u{202A}'    // Left-to-Right Embedding
        | '\u{202B}'    // Right-to-Left Embedding
        | '\u{202C}'    // Pop Directional Formatting
        | '\u{202D}'    // Left-to-Right Override
        | '\u{202E}'    // Right-to-Left Override
        | '\u{2060}'    // Word Joiner
        | '\u{2061}'    // Function Application
        | '\u{2062}'    // Invisible Times
        | '\u{2063}'    // Invisible Separator
        | '\u{2064}'    // Invisible Plus
    )
}

/// 判断一个路径是否为批处理文件（.bat / .cmd，大小写不敏感）。
fn is_batch_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".bat") || lower.ends_with(".cmd")
}

/// 用系统默认方式打开（文件/URL/文件夹/快捷方式）。
///
/// 实现：调用 Win32 `ShellExecuteExW`（`open` 动词）。这是系统「打开方式」的标准
/// 入口，Unicode 友好，能正确处理含中文/空格的路径，且能复用 `.lnk` / `.url`
/// 快捷方式内嵌的目标与参数。比 `cmd /C start` 更可靠——后者经 cmd.exe 的
/// OEM 代码页转码，含非 ASCII 字符的路径会被破坏成乱码而静默失败。
///
/// 注意：必须显式把工作目录设为文件所在的父目录，否则某些应用（如
/// LibreOffice）会在 easyop 自己的工作目录里尝试创建锁文件，导致"无法
/// 创建加锁文件 / 缺少权限"之类的报错。
pub(crate) fn shell_open(target: &str) -> Result<String, String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    if target.trim().is_empty() {
        return Err("打开目标不能为空".into());
    }
    let target_wide: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();

    // 计算工作目录：文件/文件夹所在的父目录。
    // URL（含 :// 或 http 开头）不设工作目录。
    let is_url = target.contains("://")
        || target.starts_with("http:")
        || target.starts_with("https:");
    let work_dir_wide: Vec<u16> = if is_url {
        Vec::new()
    } else {
        std::path::Path::new(target)
            .parent()
            .map(|p| {
                let s = p.to_string_lossy().to_string();
                if s.is_empty() {
                    // 根目录（如 C:\）的 parent 是空串，直接用目标本身
                    target.encode_utf16().chain(std::iter::once(0)).collect()
                } else {
                    s.encode_utf16().chain(std::iter::once(0)).collect()
                }
            })
            .unwrap_or_default()
    };

    let mut sei = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC,
        hwnd: windows::Win32::Foundation::HWND(std::ptr::null_mut()),
        lpVerb: PCWSTR::null(),   // 默认动词（通常是 "open"）
        lpFile: PCWSTR(target_wide.as_ptr()),
        lpParameters: PCWSTR::null(),
        lpDirectory: if work_dir_wide.is_empty() {
            PCWSTR::null()
        } else {
            PCWSTR(work_dir_wide.as_ptr())
        },
        nShow: SW_SHOWNORMAL.0 as i32,
        ..Default::default()
    };

    let result = unsafe { ShellExecuteExW(&mut sei) };
    if result.is_err() {
        // 失败时尝试拿更详细的错误码
        let err = unsafe { windows::Win32::Foundation::GetLastError() };
        let code = err.0 as i32;
        log_buffer::err(format!(
            "launcher: ShellExecuteExW 失败 target={target:?} err={code}"
        ));
        return Err(format!("无法打开「{target}」（错误码 {code}）"));
    }
    log_buffer::info(format!("launcher: ShellExecuteExW 成功 target={target:?}"));
    Ok(format!("已打开：{target}"))
}

/// 判断一个路径是否为 Windows 快捷方式（.lnk / .url，大小写不敏感）。
fn is_shortcut_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".lnk") || lower.ends_with(".url")
}

/// 简单参数切分：按空白分割，支持双引号包裹的成段参数。
fn split_args(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut chars = args.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => in_quote = !in_quote,
            c if c.is_whitespace() && !in_quote => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_args_basic() {
        assert_eq!(split_args("a b \"c d\" e"), vec!["a", "b", "c d", "e"]);
        assert_eq!(split_args(""), Vec::<String>::new());
    }

    #[test]
    fn clean_path_strips_quotes_and_whitespace() {
        assert_eq!(clean_path("  C:\\app\\foo.exe  "), "C:\\app\\foo.exe");
        assert_eq!(clean_path("\"C:\\Program Files\\app.exe\""), "C:\\Program Files\\app.exe");
        assert_eq!(clean_path("'C:\\my bat\\run.bat'"), "C:\\my bat\\run.bat");
        assert_eq!(clean_path("  \"  C:\\spaced.exe  \"  "), "C:\\spaced.exe");
        assert_eq!(clean_path(""), "");
        assert_eq!(clean_path("   "), "");
    }

    #[test]
    fn clean_path_strips_invisible_chars() {
        // BOM + zero-width space 混入路径
        assert_eq!(clean_path("\u{FEFF}E:\\app\\foo.exe\u{200B}"), "E:\\app\\foo.exe");
        // 路径中间有零宽字符
        assert_eq!(clean_path("E:\\app\u{200C}\\foo.exe"), "E:\\app\\foo.exe");
        // 双向控制符
        assert_eq!(clean_path("\u{202E}E:\\app\\foo.exe"), "E:\\app\\foo.exe");
    }

    #[test]
    fn batch_detection() {
        assert!(is_batch_path("C:\\run.bat"));
        assert!(is_batch_path("C:\\RUN.CMD"));
        assert!(!is_batch_path("C:\\run.exe"));
        assert!(!is_batch_path("C:\\run.lnk"));
    }
}
