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
//!   workbench       KEY4 浮窗提示词库（随方案切换的提示词清单：增删改 + 选中上屏）

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
mod mini_apps;
mod model;
mod payload;
mod pinyin;
mod ptt_hook;
mod session;
mod state;
mod system_apps;
mod screenshot;
mod text_delivery;
mod transport;
mod validate;
mod voice_edit;
mod workbench;
mod ss_palette;
mod everything;
mod plugin_registry;
mod plugin_host;
mod runner;
mod capability;
mod usage_log;
mod asr_vocab;
mod cli_terminal;
/// TTS 通用能力（Capability Core）—— 文本 → 48 kHz PCM。见 `tts.rs` 头部说明。
mod tts;
mod wifi_link;
/// Agent 任务台（第 7 课作业落地）。**纯增量**：删除本目录 + 摘命令即回滚。
mod agent_desk;

use session::{SessionManager, TriggerMode};
use state::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::Emitter;
use tauri::Manager;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

/// 显式退出意图。
///
/// Tauri 的默认行为是「所有窗口关闭 → 退出应用」。easyop 里 ⌘SS 面板、
/// 字幕浮层、编辑浮层都是**程序化创建、常驻不销毁**的窗口，收面板 / 按 Esc
/// 这类操作一旦被判定成「窗口都关了」，整个 easyop 就会静默退出。
///
/// 所以这里加一道硬性守护：只有用户**明确**选择退出（托盘「退出 easyop」、
/// 「最小化到托盘」对话框里的「退出」按钮）才置位放行，其余一律拦截。
static EXIT_INTENT: AtomicBool = AtomicBool::new(false);

/// 用户明确要求退出：置位后调用 `AppHandle::exit`。
fn request_exit(app: &tauri::AppHandle) {
    EXIT_INTENT.store(true, Ordering::SeqCst);
    app.exit(0);
}

/// 默认 PTT 键（与逆向所得原版 config.json 的 ptt_hotkey 一致）。
const DEFAULT_PTT: &str = "Ctrl+Shift+Space";

/// 后台字幕浮层尺寸（**逻辑**像素，即 CSS 像素）。
///
/// 与 `WebviewWindowBuilder::inner_size` 的语义一致：Windows 下由系统按显示器缩放比
/// 放大成物理尺寸。`cursor_subtitle_pos` 内部负责把尺寸换算到物理像素，
/// 调用方不要自己再乘缩放比。
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
fn set_wifi_config(
    state: tauri::State<'_, SessionManager>,
    link: tauri::State<'_, wifi_link::WifiLink>,
    ssid: String,
    password: String,
) -> Result<(), String> {
    state.set_wifi_config(&ssid, &password);
    // 顺手写进"历史网络"：用户在界面上填过一次，就等于声明"我可能会用它"。
    // 真正的"成功"仍以收到心跳为准（由看护线程写 last_success）。
    if !ssid.trim().is_empty() {
        if let Some(dir) = link.config_dir_for_write() {
            let mut file = wifi_link::load_known(&dir);
            if wifi_link::upsert_known(&mut file, &ssid, Some(&password)) {
                let _ = wifi_link::save_known(&dir, &file);
            }
        }
    }
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
    // 一句话把两个出口都说完，因为用户就是靠这行字记按法的。
    // ⛔ 别写"按住"：语义是**短按**（按下开始、再按一次结束）。
    format!("按一下 {DEFAULT_PTT} 开始说话，再按一下注入（不发送）；录音时按回车 = 注入并发送")
}

/// 切换触发模式：`hold`（单句 · 短按）/ `call`（打电话式连续断句）。
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

