//! Tauri 命令层：把前端请求映射到各后端模块。
//!
//! 前端仅通过这里的 `invoke` 命令与后端交互，不直接触碰 Win32/协议细节。
//! 命令分为六组：
//!   设备       device_status / push_config
//!   监听器     start_listener / stop_listener / listener_status
//!   应用库     list_apps / add_app / update_app / remove_app / test_open
//!   方案库     list_profiles / save_profile / delete_profile / new_profile
//!   提示词     workbench_prompt_add / workbench_prompt_update / workbench_prompt_remove
//!   词表       get_vocabulary（供前端渲染下拉）

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use std::collections::{HashMap, HashSet};

use crate::capability::{ApiEnvelope, ErrorCode};
use crate::license::LicenseInfo;
use crate::model::{AppEntry, ConfigAck, DeviceStatus, Profile, Prompt};
use crate::state::{self, AppState};
use crate::system_apps::SystemApp;
use crate::{device, host_action, license, status_query, validate, workbench};

/// 一组可绑定到按键的「具名动作」，前端以**独立下拉菜单**呈现（按插件归类）。
///
/// 分组真源：每个声明了 `commands` 的**可见**插件 => 一个顶层菜单
/// （`id` = 插件 id，`label` = 插件名）；隐藏插件（如 demo）不入列。
/// 主机内置功能动作单独走 `Vocabulary.core_actions`，不与插件混排。
/// 这样「实时翻译」「内容台」「风格调色盘」等插件各自成顶层菜单。
#[derive(Serialize)]
pub struct ActionGroup {
    /// 分组 id：插件 id（如 `live-translate`）。
    pub id: String,
    /// 分组展示名（下拉菜单标题），如「实时翻译」「内容台」「风格调色盘」。
    pub label: String,
    /// 该分组下的动作 id 列表（顺序即下拉顺序）。
    pub actions: Vec<String>,
}

/// 词表 / 常量（供前端渲染下拉框与做前端预校验）。
#[derive(Serialize)]
pub struct Vocabulary {
    pub named_actions: Vec<String>,
    pub action_labels: HashMap<String, String>,
    /// 按插件归类的动作分组（每个声明了命令的可见插件 => 一个顶层菜单）。
    pub action_groups: Vec<ActionGroup>,
    /// 主机内置功能动作（「功能动作」菜单用，不含插件命令）。
    pub core_actions: Vec<String>,
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
    // 只在连接状态变化时写日志：离线→在线写"连接成功"，在线→离线写"断开"。
    // 状态不变时不写，避免刷屏。
    static WAS_ONLINE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static INITIALIZED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let now_online = result.is_ok();
    let was_online = WAS_ONLINE.load(std::sync::atomic::Ordering::Relaxed);
    let initialized = INITIALIZED.load(std::sync::atomic::Ordering::Relaxed);
    if !initialized {
        INITIALIZED.store(true, std::sync::atomic::Ordering::Relaxed);
    } else if now_online && !was_online {
        crate::log_buffer::info("设备连接成功");
    } else if !now_online && was_online {
        crate::log_buffer::warn("设备断开");
    }
    WAS_ONLINE.store(now_online, std::sync::atomic::Ordering::Relaxed);
    result
}

/// 读开发板固件状态（`0x13` 状态请求 → `0x11` / kind `0x04` 分块响应）。
///
/// 返回固件版本串（如 `0.4.53-idf-v2-ble-store-v1`）—— 这是"板子上到底跑着哪一版"
/// 唯一可信的回答：它由板子自己报出，不依赖任何本地文件、也不依赖"我记得烧过"。
///
/// # 为什么单独立一条命令，不并进 `device_status`
///
/// `device_status` 是**枚举即返回**（打开句柄看一眼 VID/PID 就完），前端挂载后按秒轮询；
/// 这条则要走完整的一次"发请求 → 收齐约 10 块 → 校验 CRC"，实测约 100 ms。
/// 合并会让每次在线检测都白多花这 100 ms，而在线检测是高频路径。
///
/// # 失败不静默
///
/// "板子不在 USB 上"、"载荷不被固件接受"、"分块 CRC 不过"三种情况各返回**各自的**
/// 错误文案，前端原样显示。档位只有"读到版本"与"读不到并说明原因"两种 ——
/// 不存在"显示一个可能过期的版本号"。
#[tauri::command]
pub(crate) fn device_firmware_status() -> Result<DeviceStatus, String> {
    let result = status_query::query_device_status();
    // 只在**版本发生变化**时写日志：这条命令会被反复调用（挂载、手动重试、下发成功后），
    // 每次都打一行会把日志刷成流水，反而盖掉真正要看的东西。
    static LAST: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    match &result {
        Ok(s) if last.as_deref() != Some(s.firmware.as_str()) => {
            crate::log_buffer::info(format!("开发板固件版本：{}", s.firmware));
            *last = Some(s.firmware.clone());
        }
        Ok(_) => {}
        Err(e) => {
            // 读不到时不打日志（前端已经在界面上如实显示，日志里再刷没有新信息）；
            // 但"上一轮还读得到、这一轮读不到了"是一次真实的状态变化，必须留痕。
            if last.is_some() {
                crate::log_buffer::warn(format!("开发板固件版本读取失败：{e}"));
                *last = None;
            }
        }
    }
    result
}

