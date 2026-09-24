//! HID 0x10 配置下发与 0x11 回执读取。
//!
//! 在 `device` 枚举的匹配句柄上：把完整配置 JSON 按固件协议分块为 64 字节
//! Feature Report（Report ID + 头 + 分块），逐帧 `HidD_SetFeature` 下发，再用
//! `ReadFile + OVERLAPPED` 阻塞读取 0x11 回执（kind 0x03）。

use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT, FALSE, TRUE};
use windows::Win32::Devices::HumanInterfaceDevice::HidD_SetFeature;
use windows::Win32::Storage::FileSystem::ReadFile;
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

use crate::constants::*;
use crate::crc16::crc16_ccitt;
use crate::device;
use crate::model::ConfigAck;
use crate::payload::parse_ack;

/// 把完整配置 JSON 按固件协议分块，每块构造一条 64 字节 Feature Report。
pub fn chunk_reports(json: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    if json.is_empty() || json.len() > CONFIG_MAX_JSON_LEN {
        return Err(format!(
            "配置字节数越界: {}（允许 1..={CONFIG_MAX_JSON_LEN}）",
            json.len()
        ));
    }
    let total_len = json.len() as u16;
    let total_chunks = (json.len() + CONFIG_MAX_CHUNK_DATA - 1) / CONFIG_MAX_CHUNK_DATA;
    if total_chunks > 255 {
        return Err("配置分块超过 255 帧".to_string());
    }
    let crc = crc16_ccitt(json);
    let mut reports = Vec::with_capacity(total_chunks);
    for index in 0..total_chunks {
        let offset = index * CONFIG_MAX_CHUNK_DATA;
        let chunk_len = (json.len() - offset).min(CONFIG_MAX_CHUNK_DATA);
        let mut report = vec![0u8; 1 + CONFIG_HEADER_LEN + CONFIG_MAX_CHUNK_DATA];
        report[0] = REPORT_ID_CONFIG;
        report[1..4].copy_from_slice(&MAGIC);
        report[4] = CONFIG_VERSION;
        report[5] = index as u8;
        report[6] = total_chunks as u8;
        report[7] = (total_len & 0xFF) as u8;
        report[8] = (total_len >> 8) as u8;
        report[9] = chunk_len as u8;
        report[10] = (crc & 0xFF) as u8;
        report[11] = (crc >> 8) as u8;
        report[12..12 + chunk_len].copy_from_slice(&json[offset..offset + chunk_len]);
        reports.push(report);
    }
    Ok(reports)
}

fn send_feature(handle: HANDLE, report: &[u8]) -> Result<(), String> {
    let res = unsafe {
        HidD_SetFeature(handle, report.as_ptr() as *const core::ffi::c_void, report.len() as u32)
    };
    if !res.as_bool() {
        Err(format!("HidD_SetFeature 失败: 0x{:08X}", unsafe { GetLastError().0 }))
    } else {
        Ok(())
    }
}

/// 公开封装：阻塞读取一条 HID 输入报告（含 Report ID 首字节）。超时返回 `Ok(None)`。
pub fn read_input_report_once(handle: HANDLE, timeout_ms: u32) -> Result<Option<Vec<u8>>, String> {
    read_input_report(handle, timeout_ms)
}

