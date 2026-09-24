//! Host Action v1 常驻监听器。
//!
//! 开发板端用户在按键上绑定了 `host_action:<uuid>` 时，固件在动作触发时回发一条
//! 0x11 输入报告（kind 0x05，payload 为 36 字节规范小写 hex UUID）。本模块在后台
//! 线程持续打开 HID 设备并读取 0x11 报告，识别 kind 0x05 后按 UUID 分流：
//! - 文本注入 UUID → 主机端用剪贴板+Ctrl+V 注入含非ASCII的固定文本；
//! - 保留 UUID（方案切换）→ 主机端切换激活方案并重新下发固件；
//! - 保留 UUID（浮窗工作台）→ 主机端开合浮窗 / 移动高亮 / 确认上屏；
//! - 保留 UUID（启动终端）→ 打开（或复用）一个终端并预置命令前缀，不回车；
//! - 普通 UUID → 在应用库中解析并打开本机目标。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::constants::{
    HOST_ACTION_V1_HEADER_LEN, KIND_HOST_ACTION, REPORT_ID_APP_COMMAND, UUID_NEXT_PROFILE,
    UUID_PREVIOUS_PROFILE, UUID_TOGGLE_PROFILE,
};
use crate::device;
use crate::launcher;
use crate::log_buffer;
use crate::model::{AppEntry, Profile};
use crate::transport::read_input_report_once;

/// 启动后台监听线程；`stop` 由调用方持有，置 true 可优雅退出。
pub fn spawn_listener(
    app: AppHandle,
    apps: Arc<RwLock<Vec<AppEntry>>>,
    profiles: Arc<RwLock<Vec<Profile>>>,
    active_profile_id: Arc<RwLock<Option<String>>>,
    text_actions: Arc<RwLock<HashMap<String, String>>>,
    stop: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        listen_loop(app, apps, profiles, active_profile_id, text_actions, stop)
    });
}