/// 激活（注册）一个本地授权码：校验签名 → 通过后持久化到配置目录 `license.json`。
///
/// 校验失败返回 `InvalidInput` 错误信封，**不**写入；已存在的激活码不会被无效码覆盖。
#[tauri::command]
pub(crate) fn activate_license(app: AppHandle, code: String) -> ApiEnvelope<LicenseInfo> {
    let dir = match config_dir(&app) {
        Ok(d) => d,
        Err(e) => return ApiEnvelope::err(ErrorCode::Internal, e),
    };
    let mid = license::machine_id();
    match license::verify_license(&code, &mid) {
        Ok(info) => match state::save_license(&dir, &code) {
            Ok(()) => {
                crate::log_buffer::info(format!(
                    "激活成功：版次={} 备注={}",
                    info.edition, info.note
                ));
                ApiEnvelope::ok(info)
            }
            Err(e) => {
                ApiEnvelope::err(ErrorCode::Internal, format!("激活码有效，但保存失败：{e}"))
            }
        },
        Err(e) => ApiEnvelope::err(ErrorCode::InvalidInput, e),
    }
}

/// 读取当前注册状态：从配置目录加载已保存的激活码并重新校验（过期/篡改都会如实反映）。
///
/// 从未激活过返回"未注册（试用版）"；激活过但当前无效（过期/机器不符）返回具体原因。
#[tauri::command]
pub(crate) fn license_status(app: AppHandle) -> ApiEnvelope<LicenseInfo> {
    let dir = match config_dir(&app) {
        Ok(d) => d,
        Err(e) => return ApiEnvelope::err(ErrorCode::Internal, e),
    };
    match state::load_license(&dir) {
        Some(code) => {
            let mid = license::machine_id();
            match license::verify_license(&code, &mid) {
                Ok(info) => ApiEnvelope::ok(info),
                Err(e) => ApiEnvelope::ok(LicenseInfo::invalid(e)),
            }
        }
        None => ApiEnvelope::ok(LicenseInfo::unregistered()),
    }
}

/// 下发一个完整方案：本地校验 → 构造固件 JSON → 分块经 0x10 下发 → 读取 0x11 kind 0x03 回执。
///
/// 顺带刷新浮窗提示词库的列表。提示词**不进固件**，与本次下发没有任何数据依赖，
/// 这里只是把「方案内容可能变了、浮窗该重画一次」的触发点挂在同一条路径上 ——
/// 改造前那种「列表的 UUID 必须与本次下发写进固件的那一份完全一致，所以两件事
/// 必须共用同一个 `FirmwarePayload`」的约束已经不存在了。
#[tauri::command]
pub(crate) fn push_config(
    app: AppHandle,
    profile: Profile,
    state: State<'_, AppState>,
) -> Result<ConfigAck, String> {
    crate::log_buffer::info(format!("下发方案: {} (按键数={})", profile.name, profile.keys.len()));
    validate::validate_profile(&profile).map_err(|e| {
        crate::log_buffer::err(format!("校验失败: {e}"));
        e
    })?;
    // ⛔ 这里**必须**走"带端点"的构造：固件 `apply_json` 整体替换配置，
    // 载荷里没提到的字段一律回默认值（`wifi_ssid` / `audio_host` 默认空串）。
    // 旧代码只发 schema + target_platform + profiles ⇒ **每按一次「下发方案」，
    // 板子上已配好的 Wi-Fi 端点就被清空一次**，随之按 `audio_enabled == false`
    // 关掉 Wi-Fi。现象是"改了键位之后麦克风就不出声了"，且没有任何报错。
    let (ssid, password) = app.state::<SessionManager>().get_wifi_config();
    let built = crate::wifi_link::build_payload_with_endpoint(&app, &ssid, &password)?;
    // 更新主机端文本动作映射（含非ASCII的固定文本 → UUID）。
    *state.text_actions.write().unwrap() = built.text_map.clone();
    workbench::publish_board_input(&app, &profile);
    crate::log_buffer::info(format!("固件 JSON 已构造 ({} 字节)", built.json.len()));
    let result = crate::transport::send_config(&built.json);
    match &result {
        Ok(ack) => crate::log_buffer::info(format!("下发回执: {:?}", ack)),
        Err(e) => crate::log_buffer::err(format!("下发失败: {e}")),
    }
    result
}

