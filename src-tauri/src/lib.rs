//! easyop —— 语音输入（PTT + 云端流式识别 + 剪贴板上屏）+ EasyInput 开发板按键配置。
//!
//! 两条链路：
//!   1. 语音：任意 USB/蓝牙键盘按住 PTT 键（低级钩子拦截）→ 电脑麦克风录音
//!      → 火山引擎流式识别（边说边出字）→ 松手 → 四阶段注入光标处。
//!   2. 按键配置：HID 0x10 下发配置、0x11 回执、Host Action v1 拉起本机应用，
//!      按键方案库 + 本机应用库（功能对齐 easyinput-keycfg）。
//!
//! 模块划分：
//!   ptt_hook        WH_KEYBOARD_LL 钩子引擎（替代开发板 PTT 按键通道）
//!   session         录音识别会话状态机
//!   audio           麦克风采集 + 重采样
//!   asr             火山引擎流式识别 WebSocket 客户端
//!   text_delivery   四阶段文本注入
//!   model/payload/validate/state/launcher/system_apps/device/transport/host_action
//!                   开发板按键配置（应用库 / 方案库 / HID 传输 / Host Action）

mod asr;
mod audio;
mod clipboard_util;
mod commands;
mod constants;
mod crc16;
mod device;
mod ei_protocol;
mod host_action;
mod keyboard_audio;
mod launcher;
mod llm_client;
mod log_buffer;
mod model;
mod payload;
mod ptt_hook;
mod session;
mod state;
mod system_apps;
mod text_delivery;
mod transport;
mod validate;
mod voice_edit;

use session::{SessionManager, TriggerMode};
use state::AppState;
use std::time::Duration;
use tauri::Emitter;
use tauri::Manager;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

/// 默认 PTT 键（与逆向所得原版 config.json 的 ptt_hotkey 一致）。
const DEFAULT_PTT: &str = "Ctrl+Shift+Space";

/// 后台字幕浮层尺寸（物理像素）。
pub(crate) const OVERLAY_W: f64 = 320.0;
pub(crate) const OVERLAY_H: f64 = 72.0;

#[tauri::command]
fn set_api_key(state: tauri::State<'_, SessionManager>, key: String) -> Result<(), String> {
    state.set_api_key(&key);
    Ok(())
}

#[tauri::command]
fn set_asr_url(state: tauri::State<'_, SessionManager>, url: String) -> Result<(), String> {
    if !url.starts_with("wss://") && !url.starts_with("ws://") {
        return Err("地址必须以 wss:// 或 ws:// 开头".to_string());
    }
    state.set_asr_url(&url);
    Ok(())
}

#[tauri::command]
fn set_resource_id(state: tauri::State<'_, SessionManager>, id: String) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err("模型 ID 不能为空".to_string());
    }
    state.set_resource_id(&id);
    Ok(())
}

/// 读取已保存的 API Key（供前端启动时回填）。
#[tauri::command]
fn get_api_key(state: tauri::State<'_, SessionManager>) -> String {
    state.api_key()
}

/// 读取已保存的服务地址（供前端启动时回填）。
#[tauri::command]
fn get_asr_url(state: tauri::State<'_, SessionManager>) -> String {
    state.asr_url()
}

/// 读取已保存的模型 ID（供前端启动时回填）。
#[tauri::command]
fn get_resource_id(state: tauri::State<'_, SessionManager>) -> String {
    state.resource_id()
}

/// 设置 Wi-Fi 配置（SSID + 密码）并持久化到配置文件。
#[tauri::command]
fn set_wifi_config(state: tauri::State<'_, SessionManager>, ssid: String, password: String) -> Result<(), String> {
    state.set_wifi_config(&ssid, &password);
    Ok(())
}

/// 读取 Wi-Fi 配置（供前端启动时回填）。
#[tauri::command]
fn get_wifi_config(state: tauri::State<'_, SessionManager>) -> std::collections::HashMap<String, String> {
    let (ssid, password) = state.get_wifi_config();
    let mut m = std::collections::HashMap::new();
    m.insert("ssid".to_string(), ssid);
    m.insert("password".to_string(), password);
    m
}

/// 设置蓝牙启用状态并持久化。
#[tauri::command]
fn set_ble_enabled(state: tauri::State<'_, SessionManager>, enabled: bool) -> Result<(), String> {
    state.set_ble_enabled(enabled);
    Ok(())
}

/// 读取蓝牙启用状态。
#[tauri::command]
fn get_ble_enabled(state: tauri::State<'_, SessionManager>) -> bool {
    state.ble_enabled()
}

#[tauri::command]
fn get_hotkey() -> String {
    format!("按住 {DEFAULT_PTT} 说话，松开上屏")
}