fn listen_loop(
    app: AppHandle,
    apps: Arc<RwLock<Vec<AppEntry>>>,
    profiles: Arc<RwLock<Vec<Profile>>>,
    active_profile_id: Arc<RwLock<Option<String>>>,
    text_actions: Arc<RwLock<HashMap<String, String>>>,
    stop: Arc<AtomicBool>,
) {
    log_buffer::info("Host Action 监听器已启动");
    let mut last_connected = false;
    let mut last_heartbeat = std::time::Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let handles = try_open_all();
        if handles.is_empty() {
            if last_connected {
                log_buffer::warn("监听器：未找到 HID 设备，等待连接…");
                last_connected = false;
                last_heartbeat = std::time::Instant::now();
            }
            std::thread::sleep(Duration::from_millis(1000));
            continue;
        }
        if !last_connected {
            log_buffer::info(format!(
                "监听器：已连接 HID 设备（枚举 {} 个匹配路径，打开 {} 个集合句柄）",
                device::enumerate_device_paths().len(),
                handles.len()
            ));
            last_connected = true;
            last_heartbeat = std::time::Instant::now();
        }
        while !stop.load(Ordering::Relaxed) {
            let mut got = false;
            let mut broken = false;
            for (index, handle) in handles.iter().enumerate() {
                match read_input_report_once(*handle, 50) {
                    Ok(Some(report)) => {
                        got = true;
                        process_report(
                            &app,
                            &apps,
                            &profiles,
                            &active_profile_id,
                            &text_actions,
                            index,
                            handles.len(),
                            &report,
                        );
                    }
                    Ok(None) => continue,
                    Err(_) => {
                        broken = true;
                        break;
                    }
                }
            }
            if broken {
                log_buffer::warn("监听器：读取失败/设备断开，重连中");
                break;
            }
            if !got && last_heartbeat.elapsed() >= Duration::from_secs(30) {
                log_buffer::info(format!("监听器：存活（{} 个句柄），当前无输入报文", handles.len()));
                last_heartbeat = std::time::Instant::now();
            }
            if !got {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        for h in handles {
            device::close_handle(h);
        }
    }
    log_buffer::info("Host Action 监听器已停止");
}

/// 处理一条 HID 输入报告：任何报文都全量 hex 转储（诊断），kind 0x05 触发分流。
#[allow(clippy::too_many_arguments)]
fn process_report(
    app: &AppHandle,
    apps: &Arc<RwLock<Vec<AppEntry>>>,
    profiles: &Arc<RwLock<Vec<Profile>>>,
    active_profile_id: &Arc<RwLock<Option<String>>>,
    text_actions: &Arc<RwLock<HashMap<String, String>>>,
    index: usize,
    handles: usize,
    report: &[u8],
) {
    let id = report.first().copied().unwrap_or(0);
    let hex: String = report
        .iter()
        .take(20)
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ");
    log_buffer::info(format!(
        "收到报文 handle#{index}/{handles} (len={}, 首字节=0x{id:02X}): {hex}",
        report.len()
    ));

    if report.len() > 5
        && report[0] == REPORT_ID_APP_COMMAND
        && report[1] == KIND_HOST_ACTION
    {
        log_buffer::info(format!("收到 Host Action 报文 (len={})", report.len()));
        handle_host_action(app, apps, profiles, active_profile_id, text_actions, report);
    }
}

/// 打开所有匹配 VID/PID 且支持 Feature Report 的 HID 句柄。
fn try_open_all() -> Vec<windows::Win32::Foundation::HANDLE> {
    let mut handles = Vec::new();
    for path in device::enumerate_device_paths() {
        if let Some(handle) = device::open_matching(&path) {
            if device::has_feature_report(handle) {
                handles.push(handle);
            } else {
                device::close_handle(handle);
            }
        }
    }
    handles
}

fn handle_host_action(
    app: &AppHandle,
    apps: &Arc<RwLock<Vec<AppEntry>>>,
    profiles: &Arc<RwLock<Vec<Profile>>>,
    active_profile_id: &Arc<RwLock<Option<String>>>,
    text_actions: &Arc<RwLock<HashMap<String, String>>>,
    report: &[u8],
) {
    let start = 1 + HOST_ACTION_V1_HEADER_LEN;
    let end = start + crate::constants::HOST_ACTION_V1_UUID_LEN;
    if end > report.len() {
        log_buffer::warn(format!("Host Action 报文过短 (len={})", report.len()));
        return;
    }
    let uuid = String::from_utf8_lossy(&report[start..end]).to_string();
    if !crate::validate::is_canonical_uuid(&uuid) {
        log_buffer::warn(format!("Host Action UUID 格式非法: {uuid}"));
        return;
    }
    log_buffer::info(format!("Host Action 触发 UUID={uuid}"));

    // 1. 文本注入：UUID 在 text_actions 映射中 → 剪贴板+Ctrl+V 注入。
    {
        let guard = text_actions.read().unwrap();
        if let Some(text) = guard.get(&uuid) {
            let text = text.clone();
            drop(guard);

            // 浮窗工作台打开时不能直接注入：此刻前台是浮窗自己，Ctrl+V 会把正文
            // 粘进浮窗，而不是用户原来的编辑位置。改为让浮窗高亮对应行，由用户在
            // 浮窗里按编码器确认（`workbench_commit` 会先收起浮窗再注入）。
            //
            // 传给浮窗的是**正文**而不是 UUID：提示词库与按键绑定是两套独立数据，
            // 两者之间唯一的对应关系就是「文本相同」，页面按正文匹配即可高亮。
            if crate::workbench::is_open(app) {
                log_buffer::info("浮窗工作台已打开：文本动作转为高亮对应提示词");
                crate::workbench::emit_command(app, "activate", Some(text.clone()));
                return;
            }

            log_buffer::info(format!("文本注入: {} 字节", text.len()));
            match crate::text_delivery::deliver(&text) {
                Ok(()) => log_buffer::info("文本注入成功"),
                Err(e) => log_buffer::err(format!("文本注入失败: {e}")),
            }
            let _ = app.emit(
                "host-action",
                crate::state::HostActionEvent {
                    uuid,
                    message: "文本已注入".to_string(),
                },
            );
            return;
        }
    }

    // 2. 保留 UUID：方案切换。
    if uuid == UUID_TOGGLE_PROFILE || uuid == UUID_PREVIOUS_PROFILE || uuid == UUID_NEXT_PROFILE {
        handle_profile_switch(app, profiles, active_profile_id, text_actions, &uuid);
        return;
    }

    // 3. 保留 UUID：KEY4 浮窗工作台（开合 / 高亮上下 / 确认上屏）。
    if crate::workbench::handle_action(app, &uuid) {
        return;
    }

    // 3.5. 保留 UUID：⌘SS 命令面板（开合）。
    if crate::ss_palette::handle_action(app, &uuid) {
        return;
    }

    // 3.6. 保留 UUID：Agent 任务台的槽位按键（S1–S4）。
    //
    // 必须排在"查应用库"之前。按键的 `task_desk` 动作在固件里就是一个普通的
    // `host_action:<uuid>`（固件对按键动作是白名单制，见 constants 里的说明），
    // 但它的语义是"归入槽位"而不是"开一个程序"——放到应用库分支就会报
    // "打开失败"，把功能未实现误诊成配置错误。
    if crate::agent_desk::handle_reserved_uuid(app, &uuid) {
        return;
    }

    // 3.7. 保留 UUID：启动一个终端并把命令前缀打进去（不回车）。
    //
    // ⚠️ **必须 spawn 到独立线程**。`launch()` 里要等新窗口起来并拿到前台
    //    （上限 6 秒），再把命令打进那个窗口。直接在这里调，就是把这**一个**
    //    HID 监听线程钉住 6 秒 —— 这 6 秒里板子上任何按键都读不到，
    //    用户看到的是"按了没反应"，日志上却像设备掉线。
    //    监听线程的职责只有"读到报文 → 分流 → 立刻回去读下一条"，一点都不能多。
    //
    //    代价是"这次启动的结果"不能再靠日志顺序看出因果，所以结果单独发一条
    //    `cli-terminal` 事件给界面（成功/失败都发，失败不静默）。
    if uuid == crate::constants::UUID_CLI_TERMINAL {
        let app2 = app.clone();
        let uuid2 = uuid.clone();
        std::thread::spawn(move || {
            let (ok, message) = match crate::cli_terminal::launch() {
                Ok(m) => (true, m),
                Err(e) => {
                    log_buffer::err(format!("终端启动：{e}"));
                    (false, e)
                }
            };
            let _ = app2.emit(
                "cli-terminal",
                serde_json::json!({ "uuid": uuid2, "message": message, "ok": ok }),
            );
        });
        return;
    }

    // 4. 普通 UUID：在应用库中解析并打开。
    let result = {
        let guard = apps.read().unwrap();
        launcher::open_by_uuid(&guard, &uuid)
    };
    let msg = match &result {
        Ok(text) => {
            log_buffer::info(format!("打开成功: {text}"));
            text.clone()
        }
        Err(e) => {
            log_buffer::err(format!("打开失败: {e}"));
            e.clone()
        }
    };
    let _ = app.emit("host-action", crate::state::HostActionEvent { uuid, message: msg });
}

/// 处理方案切换：计算新激活方案 → 更新 active_profile_id → 下发固件 → 通知前端。
fn handle_profile_switch(
    app: &AppHandle,
    profiles: &Arc<RwLock<Vec<Profile>>>,
    active_profile_id: &Arc<RwLock<Option<String>>>,
    text_actions: &Arc<RwLock<HashMap<String, String>>>,
    uuid: &str,
) {
    let profiles_guard = profiles.read().unwrap();
    if profiles_guard.is_empty() {
        log_buffer::warn("方案切换：方案库为空，忽略");
        return;
    }

    let current_id = active_profile_id.read().unwrap().clone();
    let current_idx = match &current_id {
        Some(id) => profiles_guard.iter().position(|p| &p.id == id),
        None => Some(0),
    };
    let current_idx = current_idx.unwrap_or(0);

    let count = profiles_guard.len();
    let new_idx = if uuid == UUID_TOGGLE_PROFILE || uuid == UUID_NEXT_PROFILE {
        (current_idx + 1) % count
    } else if uuid == UUID_PREVIOUS_PROFILE {
        if current_idx == 0 { count - 1 } else { current_idx - 1 }
    } else {
        current_idx
    };

    if new_idx == current_idx && count > 1 {
        log_buffer::info("方案切换：仅一个方案，无需切换");
        return;
    }

    let new_profile = &profiles_guard[new_idx];
    let new_id = new_profile.id.clone();
    let new_name = new_profile.name.clone();
    log_buffer::info(format!(
        "方案切换：{} → {}（索引 {current_idx} → {new_idx}）",
        current_id.as_deref().unwrap_or("(无)"),
        new_name
    ));

    *active_profile_id.write().unwrap() = Some(new_id.clone());

    // 下发新方案到固件，同时更新文本动作映射与浮窗提示词列表。
    //
    // 「内容随方案切换」正是靠这一行：提示词挂在方案上，任何一次激活方案变更都
    // 会重新发布一次快照，浮窗若正开着就当场换内容。
    //
    // ⛔ 载荷必须**带上 Wi-Fi 端点**：固件 `apply_json` 是整体替换，载荷里没提到的
    // 字段回默认值（`wifi_ssid` / `audio_host` 默认空串）⇒ 不带端点就是
    // **一按「切换方案」就把板子的 Wi-Fi 配置清空**。别改回 `build_firmware_payload`。
    let (ssid, password) = app.state::<crate::session::SessionManager>().get_wifi_config();
    let built = match crate::wifi_link::build_payload_with_endpoint(app, &ssid, &password) {
        Ok(built) => built,
        Err(e) => {
            log_buffer::err(format!("方案切换：构造下发载荷失败 {e}"));
            return;
        }
    };
    *text_actions.write().unwrap() = built.text_map.clone();
    crate::workbench::publish_board_input(app, new_profile);
    log_buffer::info(format!("方案切换：下发固件 ({} 字节)", built.json.len()));
    match crate::transport::send_config(&built.json) {
        Ok(ack) => log_buffer::info(format!("方案切换：下发回执 {:?}", ack)),
        Err(e) => log_buffer::err(format!("方案切换：下发失败 {e}")),
    }

    let _ = app.emit(
        "profile-switched",
        crate::state::ProfileSwitchedEvent {
            profile_id: new_id,
            profile_name: new_name.clone(),
            message: format!("已切换到方案「{new_name}」"),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_flag_roundtrip() {
        let f = Arc::new(AtomicBool::new(false));
        f.store(true, Ordering::Relaxed);
        assert!(f.load(Ordering::Relaxed));
    }
}