/// 手动开始/结束录音（界面 ● 按钮）。
///
/// ⛔ 它**就是** `ptt_down`，与按 KEY1 完全等价。v0.29.0 曾让两者分叉
/// （`ptt_down_manual`，按钮在那个版本里不该消费"待确认"），那套理由随两段式
/// 一起作废了：现在两者都是"按一下开始、再按一下结束并注入"，没有任何分歧。
/// **不要再为它们造第二条路径** —— 两个入口行为不一致，正是本次要消灭的那类矛盾。
#[tauri::command]
fn start_recording(state: tauri::State<'_, SessionManager>, app: tauri::AppHandle) {
    state.ptt_down(app);
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
        if let Some((x, y)) = cursor_subtitle_pos(&win, OVERLAY_W, OVERLAY_H) {
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

// ── 注入失败的留底（**异常路径专用**）──
//
// 背景：语音是"短按两下交付完"（按一下开始 → 再按一下注入；或录音时按回车 = 注入并发送），
// 正常流程里**没有**任何需要用户确认的中间态 —— 那正是 v0.29.0 的 `pending_inject`
// 被删掉的原因（它把同一件事变成了按三下）。
//
// 但"注入"这一步本身会失败（粘贴键没送到 / 剪贴板拿不到 / 目标窗口权限不足）。
// 识别已经成功了，此时把整句话扔掉等于让用户重新说一遍 —— 所以失败的内容留在
// 这里，由用户手动重试。
// ⛔ 它**不在正常流程上**：正常路径永远不碰这几个命令。

/// 读当前的失败留底（`null` = 没有）。界面挂载时用它对齐状态
/// —— 事件是瞬时的，面板挂载前发出的那条收不到。
#[tauri::command]
fn get_failed_inject(state: tauri::State<'_, crate::session::SessionManager>) -> Option<String> {
    state.failed_inject_text()
}

/// 重新注入上一次失败的内容。
///
/// ⛔ **不回车**：用户点的是"重新注入"，不是"重新发送"。提交权归用户，
/// 不因为一次重试而放宽（要发送他自己会按回车）。
#[tauri::command]
async fn retry_failed_inject(
    state: tauri::State<'_, crate::session::SessionManager>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    state.retry_failed_inject(app).await
}

/// 丢弃失败留底，返回被丢弃的内容（供界面说清"丢掉了什么"）。
#[tauri::command]
fn clear_failed_inject(state: tauri::State<'_, crate::session::SessionManager>) -> Option<String> {
    let dropped = state.discard_failed_inject();
    if let Some(t) = dropped.as_deref() {
        log_buffer::info(format!(
            "语音：用户丢弃了注入失败的内容（{} 字）",
            t.chars().count()
        ));
    }
    dropped
}

// ── 浮窗提示词库命令（Phase 3）──

/// 开合浮窗提示词库，返回操作后是否可见。
#[tauri::command]
fn workbench_toggle(app: tauri::AppHandle) -> bool {
    workbench::toggle(&app)
}

/// 收起浮窗提示词库并把焦点还给原窗口。
#[tauri::command]
fn workbench_hide(app: tauri::AppHandle) {
    workbench::hide(&app);
}

/// 查询浮窗提示词库是否可见（前端据此同步按钮态）。
#[tauri::command]
fn workbench_is_open(app: tauri::AppHandle) -> bool {
    workbench::is_open(&app)
}

/// 拉取浮窗列表快照（方案名 + 该方案的提示词）。
///
/// 由**当前激活方案**现场推导，而不是读一份缓存：浮窗窗口常驻不销毁，页面只在每次
/// 打开时拉一次，任何缓存一旦落后就会让浮窗显示别的方案的内容。
///
/// 解析口径与 `host_action::handle_profile_switch` 保持一致：`active_profile_id`
/// 为空时取方案库第一项（前端启动后未必已经调用过 `set_active_profile`），
/// 指向的 id 已被删除时同样退到第一项。
#[tauri::command]
fn workbench_list(state: tauri::State<'_, AppState>) -> workbench::BoardInputEvent {
    let profiles = state.profiles.read().unwrap();
    let active = state.active_profile_id.read().unwrap();
    let profile = active
        .as_deref()
        .and_then(|id| profiles.iter().find(|p| p.id == id))
        .or_else(|| profiles.first());
    match profile {
        Some(profile) => workbench::snapshot(profile),
        // 方案库为空（正常流程不会发生，保底避免 panic）。
        None => workbench::BoardInputEvent::default(),
    }
}

/// 把浮窗里当前高亮的提示词上屏到原窗口（收起浮窗 → 焦点回退 → 剪贴板注入）。
///
/// `text_delivery::deliver` 要阻塞 1.2s 以上等待目标应用消费粘贴，所以整体挪到
/// 阻塞线程池执行；命令保持 `async` 以免占住 Tauri 的事件循环。
#[tauri::command]
async fn workbench_commit(app: tauri::AppHandle, text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || workbench::commit_selected(&app, &text))
        .await
        .map_err(|e| format!("确认上屏任务异常：{e}"))?
}

// ── ⌘SS 命令面板命令（Phase 4A）──

/// 显示 ⌘SS 命令面板。
#[tauri::command]
fn show_ss_palette(app: tauri::AppHandle) {
    ss_palette::show(&app);
}

/// 隐藏 ⌘SS 命令面板。
#[tauri::command]
fn hide_ss_palette(app: tauri::AppHandle) {
    ss_palette::hide(&app);
}

/// 开合 ⌘SS 命令面板。
#[tauri::command]
fn toggle_ss_palette(app: tauri::AppHandle) -> bool {
    ss_palette::toggle(&app)
}

/// ⌘SS 面板是否打开。
#[tauri::command]
fn ss_palette_is_open(app: tauri::AppHandle) -> bool {
    ss_palette::is_open(&app)
}

/// ⌘SS 搜索：返回匹配的应用/命令列表。
#[tauri::command]
fn ss_search(query: String, state: tauri::State<'_, AppState>) -> Vec<ss_palette::SearchResult> {
    let guard = state.apps.read().unwrap();
    ss_palette::search(&query, &guard)
}

/// ⌘SS 刷新应用索引（强制重新扫描）。
#[tauri::command]
fn ss_refresh_apps() -> usize {
    let apps = crate::system_apps::refresh_app_cache();
    apps.len()
}

// ── 小程序：把外部 App / Web 项目集成进 easyop ──
//
// ---------------------------------------------------------------------------
// 「插件」= 放在 `app_config_dir()/plugins/<id>/` 下的扩展模块（含 plugin.json 清单）。
// 注册表由 `plugin_registry` 模块持有，这里是给前端的出入口。
// 第一阶段只提供「发现」：列出 / 重扫 / 拿根目录。激活（建隐藏 webview + 注入 eo.*）
// 由后续 `plugin_host` 承担——先有登记，才有加载。
// ---------------------------------------------------------------------------

/// 列出已发现的插件（含各自声明的命令与工具）。
#[tauri::command]
fn plugins_list() -> Vec<plugin_registry::PluginInfo> {
    plugin_registry::list()
}

/// 重新扫描插件目录。用户新放插件后无需重启即可刷新。
#[tauri::command]
fn plugins_rescan() -> usize {
    plugin_registry::rescan()
}

/// 插件根目录路径（供前端「打开插件目录」）。
#[tauri::command]
fn plugins_dir() -> String {
    plugin_registry::plugins_root_string()
}

/// 插件页签信息（供前端侧栏动态追加导航项）。
#[derive(Clone, serde::Serialize)]
struct PluginPageInfo {
    plugin_id: String,
    page_id: String,
    title: String,
    icon: String,
    order: i32,
}

/// 返回所有插件声明的页签。启动时前端拉取，追加到侧栏导航。
#[tauri::command]
fn plugin_pages() -> Vec<PluginPageInfo> {
    plugin_registry::all_pages()
        .into_iter()
        .map(|(pid, p)| PluginPageInfo {
            plugin_id: pid,
            page_id: p.id,
            title: p.title,
            icon: p.icon,
            order: p.order,
        })
        .collect()
}

/// 触发插件激活（不执行命令，仅 activate + registerPage）。
/// 前端侧栏点击插件页签时调用，commandId 用 `__page__` sentinel。
#[tauri::command]
async fn plugin_trigger_activate(
    app: tauri::AppHandle,
    plugin_id: String,
) -> Result<(), String> {
    plugin_host::activate(&app, &plugin_id, "__page__").await?;
    Ok(())
}


// 「小程序」= 用户登记进来的**外部**目标（本机 exe / 网址 / 项目目录），
// 面板里点一下就能启动它。注册表由 `mini_apps` 模块持有，这里是给前端的出入口。

/// 列出已集成的小程序。
#[tauri::command]
fn mini_apps_list() -> Vec<mini_apps::MiniApp> {
    mini_apps::list()
}

/// 集成一个新的小程序。
#[tauri::command]
fn mini_apps_add(
    name: String,
    kind: String,
    target: String,
    args: Option<String>,
    note: Option<String>,
) -> Result<mini_apps::MiniApp, String> {
    mini_apps::add(
        &name,
        &kind,
        &target,
        args.as_deref().unwrap_or(""),
        note.as_deref().unwrap_or(""),
    )
}

/// 移除一个小程序。
#[tauri::command]
fn mini_apps_remove(id: String) -> Result<bool, String> {
    mini_apps::remove(&id)
}

/// 启动一个小程序（本质是打开被集成的那个外部目标）。
#[tauri::command]
fn mini_apps_launch(id: String) -> Result<String, String> {
    mini_apps::launch(&id)
}

/// 添加小程序时按关键词建议本机应用（复用应用索引，不额外扫盘）。
#[tauri::command]
fn mini_apps_suggest(query: String, limit: Option<usize>) -> Vec<system_apps::SystemApp> {
    mini_apps::suggest_system(&query, limit.unwrap_or(12))
}

/// ⌘SS Everything 搜索：搜索全盘文件（需配置 Everything 路径）。
#[tauri::command]
fn ss_everything_search(
    query: String,
    state: tauri::State<'_, SessionManager>,
) -> Result<Vec<everything::EverythingResult>, String> {
    let everything_path = state.everything_path();
    everything::search(&everything_path, &query, 50)
}

/// 检查 Everything 是否可用。
#[tauri::command]
fn everything_available(state: tauri::State<'_, SessionManager>) -> bool {
    everything::is_available(&state.everything_path())
}

/// 执行 ⌘SS 搜索结果项。
#[tauri::command]
async fn ss_execute(
    kind: String,
    target: String,
    title: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    // 插件命令：走激活层（plugin_host），不走 ss_palette::execute。
    if kind == "plugin" {
        let plugin_id = plugin_registry::find_plugin_by_command(&target)
            .ok_or(format!("找不到命令所属插件：{target}"))?;
        plugin_host::activate(&app, &plugin_id, &target).await?;
        return Ok(());
    }

    let guard = state.apps.read().unwrap();
    let config_dir = app
        .path()
        .app_config_dir()
        .ok();
    crate::ss_palette::execute(&kind, &target, &title, &guard, config_dir)
}

/// 弹出 ⌘SS 搜索结果的资源管理器原生右键菜单。
///
/// 返回 `true` 表示用户确实选了菜单里的某一项（此时面板自动收起且**不**抢回焦点，
/// 好让新打开的窗口留在前台），`false` 表示菜单被取消。
#[tauri::command]
fn ss_show_context_menu(
    kind: String,
    target: String,
    title: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    let hwnd = palette_hwnd(&app);
    let guard = state.apps.read().unwrap();
    let acted = crate::ss_palette::show_context_menu(&kind, &target, &title, &guard, hwnd)?;
    if acted {
        crate::ss_palette::hide_silent(&app);
    }
    Ok(acted)
}

/// ⌘SS 精简菜单：执行指定 Shell 动词（open / openas / properties 等）。
#[tauri::command]
fn ss_shell_verb(verb: String, target: String, app: tauri::AppHandle) -> Result<(), String> {
    crate::ss_palette::shell_verb(&verb, &target)?;
    // 执行后自动收起面板，让新窗口留在前台
    crate::ss_palette::hide_silent(&app);
    Ok(())
}

/// ⌘SS 精简菜单：复制文件路径到剪贴板。
#[tauri::command]
fn ss_copy_path(target: String) -> Result<(), String> {
    crate::ss_palette::copy_path_to_clipboard(&target)
}

/// ⌘SS 精简菜单：打开文件所在文件夹并选中该文件。
#[tauri::command]
fn ss_open_containing_folder(target: String, app: tauri::AppHandle) -> Result<(), String> {
    crate::ss_palette::open_containing_folder(&target)?;
    crate::ss_palette::hide_silent(&app);
    Ok(())
}

/// 取 ⌘SS 面板窗口的 HWND。
///
/// 本工程用的 `windows` crate 是 0.58，而 tauri 依赖的是 0.61，两边的 `HWND` 是不同
/// 类型，不能互相传递；统一降级成 `isize` 在边界上传递。
#[cfg(windows)]
fn palette_hwnd(app: &tauri::AppHandle) -> isize {
    app.get_webview_window(crate::ss_palette::LABEL)
        .and_then(|w| w.hwnd().ok())
        .map(|h| h.0 as isize)
        .unwrap_or(0)
}

#[cfg(not(windows))]
fn palette_hwnd(_app: &tauri::AppHandle) -> isize {
    0
}

/// 调整 ⌘SS 面板窗口高度（前端根据搜索结果条数动态调用）。
#[tauri::command]
fn ss_resize(height: f64, app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Size::Logical;
    if let Some(win) = app.get_webview_window(crate::ss_palette::LABEL) {
        let _ = win.set_size(Logical(tauri::LogicalSize {
            width: crate::ss_palette::SS_PALETTE_W,
            height,
        }));
    }
    Ok(())
}

/// 截图小程序：抓取整个屏幕存成 PNG，返回文件路径。
///
/// 抓之前先把面板自己藏起来、抓完立刻显示回来 —— 否则截图里会印着命令面板本身。
/// 这里刻意**不走** `ss_palette::show()`：那个函数会发 `ss-palette-show` 事件，
/// 前端收到就把搜索状态重置了，而截图小程序正靠这份状态展示「已保存到 …」。
///
/// 抓屏 + PNG 压缩加起来要几百毫秒，所以放到阻塞线程池里跑，别卡住事件循环。
#[tauri::command]
async fn ss_capture_screen(app: tauri::AppHandle) -> Result<String, String> {
    let win = app.get_webview_window(crate::ss_palette::LABEL);
    if let Some(w) = &win {
        let _ = w.hide();
    }

    let captured = tauri::async_runtime::spawn_blocking(move || {
        // 给合成器一点时间把面板从画面里摘掉，否则可能抓到一层残影。
        std::thread::sleep(std::time::Duration::from_millis(220));
        crate::screenshot::capture_to_pictures()
    })
    .await
    .map_err(|e| format!("截图任务异常: {e}"))?;

    if let Some(w) = &win {
        let _ = w.show();
        let _ = w.set_focus();
    }

    captured
}

/// 设置后台字幕浮层启用开关（由前端设置面板调用）。
#[tauri::command]
fn set_overlay_enabled(
    app: tauri::AppHandle,
    state: tauri::State<'_, SessionManager>,
    enabled: bool,
) -> Result<bool, String> {
    state.set_overlay_enabled(enabled);
    if enabled {
        // 录音中途把开关打开 ⇒ 立刻把浮层补上。
        //
        // 后端只在**录音开始**那一刻读过一次开关（`start_hold` / `start_call` 里的
        // `session.overlay_enabled()`），那一瞬间它还是关的。不补这一手，用户把开关打开后
        // 要等到下一轮录音才看得见字幕 —— 他会以为开关没生效。
        // 判据用 `is_listening()`：不在录音中就什么都不做（没内容可显示，也没有该弹出的时机）。
        if state.is_listening() {
            crate::session::show_overlay(&app);
        }
    } else if let Some(win) = app.get_webview_window("overlay") {
        // 关掉开关 ⇒ 把可能正挂在屏幕上的那个收掉。
        //
        // ⛔ 这一句只能在后端：开关的真源是 `SessionManager`，而**"用户把开关关掉"这个时机
        //    前端抓不到**（浮层的显隐权已整体收归后端，理由见 `VoicePanel.tsx` 里那段说明）。
        //    少了它，关掉开关后浮层会一直挂到下一轮录音结束才消失 —— 用户会以为开关坏了。
        let _ = win.hide();
    }
    Ok(enabled)
}

/// 读取后台字幕浮层启用开关（供前端启动时回填）。
#[tauri::command]
fn get_overlay_enabled(state: tauri::State<'_, SessionManager>) -> bool {
    state.overlay_enabled()
}

/// 设置 Everything 可执行文件路径。
#[tauri::command]
fn set_everything_path(
    state: tauri::State<'_, SessionManager>,
    path: String,
) -> Result<String, String> {
    state.set_everything_path(path.clone());
    Ok(path)
}

/// 读取 Everything 可执行文件路径。
#[tauri::command]
fn get_everything_path(state: tauri::State<'_, SessionManager>) -> String {
    state.everything_path()
}

/// 设置文本注入的粘贴方式。
///
/// 返回**规范化后**的字符串（`Ctrl+V` → `ctrl_v`）：前端拿它回填界面，
/// 就不会出现"界面显示用户写的、后端存的是另一个"这种回读不一致。
/// 无法识别的值返回 `Err`，**不静默回落默认值**。
#[tauri::command]
fn set_paste_style(
    state: tauri::State<'_, SessionManager>,
    style: String,
) -> Result<String, String> {
    let s = state.set_paste_style(&style)?;
    Ok(s.as_str().to_string())
}

/// 读取文本注入的粘贴方式。
#[tauri::command]
fn get_paste_style() -> String {
    crate::text_delivery::paste_style().as_str().to_string()
}

/// 设置"粘贴完成后是否再按一次回车"。
#[tauri::command]
fn set_submit_after_paste(state: tauri::State<'_, SessionManager>, enabled: bool) {
    state.set_submit_after_paste(enabled);
}

/// 读取"粘贴完成后是否再按一次回车"。
#[tauri::command]
fn get_submit_after_paste() -> bool {
    crate::text_delivery::submit_after_paste()
}

/// 设置是否启用 ⌘SS 精简右键菜单。
#[tauri::command]
fn set_compact_context_menu(enabled: bool, state: tauri::State<'_, SessionManager>) {
    state.set_compact_context_menu(enabled);
}

/// 读取是否启用 ⌘SS 精简右键菜单。
#[tauri::command]
fn get_compact_context_menu(state: tauri::State<'_, SessionManager>) -> bool {
    state.compact_context_menu()
}

/// 查询开发板 Wi-Fi 音频是否在线（前端状态栏显示）。
#[tauri::command]
fn keyboard_audio_online(state: tauri::State<'_, SessionManager>) -> bool {
    state.keyboard_audio_online()
}

/// 真正退出应用（「最小化到托盘」对话框里的「退出」分支）。
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    log_buffer::info("收到显式退出请求：quit_app");
    request_exit(&app);
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

/// 返回日志目录（落盘文件所在）。
///
/// 界面上要能"一键打开日志目录"：排障时最需要的动作是把当天那份日志直接发出来，
/// 而让用户自己去找 `%APPDATA%\com.easyop.app\logs` 是一道没必要的门槛。
#[tauri::command]
fn get_log_dir() -> Result<String, String> {
    log_buffer::dir()
        .map(|p| p.to_string_lossy().to_string())
        .ok_or_else(|| "日志目录尚未初始化".to_string())
}

/// 在资源管理器中打开日志目录。
#[tauri::command]
fn open_log_dir() -> Result<String, String> {
    let dir = log_buffer::dir().ok_or_else(|| "日志目录尚未初始化".to_string())?;
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("打开日志目录失败: {e}"))?;
    }
    #[cfg(not(windows))]
    {
        return Err("仅支持 Windows".to_string());
    }
    log_buffer::info(format!("打开日志目录：{}", dir.display()));
    Ok(dir.to_string_lossy().to_string())
}