/// 切换触发模式：`hold`（按住说）/ `call`（打电话式连续断句）。
#[tauri::command]
fn set_mode(state: tauri::State<'_, SessionManager>, mode: String) -> Result<String, String> {
    let m = match mode.as_str() {
        "hold" => TriggerMode::Hold,
        "call" => TriggerMode::Call,
        other => return Err(format!("未知模式: {other}")),
    };
    state.set_mode(m);
    Ok(mode)
}

/// 读取当前触发模式（供前端启动时回填）。
#[tauri::command]
fn get_mode(state: tauri::State<'_, SessionManager>) -> String {
    match state.mode() {
        TriggerMode::Hold => "hold".to_string(),
        TriggerMode::Call => "call".to_string(),
    }
}

/// 运行时热替换 PTT 键，如 `F13`、`RAlt`、`Ctrl+Shift+Space`。
#[tauri::command]
fn set_ptt_key(
    spec: String,
    session: tauri::State<'_, SessionManager>,
) -> Result<String, String> {
    let key = ptt_hook::PttKey::parse(&spec)?;
    let label = key.label.clone();
    ptt_hook::set_ptt_key(key)?;
    log_buffer::info(format!("PTT 键已设置: {label}"));
    session.persist_all();
    Ok(label)
}

/// 手动开始录音（替代 PTT 按下）。
#[tauri::command]
fn start_recording(state: tauri::State<'_, SessionManager>, app: tauri::AppHandle) {
    state.ptt_down(app);
}

/// 手动停止录音（替代 PTT 松开）。
#[tauri::command]
fn stop_recording(state: tauri::State<'_, SessionManager>) {
    state.force_stop();
}

/// 调试/备用：直接走注入引擎上屏。
#[tauri::command]
fn inject_text(text: String) -> Result<(), String> {
    text_delivery::deliver(&text)
}

/// 隐藏主窗口到系统托盘（后台运行）。
#[tauri::command]
fn hide_to_tray(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
}

/// 从托盘恢复主窗口。
#[tauri::command]
fn show_main(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// 显示后台字幕浮层：先定位到鼠标光标下方居中，再显示。弹出后不再跟随移动。
#[tauri::command]
fn show_overlay(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("overlay") {
        if let Some((x, y)) = cursor_subtitle_pos(OVERLAY_W, OVERLAY_H) {
            let _ = win.set_position(tauri::PhysicalPosition::new(
                x.round() as i32,
                y.round() as i32,
            ));
        }
        let _ = win.show();
    }
}

/// 隐藏后台字幕浮层。
#[tauri::command]
fn hide_overlay(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("overlay") {
        let _ = win.hide();
    }
}

/// 设置后台字幕浮层启用开关（由前端设置面板调用）。
#[tauri::command]
fn set_overlay_enabled(
    state: tauri::State<'_, SessionManager>,
    enabled: bool,
) -> Result<bool, String> {
    state.set_overlay_enabled(enabled);
    Ok(enabled)
}

/// 读取后台字幕浮层启用开关（供前端启动时回填）。
#[tauri::command]
fn get_overlay_enabled(state: tauri::State<'_, SessionManager>) -> bool {
    state.overlay_enabled()
}

/// 查询开发板 Wi-Fi 音频是否在线（前端状态栏显示）。
#[tauri::command]
fn keyboard_audio_online(state: tauri::State<'_, SessionManager>) -> bool {
    state.keyboard_audio_online()
}

/// 真正退出应用（「最小化到托盘」对话框里的「退出」分支）。
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

/// 拉取日志缓冲区中序号大于 `since` 的条目（前端「日志」面板增量轮询）。
#[tauri::command]
fn get_logs(since: u64) -> Vec<log_buffer::LogEntry> {
    log_buffer::since(since)
}

/// 清空日志缓冲区。
#[tauri::command]
fn clear_logs() {
    log_buffer::clear();
}

/// 后台字幕浮层定位：跟随鼠标光标，返回「光标下方居中」的物理坐标，并钳制到屏幕内。
pub(crate) fn cursor_subtitle_pos(overlay_w: f64, overlay_h: f64) -> Option<(f64, f64)> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
        };
        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_err() {
                return None;
            }
            let screen_w = GetSystemMetrics(SM_CXSCREEN) as f64;
            let screen_h = GetSystemMetrics(SM_CYSCREEN) as f64;
            // 光标下方居中显示，留出光标高度余量。
            let x = (pt.x as f64 - overlay_w / 2.0).clamp(0.0, (screen_w - overlay_w).max(0.0));
            let y = (pt.y as f64 + 20.0).clamp(0.0, (screen_h - overlay_h).max(0.0));
            Some((x, y))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (overlay_w, overlay_h);
        None
    }
}

// ── 语音编辑命令 ──