/// 返回词表与常量，供前端一次性拉取。
#[tauri::command]
pub(crate) fn get_vocabulary() -> Vocabulary {
    // 隐藏的插件（如 demo）不进按键菜单与校验词表。
    let hidden_ids: Vec<String> = crate::plugin_registry::list()
        .iter()
        .filter(|p| p.hidden)
        .map(|p| p.id.clone())
        .collect();
    let is_hidden = |id: &str| -> bool {
        let pid = id.split('.').next().unwrap_or("");
        hidden_ids.iter().any(|h| h == pid)
    };

    let mut named_actions: Vec<String> = crate::constants::NAMED_ACTIONS
        .iter()
        .map(|s| s.to_string())
        .collect();
    named_actions.extend(
        crate::plugin_registry::action_uuids()
            .keys()
            .filter(|k| !is_hidden(k))
            .cloned(),
    );

    // 按插件分组：仅可见插件；每个声明了命令的插件 => 一个顶层菜单。
    let mut groups: Vec<ActionGroup> = Vec::new();
    for p in crate::plugin_registry::list() {
        if p.hidden || p.commands.is_empty() {
            continue;
        }
        let actions: Vec<String> = p.commands.iter().map(|c| c.id.clone()).collect();
        groups.push(ActionGroup {
            id: p.id.clone(),
            label: p.name.clone(),
            actions,
        });
    }
    // 未被任何插件认领的主机内置动作 => 「功能动作」核心菜单（独立项）。
    let placed: HashSet<String> = groups
        .iter()
        .flat_map(|g| g.actions.iter().cloned())
        .collect();
    let core: Vec<String> = crate::constants::NAMED_ACTIONS
        .iter()
        .filter(|a| {
            let s: &str = *a;
            !placed.iter().any(|p| p.as_str() == s)
        })
        .map(|s| s.to_string())
        .collect();

    // 动作标签：去掉隐藏插件，避免菜单里残留。
    let mut action_labels = crate::plugin_registry::action_labels();
    action_labels.retain(|k, _| !is_hidden(k));

    Vocabulary {
        named_actions,
        action_labels,
        action_groups: groups,
        core_actions: core,
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

// ---------- 提示词库 CRUD（KEY4 浮窗里直接增删改）----------

/// 在指定方案上做一次提示词变更：改内存 → 校验 → 落盘 → 广播新快照。
///
/// 这几件事**必须绑定在一起**，页面上的增删改全部走这里：
/// - 只改内存不落盘，重启后浮窗里刚加的词就消失了；
/// - 只落盘不广播，正开着的浮窗会一直显示改动前的列表，用户以为没生效；
/// - 校验放在落盘前，避免把空词 / 超长词写进 `profiles.json`。
///
/// **先落盘、后广播**：广播是在向界面承诺「你看到的就是最终状态」，
/// 写盘失败时不该给出这个承诺，否则界面与磁盘会静默分叉。
fn mutate_prompts<F>(
    app: &AppHandle,
    state: &State<'_, AppState>,
    profile_id: &str,
    mutate: F,
) -> Result<workbench::BoardInputEvent, String>
where
    F: FnOnce(&mut Vec<Prompt>) -> Result<(), String>,
{
    let (updated, all) = {
        let mut guard = state.profiles.write().unwrap();
        let slot = guard
            .iter_mut()
            .find(|p| p.id == profile_id)
            .ok_or_else(|| format!("方案不存在：{profile_id}"))?;
        mutate(&mut slot.prompts)?;
        validate::validate_prompts(&slot.prompts)?;
        (slot.clone(), guard.clone())
    };
    persist_profiles(app, &all)?;
    workbench::publish_board_input(app, &updated);
    Ok(workbench::snapshot(&updated))
}

/// 向指定方案的提示词库末尾追加一条，返回改动后的完整快照。
#[tauri::command]
pub(crate) fn workbench_prompt_add(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    text: String,
) -> Result<workbench::BoardInputEvent, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("提示词内容不能为空".to_string());
    }
    mutate_prompts(&app, &state, &profile_id, move |prompts| {
        prompts.push(Prompt { id: Uuid::new_v4().to_string(), text });
        Ok(())
    })
}

