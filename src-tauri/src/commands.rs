//! Tauri 命令层：把前端请求映射到各后端模块。
//!
//! 前端仅通过这里的 `invoke` 命令与后端交互，不直接触碰 Win32/协议细节。
//! 命令分为五组：
//!   设备       device_status / push_config
//!   监听器     start_listener / stop_listener / listener_status
//!   应用库     list_apps / add_app / update_app / remove_app / test_open
//!   方案库     list_profiles / save_profile / delete_profile / new_profile
//!   词表       get_vocabulary（供前端渲染下拉）

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::model::{AppEntry, ConfigAck, Profile};
use crate::state::{self, AppState};
use crate::system_apps::SystemApp;
use crate::{device, host_action, payload, transport, validate};

/// 词表 / 常量（供前端渲染下拉框与做前端预校验）。
#[derive(Serialize)]
pub struct Vocabulary {
    pub named_actions: Vec<&'static str>,
    pub modifiers: Vec<&'static str>,
    pub named_keys: Vec<&'static str>,
    pub text_max_utf8_bytes: usize,
    pub host_action_prefix: &'static str,
    pub schema: &'static str,
    pub keys: Vec<&'static str>,
    pub encoder_inputs: Vec<&'static str>,
}

fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map_err(|e| format!("无法获取配置目录：{e}"))
}

/// 检测开发板 HID 是否在线并返回描述。
#[tauri::command]
pub(crate) fn device_status() -> Result<String, String> {
    let result = device::device_status();
    // 日志限频：每 5 秒最多写一条设备检测日志，避免刷屏。
    static LAST_LOG: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let last = LAST_LOG.load(std::sync::atomic::Ordering::Relaxed);
    if now.saturating_sub(last) >= 5 {
        LAST_LOG.store(now, std::sync::atomic::Ordering::Relaxed);
        match &result {
            Ok(desc) => crate::log_buffer::info(format!("设备检测: {desc}")),
            Err(e) => crate::log_buffer::warn(format!("设备检测: {e}")),
        }
    }
    result
}

/// 下发一个完整方案：本地校验 → 构造固件 JSON → 分块经 0x10 下发 → 读取 0x11 kind 0x03 回执。
#[tauri::command]
pub(crate) fn push_config(profile: Profile, state: State<'_, AppState>) -> Result<ConfigAck, String> {
    crate::log_buffer::info(format!("下发方案: {} (按键数={})", profile.name, profile.keys.len()));
    validate::validate_profile(&profile).map_err(|e| {
        crate::log_buffer::err(format!("校验失败: {e}"));
        e
    })?;
    let mut text_map = std::collections::HashMap::new();
    let json = payload::to_firmware_json(&profile, &mut text_map);
    // 更新主机端文本动作映射（含非ASCII的固定文本 → UUID）。
    *state.text_actions.write().unwrap() = text_map;
    crate::log_buffer::info(format!("固件 JSON 已构造 ({} 字节)", json.len()));
    let result = crate::transport::send_config(&json);
    match &result {
        Ok(ack) => crate::log_buffer::info(format!("下发回执: {:?}", ack)),
        Err(e) => crate::log_buffer::err(format!("下发失败: {e}")),
    }
    result
}

/// 返回词表与常量，供前端一次性拉取。
#[tauri::command]
pub(crate) fn get_vocabulary() -> Vocabulary {
    Vocabulary {
        named_actions: crate::constants::NAMED_ACTIONS.to_vec(),
        modifiers: crate::constants::HOTKEY_MODIFIER_TOKENS.to_vec(),
        named_keys: crate::constants::HOTKEY_NAMED_KEYS.to_vec(),
        text_max_utf8_bytes: crate::constants::FIXED_TEXT_MAX_UTF8_BYTES,
        host_action_prefix: crate::constants::HOST_ACTION_PREFIX,
        schema: crate::constants::SCHEMA_VERSION,
        keys: crate::model::KEY_INPUTS.to_vec(),
        encoder_inputs: crate::model::ENCODER_INPUTS.to_vec(),
    }
}