/// 阻塞读取一条 HID 输入报告（含 Report ID 首字节）。超时返回 `Ok(None)`。
fn read_input_report(handle: HANDLE, timeout_ms: u32) -> Result<Option<Vec<u8>>, String> {
    let event = unsafe { CreateEventW(None, TRUE, FALSE, PCWSTR::null()) }
        .map_err(|e| format!("CreateEventW: {e}"))?;
    let mut overlapped = OVERLAPPED::default();
    overlapped.hEvent = event;
    let mut buf = [0u8; 64];
    let mut bytes = 0u32;
    let res = unsafe {
        ReadFile(
            handle,
            Some(&mut buf),
            Some(&mut bytes as *mut u32),
            Some(&mut overlapped as *mut OVERLAPPED),
        )
    };
    let transferred = match res {
        Ok(()) => bytes,
        Err(e) => {
            if e.code().0 as u32 == HRESULT_ERROR_IO_PENDING {
                let wait = unsafe { WaitForSingleObject(event, timeout_ms) };
                if wait == WAIT_OBJECT_0 {
                    let mut n = 0u32;
                    unsafe { GetOverlappedResult(handle, &overlapped, &mut n, FALSE) }
                        .map_err(|e| format!("GetOverlappedResult: {e}"))?;
                    n
                } else if wait == WAIT_TIMEOUT {
                    // 超时后必须取消仍未完成的 OVERLAPPED 读，否则句柄上会残留一个
                    // 悬挂的后台读：下一次迭代再发起 ReadFile 就变成「同一个 HID 句柄
                    // 上有两个并发的读」，HID 驱动会把每条报告投递给最老的、事件已关闭
                    // 的那个读，导致所有报告都被吞掉。这里取消并等待它真正结束。
                    unsafe {
                        let _ = CancelIoEx(handle, Some(&overlapped as *const OVERLAPPED));
                        let mut n = 0u32;
                        // 取消后 GetOverlappedResult 返回 ERROR_OPERATION_ABORTED(995)，
                        // 此处仅用于释放该 OVERLAPPED 并等待残留后台读结束。
                        let _ = GetOverlappedResult(handle, &overlapped, &mut n, TRUE);
                    }
                    unsafe {
                        let _ = windows::Win32::Foundation::CloseHandle(event);
                    }
                    return Ok(None);
                } else {
                    unsafe {
                        let _ = windows::Win32::Foundation::CloseHandle(event);
                    }
                    return Err("等待 HID 输入报告失败".to_string());
                }
            } else {
                unsafe {
                    let _ = windows::Win32::Foundation::CloseHandle(event);
                }
                return Err(format!("ReadFile: {e}"));
            }
        }
    };
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(event);
    }
    Ok(Some(buf[..(transferred as usize).min(buf.len())].to_vec()))
}

/// 主命令：发送完整配置 JSON 并等待 kind 0x03 回执。
pub fn send_config(json: &str) -> Result<ConfigAck, String> {
    let reports = chunk_reports(json.as_bytes())?;
    let paths = device::enumerate_device_paths();
    if paths.is_empty() {
        return Err(format!(
            "未找到开发板（VID {:04X} / PID {:04X}），请确认已通过 USB 连接",
            VID, PID
        ));
    }
    let mut last_err = "所有候选 HID 设备均无法下发配置".to_string();
    for path in paths {
        let Some(handle) = device::open_matching(&path) else { continue };

        let mut send_ok = true;
        for (i, report) in reports.iter().enumerate() {
            if let Err(e) = send_feature(handle, report) {
                last_err = format!("{e}（帧 {}/{}）", i + 1, reports.len());
                send_ok = false;
                break;
            }
            // 让固件逐帧接收，避免控制传输过密。
            std::thread::sleep(Duration::from_millis(20));
        }
        if !send_ok {
            device::close_handle(handle);
            continue;
        }

        // 等待 0x11 kind 0x03 回执；忽略其它 0x11 命令，直到超时。
        let deadline = Instant::now() + Duration::from_millis(1500);
        let mut acked = None;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match read_input_report(handle, remaining.as_millis() as u32) {
                Ok(Some(report))
                    if report.len() > 1
                        && report[0] == REPORT_ID_APP_COMMAND
                        && report[1] == KIND_CONFIG_ACK =>
                {
                    acked = Some(parse_ack(&report));
                    break;
                }
                Ok(Some(_)) => continue,
                Ok(None) | Err(_) => break,
            }
        }
        device::close_handle(handle);

        if let Some(ack) = acked {
            return Ok(ack);
        }
        last_err = "配置已下发但 1.5s 内未收到 0x11 回执，请确认开发板仍在线".to_string();
    }
    Err(last_err)
}

