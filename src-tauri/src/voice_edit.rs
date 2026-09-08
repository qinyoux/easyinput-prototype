//! 语音编辑编排器：选中文本提取 → LLM 流式编辑 → 结果回写。
//!
//! 编排流程：
//! 1. 提取用户选中文本（clipboard_util::get_selected_text）
//! 2. 将选中文本 + 语音识别的修改要求发送给 LLM（llm_client::stream_edit）
//! 3. LLM 流式返回编辑结果，实时推送到前端
//! 4. 编辑完成后替换选中文本（clipboard_util::replace_selected_text）
//!
//! 降级策略：
//! - LLM 失败 → 不修改目标应用文本，选中文本保持原状，浮层显示错误
//! - 选中文本为空 → 提示用户先选中目标文本

use tauri::{Emitter, Manager};

use crate::clipboard_util;
use crate::llm_client::{LlmClient, LlmEditRequest, LlmEditResult};
use crate::log_buffer;

/// 异步等待目标应用获得前台焦点（非自身窗口）。
/// 轮询前台窗口，条件满足即提前返回，最多等待 max_ms。
async fn wait_for_target_focus(app: &tauri::AppHandle, max_ms: u64) {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let step = 50;
    let mut elapsed = 0u64;

    // 获取自身窗口句柄集合，用于判断前台窗口是否是本应用。
    let our_hwnds: Vec<usize> = ["overlay", "edit_overlay", "main"]
        .iter()
        .filter_map(|label| app.get_webview_window(label))
        .filter_map(|w| w.hwnd().ok())
        .map(|h| h.0 as usize)
        .collect();

    while elapsed < max_ms {
        tokio::time::sleep(std::time::Duration::from_millis(step)).await;
        elapsed += step;

        let fg = unsafe { GetForegroundWindow() };
        if fg.0.is_null() {
            continue;
        }

        let fg_addr = fg.0 as usize;
        if !our_hwnds.contains(&fg_addr) {
            log_buffer::info(format!("目标应用已获焦点（等待 {elapsed}ms）"));
            return;
        }
    }
    log_buffer::info(format!("等待目标焦点超时（{max_ms}ms），继续执行"));
}

/// 语音编辑编排器。
#[derive(Clone)]
pub struct VoiceEditManager {
    llm: LlmClient,
}

impl VoiceEditManager {
    pub fn new(llm: LlmClient) -> Self {
        Self { llm }
    }

    /// 获取 LLM 客户端引用（供配置命令使用）。
    pub fn llm_client(&self) -> &LlmClient {
        &self.llm
    }

    /// 执行语音编辑：选中文本 + 修改要求 → LLM → 替换。
    ///
    /// 这是编辑流程的核心入口，由 session.rs 在编辑 PTT toggle 确认后调用。
    /// `voice_text` 是 ASR 识别出的修改要求文本。
    pub async fn execute_edit(
        &self,
        voice_text: String,
        app: tauri::AppHandle,
    ) -> Result<LlmEditResult, String> {
        // 确保普通 overlay 被隐藏，编辑流程只显示编辑 overlay。
        if let Some(win) = app.get_webview_window("overlay") {
            let _ = win.hide();
        }

        let _ = app.emit("edit-state", "正在提取选中文本…");

        // 临时隐藏编辑浮层，避免干扰目标应用焦点（Ctrl+C 需要发送到目标应用）。
        if let Some(win) = app.get_webview_window("edit_overlay") {
            let _ = win.hide();
        }
        wait_for_target_focus(&app, 500).await;

        let selected_text = match clipboard_util::get_selected_text() {
            Ok(text) => {
                log_buffer::info(format!("选中文本已提取 ({} 字)", text.len()));
                text
            }
            Err(e) => {
                // 重新显示浮层以展示错误信息，2 秒后自动关闭。
                if let Some(win) = app.get_webview_window("edit_overlay") {
                    let _ = win.show();
                }
                let _ = app.emit("edit-state", &e);
                let app_err = app.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    if let Some(win) = app_err.get_webview_window("edit_overlay") {
                        let _ = win.hide();
                    }
                });
                return Err(e);
            }
        };

        // 重新显示浮层以展示编辑进度。
        if let Some(win) = app.get_webview_window("edit_overlay") {
            let _ = win.show();
        }

        let _ = app.emit("edit-state", "正在编辑…");
        let _ = app.emit("edit-selected", &selected_text);

        let req = LlmEditRequest {
            selected_text: selected_text.clone(),
            voice_text: voice_text.clone(),
        };

        let result = match self.llm.stream_edit(req, app.clone()).await {
            Ok(r) => r,
            Err(e) => {
                log_buffer::warn(format!("LLM 编辑失败: {e}"));
                let _ = app.emit("edit-state", format!("LLM 不可用: {e}"));

                // 编辑失败：不修改目标应用文本，选中文本保持原状。
                // 重新显示浮层展示错误，3 秒后自动关闭。
                if let Some(win) = app.get_webview_window("edit_overlay") {
                    let _ = win.show();
                }
                let app_err = app.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    if let Some(win) = app_err.get_webview_window("edit_overlay") {
                        let _ = win.hide();
                    }
                });

                let result = LlmEditResult {
                    original: selected_text.clone(),
                    modified: selected_text,
                    success: false,
                    error: Some(e.clone()),
                    duration_ms: 0,
                    fallback: Some("llm_provider_error".to_string()),
                };
                return Ok(result);
            }
        };

        if !result.success {
            let err_msg = result.error.clone().unwrap_or_else(|| "empty_result".to_string());
            log_buffer::warn(format!("LLM 编辑未成功: {err_msg}"));
            let _ = app.emit("edit-state", &err_msg);

            // LLM 返回空结果：不修改目标应用文本，选中文本保持原状。
            // 重新显示浮层展示错误，3 秒后自动关闭。
            if let Some(win) = app.get_webview_window("edit_overlay") {
                let _ = win.show();
            }
            let app_err = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                if let Some(win) = app_err.get_webview_window("edit_overlay") {
                    let _ = win.hide();
                }
            });
            return Ok(result);
        }

        let _ = app.emit("edit-state", "正在替换选中文本…");

        // 临时隐藏浮层，避免干扰目标应用焦点（Ctrl+V 需要发送到目标应用）。
        if let Some(win) = app.get_webview_window("edit_overlay") {
            let _ = win.hide();
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        match clipboard_util::replace_selected_text(&result.modified) {
            Ok(()) => {
                log_buffer::info("选中文本已替换");
            }
            Err(e) => {
                log_buffer::warn(format!("替换选中文本失败，降级直接上屏: {e}"));
                let _ = app.emit("edit-state", format!("替换失败: {e}，直接上屏"));
                crate::text_delivery::deliver(&result.modified).map_err(|e| e.to_string())?;
            }
        }

        let _ = app.emit("edit-done", &result);
        let _ = app.emit("edit-state", &format!(
            "编辑完成 ({}ms)",
            result.duration_ms
        ));

        // 成功完成后延迟 1 秒关闭浮层，让用户看到结果。
        let app_done = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if let Some(win) = app_done.get_webview_window("edit_overlay") {
                let _ = win.hide();
            }
        });

        Ok(result)
    }
}