// ---------- Host Action 监听器 ----------

/// 启动 Host Action 常驻监听线程；已在运行则报错。
#[tauri::command]
pub(crate) fn start_listener(app: AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    if state.listener_running.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("监听器已在运行".to_string());
    }
    let apps = state.apps.clone();
    let profiles = state.profiles.clone();
    let active_profile_id = state.active_profile_id.clone();
    let text_actions = state.text_actions.clone();
    let stop = state.take_stop();
    host_action::spawn_listener(app, apps, profiles, active_profile_id, text_actions, stop);
    state.set_listener(true);
    Ok(true)
}

/// 停止 Host Action 常驻监听线程。
#[tauri::command]
pub(crate) fn stop_listener(state: State<'_, AppState>) -> Result<bool, String> {
    state.set_listener(false);
    Ok(true)
}

/// 查询监听器运行状态。
#[tauri::command]
pub(crate) fn listener_status(state: State<'_, AppState>) -> bool {
    state.listener_running.load(std::sync::atomic::Ordering::Relaxed)
}

// ---------- 系统应用枚举 ----------

/// 枚举本机已注册/可见的应用（开始菜单可启动项），供「打开应用」快速选择。
#[tauri::command]
pub(crate) fn list_system_apps() -> Result<Vec<SystemApp>, String> {
    Ok(crate::system_apps::list_system_apps())
}

// ---------- 应用库 CRUD ----------

/// 手动「测试打开」一个应用库条目。
#[tauri::command]
pub(crate) fn test_open(entry: AppEntry) -> Result<String, String> {
    crate::launcher::open_entry(&entry)
}

/// 读取应用库。
#[tauri::command]
pub(crate) fn list_apps(state: State<'_, AppState>) -> Vec<AppEntry> {
    state.apps.read().unwrap().clone()
}

/// 新增应用库条目；uuid 留空时自动生成规范 UUID。
#[tauri::command]
pub(crate) fn add_app(
    app: AppHandle,
    state: State<'_, AppState>,
    mut entry: AppEntry,
) -> Result<AppEntry, String> {
    if entry.title.trim().is_empty() {
        return Err("展示名不能为空".to_string());
    }
    if entry.uuid.trim().is_empty() {
        entry.uuid = Uuid::new_v4().to_string();
    }
    entry.uuid = entry.uuid.to_ascii_lowercase();
    if !validate::is_canonical_uuid(&entry.uuid) {
        return Err("应用 UUID 格式非法（需规范小写 hex UUID）".to_string());
    }
    if !validate::is_valid_app_target(&entry.target) {
        return Err("打开目标不完整".to_string());
    }
    let mut guard = state.apps.write().unwrap();
    if guard.iter().any(|e| e.uuid.eq_ignore_ascii_case(&entry.uuid)) {
        return Err("已存在相同 UUID 的应用".to_string());
    }
    guard.push(entry.clone());
    persist_apps(&app, &guard)?;
    Ok(entry)
}

/// 更新应用库条目（按 uuid）。
#[tauri::command]
pub(crate) fn update_app(
    app: AppHandle,
    state: State<'_, AppState>,
    entry: AppEntry,
) -> Result<AppEntry, String> {
    if entry.title.trim().is_empty() {
        return Err("展示名不能为空".to_string());
    }
    if !validate::is_canonical_uuid(&entry.uuid) {
        return Err("应用 UUID 格式非法（需规范小写 hex UUID）".to_string());
    }
    if !validate::is_valid_app_target(&entry.target) {
        return Err("打开目标不完整".to_string());
    }
    let mut guard = state.apps.write().unwrap();
    let Some(slot) = guard.iter_mut().find(|e| e.uuid.eq_ignore_ascii_case(&entry.uuid)) else {
        return Err("未找到要更新的应用".to_string());
    };
    *slot = entry.clone();
    persist_apps(&app, &guard)?;
    Ok(entry)
}