/// 改写指定方案里某条提示词的正文（按条目 id 定位），返回改动后的完整快照。
///
/// 条目 id 保持不变：它是页面定位「改哪一条」的唯一凭据，换了 id 会让浮窗当前
/// 高亮的那一行在改动后失去指向。
#[tauri::command]
pub(crate) fn workbench_prompt_update(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    id: String,
    text: String,
) -> Result<workbench::BoardInputEvent, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("提示词内容不能为空".to_string());
    }
    mutate_prompts(&app, &state, &profile_id, move |prompts| {
        let slot = prompts
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| "提示词不存在（可能已被删除）".to_string())?;
        slot.text = text;
        Ok(())
    })
}

/// 从指定方案的提示词库里删除一条（按条目 id 定位），返回改动后的完整快照。
#[tauri::command]
pub(crate) fn workbench_prompt_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    id: String,
) -> Result<workbench::BoardInputEvent, String> {
    mutate_prompts(&app, &state, &profile_id, move |prompts| {
        let before = prompts.len();
        prompts.retain(|p| p.id != id);
        if prompts.len() == before {
            // 明确报错而不是静默成功：多半是主窗口刚删过同一条，让浮窗页面有机会刷新。
            return Err("提示词不存在（可能已被删除）".to_string());
        }
        Ok(())
    })
}

// ---------- 通信配置（Wi-Fi / 蓝牙 / 麦克风源）----------

use crate::session::{SessionManager, MicSource};

/// `wifi_connect` 的返回：**如实说明这一次到底发生了什么**。
///
/// 旧版本返回 `()`，界面上"连接成功"是**本地硬写**的（`setWifiConnected(true)`），
/// 与板子真实状态无关。这个结构就是为了让"下发成功但板子没连上"能被看见。
#[derive(serde::Serialize)]
pub(crate) struct WifiConnectOutcome {
    pub ssid: String,
    /// 用哪条通道送到的：`usb-hid` / `wifi-udp` / `usb-hid + wifi-udp`。
    pub channel: String,
    /// 板子是否已把端点写进 NVS ⇒ **下次上电它能自己连，不需要上位机在场**。
    pub saved_to_board: bool,
    /// 本机 IPv4（板子要回连的地址）。
    pub lan_ipv4: Option<String>,
    /// 还没连上时给用户的一句话（连上了是 `None`）。
    pub hint: Option<String>,
}