// ---------------------------------------------------------------------------
// 「启动终端并预置命令」—— 配置读写 + 手动触发
// ---------------------------------------------------------------------------

/// 读终端启动配置。
///
/// 界面上的每一格都必须显示**后端真正生效的那个值**：默认命令和后端常量是
/// 同一处来源，界面上另抄一份的话，改了后端默认值界面不跟着动，
/// 用户看到的就是一个"改了没生效"的假象。
#[tauri::command]
fn cli_terminal_get_config() -> cli_terminal::CliTerminalConfig {
    cli_terminal::config()
}

/// 写终端启动配置。返回保存后的规范化配置（命令/目录已 trim），供界面回填。
#[tauri::command]
fn cli_terminal_set_config(config: cli_terminal::CliTerminalConfig) -> Result<cli_terminal::CliTerminalConfig, String> {
    cli_terminal::set_config(config)?;
    Ok(cli_terminal::config())
}

/// 手动触发一次"启动终端并预置命令"，等价于按下绑定键。
///
/// 必须有这个入口，否则用户只能靠"去板子上按一下"来验证配置对不对 ——
/// 那是把最容易出错的一步（命令配没配对）放在最难试的地方。
#[tauri::command]
async fn cli_terminal_launch() -> Result<String, String> {
    // launch() 要等窗口起来（上限 6 秒），是阻塞活，放阻塞线程池里别卡事件循环。
    tauri::async_runtime::spawn_blocking(cli_terminal::launch)
        .await
        .map_err(|e| format!("启动终端任务异常: {e}"))?
}

