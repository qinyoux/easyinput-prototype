//! 后端共享状态：应用库、按键方案库、Host Action 监听器运行标志，以及 JSON 持久化。
//!
//! 通过 Tauri `manage()` 注入为全局 `AppState`，供各命令与后端线程共享。
//! 应用库（`apps`）会在启动 Host Action 监听器时被克隆进后台线程，用于按 UUID 打开目标。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};

use crate::model::{AppEntry, Profile};

/// Host Action v1 事件：固件回发某个 UUID，本机据此打开目标后转发给前端展示。
#[derive(Serialize, Clone, Debug)]
pub struct HostActionEvent {
    /// 触发动作对应的 UUID。
    pub uuid: String,
    /// 打开结果说明（成功或失败）。
    pub message: String,
}

/// 方案切换事件：主机端完成方案切换并下发固件后通知前端同步 UI。
#[derive(Serialize, Clone, Debug)]
pub struct ProfileSwitchedEvent {
    /// 新激活方案的 ID。
    pub profile_id: String,
    /// 新激活方案的名称。
    pub profile_name: String,
    /// 切换结果说明。
    pub message: String,
}

/// 后端全局共享状态。
pub struct AppState {
    /// 本机应用库（UUID → 打开目标）。监听器线程持有其 `Arc`。
    pub apps: Arc<RwLock<Vec<AppEntry>>>,
    /// 按键方案库（可保存/切换的 profile 集合）。
    pub profiles: Arc<RwLock<Vec<Profile>>>,
    /// 当前激活方案 ID（前端 UI 切换或固件按键触发切换时同步）。
    pub active_profile_id: Arc<RwLock<Option<String>>>,
    /// 含非ASCII字符的固定文本动作映射（UUID → 文本内容）。
    /// 下发配置时生成，固件回发 UUID 后主机用剪贴板+Ctrl+V 注入。
    pub text_actions: Arc<RwLock<HashMap<String, String>>>,
    /// Host Action 监听器是否正在运行（供前端轮询/展示）。
    pub listener_running: Arc<AtomicBool>,
    /// 当前监听线程的停止标志。置 true 可优雅退出；重启时替换为新的标志。
    pub listener_stop: Mutex<Option<Arc<AtomicBool>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            apps: Arc::new(RwLock::new(Vec::new())),
            profiles: Arc::new(RwLock::new(vec![Profile::factory_default("默认方案", "default")])),
            active_profile_id: Arc::new(RwLock::new(None)),
            text_actions: Arc::new(RwLock::new(HashMap::new())),
            listener_running: Arc::new(AtomicBool::new(false)),
            listener_stop: Mutex::new(None),
        }
    }
}

impl AppState {
    /// 标记监听器状态。
    pub fn set_listener(&self, running: bool) {
        self.listener_running.store(running, Ordering::Relaxed);
        if !running {
            // 请求当前监听线程退出；占位为 None（尚未启动时 no-op）。
            if let Ok(mut slot) = self.listener_stop.lock() {
                if let Some(stop) = slot.take() {
                    stop.store(true, Ordering::Relaxed);
                }
            }
        }
    }

    /// 获取（并登记）一个新的停止标志，供刚启动的监听线程使用。
    pub fn take_stop(&self) -> Arc<AtomicBool> {
        let stop = Arc::new(AtomicBool::new(false));
        if let Ok(mut slot) = self.listener_stop.lock() {
            // 先请求可能仍在运行的旧线程退出。
            if let Some(old) = slot.take() {
                old.store(true, Ordering::Relaxed);
            }
            *slot = Some(stop.clone());
        }
        stop
    }
}

/// 数据文件路径（在 Tauri 配置目录下）。
pub fn profiles_path(config_dir: &PathBuf) -> PathBuf {
    config_dir.join("profiles.json")
}
pub fn apps_path(config_dir: &PathBuf) -> PathBuf {
    config_dir.join("apps.json")
}

/// 从磁盘加载方案库；文件不存在时返回默认单方案。
pub fn load_profiles(config_dir: &PathBuf) -> Vec<Profile> {
    load_json(config_dir, profiles_path(config_dir)).unwrap_or_else(|| {
        vec![Profile::factory_default("默认方案", "default")]
    })
}

/// 从磁盘加载应用库；文件不存在时返回空库。
pub fn load_apps(config_dir: &PathBuf) -> Vec<AppEntry> {
    load_json(config_dir, apps_path(config_dir)).unwrap_or_default()
}

fn load_json<T: for<'de> Deserialize<'de>>(config_dir: &PathBuf, path: PathBuf) -> Option<T> {
    let _ = fs::create_dir_all(config_dir);
    let data = fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

/// 持久化方案库。
pub fn save_profiles(config_dir: &PathBuf, profiles: &[Profile]) -> Result<(), String> {
    save_json(config_dir, profiles_path(config_dir), profiles)
}

/// 持久化应用库。
pub fn save_apps(config_dir: &PathBuf, apps: &[AppEntry]) -> Result<(), String> {
    save_json(config_dir, apps_path(config_dir), apps)
}

/// 激活码（授权码）文件路径。
pub fn license_path(config_dir: &PathBuf) -> PathBuf {
    config_dir.join("license.json")
}

/// 从磁盘加载已保存的激活码（原始字符串）；不存在时返回 None。
pub fn load_license(config_dir: &PathBuf) -> Option<String> {
    load_json(config_dir, license_path(config_dir))
}

/// 持久化激活码（原始字符串写入 JSON 文件）。
pub fn save_license(config_dir: &PathBuf, code: &str) -> Result<(), String> {
    save_json(config_dir, license_path(config_dir), code)
}

fn save_json<T: Serialize + ?Sized>(config_dir: &PathBuf, path: PathBuf, value: &T) -> Result<(), String> {
    fs::create_dir_all(config_dir).map_err(|e| format!("创建配置目录失败：{e}"))?;
    let json = serde_json::to_string_pretty(value).map_err(|e| format!("序列化失败：{e}"))?;
    fs::write(path, json).map_err(|e| format!("写入配置失败：{e}"))
}