/// 下发 Wi-Fi 端点并持久化：**把当前 Wi-Fi 写进板子的完整配置载荷**。
///
/// # ⛔ 旧实现为什么从来没生效过（2026-09-24 查清）
///
/// 旧实现发的是 `{"cmd":"wifi_connect","ssid":…,"password":…}`，但固件的配置解析器
/// （`config_payload.cpp::parse_config_payload`）
///
/// 1. **没有 `cmd` 这个字段**：顶层只认 `schema` / `profiles` / `ptt_hotkey` /
///    `audio_enabled` / `audio_source` / `microphone_source` / `audio_host` /
///    `audio_port` / `wifi_ssid` / `wifi_password` / `speaker_sync_key` 等固定键；
/// 2. **`schema` 必填且必须等于 `ai_keyboard.v1`**，否则整包判 `InvalidSchema`
///    直接丢弃；
/// 3. 而 `transport::send_command` **不等回执** ⇒ 上位机日志照样写"命令已下发"。
///
/// 三者叠加 = **点一下"连接"，界面变绿，板子什么都没收到**。用户唯一的线索是
/// "麦克风还是不出声"，而那个现象有十几种解释。
///
/// # 现在的做法
///
/// 走 [`crate::wifi_link`]：拼**完整**载荷（键位方案 + Wi-Fi 端点），
/// 先试 Wi-Fi UDP 直投（板子已在线时不用插 USB），再试 USB HID 配置通道
/// （首次配网唯一可用的通道），并且**等 `0x11` 回执拿到 `saved`**。
/// 「连接成功」以**收到板子心跳**为准，由看护线程判定并写进历史。
#[tauri::command]
pub(crate) fn wifi_connect(
    app: AppHandle,
    state: State<'_, SessionManager>,
    link: State<'_, crate::wifi_link::WifiLink>,
    ssid: String,
    password: String,
) -> Result<WifiConnectOutcome, String> {
    let ssid = ssid.trim().to_string();
    if ssid.is_empty() {
        return Err("Wi-Fi SSID 不能为空".to_string());
    }
    // 先落本地（config.json）—— 它是"上位机认为该连哪个网"的真相源。
    state.set_wifi_config(&ssid, &password);
    crate::log_buffer::info(format!(
        "Wi-Fi 连接：下发端点（SSID={}，密码长度={}）",
        crate::wifi_link::mask_ssid(&ssid),
        password.len()
    ));

    let outcome = crate::wifi_link::provision_with(
        &app,
        &link,
        Some((&ssid, &password)),
        "手动连接",
        true,
    )?;
    // 登记成"等确认"：看护线程收到心跳时才把这条算作成功。
    link.note_manual(&ssid);

    let lan_ipv4 = crate::wifi_link::cached_lan_ipv4().into_iter().next();
    if !outcome.any_ok() {
        let hint = outcome
            .error_text()
            .unwrap_or_else(|| "两条下发通道都不可用".to_string());
        crate::log_buffer::err(format!("Wi-Fi 连接：下发失败 — {hint}"));
        return Err(hint);
    }

    let hint = if outcome.saved() {
        None
    } else {
        Some(
            "端点已发出，但没拿到板子的存盘回执（多半只走了 UDP 直投）。\
             以「Wi-Fi 已连接」徽标为准。"
                .to_string(),
        )
    };
    Ok(WifiConnectOutcome {
        ssid,
        channel: outcome.channel_label(),
        saved_to_board: outcome.saved(),
        lan_ipv4,
        hint,
    })
}

/// 断开开发板的 Wi-Fi 音频。
///
/// # ⛔ 旧实现同样从来没生效过
///
/// 旧实现发 `{"cmd":"wifi_disconnect"}`，理由与 [`wifi_connect`] 完全一样
/// （固件没有 `cmd` 字段、`schema` 必填）。而且它**连本地配置都没清** ——
/// 于是"断开"之后 `mic_source` 仍是 `wifi`，下次启动看护线程又把它连回去。
///
/// # 现在怎么断开
///
/// 下发一份 **`wifi_ssid` 为空**的完整载荷：固件的
/// `ConfigState::audio_enabled()` 是由 `wifi_ssid`/`audio_host`/`audio_port`
/// 三者推导出来的（都非空才为真），所以空 SSID 就等于"关掉这条通道"，
/// 板子会主动释放 Wi-Fi 省电。这是**唯一**在固件现有协议下语义正确的断开方式。
#[tauri::command]
pub(crate) fn wifi_disconnect(
    app: AppHandle,
    state: State<'_, SessionManager>,
    link: State<'_, crate::wifi_link::WifiLink>,
) -> Result<String, String> {
    crate::log_buffer::info("Wi-Fi 断开：下发空端点（板子将释放 Wi-Fi）");
    // 本地也要清掉，否则下次启动又会被自动连回去 —— 这正是旧实现的漏洞。
    state.set_wifi_config("", "");
    let outcome =
        crate::wifi_link::provision_with(&app, &link, Some(("", "")), "手动断开", true)?;
    if !outcome.any_ok() {
        let hint = outcome.error_text().unwrap_or_else(|| "下发失败".to_string());
        crate::log_buffer::err(format!("Wi-Fi 断开：下发失败 — {hint}"));
        return Err(hint);
    }
    Ok(format!("已下发断开（通道 {}）", outcome.channel_label()))
}

/// Wi-Fi 链路的实时状态：给「设置 → 通用」那张卡片看。
#[tauri::command]
pub(crate) fn wifi_link_status(link: State<'_, crate::wifi_link::WifiLink>) -> crate::wifi_link::WifiLinkStatus {
    link.status()
}