/// 删除应用库条目（按 uuid）。
#[tauri::command]
pub(crate) fn remove_app(
    app: AppHandle,
    state: State<'_, AppState>,
    uuid: String,
) -> Result<(), String> {
    let mut guard = state.apps.write().unwrap();
    let before = guard.len();
    guard.retain(|e| !e.uuid.eq_ignore_ascii_case(&uuid));
    if guard.len() == before {
        return Err("未找到要删除的应用".to_string());
    }
    persist_apps(&app, &guard)
}

fn persist_apps(app: &AppHandle, apps: &[AppEntry]) -> Result<(), String> {
    let dir = config_dir(app)?;
    state::save_apps(&dir, apps)
}

// ---------- 方案库 CRUD ----------

/// 读取方案库。
#[tauri::command]
pub(crate) fn list_profiles(state: State<'_, AppState>) -> Vec<Profile> {
    state.profiles.read().unwrap().clone()
}

/// 新建一个默认方案，加入方案库并持久化；返回新建的方案。
#[tauri::command]
pub(crate) fn new_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<Profile, String> {
    let id = Uuid::new_v4().to_string();
    let profile = Profile::factory_default(
        if name.trim().is_empty() { "未命名方案" } else { &name },
        id,
    );
    let mut guard = state.profiles.write().unwrap();
    guard.push(profile.clone());
    persist_profiles(&app, &guard)?;
    Ok(profile)
}

/// 保存（新增或按 id 更新）方案并持久化；返回保存后的方案。
#[tauri::command]
pub(crate) fn save_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    profile: Profile,
) -> Result<Profile, String> {
    validate::validate_profile(&profile)?;
    let mut guard = state.profiles.write().unwrap();
    if let Some(slot) = guard.iter_mut().find(|p| p.id == profile.id) {
        *slot = profile.clone();
    } else {
        guard.push(profile.clone());
    }
    persist_profiles(&app, &guard)?;
    Ok(profile)
}

/// 删除方案（按 id）；至少保留最后一个方案。
#[tauri::command]
pub(crate) fn delete_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let mut guard = state.profiles.write().unwrap();
    if guard.len() <= 1 {
        return Err("至少需要保留一个方案".to_string());
    }
    guard.retain(|p| p.id != id);
    persist_profiles(&app, &guard)?;
    // 若删除的正是激活方案，回退到第一个。
    let mut active = state.active_profile_id.write().unwrap();
    if let Some(ref aid) = *active {
        if aid == &id {
            *active = guard.first().map(|p| p.id.clone());
        }
    }
    Ok(())
}

/// 设置当前激活方案 ID（前端切换方案时调用，同步主机端状态）。
#[tauri::command]
pub(crate) fn set_active_profile(
    state: State<'_, AppState>,
    id: Option<String>,
) -> Result<(), String> {
    *state.active_profile_id.write().unwrap() = id;
    Ok(())
}

fn persist_profiles(app: &AppHandle, profiles: &[Profile]) -> Result<(), String> {
    let dir = config_dir(app)?;
    state::save_profiles(&dir, profiles)
}

// ---------- 通信配置（Wi-Fi / 蓝牙 / 麦克风源）----------

use crate::session::{SessionManager, MicSource};
use serde_json::json;

/// 下发 Wi-Fi 连接命令到开发板：开发板将连接指定 SSID/密码的无线网络。
#[tauri::command]
pub(crate) fn wifi_connect(ssid: String, password: String) -> Result<(), String> {
    if ssid.trim().is_empty() {
        return Err("Wi-Fi SSID 不能为空".to_string());
    }
    crate::log_buffer::info(format!("下发 Wi-Fi 连接命令: SSID={ssid} 密码长度={}", password.len()));
    let json = json!({
        "cmd": "wifi_connect",
        "ssid": ssid,
        "password": password,
    })
    .to_string();
    transport::send_command(&json).map_err(|e| {
        crate::log_buffer::err(format!("Wi-Fi 连接命令下发失败: {e}"));
        e
    })?;
    crate::log_buffer::info("Wi-Fi 连接命令已下发");
    Ok(())
}