/// 发送命令 JSON 到开发板（通过 0x10 Feature Report），不等待配置回执。
///
/// 用于 Wi-Fi 连接/断开、蓝牙开关/配对等控制命令。仅打开支持 Feature Report 的集合
/// （app-command 集合），跳过键盘集合以避免干扰系统键盘事件路由。
pub fn send_command(json: &str) -> Result<(), String> {
    let reports = chunk_reports(json.as_bytes())?;
    let paths = device::enumerate_device_paths();
    if paths.is_empty() {
        return Err(format!(
            "未找到开发板（VID {:04X} / PID {:04X}），请确认已通过 USB 连接",
            VID, PID
        ));
    }
    let mut last_err = "所有候选 HID 设备均无法下发命令".to_string();
    for path in paths {
        let Some(handle) = device::open_matching(&path) else { continue };
        if !device::has_feature_report(handle) {
            device::close_handle(handle);
            continue;
        }
        let mut send_ok = true;
        for (i, report) in reports.iter().enumerate() {
            if let Err(e) = send_feature(handle, report) {
                last_err = format!("{e}（帧 {}/{}）", i + 1, reports.len());
                send_ok = false;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        device::close_handle(handle);
        if send_ok {
            return Ok(());
        }
    }
    Err(last_err)
}

/// 下发一条**已构造好的** Feature Report（首字节即 Report ID），不等待任何回执。
///
/// 与 [`send_command`] 的区别：那个把入参当"配置载荷"分块（带 Magic / CRC / 分块头），
/// 这个只负责"把这串字节送到支持 Feature Report 的集合上"。**协议含义由调用方负责**
/// —— 例如任务台的 `0x16` 逐像素帧、`0x12` Agent 状态帧。
///
/// 与 `send_command` 同理，只打开支持 Feature Report 的集合（app-command 集合），
/// 跳过键盘集合：打开键盘集合会干扰系统键盘事件路由。
pub fn send_feature_report(report: &[u8]) -> Result<(), String> {
    if report.is_empty() {
        return Err("Feature Report 不能为空".to_string());
    }
    let paths = device::enumerate_device_paths();
    if paths.is_empty() {
        return Err(format!(
            "未找到开发板（VID {:04X} / PID {:04X}），请确认已通过 USB 连接",
            VID, PID
        ));
    }
    let mut last_err = "所有候选 HID 设备均无法下发该 Feature Report".to_string();
    for path in paths {
        let Some(handle) = device::open_matching(&path) else { continue };
        if !device::has_feature_report(handle) {
            device::close_handle(handle);
            continue;
        }
        let result = send_feature(handle, report);
        device::close_handle(handle);
        match result {
            Ok(()) => return Ok(()),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_single_frame() {
        let json = br#"{"profiles":[{"keys":{}}]}"#;
        let reports = chunk_reports(json).unwrap();
        assert_eq!(reports.len(), 1);
        let r = &reports[0];
        assert_eq!(r[0], 0x10);
        assert_eq!(&r[1..4], b"S3C");
        assert_eq!(r[4], 1);
        assert_eq!(r[5], 0);
        assert_eq!(r[6], 1);
        assert_eq!(r[7], json.len() as u8);
        assert_eq!(r[8], 0);
        assert_eq!(r[9], json.len() as u8);
        assert_eq!(&r[12..12 + json.len()], json);
    }

    #[test]
    fn chunk_multi_frame() {
        let json = vec![b'A'; CONFIG_MAX_CHUNK_DATA * 3 + 7];
        let reports = chunk_reports(&json).unwrap();
        assert_eq!(reports.len(), 4);
        assert_eq!(reports[0][6], 4);
        assert_eq!(reports[3][5], 3);
        assert_eq!(reports[3][9], 7);
        let crc = crc16_ccitt(&json);
        assert_eq!(u16::from_le_bytes([reports[0][10], reports[0][11]]), crc);
    }

    #[test]
    fn chunk_rejects_oversize() {
        assert!(chunk_reports(&vec![b'A'; CONFIG_MAX_JSON_LEN + 1]).is_err());
        assert!(chunk_reports(b"").is_err());
    }
}
