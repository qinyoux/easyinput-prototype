//! 截图小程序的后端能力：抓取整个虚拟桌面 → 编码 PNG → 存到「图片」文件夹。
//!
//! 为什么不用现成的截图库：本项目已经在用 `windows` crate 直接调 Win32（注册表、
//! ShellExecute、窗口枚举），这里沿用同一套做法，用 GDI 全屏 BitBlt 抓一帧，
//! 不引入新的依赖。
//!
//! PNG 编码分两块：
//! - **压缩**：直接复用已经存在的 `flate2`（原本给 ASR 的 gzip 用），产出 zlib 流；
//! - **封装**：PNG 的签名 / IHDR / IDAT / IEND 四段加一个 CRC32，自己拼就行。
//!
//! 图像一律按**顶层**（top-down）32 位 DIB 抓取，省掉一次翻转；输出用真彩色
//! （color type 2，无 alpha）—— 屏幕是实心画面，alpha 通道纯属浪费体积。

use std::path::PathBuf;

/// 抓取整个屏幕并保存为 PNG，返回保存后的绝对路径。
pub(crate) fn capture_to_pictures() -> Result<String, String> {
    #[cfg(windows)]
    {
        let (w, h, pixels) = capture_screen_bgra()?;
        let png = encode_png(w as u32, h as u32, &pixels)?;

        let dir = pictures_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建截图目录失败: {e}"))?;
        let path = dir.join(format!("easyop_{}.png", timestamp_compact()));
        std::fs::write(&path, &png).map_err(|e| format!("写入截图文件失败: {e}"))?;

        crate::log_buffer::info(format!(
            "⌘SS 截图：已保存 {w}×{h}（{:.1} MB）到 {}",
            png.len() as f64 / 1024.0 / 1024.0,
            path.display()
        ));
        Ok(path.to_string_lossy().to_string())
    }
    #[cfg(not(windows))]
    {
        Err("截图目前仅支持 Windows".to_string())
    }
}

/// 截图保存目录：`%USERPROFILE%\Pictures\easyop`，取不到就退到临时目录。
fn pictures_dir() -> PathBuf {
    match std::env::var("USERPROFILE") {
        Ok(profile) if !profile.is_empty() => PathBuf::from(profile).join("Pictures").join("easyop"),
        _ => std::env::temp_dir().join("easyop-screenshots"),
    }
}

// ── 屏幕抓取 ──

/// 抓取整个虚拟桌面（含多显示器），返回 `(宽, 高, BGRA 像素)`。
///
/// 之所以抓「虚拟桌面」而不是主屏：多显示器时用户可以横跨两块屏，只抓主屏会漏内容。
/// 虚拟桌面的原点可能在主屏左上角之外（副屏在主屏左边时为负坐标），所以 BitBlt 的
/// 源坐标要用 `SM_XVIRTUALSCREEN` / `SM_YVIRTUALSCREEN`，不能写死 0。
#[cfg(windows)]
fn capture_screen_bgra() -> Result<(i32, i32, Vec<u8>), String> {
    use std::ffi::c_void;
    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetDeviceCaps,
        ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DESKTOPHORZRES,
        DESKTOPVERTRES, DIB_RGB_COLORS, HGDIOBJ, SRCCOPY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };

    // 桌面窗口 = 整个虚拟桌面，传空 HWND 即取屏幕 DC。
    let null_hwnd = HWND(std::ptr::null_mut());

    // SAFETY: 全部是 Win32 GDI 的标准调用序列；拿到的每个句柄都在本函数内配对释放，
    // 像素缓冲区在 DIB 被选中期间只读。
    unsafe {
        let screen = GetDC(null_hwnd);
        if screen.is_invalid() {
            return Err("获取屏幕设备上下文失败".to_string());
        }

        let (x, y, w, h) = {
            let mut vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let mut vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let mut vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let mut vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            if vw <= 0 || vh <= 0 {
                // 虚拟桌面查询失败时退到「主屏」口径，至少能出一张图。
                vx = 0;
                vy = 0;
                vw = GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN);
                vh = GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN);
            }

            // DPI 兜底：若进程不是 per-monitor DPI aware，GetSystemMetrics 给的是被系统
            // 缩放过的逻辑像素，直接按它建 DIB 会得到一张糊图。DESKTOPHORZRES 返回的是
            // 物理像素宽，比逻辑值大时按比例把抓取区域放大回去。
            let phys_w = GetDeviceCaps(screen, DESKTOPHORZRES);
            let phys_h = GetDeviceCaps(screen, DESKTOPVERTRES);
            if phys_w > vw && phys_h > vh && vw > 0 && vh > 0 {
                let sx = phys_w as f64 / vw as f64;
                let sy = phys_h as f64 / vh as f64;
                crate::log_buffer::info(format!(
                    "⌘SS 截图：检测到 DPI 缩放 {sx:.2}×{sy:.2}，按物理像素抓取"
                ));
                let nx = (vx as f64 * sx).round() as i32;
                let ny = (vy as f64 * sy).round() as i32;
                vx = nx;
                vy = ny;
                vw = phys_w;
                vh = phys_h;
            }

            (vx, vy, vw, vh)
        };

        if w <= 0 || h <= 0 {
            let _ = ReleaseDC(null_hwnd, screen);
            return Err("屏幕尺寸异常，无法截图".to_string());
        }

        let mem = CreateCompatibleDC(screen);
        if mem.is_invalid() {
            let _ = ReleaseDC(null_hwnd, screen);
            return Err("创建内存设备上下文失败".to_string());
        }

        // 负高度 = 顶层 DIB，省掉抓完再上下翻转的一步。
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: (w as u32) * (h as u32) * 4,
            ..Default::default()
        };

        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap = match CreateDIBSection(
            screen,
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            HANDLE(std::ptr::null_mut()),
            0,
        ) {
            Ok(b) => b,
            Err(e) => {
                let _ = DeleteDC(mem);
                let _ = ReleaseDC(null_hwnd, screen);
                return Err(format!("创建位图失败: {e}"));
            }
        };
        if bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(mem);
            let _ = ReleaseDC(null_hwnd, screen);
            return Err("位图像素缓冲区为空".to_string());
        }

        let old = SelectObject(mem, bitmap);

        // SRCCOPY = 直接拷贝；CAPTUREBLT 让分层窗口（部分悬浮窗、水印）也能进画面。
        let copy = BitBlt(
            mem,
            0,
            0,
            w,
            h,
            screen,
            x,
            y,
            windows::Win32::Graphics::Gdi::ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0),
        );

        // 先把像素拷出来，再归还句柄 —— 位图一旦被反选/删除，bits 就悬空了。
        let result = match copy {
            Ok(()) => {
                let len = (w as usize) * (h as usize) * 4;
                let mut buf = vec![0u8; len];
                std::ptr::copy_nonoverlapping(bits as *const u8, buf.as_mut_ptr(), len);
                Ok((w, h, buf))
            }
            Err(e) => Err(format!("屏幕抓取失败: {e}")),
        };

        let _ = SelectObject(mem, old);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem);
        let _ = ReleaseDC(null_hwnd, screen);

        result
    }
}