/// 设置 LLM 配置（后端 / 端点 / 模型 / API Key / 超时）。
#[tauri::command]
fn set_llm_config(
    state: tauri::State<'_, voice_edit::VoiceEditManager>,
    session: tauri::State<'_, SessionManager>,
    config: llm_client::LlmConfig,
) -> Result<(), String> {
    log_buffer::info(format!(
        "LLM 配置保存: backend={:?} model={} endpoint={} timeout={}s key_len={}",
        config.backend, config.model, config.endpoint, config.timeout_secs, config.api_key.len()
    ));
    state.llm_client().set_config(config);
    session.persist_all();
    Ok(())
}

/// 读取当前 LLM 配置（供前端回填）。
#[tauri::command]
fn get_llm_config(
    state: tauri::State<'_, voice_edit::VoiceEditManager>,
) -> llm_client::LlmConfig {
    state.llm_client().config()
}

/// 测试 LLM 连接（发送简短请求验证 API Key）。
#[tauri::command]
async fn test_llm_connection(
    state: tauri::State<'_, voice_edit::VoiceEditManager>,
) -> Result<String, String> {
    let cfg = state.llm_client().config();
    log_buffer::info(format!("测试 LLM 连接: backend={:?} endpoint={} model={}", cfg.backend, cfg.endpoint, cfg.model));
    let result = state.llm_client().test_connection().await;
    match &result {
        Ok(msg) => log_buffer::info(format!("LLM 连接测试成功: {msg}")),
        Err(e) => log_buffer::warn(format!("LLM 连接测试失败: {e}")),
    }
    result
}

/// 设置编辑 PTT 键（如 `Ctrl+Shift+E`），传空字符串清除。
#[tauri::command]
fn set_edit_ptt_key(
    spec: String,
    session: tauri::State<'_, SessionManager>,
) -> Result<String, String> {
    if spec.is_empty() {
        ptt_hook::set_edit_ptt_key(None)?;
        log_buffer::info("编辑 PTT 键已清除");
        session.persist_all();
        return Ok("已清除".to_string());
    }
    let key = ptt_hook::PttKey::parse(&spec)?;
    let label = key.label.clone();
    ptt_hook::set_edit_ptt_key(Some(key))?;
    log_buffer::info(format!("编辑 PTT 键已设置: {label}"));
    session.persist_all();
    Ok(label)
}

/// 获取当前编辑阶段（idle / listening / processing）。
#[tauri::command]
fn get_edit_phase(state: tauri::State<'_, SessionManager>) -> session::EditPhase {
    state.edit_phase()
}

