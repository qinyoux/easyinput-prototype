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

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::log_buffer;

/// 请求体预览（截断后进日志）。
///
/// 日志里只写"prompt 长度 173"是没法排查的 —— 出的错往往是**内容**引起的
/// （模板少了占位符、上下文里混进了别的语言、模型名写错）。截断到 600 字符
/// 足以看清结构，又不至于让一条日志刷满整屏。
fn body_preview(body: &serde_json::Value) -> String {
    const MAX: usize = 600;
    let s = serde_json::to_string(body).unwrap_or_default();
    if s.chars().count() <= MAX {
        return s;
    }
    let head: String = s.chars().take(MAX).collect();
    format!("{head}…（共 {} 字符）", s.chars().count())
}

/// 响应头里可用于向服务端追查的 ID。
///
/// 提工单时对方第一句话就是"给我 request id"。没有它，服务端侧的日志翻不出来，
/// 而这类查不动的故障最容易被归为"偶发"然后搁置。
fn trace_id(resp: &reqwest::Response) -> String {
    for k in ["x-request-id", "x-tt-logid", "x-tt-log-id", "request-id", "x-trace-id"] {
        if let Some(v) = resp.headers().get(k).and_then(|v| v.to_str().ok()) {
            if !v.is_empty() {
                return format!("{k}={v}");
            }
        }
    }
    "-".to_string()
}

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

// ---------------------------------------------------------------------------
// 通用对话接口（Capability Core 的 `chat` / `chat_stream`）
//
// `stream_edit` 是特化的单轮编辑（system prompt 写死为"文本编辑助手"），只服务
// 语音编辑那一条链路。插件体系的 `eo.chat`、内容台的生成/润色都需要**通用**补全，
// 因此这里补一对与后端解耦的通用接口。两者共用同一份 `LlmConfig` 与连接池。
// ---------------------------------------------------------------------------

/// 通用对话消息（OpenAI 兼容的 role/content 二元组）。
///
/// 需要 `Deserialize`：前端经 `invoke` 传入时直接反序列化。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".to_string(), content: content.into() }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".to_string(), content: content.into() }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: "assistant".to_string(), content: content.into() }
    }
}

/// 通用对话选项。全部字段可选，缺省即走后端默认。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChatOptions {
    /// 前置 system 提示词。与 `messages` 里已有的 system 并存（它排在最前）。
    #[serde(default)]
    pub system: Option<String>,
    /// 采样温度。仅当显式给出时才写入请求体——避免给不支持该参数的模型添乱。
    #[serde(default)]
    pub temperature: Option<f32>,
    /// 输出上限。同上，仅显式给出时才写。
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// 是否对输出做清理（去 markdown 围栏、去首尾配对引号）。缺省 true。
    /// 生成正文时保持 true；生成 JSON / 代码时应显式传 false。
    #[serde(default)]
    pub clean: Option<bool>,
}