// ── PNG 编码 ──

/// 把 BGRA 像素编码成 PNG（真彩色、无 alpha、逐行 filter=None）。
fn encode_png(width: u32, height: u32, bgra: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;

    let stride = width as usize * 4;
    let expected = stride * height as usize;
    if bgra.len() < expected {
        return Err(format!(
            "像素数据长度不匹配：期望 {expected} 字节，实际 {}",
            bgra.len()
        ));
    }

    // 1. 拼原始扫描线：每行开头加一个 filter 字节（0 = None），像素做 BGRA → RGB。
    let mut raw = Vec::with_capacity(height as usize * (1 + width as usize * 3));
    for y in 0..height as usize {
        raw.push(0u8);
        let row = &bgra[y * stride..y * stride + stride];
        for px in row.chunks_exact(4) {
            raw.push(px[2]);
            raw.push(px[1]);
            raw.push(px[0]);
        }
    }

    // 2. zlib 压缩（PNG 的 IDAT 载荷就是 zlib 流，不是裸 deflate）。
    let mut encoder =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    encoder
        .write_all(&raw)
        .map_err(|e| format!("压缩截图数据失败: {e}"))?;
    let idat = encoder
        .finish()
        .map_err(|e| format!("压缩截图数据失败: {e}"))?;
    drop(raw);

    // 3. 组装 PNG。
    let mut out = Vec::with_capacity(idat.len() + 64);
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8); // 位深 8
    ihdr.push(2); // 颜色类型 2 = 真彩色
    ihdr.push(0); // 压缩方法（固定 0 = deflate）
    ihdr.push(0); // 过滤方法（固定 0）
    ihdr.push(0); // 非隔行扫描
    write_chunk(&mut out, b"IHDR", &ihdr);
    write_chunk(&mut out, b"IDAT", &idat);
    write_chunk(&mut out, b"IEND", &[]);

    Ok(out)
}

/// 写一个 PNG 数据块：长度 + 类型 + 数据 + CRC32（CRC 只覆盖类型和数据）。
fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);

    let mut crc = crc32_init();
    crc = crc32_update(crc, kind);
    crc = crc32_update(crc, data);
    out.extend_from_slice(&(crc ^ 0xFFFF_FFFF).to_be_bytes());
}

/// CRC32（PNG 用的是标准 IEEE 多项式 0xEDB88320）。
///
/// 这里用最朴素的逐位算法而不是查表：截图一次只算两份数据（IHDR 13 字节 + IDAT
/// 一到几 MB），逐位实现够快，还省掉一张表和一个 `once_cell` 之类的初始化依赖。
fn crc32_init() -> u32 {
    0xFFFF_FFFF
}

fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    crc
}

// ── 文件名时间戳 ──

/// `20260918_153012` 形式的本地时间戳（UTC+8，与 `log_buffer` 的口径一致）。
fn timestamp_compact() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + 8 * 3600;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}_{:02}{:02}{:02}",
        rem / 3600,
        (rem / 60) % 60,
        rem % 60
    )
}

/// 把「距 1970-01-01 的天数」换算成公历年月日。
///
/// 出自 Howard Hinnant 的 `civil_from_days` 算法：先把纪元挪到 0000-03-01 让闰年规则
/// 变成整齐的 4/100/400 周期，再做整除拆分，全程整数运算、无查表。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}