pub fn run() {
    tauri::Builder::default()
        .manage(SessionManager::new())
        .manage(AppState::default())
        .setup(|app| {
            log_buffer::info("应用启动");
            // 加载已保存的 API / 服务配置，以及应用库 / 方案库。
            if let Ok(dir) = app.path().app_config_dir() {
                app.state::<SessionManager>().init_config_file(dir.join("config.json"));
                let st = app.state::<AppState>();
                *st.apps.write().unwrap() = state::load_apps(&dir);
                *st.profiles.write().unwrap() = state::load_profiles(&dir);
            }

            // 初始化开发板 Wi-Fi 音频通道（UDP 17333）。绑定失败时静默回退电脑麦克风。
            match keyboard_audio::KeyboardAudioManager::new(keyboard_audio::DEFAULT_PORT) {
                Ok(ka) => {
                    app.state::<SessionManager>().set_keyboard_audio(ka);
                }
                Err(e) => {
                    log_buffer::warn(format!("键盘音频通道初始化失败，将使用电脑麦克风：{e}"));
                }
            }

            // 初始化 LLM 客户端 + 语音编辑管理器。
            let llm_config = llm_client::LlmConfig::default();
            let llm_client = llm_client::LlmClient::new(llm_config);
            let voice_edit_mgr = voice_edit::VoiceEditManager::new(llm_client);
            app.state::<SessionManager>().set_voice_edit(voice_edit_mgr.clone());
            app.manage(voice_edit_mgr);

            // 所有模块已初始化，重新加载配置以分发 LLM / PTT 等配置。
            // 放在 PTT 引擎启动之前，避免 PttKey::parse / PttEngine::start 失败时跳过配置加载。
            app.state::<SessionManager>().reload_config();

            // 自动启动 Host Action 常驻监听：保证按键触发本机应用无需用户手动开启。
            let handle = app.handle().clone();
            let st = app.state::<AppState>();
            let apps = st.apps.clone();
            let profiles = st.profiles.clone();
            let active_profile_id = st.active_profile_id.clone();
            let text_actions = st.text_actions.clone();
            let stop = st.take_stop();
            host_action::spawn_listener(handle, apps, profiles, active_profile_id, text_actions, stop);
            st.set_listener(true);

            let ptt = ptt_hook::PttKey::parse(DEFAULT_PTT)?;
            let engine = ptt_hook::PttEngine::start(ptt)?;

            // PTT 引擎启动后，重新分发 PTT 键配置（此前 SHARED 未初始化，set_ptt_key/set_edit_ptt_key 静默失败）。
            app.state::<SessionManager>().reload_config();
            // 若持久化配置中没有编辑键，设置默认 Ctrl+Shift+E。
            if ptt_hook::edit_ptt_key().is_none() {
                if let Ok(edit_key) = ptt_hook::PttKey::parse("Ctrl+Shift+E") {
                    let _ = ptt_hook::set_edit_ptt_key(Some(edit_key));
                    log_buffer::info("已设置默认编辑 PTT 键: Ctrl+Shift+E");
                }
            }

            // 自动连接 Wi-Fi（如果配置文件中有保存的 SSID）。
            {
                let (ssid, password) = app.state::<SessionManager>().get_wifi_config();
                if !ssid.is_empty() {
                    crate::log_buffer::info(format!("自动连接 Wi-Fi: SSID={ssid}"));
                    if let Err(e) = commands::wifi_connect(ssid, password) {
                        crate::log_buffer::warn(format!("自动连接 Wi-Fi 失败: {e}"));
                    }
                }
            }

            // 事件调度线程：钩子事件 → 会话状态机。
            let manager = (*app.state::<SessionManager>()).clone();
            let app_handle = app.handle().clone();
            std::thread::spawn(move || loop {
                match engine.recv_timeout(Duration::from_millis(200)) {
                    Some(ptt_hook::PttEvent::Down) => manager.ptt_down(app_handle.clone()),
                    Some(ptt_hook::PttEvent::Up) => manager.ptt_up(),
                    Some(ptt_hook::PttEvent::EditToggle) => {
                        manager.edit_ptt_down(app_handle.clone())
                    }
                    None => {}
                }
            });

            // 拦截主窗口关闭：不立即退出，转发给前端弹「最小化到托盘 / 退出」选择。
            if let Some(main) = app.get_webview_window("main") {
                let app_on_close = app.handle().clone();
                main.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = app_on_close.emit("close-requested", ());
                    }
                });
            }

            // 系统托盘：打开主窗口 / 退出。
            let show_item = MenuItemBuilder::with_id("show", "打开 easyop").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "退出 easyop").build(app)?;
            let menu = MenuBuilder::new(app).item(&show_item).item(&quit_item).build()?;
            let tray = TrayIconBuilder::with_id("easyop")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip("easyop · 语音输入 / 按键配置")
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                })
                .build(app)?;
            let _ = tray;

            // 后台字幕浮层（跟随语音输入做实时转写）。always_on_top 置顶，透明背景。
            // 弹出时定位到鼠标光标下方居中，弹出后不再跟随移动。
            tauri::WebviewWindowBuilder::new(
                app,
                "overlay",
                tauri::WebviewUrl::App("overlay.html".into()),
            )
            .title("easyop 字幕")
            .inner_size(OVERLAY_W, OVERLAY_H)
            .position(80.0, 80.0)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .shadow(false)
            .visible(false)
            .build()?;

            // 编辑浮层（语音编辑专用，显示原文/识别文本/LLM流式结果）。
            tauri::WebviewWindowBuilder::new(
                app,
                "edit_overlay",
                tauri::WebviewUrl::App("edit_overlay.html".into()),
            )
            .title("easyop 编辑")
            .inner_size(400.0, 120.0)
            .position(80.0, 80.0)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .shadow(false)
            .visible(false)
            .build()?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            set_api_key,
            set_asr_url,
            set_resource_id,
            get_api_key,
            get_asr_url,
            get_resource_id,
            set_wifi_config,
            get_wifi_config,
            set_ble_enabled,
            get_ble_enabled,
            get_hotkey,
            set_ptt_key,
            set_mode,
            get_mode,
            start_recording,
            stop_recording,
            inject_text,
            hide_to_tray,
            show_main,
            show_overlay,
            hide_overlay,
            set_overlay_enabled,
            get_overlay_enabled,
            keyboard_audio_online,
            quit_app,
            get_logs,
            clear_logs,
            commands::device_status,
            commands::push_config,
            commands::get_vocabulary,
            commands::start_listener,
            commands::stop_listener,
            commands::listener_status,
            commands::test_open,
            commands::list_system_apps,
            commands::list_apps,
            commands::add_app,
            commands::update_app,
            commands::remove_app,
            commands::list_profiles,
            commands::new_profile,
            commands::save_profile,
            commands::delete_profile,
            commands::set_active_profile,
            commands::wifi_connect,
            commands::wifi_disconnect,
            commands::ble_enable,
            commands::ble_disable,
            commands::ble_pair,
            commands::ble_status,
            commands::set_mic_source,
            commands::get_mic_source,
            set_llm_config,
            get_llm_config,
            test_llm_connection,
            set_edit_ptt_key,
            get_edit_phase
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