/// 浮层贴光标定位：返回「光标附近居中」的**物理**坐标，并钳制到光标所在显示器内。
///
/// **这里必须显式做一次逻辑 → 物理换算。** `overlay_w` / `overlay_h` 传进来的是
/// **逻辑**像素 —— 与 `WebviewWindowBuilder::inner_size` 同一坐标系，Windows 下要按
/// 显示器缩放比放大才是真实窗口尺寸；而 `GetCursorPos` 与显示器指标来自 Win32，是
/// **物理**像素。两者混用会得到「水平右偏半个窗宽 + 底部被裁出屏外」的几何错位：
/// 实测 200% 缩放下光标物理 (1800,1000)、期望窗宽 480 逻辑（=960 物理），旧算法拿
/// 480 当物理值算居中，落位 (1560,1020)-(2520,1740) —— 屏幕只有 2560x1440，
/// 于是右边多出 240 像素、底部 300 像素整块丢在屏幕外。
///
/// 钳制范围取**光标所在那块显示器**（`monitor_from_point`）而非主屏 `SM_CXSCREEN`：
/// 后者在副屏上会把浮层推到另一块屏幕上。取不到显示器信息时退回主屏指标。
///
/// 纵向优先放在光标下方；下方放不下就翻到光标上方 —— 浮窗工作台有半屏高，
/// 在屏幕下半部分触发时「下方」几乎总是放不下。
pub(crate) fn cursor_subtitle_pos(
    win: &tauri::WebviewWindow,
    overlay_w: f64,
    overlay_h: f64,
) -> Option<(f64, f64)> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_err() {
                return None;
            }
            let (cx, cy) = (pt.x as f64, pt.y as f64);
            // 统一到物理像素：光标是物理的，窗口尺寸是逻辑的，先换算再算几何。
            let scale = win.scale_factor().unwrap_or(1.0);
            let (w, h) = (overlay_w * scale, overlay_h * scale);
            let (left, top, right, bottom) = monitor_bounds(win, cx, cy);

            // 光标下方居中显示，留出光标高度余量。
            let x = (cx - w / 2.0).clamp(left, (right - w).max(left));
            let below = cy + 20.0;
            let y = if below + h <= bottom {
                below
            } else {
                // 下方放不下：翻到光标上方，再交给下面的钳制兜底。
                cy - 20.0 - h
            };
            Some((x, y.clamp(top, (bottom - h).max(top))))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (win, overlay_w, overlay_h);
        None
    }
}

