//! 能力内核（Capability Core）—— easyop 所有扩展面（进程内 eo.* / HTTP 桥 / CLI）的
//! 统一能力定义 + 统一错误码表 + 统一返回信封。
//!
//! # 为什么需要能力内核
//!
//! 同一批能力曾被定义两次（桥 `/llm` 与插件 `eo.llm`），两套名字、两套错误语义、两套权限
//! 模型，必然分叉。本模块是**唯一实现**的锚点：三个适配面只做传输，不做逻辑。
//!
//! # 能力清单（落点已核实）
//!
//! | 能力              | Core 签名                              | 真实落点                          |
//! |-------------------|----------------------------------------|-----------------------------------|
//! | 注入文本          | `inject(text)`                         | `text_delivery::deliver` (:40)    |
//! | LLM 补全          | `chat(messages, opts)`                 | `llm_client::LlmClient::chat`     |
//! | LLM 流式          | `chat_stream(messages, opts, on_delta)`| `llm_client::LlmClient::chat_stream` |
//! | 读选中文本        | `get_selection()`                      | `clipboard_util::get_selected_text` (:28) |
//! | 替换选中文本      | `replace_selection(text)`              | `clipboard_util::replace_selected_text` (:65) |
//! | 剪贴板读          | `clipboard_read()`                     | **需新增**（现 `clipboard_util` 只做模拟 Ctrl+C 取选中） |
//! | 剪贴板写          | `clipboard_write(text)`                | **需新增**                        |
//! | 打开产物目录      | `reveal(path)`                         | `ss_open_containing_folder`（lib.rs:466） |
//! | 提示词列表        | `prompts_list()`                       | `commands::workbench_prompt_*`    |
//! | 派发具名动作      | `dispatch_action(name)`                | **需新增白名单入口**（`host_action::handle_host_action` 是私有 fn） |
//! | 日志              | `log(level, msg)`                      | `log_buffer`                      |
//! | 子进程执行        | `run(program, args, opts)`             | `runner::run` / `runner::run_stream` |
//!
//! # 三个适配面
//!
//! | 面   | 传输            | 消费者                 |
//! |------|-----------------|------------------------|
//! | 面1  | 进程内 `eo.*`   | 组件 / 插件 webview    |
//! | 面2  | HTTP bridge     | 外部 exe / 脚本        |
//! | 面3  | CLI 单次调用    | cron / 批处理          |
//!
//! 三条硬约定（见《easyop 扩展机制规范》§二）：
//! 1. 三个面的能力集完全一致——只是传输方式不同。
//! 2. 三面共享同一份错误码表（本模块）。
//! 3. 桥只暴露"宿主能力"，绝不暴露"业务动作"。

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// 统一错误码表（12 档 + SELECTOR_DRIFT）
// ---------------------------------------------------------------------------

/// 统一错误码。
///
/// 三个适配面（进程内 / HTTP / CLI）共用这一份码表。
/// `SELECTOR_DRIFT` 单列一码：平台改版导致 CSS 选择器失效，与普通网络失败
/// 需要区分——前者要提示重跑 `probe`，后者只需重试。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// 成功。
    Ok,
    /// 配置未就绪（API Key 未填、模型未选等）。
    NotConfigured,
    /// 资源不存在（选题 / 插件 / 文件）。
    NotFound,
    /// 能力未授权（capability 声明缺失或被拒）。
    PermissionDenied,
    /// 网络层失败（连接超时、DNS 失败等）。
    NetworkError,
    /// 上游服务返回错误（LLM 5xx、ASR 拒绝等）。
    ProviderError,
    /// 操作超时。
    Timeout,
    /// 被调用方取消。
    Cancelled,
    /// 参数校验失败。
    InvalidInput,
    /// 并发冲突（台账版本不匹配、单实例已开等）。
    Conflict,
    /// easyop 内部错误（不该发生，需排查）。
    Internal,
    /// 子进程非零退出（runner 执行失败）。
    RunnerFailed,
    /// 平台改版导致选择器失效——提示重跑 probe，不自动重试。
    SelectorDrift,
}

impl ErrorCode {
    /// 是否成功。
    pub fn is_ok(&self) -> bool {
        matches!(self, ErrorCode::Ok)
    }

    /// 是否可重试（网络/超时可重试；配置/权限/选择器漂移不可）。
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ErrorCode::NetworkError | ErrorCode::Timeout | ErrorCode::ProviderError
        )
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorCode::Ok => write!(f, "ok"),
            ErrorCode::NotConfigured => write!(f, "not_configured"),
            ErrorCode::NotFound => write!(f, "not_found"),
            ErrorCode::PermissionDenied => write!(f, "permission_denied"),
            ErrorCode::NetworkError => write!(f, "network_error"),
            ErrorCode::ProviderError => write!(f, "provider_error"),
            ErrorCode::Timeout => write!(f, "timeout"),
            ErrorCode::Cancelled => write!(f, "cancelled"),
            ErrorCode::InvalidInput => write!(f, "invalid_input"),
            ErrorCode::Conflict => write!(f, "conflict"),
            ErrorCode::Internal => write!(f, "internal"),
            ErrorCode::RunnerFailed => write!(f, "runner_failed"),
            ErrorCode::SelectorDrift => write!(f, "selector_drift"),
        }
    }
}

impl Default for ErrorCode {
    fn default() -> Self {
        ErrorCode::Ok
    }
}