/// 构建 chat/completions 的 messages 数组（含可选的前置 system）。
fn build_chat_messages(messages: &[ChatMessage], opts: &ChatOptions) -> Vec<serde_json::Value> {
    let mut out: Vec<serde_json::Value> = Vec::with_capacity(messages.len() + 1);
    if let Some(sys) = opts.system.as_ref().filter(|s| !s.trim().is_empty()) {
        out.push(serde_json::json!({ "role": "system", "content": sys }));
    }
    for m in messages {
        out.push(serde_json::json!({ "role": m.role, "content": m.content }));
    }
    out
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
        log_buffer::info(format!("LLM 请求体: {}", body_preview(&body)));

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
            let tid = trace_id(&resp);
            let bytes = resp.bytes().await.unwrap_or_default();
            let text = String::from_utf8_lossy(&bytes);
            let err = format!("LLM 返回错误 {status}: {text}");
            log_buffer::err(format!(
                "{err}｜endpoint={}｜耗时={}ms｜{tid}",
                config.endpoint,
                start.elapsed().as_millis()
            ));
            return Err(err);
        }

        log_buffer::info(format!(
            "LLM 响应: status={} content-type={:?} content-length={:?} 耗时={}ms {tid}",
            resp.status(),
            resp.headers().get("content-type").map(|v| v.to_str().unwrap_or("?")),
            resp.headers().get("content-length").map(|v| v.to_str().unwrap_or("?")),
            start.elapsed().as_millis(),
            tid = trace_id(&resp),
        ));

        let mut full_text = String::new();

        // ⛔ 明确告诉前端"改写流开始"：前端的字幕区在识别阶段显示的是 ASR 原话，
        // 从这一刻起要换成 LLM 的流式输出。少了这个信号，前端把 LLM 增量**追加**
        // 到还没清掉的原话后面，用户看到的就是"字幕里混进了别的字"——
        // 两段语义完全不同、来源也不同的文本被拼成了一段。
        let _ = app.emit("edit-llm-start", ());

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

    /// 通用补全（非流式）。返回清理后的纯文本。
    ///
    /// 这是插件 `eo.chat` 与内容台生成链路的统一落点，与 `stream_edit` 共用
    /// 同一份 `LlmConfig` 与 reqwest 连接池。
    pub async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        opts: ChatOptions,
    ) -> Result<String, String> {
        let config = self.config();
        if config.api_key.is_empty() {
            return Err("LLM API Key 未配置，请在设置中填写".to_string());
        }

        let mut body = serde_json::json!({
            "model": config.model,
            "stream": false,
            "messages": build_chat_messages(&messages, &opts),
        });
        if let Some(t) = opts.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(mt) = opts.max_tokens {
            body["max_tokens"] = serde_json::json!(mt);
        }

        let start = std::time::Instant::now();
        log_buffer::info(format!(
            "LLM chat 请求: model={} endpoint={} msgs={} temperature={:?} max_tokens={:?}",
            config.model, config.endpoint, messages.len(), opts.temperature, opts.max_tokens
        ));
        log_buffer::info(format!("LLM chat 请求体: {}", body_preview(&body)));
        let http_client = self.inner.client.lock().unwrap().clone();
        let resp = http_client
            .post(&config.endpoint)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("LLM 请求失败: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let tid = trace_id(&resp); // 必须在 bytes() 消费 resp 之前取
            let bytes = resp.bytes().await.unwrap_or_default();
            let err = format!("LLM 返回错误 {status}: {}", String::from_utf8_lossy(&bytes));
            log_buffer::err(format!(
                "{err}｜endpoint={}｜耗时={}ms｜{tid}",
                config.endpoint,
                start.elapsed().as_millis()
            ));
            return Err(err);
        }

        let bytes = resp.bytes().await.map_err(|e| format!("响应读取失败: {e}"))?;
        let v: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| format!("响应解析失败: {e}"))?;
        let content = v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();

        if content.trim().is_empty() {
            return Err("LLM 返回空内容".to_string());
        }

        let out = if opts.clean.unwrap_or(true) {
            clean_llm_output(&content)
        } else {
            content.trim().to_string()
        };
        log_buffer::info(format!(
            "LLM chat 完成: duration={}ms out_len={} msgs={}",
            start.elapsed().as_millis(),
            out.len(),
            messages.len()
        ));
        Ok(out)
    }

    /// 通用补全（真流式）。每个增量文本块通过 `on_delta` 回调抛出，返回拼接后的完整文本。
    ///
    /// 与 `stream_edit` 的关键区别：`stream_edit` 用 `bytes()` 整读响应体后再按行切 SSE，
    /// 首字延迟等于整段生成时间；这里走 `bytes_stream()` 逐块解析，是真流式。
    /// 生成 950 字正文这类长输出时必须用这个，否则用户要干等十几秒才见到第一个字。
    pub async fn chat_stream<F>(
        &self,
        messages: Vec<ChatMessage>,
        opts: ChatOptions,
        mut on_delta: F,
    ) -> Result<String, String>
    where
        F: FnMut(&str),
    {
        let config = self.config();
        if config.api_key.is_empty() {
            return Err("LLM API Key 未配置，请在设置中填写".to_string());
        }

        let mut body = serde_json::json!({
            "model": config.model,
            "stream": true,
            "messages": build_chat_messages(&messages, &opts),
        });
        if let Some(t) = opts.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(mt) = opts.max_tokens {
            body["max_tokens"] = serde_json::json!(mt);
        }

        let start = std::time::Instant::now();
        log_buffer::info(format!(
            "LLM stream 请求: model={} endpoint={} msgs={} temperature={:?} max_tokens={:?}",
            config.model, config.endpoint, messages.len(), opts.temperature, opts.max_tokens
        ));
        log_buffer::info(format!("LLM stream 请求体: {}", body_preview(&body)));
        let http_client = self.inner.client.lock().unwrap().clone();
        let resp = http_client
            .post(&config.endpoint)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("LLM 请求失败: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let tid = trace_id(&resp); // 必须在 bytes() 消费 resp 之前取
            let bytes = resp.bytes().await.unwrap_or_default();
            let err = format!("LLM 返回错误 {status}: {}", String::from_utf8_lossy(&bytes));
            log_buffer::err(format!(
                "{err}｜endpoint={}｜耗时={}ms｜{tid}",
                config.endpoint,
                start.elapsed().as_millis()
            ));
            return Err(err);
        }

        let mut full_text = String::new();
        // chunk 边界可能切断一行 SSE，因此用行缓冲：只有遇到换行才解析。
        let mut buf = String::new();
        let mut stream = resp.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("SSE 读取失败: {e}"))?;
            buf.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(pos) = buf.find('\n') {
                let line = buf[..pos].trim().to_string();
                buf.drain(..pos + 1);
                if line.is_empty() || line.starts_with(':') {
                    continue;
                }
                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        continue;
                    }
                    if let Some(delta) = parse_sse_delta(data) {
                        if !delta.is_empty() {
                            full_text.push_str(&delta);
                            on_delta(&delta);
                        }
                    }
                }
            }
        }

        // 收尾：处理最后一行没有换行结尾的残留。
        let tail = buf.trim();
        if let Some(data) = tail.strip_prefix("data: ") {
            if data != "[DONE]" {
                if let Some(delta) = parse_sse_delta(data) {
                    full_text.push_str(&delta);
                    on_delta(&delta);
                }
            }
        }

        if full_text.trim().is_empty() {
            return Err("LLM 返回空内容".to_string());
        }

        let out = if opts.clean.unwrap_or(true) {
            clean_llm_output(&full_text)
        } else {
            full_text.trim().to_string()
        };
        log_buffer::info(format!(
            "LLM chat_stream 完成: duration={}ms out_len={} msgs={}",
            start.elapsed().as_millis(),
            out.len(),
            messages.len()
        ));
        Ok(out)
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
