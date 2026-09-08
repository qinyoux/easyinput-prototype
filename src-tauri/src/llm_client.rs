//! LLM 客户端：流式文本编辑请求。
//!
//! 支持多种后端（火山引擎豆包 / OpenAI / Anthropic / Ollama / 自定义），
//! 统一使用 OpenAI 兼容的 chat/completions + SSE 流式协议。
//!
//! 延迟优化：
//! - 连接池复用（reqwest 内置）
//! - SSE 流式输出（首字延迟 ~200-800ms）
//! - Prompt 极致精简（system prompt 固定 48 字）

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::log_buffer;

/// LLM 后端类型。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LlmBackend {
    Doubao,
    Openai,
    Anthropic,
    Ollama,
    Custom,
}

impl Default for LlmBackend {
    fn default() -> Self {
        LlmBackend::Doubao
    }
}

/// LLM 配置（持久化到 config.json）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LlmConfig {
    pub backend: LlmBackend,
    pub endpoint: String,
    pub model: String,
    pub api_key: String,
    pub timeout_secs: u32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            backend: LlmBackend::Doubao,
            endpoint: "https://ark.cn-beijing.volces.com/api/v3/chat/completions".to_string(),
            model: "ep-20240903141530-8x6s4".to_string(),
            api_key: String::new(),
            timeout_secs: 30,
        }
    }
}

/// LLM 编辑请求。
pub struct LlmEditRequest {
    pub selected_text: String,
    pub voice_text: String,
}

/// LLM 编辑结果。
#[derive(Clone, Debug, Serialize)]
pub struct LlmEditResult {
    pub original: String,
    pub modified: String,
    pub success: bool,
    pub error: Option<String>,
    pub duration_ms: u64,
    /// 降级方式（如 "llm_provider_error"、"empty_result"），成功时为 None。
    pub fallback: Option<String>,
}

const SYSTEM_PROMPT: &str = "你是文本编辑助手。根据修改要求对原文进行编辑，直接返回编辑后的完整文本，不要包含任何解释、标注、引号或前后缀。";

/// LLM 客户端（线程安全，可克隆）。
#[derive(Clone)]
pub struct LlmClient {
    inner: Arc<LlmInner>,
}

/// 根据超时秒数构建 reqwest 客户端。
fn build_http_client(timeout_secs: u32) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs as u64))
        .pool_idle_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(4)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .build()
        .expect("reqwest client build")
}

struct LlmInner {
    client: Mutex<reqwest::Client>,
    config: Mutex<LlmConfig>,
}

impl LlmClient {
    /// 创建客户端，使用给定配置。
    pub fn new(config: LlmConfig) -> Self {
        let client = build_http_client(config.timeout_secs);

        Self {
            inner: Arc::new(LlmInner {
                client: Mutex::new(client),
                config: Mutex::new(config),
            }),
        }
    }

    /// 更新配置，并重建 HTTP 客户端以应用新的 timeout。
    pub fn set_config(&self, config: LlmConfig) {
        log_buffer::info(format!(
            "LLM 配置更新: backend={:?} model={} endpoint={} timeout={}s key_len={}",
            config.backend, config.model, config.endpoint, config.timeout_secs, config.api_key.len()
        ));
        let client = build_http_client(config.timeout_secs);
        *self.inner.client.lock().unwrap() = client;
        *self.inner.config.lock().unwrap() = config;
    }

    /// 获取当前配置。
    pub fn config(&self) -> LlmConfig {
        self.inner.config.lock().unwrap().clone()
    }