// ---------------------------------------------------------------------------
// 统一返回信封
// ---------------------------------------------------------------------------

/// 统一返回信封——三个适配面对外返回的统一外壳。
///
/// 前端 / 桥 / CLI 都能靠 `ok` 一字段判断成败，靠 `code` 精确分类错误，
/// 靠 `message` 给用户看人话，靠 `data` 拿数据。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiEnvelope<T> {
    pub ok: bool,
    pub code: ErrorCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl<T> ApiEnvelope<T> {
    /// 成功信封。
    pub fn ok(data: T) -> Self {
        Self {
            ok: true,
            code: ErrorCode::Ok,
            data: Some(data),
            message: None,
        }
    }

    /// 成功但无数据（如纯写入操作）。
    pub fn ok_empty() -> Self {
        Self {
            ok: true,
            code: ErrorCode::Ok,
            data: None,
            message: None,
        }
    }

    /// 错误信封。
    pub fn err(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            code,
            data: None,
            message: Some(message.into()),
        }
    }

    /// 从 `Result` 构造：`Ok` → 成功信封，`Err` → `Internal` 错误信封。
    pub fn from_result(result: Result<T, String>) -> Self {
        match result {
            Ok(data) => Self::ok(data),
            Err(msg) => Self::err(ErrorCode::Internal, msg),
        }
    }
}

// ---------------------------------------------------------------------------
// 能力名（capability 声明用）
// ---------------------------------------------------------------------------

/// 能力名常量——`plugin.json` 的 `capabilities` 字段与代码层过滤共用同一份字符串。
///
/// 插件清单里写 `"capabilities": ["inject", "llm"]`，代码层用 `CAP_INJECT` 比对，
/// 避免字符串拼写漂移。
pub const CAP_INJECT: &str = "inject";
pub const CAP_LLM: &str = "llm";
pub const CAP_SELECTED_TEXT: &str = "selected-text";
pub const CAP_CLIPBOARD: &str = "clipboard";
pub const CAP_STORAGE: &str = "storage";
pub const CAP_RUNNER: &str = "runner";
pub const CAP_REVEAL: &str = "reveal";
pub const CAP_PROMPTS: &str = "prompts";
pub const CAP_DISPATCH_ACTION: &str = "dispatch-action";
pub const CAP_LOG: &str = "log";
/// 文本转语音（`tts::TtsClient`）—— 内容台配音 / 语音编辑播报 / 任务台总结共用。
pub const CAP_TTS: &str = "tts";
/// Agent 任务台编排（`agent_desk`）—— 槽位 / 队列 / 未读 / 播报。
pub const CAP_AGENT_DESK: &str = "agent-desk";

/// 全部能力名（供"列出所有可声明能力"用）。
pub const ALL_CAPABILITIES: &[&str] = &[
    CAP_INJECT,
    CAP_LLM,
    CAP_SELECTED_TEXT,
    CAP_CLIPBOARD,
    CAP_STORAGE,
    CAP_RUNNER,
    CAP_REVEAL,
    CAP_PROMPTS,
    CAP_DISPATCH_ACTION,
    CAP_LOG,
    CAP_TTS,
    CAP_AGENT_DESK,
];

/// 校验插件声明的能力是否都在已知列表里。返回未知的 capability 名。
pub fn unknown_capabilities(declared: &[String]) -> Vec<String> {
    declared
        .iter()
        .filter(|c| !ALL_CAPABILITIES.contains(&c.as_str()))
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_ok() {
        let e = ApiEnvelope::ok(42u32);
        assert!(e.ok);
        assert_eq!(e.code, ErrorCode::Ok);
        assert_eq!(e.data, Some(42));
    }

    #[test]
    fn envelope_err() {
        let e: ApiEnvelope<()> = ApiEnvelope::err(ErrorCode::NotConfigured, "Key 未填");
        assert!(!e.ok);
        assert_eq!(e.code, ErrorCode::NotConfigured);
        assert_eq!(e.message.as_deref(), Some("Key 未填"));
    }

    #[test]
    fn envelope_from_result() {
        let ok: ApiEnvelope<u32> = ApiEnvelope::from_result(Ok(7));
        assert!(ok.ok && ok.data == Some(7));

        let err: ApiEnvelope<u32> = ApiEnvelope::from_result(Err("炸了".to_string()));
        assert!(!err.ok);
        assert_eq!(err.code, ErrorCode::Internal);
    }

    #[test]
    fn error_code_retryable() {
        assert!(ErrorCode::NetworkError.is_retryable());
        assert!(ErrorCode::Timeout.is_retryable());
        assert!(!ErrorCode::NotConfigured.is_retryable());
        assert!(!ErrorCode::SelectorDrift.is_retryable());
    }

    #[test]
    fn unknown_caps_detected() {
        let declared = vec![
            "inject".to_string(),
            "llm".to_string(),
            "teleport".to_string(),
        ];
        let unknown = unknown_capabilities(&declared);
        assert_eq!(unknown, vec!["teleport".to_string()]);
    }

    #[test]
    fn serde_roundtrip() {
        let e = ApiEnvelope::ok("hello".to_string());
        let json = serde_json::to_string(&e).unwrap();
        let back: ApiEnvelope<String> = serde_json::from_str(&json).unwrap();
        assert!(back.ok);
        assert_eq!(back.data.as_deref(), Some("hello"));
    }
}