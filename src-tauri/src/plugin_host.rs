//! 插件激活层（plugin_host）—— 通过 Tauri 事件把插件代码推给前端执行。
//!
//! 发现层（`plugin_registry`）只负责"读清单"，本模块负责"跑代码"。
//!
//! 流程：查插件 → 读 `index.js` → emit `plugin:activate` 事件 →
//! 前端 `pluginHost.ts` 监听 → 定义 eo.* + blob import + activate + 执行命令。
//!
//! # 为什么不用 eval / 隐藏 webview
//!
//! Tauri 2 的 `WebviewWindow::eval` 和 `initialization_script` 在动态创建的
//! webview 里存在时序问题（JS 可能在页面加载前被丢弃）。改用事件方案：
//! Rust emit 事件 → 前端 listener 接收并执行，时序由前端 DOM 就绪保证。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::clipboard_util;
use crate::llm_client;
use crate::log_buffer;
use crate::plugin_registry;
use crate::runner;
use crate::text_delivery;
use crate::voice_edit;

// ---------------------------------------------------------------------------
// 激活
// ---------------------------------------------------------------------------

/// 激活插件并执行指定命令。
///
/// 幂等：若插件 webview 已存在（已激活），直接执行命令不再重新 activate。
/// 返回 `{"activated": true, "executed": "<command_id>"}`。
pub async fn activate(
    app: &tauri::AppHandle,
    plugin_id: &str,
    command_id: &str,
) -> Result<serde_json::Value, String> {
    let plugin = plugin_registry::find_loaded(plugin_id)
        .ok_or(format!("插件不存在：{plugin_id}"))?;

    let index_path = Path::new(&plugin.dir).join(&plugin.manifest.main);
    let index_code = std::fs::read_to_string(&index_path)
        .map_err(|e| format!("读取插件入口失败 {index_path:?}: {e}"))?;


    log_buffer::info(format!(
        "plugin_host::activate 开始：{plugin_id} 命令 {command_id}，入口 {index_code_len} 字节",
        index_code_len = index_code.len()
    ));

    // 通过 Tauri 事件把插件代码推给前端，由前端执行（绕过 eval 时序问题）。
    // 前端监听 `plugin:activate` 事件，收到后定义 eo.* + import 插件 + activate + 执行命令。
    use tauri::Emitter;
    let payload = serde_json::json!({
        "pluginId": plugin_id,
        "commandId": command_id,
        "code": index_code,
        // 插件自己的目录。插件往往自带脚本 / 资源（内容台要调 tools/studio_io.py），
        // 而 `eo.*` 目前没有「取自身路径」的能力——由 host 直接给，省得插件去猜。
        "dir": plugin.dir.to_string_lossy(),
    });
    app.emit("plugin:activate", payload).map_err(|e| {
        log_buffer::err(format!("emit plugin:activate 失败：{e}"));
        format!("emit plugin:activate 失败: {e}")
    })?;

    log_buffer::info(format!(
        "plugin:activate 事件已 emit：{plugin_id}，命令 {command_id}（等前端执行）"
    ));

    Ok(serde_json::json!({ "activated": true, "executed": command_id }))
}


// ---------------------------------------------------------------------------
// eo.* Tauri commands
// ---------------------------------------------------------------------------

/// 读选中文本。
#[tauri::command]
pub async fn eo_get_selection() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| clipboard_util::get_selected_text())
        .await
        .map_err(|e| format!("get_selection 失败: {e}"))?
}

/// 替换选中文本。
#[tauri::command]
pub async fn eo_replace_selection(text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || clipboard_util::replace_selected_text(&text))
        .await
        .map_err(|e| format!("replace_selection 失败: {e}"))?
}

/// 注入文本到光标处。
#[tauri::command]
pub async fn eo_inject(text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || text_delivery::deliver(&text))
        .await
        .map_err(|e| format!("inject 失败: {e}"))?
}

/// 读插件存储。返回 `null` 表示 key 不存在。
#[tauri::command]
pub async fn eo_storage_get(
    plugin_id: String,
    key: String,
) -> Result<serde_json::Value, String> {
    let path = storage_path(&plugin_id)?;
    if !path.exists() {
        return Ok(serde_json::Value::Null);
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取存储失败: {e}"))?;
    let map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&raw).unwrap_or_default();
    Ok(map.get(&key).cloned().unwrap_or(serde_json::Value::Null))
}

/// 写插件存储（原子写）。
#[tauri::command]
pub async fn eo_storage_set(
    plugin_id: String,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    let path = storage_path(&plugin_id)?;

    // 确保目录存在
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建存储目录失败: {e}"))?;
    }

    let mut map: serde_json::Map<String, serde_json::Value> = if path.exists() {
        let raw = std::fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&raw).unwrap_or_default()
    } else {
        serde_json::Map::new()
    };
    map.insert(key, value);

    // 原子写：tmp → rename
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(&map)
        .map_err(|e| format!("序列化存储失败: {e}"))?;
    std::fs::write(&tmp, body).map_err(|e| format!("写入存储失败: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("重命名存储失败: {e}"))?;
    Ok(())
}

/// 日志：info。
#[tauri::command]
pub fn eo_log_info(msg: String) -> Result<(), String> {
    log_buffer::info(format!("[plugin] {msg}"));
    Ok(())
}

/// 日志：warn。
#[tauri::command]
pub fn eo_log_warn(msg: String) -> Result<(), String> {
    log_buffer::warn(format!("[plugin] {msg}"));
    Ok(())
}

/// 日志：error。
#[tauri::command]
pub fn eo_log_error(msg: String) -> Result<(), String> {
    log_buffer::err(format!("[plugin] {msg}"));
    Ok(())
}

/// LLM 补全（非流式）。
#[tauri::command]
pub async fn eo_chat(
    state: tauri::State<'_, voice_edit::VoiceEditManager>,
    messages: Vec<llm_client::ChatMessage>,
    opts: llm_client::ChatOptions,
) -> Result<String, String> {
    state.llm_client().chat(messages, opts).await
}

/// `eo.run` 的参数（从 JS 传入，转成 `RunnerOptions`）。
///
/// 不直接用 `RunnerOptions` 因为 `Duration` / `PathBuf` 不实现 `Deserialize`。
#[derive(Deserialize)]
pub struct EoRunOptions {
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub cwd: Option<String>,
}

/// 子进程执行。
#[tauri::command]
pub async fn eo_run(
    program: String,
    args: Vec<String>,
    opts: Option<EoRunOptions>,
) -> Result<runner::RunnerOutput, String> {
    let mut ro = runner::RunnerOptions::new();
    if let Some(opts) = opts {
        if let Some(t) = opts.timeout_secs {
            ro = ro.timeout(Duration::from_secs(t));
        }
        if let Some(cwd) = opts.cwd {
            ro = ro.cwd(cwd);
        }
    }
    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    runner::run(&program, &args_ref, &ro).await
}

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

/// 插件存储文件路径：`plugins/<id>/storage.json`。
fn storage_path(plugin_id: &str) -> Result<PathBuf, String> {
    let root = plugin_registry::plugins_root().ok_or("插件目录未初始化")?;
    Ok(root.join(plugin_id).join("storage.json"))
}