/// 下发 Wi-Fi 断开命令到开发板。
#[tauri::command]
pub(crate) fn wifi_disconnect() -> Result<(), String> {
    crate::log_buffer::info("下发 Wi-Fi 断开命令");
    let json = json!({ "cmd": "wifi_disconnect" }).to_string();
    transport::send_command(&json).map_err(|e| {
        crate::log_buffer::err(format!("Wi-Fi 断开命令下发失败: {e}"));
        e
    })?;
    crate::log_buffer::info("Wi-Fi 断开命令已下发");
    Ok(())
}

/// 下发蓝牙启用命令到开发板：开发板将开启 BLE HID 广播。
#[tauri::command]
pub(crate) fn ble_enable() -> Result<(), String> {
    crate::log_buffer::info("下发蓝牙启用命令");
    let json = json!({ "cmd": "ble_enable" }).to_string();
    transport::send_command(&json).map_err(|e| {
        crate::log_buffer::err(format!("蓝牙启用命令下发失败: {e}"));
        e
    })?;
    crate::log_buffer::info("蓝牙启用命令已下发");
    Ok(())
}

/// 下发蓝牙关闭命令到开发板。
#[tauri::command]
pub(crate) fn ble_disable() -> Result<(), String> {
    crate::log_buffer::info("下发蓝牙关闭命令");
    let json = json!({ "cmd": "ble_disable" }).to_string();
    transport::send_command(&json).map_err(|e| {
        crate::log_buffer::err(format!("蓝牙关闭命令下发失败: {e}"));
        e
    })?;
    crate::log_buffer::info("蓝牙关闭命令已下发");
    Ok(())
}

/// 下发蓝牙配对命令到开发板：开发板将进入 BLE 配对模式。
#[tauri::command]
pub(crate) fn ble_pair() -> Result<(), String> {
    crate::log_buffer::info("下发蓝牙配对命令");
    let json = json!({ "cmd": "ble_pair" }).to_string();
    transport::send_command(&json).map_err(|e| {
        crate::log_buffer::err(format!("蓝牙配对命令下发失败: {e}"));
        e
    })?;
    crate::log_buffer::info("蓝牙配对命令已下发");
    Ok(())
}

/// 查询开发板蓝牙连接状态（通过键盘音频心跳的 flags 推断）。
#[tauri::command]
pub(crate) fn ble_status(state: State<'_, SessionManager>) -> String {
    if state.keyboard_audio_online() {
        "在线（Wi-Fi 音频通道活跃）".to_string()
    } else {
        "离线".to_string()
    }
}

/// 设置麦克风来源：影响后续录音时的音频源选择。
#[tauri::command]
pub(crate) fn set_mic_source(
    state: State<'_, SessionManager>,
    source: String,
) -> Result<String, String> {
    let src = match source.as_str() {
        "auto" => MicSource::Auto,
        "pc" => MicSource::Pc,
        "wifi" => MicSource::Wifi,
        "bluetooth" => MicSource::Bluetooth,
        other => return Err(format!("未知麦克风来源: {other}")),
    };
    state.set_mic_source(src);
    crate::log_buffer::info(format!("麦克风来源已切换: {source}"));
    Ok(source)
}

/// 查询当前麦克风来源设置。
#[tauri::command]
pub(crate) fn get_mic_source(state: State<'_, SessionManager>) -> String {
    match state.mic_source() {
        MicSource::Auto => "auto".to_string(),
        MicSource::Pc => "pc".to_string(),
        MicSource::Wifi => "wifi".to_string(),
        MicSource::Bluetooth => "bluetooth".to_string(),
    }
}