    /// 流式编辑请求：将选中文本 + 修改要求发送给 LLM，流式返回编辑结果。
    ///
    /// 通过 Tauri Event `edit-llm-chunk` 实时推送增量文本到前端。
    pub async fn stream_edit(
        &self,
        req: LlmEditRequest,
        app: tauri::AppHandle,
    ) -> Result<LlmEditResult, String> {
        let config = self.config();
        if config.api_key.is_empty() {
            return Err("LLM API Key 未配置，请在设置中填写".to_string());
        }

        let start = std::time::Instant::now();
        let user_content = format!(
            "原文：\n{}\n\n修改要求：\n{}",
            req.selected_text, req.voice_text
        );

        let body = serde_json::json!({
            "model": config.model,
            "stream": true,
            "messages": [
                { "role": "system", "content": SYSTEM_PROMPT },
                { "role": "user", "content": user_content },
            ],
        });

        log_buffer::info(format!(
            "LLM 请求: backend={:?} model={} endpoint={} selected_len={} voice_len={} key_len={}",
            config.backend,
            config.model,
            config.endpoint,
            req.selected_text.len(),
            req.voice_text.len(),
            config.api_key.len()
        ));

        // clone Client（内部 Arc，开销极低）后立即释放 MutexGuard，避免跨 await 持有非 Send 锁。
        let http_client = self.inner.client.lock().unwrap().clone();
        let body_str = serde_json::to_string(&body).unwrap_or_default();
        let http_req = http_client
            .post(&config.endpoint)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .body(body_str);

        let resp = http_req
            .send()
            .await
            .map_err(|e| format!("LLM 请求失败: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let bytes = resp.bytes().await.unwrap_or_default();
            let text = String::from_utf8_lossy(&bytes);
            let err = format!("LLM 返回错误 {status}: {text}");
            log_buffer::err(format!("{err}"));
            return Err(err);
        }

        log_buffer::info(format!(
            "LLM 响应: status={} content-type={:?} content-length={:?}",
            resp.status(),
            resp.headers().get("content-type").map(|v| v.to_str().unwrap_or("?")),
            resp.headers().get("content-length").map(|v| v.to_str().unwrap_or("?")),
        ));

        let mut full_text = String::new();

        // 用 bytes() + from_utf8_lossy 代替 text()，绕过 reqwest 在 rustls-tls
        // (default-features=false) 下 text() 的 "error decoding response body" 问题。
        let body_bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("SSE 读取失败: {e}"))?;
        log_buffer::info(format!("LLM 响应体: {} 字节", body_bytes.len()));
        let body_text = String::from_utf8_lossy(&body_bytes);

        // 解析 SSE 事件并逐个推送前端。
        for line in body_text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    continue;
                }
                if let Some(delta) = parse_sse_delta(data) {
                    full_text.push_str(&delta);
                    let _ = app.emit("edit-llm-chunk", &delta);
                }
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;
        let modified = clean_llm_output(&full_text);

        if modified.is_empty() {
            return Ok(LlmEditResult {
                original: req.selected_text.clone(),
                modified: req.selected_text,
                success: false,
                error: Some("empty_result".to_string()),
                duration_ms,
                fallback: Some("empty_result".to_string()),
            });
        }

        log_buffer::info(format!(
            "LLM 完成: duration={}ms output_len={}",
            duration_ms,
            modified.len()
        ));

        Ok(LlmEditResult {
            original: req.selected_text,
            modified,
            success: true,
            error: None,
            duration_ms,
            fallback: None,
        })
    }

    /// 测试连接（非流式，发送简短请求验证 API Key）。
    pub async fn test_connection(&self) -> Result<String, String> {
        let config = self.config();
        if config.api_key.is_empty() {
            return Err("API Key 未配置".to_string());
        }

        let body = serde_json::json!({
            "model": config.model,
            "stream": false,
            "messages": [
                { "role": "user", "content": "ping" },
            ],
            "max_tokens": 5,
        });

        let http_client = self.inner.client.lock().unwrap().clone();
        let resp = http_client
            .post(&config.endpoint)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("连接失败: {e}"))?;

        if resp.status().is_success() {
            Ok(format!("连接成功 (model={})", config.model))
        } else {
            Err(format!("连接失败 (status={})", resp.status()))
        }
    }
}

/// 解析 SSE data 行中的 delta 文本。
fn parse_sse_delta(data: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(data).ok()?;
    let delta = v.get("choices")?.get(0)?.get("delta")?.get("content")?;
    delta.as_str().map(|s| s.to_string())
}

/// 清理 LLM 输出：去除首尾空白、引号包裹、markdown 代码块包裹。
fn clean_llm_output(raw: &str) -> String {
    let mut s = raw.trim().to_string();

    // 去除 markdown 代码块包裹（```...``` 或 ```text\n...\n```）
    if s.starts_with("```") {
        // 去掉开头的 ``` 或 ```lang
        if let Some(nl) = s.find('\n') {
            let first_line = &s[..nl].trim();
            if first_line.starts_with("```") {
                s = s[nl + 1..].to_string();
            }
        }
        // 去掉结尾的 ```
        if s.ends_with("```") {
            s = s[..s.len() - 3].trim_end().to_string();
        }
    }

    // 去除首尾配对引号（"..."、'...'、"..."、"..."）
    let pairs: &[(char, char)] = &[
        ('"', '"'),
        ('\'', '\''),
        ('\u{201C}', '\u{201D}'), // 中文左右双引号
        ('\u{2018}', '\u{2019}'), // 中文左右单引号
    ];
    for &(open, close) in pairs {
        if s.len() >= 2 && s.starts_with(open) && s.ends_with(close) {
            let inner = &s[open.len_utf8()..s.len() - close.len_utf8()];
            let trimmed = inner.trim();
            if !trimmed.is_empty() {
                s = trimmed.to_string();
            }
        }
    }

    s.trim().to_string()
}