/// 光标所在显示器在**物理像素**下的可用范围（左、上、右、下）。
///
/// 经 `WebviewWindow::monitor_from_point` 查询，保证多显示器下用的是光标真正所在的那块屏。
/// 查询失败时退回主屏的 `SM_CXSCREEN` / `SM_CYSCREEN`：钳制范围宁可粗糙也不能没有，
/// 否则浮层会直接跑到屏幕外。
#[cfg(windows)]
fn monitor_bounds(win: &tauri::WebviewWindow, cx: f64, cy: f64) -> (f64, f64, f64, f64) {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    if let Ok(Some(monitor)) = win.monitor_from_point(cx, cy) {
        let pos = monitor.position();
        let size = monitor.size();
        return (
            pos.x as f64,
            pos.y as f64,
            (pos.x + size.width as i32) as f64,
            (pos.y + size.height as i32) as f64,
        );
    }

    let (w, h) = unsafe {
        (
            GetSystemMetrics(SM_CXSCREEN) as f64,
            GetSystemMetrics(SM_CYSCREEN) as f64,
        )
    };
    (0.0, 0.0, w, h)
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
                // ⛔ 日志落盘放在这一块的最前面：日志的价值全在"出事那一刻"，
                // 而下面每一项加载都可能失败 —— 排在它们后面就会漏掉最关键的启动错误。
                log_buffer::init(&dir);
                app.state::<SessionManager>().init_config_file(dir.join("config.json"));
                let st = app.state::<AppState>();
                *st.apps.write().unwrap() = state::load_apps(&dir);
                *st.profiles.write().unwrap() = state::load_profiles(&dir);
                // 加载 ⌘SS 使用频次统计
                crate::ss_palette::load_usage_stats(&dir);
                // 加载「小程序」注册表：被集成进 easyop 的外部 App / Web 项目。
                mini_apps::init(&dir);
                // 加载「插件」注册表：放在 plugins/ 下的扩展模块（发现层）。
                plugin_registry::init(&dir);
                // 初始化使用记录存储层（工作台"使用记录中心"）。
                usage_log::init(&dir);
                // 初始化 ASR 自定义词库。
                asr_vocab::init(&dir);
                // 初始化「启动终端」配置（cli_terminal.json）。
                cli_terminal::init(&dir);
                // 初始化 Agent 任务台：配置（agent_desk.json）+ TTS 客户端。
                // 配置独立成文件，不动 config.json —— 多 agent 共树下改动面即风险面。
                app.manage(agent_desk::create_manager(&dir));
                // 启动灯语心跳泵：0x16 帧自带 ttl（到期固件自动回落，所以"上位机没了
                // 灯就自己灭"），但活着的时候必须每 5 秒续期一次，否则灯会周期性闪回。
                agent_desk::start_led_pump(app.handle().clone());
            }

            // 后台预热「本机应用索引」：扫描注册表 / 开始菜单 / 常见安装目录，
            // 并预计算每项的拼音检索键。放在启动阶段做，用户第一次敲字时
            // 搜索路径上就只剩匹配计算，不再现场扫盘 —— 这是「首次搜索卡顿」的修复。
            system_apps::warm_up();

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

            // 后台采样外部前台窗口：浮窗被界面按钮（而非硬件 KEY4）唤起时，弹出瞬间
            // 前台已经是 easyop 自己，只有靠这个线程的持续记录才能拿到焦点归还目标。
            workbench::spawn_foreground_tracker();

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
            //
            // ⛔ 这里过去直接调 `commands::wifi_connect(ssid, password)` —— 那个命令
            // 发的是 `{"cmd":"wifi_connect"}`，而固件配置协议**没有 `cmd` 字段**
            // 且 `schema` 必填 ⇒ 整包被拒；又因为不等回执，日志里只留下一行
            // "Wi-Fi 连接命令已下发"。也就是说：**这段"自动连接"从来没有生效过**。
            //
            // 现在交给 Wi-Fi 链路看护线程（`wifi_link`）：它在自己的节奏里做
            // "探测环境 → 选网 → 下发完整载荷 → 等心跳确认 → 写历史 → 失败退避重试"，
            // 并且和「设置」里的手动「连接」按钮走**同一条**下发路径。
            {
                app.manage(wifi_link::WifiLink::new());
                let link = app.state::<wifi_link::WifiLink>();
                if let Ok(dir) = app.path().app_config_dir() {
                    link.bind_config_dir(dir.clone());
                    // 迁移：把 config.json 里已存的 SSID 补进历史，否则第一次启动
                    // 会显示"没有历史网络"，老用户的既有配置等于白存。
                    let (ssid, password) = app.state::<SessionManager>().get_wifi_config();
                    wifi_link::migrate_legacy_credentials(&dir, &ssid, &password);
                }
                wifi_link::start(app.handle().clone(), (*link).clone());
            }

            // 事件调度线程：钩子事件 → 会话状态机。
            let manager = (*app.state::<SessionManager>()).clone();

            // ★ 装配回车判据：钩子层不存副本，直接问状态机。
            //
            // 用回调而不是"往钩子里塞一个开关"，是为了让"回车该不该被接管"只有一个
            // 真相源。副本会老化，而老化的后果是**把用户所有回车都吞掉** ——
            // 现象与"键盘坏了"一模一样，原因却在一段跟他无关的状态里。
            // （判据必须无锁、无阻塞：它在键盘钩子回调里被调用。见 `grabs_enter`。）
            ptt_hook::set_enter_gate({
                let m = manager.clone();
                move || m.grabs_enter()
            });

            let app_handle = app.handle().clone();
            std::thread::spawn(move || loop {
                match engine.recv_timeout(Duration::from_millis(200)) {
                    // ⛔ 三个事件，**只认"按下"**：固件对 `VoicePttHold` / `EditPttHold`
                    // 发的确实是 keydown/keyup 一对，但产品语义是**短按**
                    // （按一下开始、再按一下结束），keyup 不参与任何判断
                    // —— 理由与那次误改的经过都记在 `PttEvent` 的注释里。
                    Some(ptt_hook::PttEvent::Down) => manager.ptt_down(app_handle.clone()),
                    Some(ptt_hook::PttEvent::EditToggle) => {
                        manager.edit_ptt_down(app_handle.clone())
                    }
                    // 回车：只有在"这一轮的话会进输入框"时才会被报上来
                    // （判据在会话状态机里），一次按键 = 结束 + 注入 + 发送。
                    Some(ptt_hook::PttEvent::EnterSubmit) => {
                        let _ = manager.enter_submit(app_handle.clone());
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
                    "quit" => {
                        log_buffer::info("收到显式退出请求：托盘菜单「退出 easyop」");
                        request_exit(app);
                    }
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

            // KEY4 浮窗工作台（Phase 3）：无边框置顶浮窗，初始隐藏，
            // 由 Host Action 的 workbench UUID 或主窗口命令触发显示。
            workbench::build_window(app.handle())?;

            // ⌘SS 命令面板（Phase 4A）：屏幕居中无边框置顶浮窗，初始隐藏，
            // 由 Host Action 的 ss_palette UUID 或主窗口命令触发显示。
            if let Err(e) = ss_palette::build_window(app.handle()) {
                log_buffer::err(format!("⌘SS 面板窗口创建失败: {e}"));
            }

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
            inject_text,
            hide_to_tray,
            show_main,
            show_overlay,
            hide_overlay,
            // 注入失败的留底（异常路径专用）
            get_failed_inject,
            retry_failed_inject,
            clear_failed_inject,
            set_overlay_enabled,
            get_overlay_enabled,
            set_everything_path,
            get_everything_path,
            // 文本注入（A″：语音打进终端 / TUI 场景）
            set_paste_style,
            get_paste_style,
            set_submit_after_paste,
            get_submit_after_paste,
            set_compact_context_menu,
            get_compact_context_menu,
            keyboard_audio_online,
            quit_app,
            get_logs,
            clear_logs,
            get_log_dir,
            open_log_dir,
            // 「启动终端并预置命令」（按键 → 开终端 → 打命令前缀，不回车）
            cli_terminal_get_config,
            cli_terminal_set_config,
            cli_terminal_launch,
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
            commands::wifi_link_status,
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
            get_edit_phase,
            workbench_toggle,
            workbench_hide,
            workbench_is_open,
            workbench_list,
            workbench_commit,
            commands::workbench_prompt_add,
            commands::workbench_prompt_update,
            commands::workbench_prompt_remove,
            show_ss_palette,
            hide_ss_palette,
            toggle_ss_palette,
            ss_palette_is_open,
            ss_search,
            ss_execute,
            ss_show_context_menu,
            ss_shell_verb,
            ss_copy_path,
            ss_open_containing_folder,
            ss_resize,
            ss_refresh_apps,
            mini_apps_list,
            mini_apps_add,
            mini_apps_remove,
            mini_apps_launch,
            mini_apps_suggest,
            ss_everything_search,
            everything_available,
            ss_capture_screen,
            plugins_list,
            plugins_rescan,
            plugins_dir,
            plugin_pages,
            plugin_trigger_activate,
            plugin_host::eo_get_selection,
            plugin_host::eo_replace_selection,
            plugin_host::eo_inject,
            plugin_host::eo_storage_get,
            plugin_host::eo_storage_set,
            plugin_host::eo_log_info,
            plugin_host::eo_log_warn,
            plugin_host::eo_log_error,
            plugin_host::eo_chat,
            plugin_host::eo_run,
            usage_log::usage_log_query,
            usage_log::usage_log_daily_counts,
            asr_vocab::asr_vocab_list,
            asr_vocab::asr_vocab_add,
            asr_vocab::asr_vocab_remove,
            // Agent 任务台（Phase 1：配置层 + TTS 试合成）
            agent_desk::desk_get_config,
            agent_desk::desk_set_config,
            agent_desk::desk_set_tts_config,
            agent_desk::desk_snapshot,
            agent_desk::desk_probe_driver,
            agent_desk::desk_tts_test,
            // Agent 任务台（Phase 2：运行时编排 —— 队列 / 未读 / 播放租约）
            agent_desk::desk_runtime_state,
            agent_desk::desk_task_history,
            agent_desk::desk_task_log,
            agent_desk::desk_dispatch_task,
            agent_desk::desk_cancel_pending,
            agent_desk::desk_cancel_running,
            agent_desk::desk_clear_pending,
            agent_desk::desk_grant_playback,
            agent_desk::desk_ack_playback,
            agent_desk::desk_release_playback,
            // Agent 任务台（Phase 3 · 「后声」：状态播报 / 未读播报 —— PC 侧喇叭）
            agent_desk::desk_notice_status,
            agent_desk::desk_prepare_notices,
            agent_desk::desk_play_notice,
            agent_desk::desk_session_info,
            agent_desk::desk_reset_session,
            agent_desk::desk_render_result,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // 退出守护：拦截 Tauri 默认的「所有窗口关闭 → 退出应用」。
            //
            // 用户反馈的现象是「在 ⌘SS 面板按 Esc 收面板，easyop 整体退出了」。
            // easyop 的浮层窗口都是程序化创建、常驻不销毁的，任何一次误判
            // 「窗口都关了」都会让整个应用静默消失。因此这里只放行显式退出意图
            // （`quit_app` 命令 / 托盘「退出 easyop」），其余一律 prevent_exit。
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if EXIT_INTENT.load(Ordering::SeqCst) {
                    log_buffer::info("确认显式退出意图，允许 easyop 退出");
                    // 停掉板子音频监听线程。**必须在这里显式停** —— 那个线程刻意
                    // 不挂在 `Drop` 上（挂了的话，录音函数里任意一个局部克隆释放
                    // 都会把它带走，见 keyboard_audio 里的说明）。
                    _app.state::<SessionManager>().shutdown_keyboard_audio();
                    // 停掉 Wi-Fi 链路看护线程。理由同上：它同样不挂 `Drop`
                    // （里面每秒一次的环境探测不该被某个局部克隆的释放带走）。
                    _app.state::<wifi_link::WifiLink>().stop();
                } else {
                    log_buffer::info("拦截退出请求：非显式退出（可能只是收起了浮层/面板）");
                    api.prevent_exit();
                }
            }
        });
}