/// 蓝牙开关/配对类命令的**如实答复**。
///
/// 三个命令共用一个原因，避免三处各写一遍而在将来的某一次修改里走样。
///
/// # 为什么它们是错误而不是空操作（对固件源码核实，09-21）
///
/// 1. 固件的配置载荷解析器只认一组**固定的顶层键** —— `schema` / `profiles` /
///    `ptt_hotkey` / `ptt_hotkey_source` / `edit_ptt_hotkey` / `hotkey_mode` /
///    `target_platform` / `audio_enabled` / `audio_source` / `microphone_source` /
///    `audio_host` / `audio_port` / `wifi_ssid` / `wifi_password` /
///    `speaker_sync_key` 等，**没有 `cmd` 字段**。所以 `{"cmd":"ble_enable"}`
///    到达固件之后没有任何分支会处理它。
/// 2. 下发走的是 `transport::send_command`，**不等回执** ⇒ 命令被无视也无人知晓。
///    于是"开关点下去 → 界面切成开 → 板子毫无变化"这个状态可以一直存在。
/// 3. 固件的 BLE HID 是**常开**的（广播、连接身份、HID 上报都在跑，没有开关），
///    本来也不需要这两个命令；蓝牙配对由 Windows 侧完成。
///
/// 按「失败不静默」：宁可让界面明确报错，也不要留一个"按了等于没按"的开关
/// —— 那会让用户在错误的方向上花掉整个下午。
const BLE_NOT_SUPPORTED: &str = "开发板固件没有蓝牙开关/配对命令：它的配置协议只接受固定的配置字段（没有 cmd 指令），\
所以这个开关不会改变开发板行为。开发板的蓝牙 HID 是常开的 —— 请在 Windows「蓝牙和其他设备」里与它配对，本应用不参与配对。";

/// 下发蓝牙启用命令到开发板。
///
/// ⛔ **这个命令从来没有生效过。** 现在改为如实报错，不再假装成功（见 [`BLE_NOT_SUPPORTED`]）。
#[tauri::command]
pub(crate) fn ble_enable() -> Result<(), String> {
    crate::log_buffer::warn("蓝牙启用命令：固件未实现该命令，已如实报错");
    Err(BLE_NOT_SUPPORTED.to_string())
}

/// 下发蓝牙关闭命令到开发板。同 [`ble_enable`]，如实报错。
#[tauri::command]
pub(crate) fn ble_disable() -> Result<(), String> {
    crate::log_buffer::warn("蓝牙关闭命令：固件未实现该命令，已如实报错");
    Err(BLE_NOT_SUPPORTED.to_string())
}

/// 下发蓝牙配对命令到开发板。同 [`ble_enable`]，如实报错。
#[tauri::command]
pub(crate) fn ble_pair() -> Result<(), String> {
    crate::log_buffer::warn("蓝牙配对命令：固件未实现该命令，已如实报错");
    Err(BLE_NOT_SUPPORTED.to_string())
}

/// 查询开发板蓝牙连接状态。
///
/// # ⛔ 这里以前报的根本不是蓝牙
///
/// 旧实现是 `if keyboard_audio_online() { "在线（Wi-Fi 音频通道活跃）" } else { "离线" }`
/// —— 它读的是**板子 Wi-Fi UDP 音频通道的心跳**，与蓝牙没有任何关系。两件不相干的
/// 事被捆成一个标签之后：只要板子的 Wi-Fi 音频没上来（哪怕蓝牙连得好好的），
/// 界面就断言"蓝牙离线"，用户于是去查蓝牙设置，而问题在别处。**排查方向从一开始就是错的。**
///
/// # 真实情况
///
/// 开发板**确实**有真实的蓝牙连接状态，而且字段很全：
/// `{"ble":{"connected":1,"valid":1,"itvl":..,"latency":..,"timeout":..}}`。
/// 但它**只经 BLE GATT 特征发布**（固件 `publish_config_status` →
/// `app->ble.publish_status_json`），**不走 USB HID 的 `0x11` 回执**。
/// 本应用没有 BLE central 能力，所以**读不到**。
///
/// 正确做法是承认读不到 —— 拿另一个通道的状态顶替它，比什么都不显示更坏。
/// Wi-Fi 音频通道的状态另有其出口：`keyboard_audio_online` 命令（语音面板的麦克风徽标）。
#[tauri::command]
pub(crate) fn ble_status() -> String {
    "读不到（开发板只在蓝牙 GATT 通道上发布蓝牙状态，本应用未实现蓝牙读取）".to_string()
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
