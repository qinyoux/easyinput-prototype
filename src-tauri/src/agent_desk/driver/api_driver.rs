//! OpenAI 兼容 HTTP 执行器。
//!
//! # 为什么直接复用一个独立配置的 `LlmClient`
//!
//! 第一性原理：`LlmClient` 的真实身份是「**一个持有 OpenAI 兼容配置的流式客户端**」，
//! 而不是「全局语音编辑通道」。后者只是它的一个使用场景。
//!
//! - 它读 `self.config()`，而 `LlmClient::new(config)` 接受任意配置
//!   ⇒ **可以独立建一个，不会跟「语音编辑配置」互相覆盖**。
//! - `chat_stream` 是干净的回调式流（不像 `stream_edit` 会 emit `edit-llm-chunk`
//!   事件）⇒ 拿来做 driver 不会污染前端的编辑链路。
//!
//! ⇒ 结果是**零改动、零重复**：不改 `llm_client.rs` 一行，也不重写一遍 SSE 解析。
//!
//! 反过来说，如果把 driver 接到 `DeskManager` 里那个全局 LLM 客户端上，
//! 就会出现"设置页改了语音编辑的 key，任务台的 driver 跟着变了"——
//! 这正是项目里说的**最难查的那种错**（配置串了但没有任何报错）。
//!
//! # 完成信号
//!
//! `HttpStreamEnd`：HTTP 流正常读到结束（`chat_stream` 返回 `Ok`）。
//! 这是最无歧义的信号——服务端把 `[DONE]` 发完了，就是真的发完了。
//!
//! `LogPattern` 也支持（在模型全文里找权威串）。`ProcessExitZero` 与
//! `AsyncTaskPoll` 在 HTTP 形态下没有意义，由 `driver::validate_driver` 提前拦掉。

use crate::agent_desk::config::{CompletionKind, DriverConfig};
use crate::llm_client::{ChatMessage, ChatOptions, LlmBackend, LlmClient, LlmConfig};
use crate::log_buffer;

use super::{truncate, EventSink, RunOutcome, RunRequest, RunStatus};

/// 执行一次 HTTP 调用。
pub async fn run(
    driver: &DriverConfig,
    req: &RunRequest,
    sink: &mut dyn EventSink,
    cancel: crate::runner::CancelToken,
) -> RunOutcome {
    let started = std::time::Instant::now();

    let timeout_secs = driver_timeout_secs(driver, req);
    // 每个 driver 一个独立客户端：配置来自 DriverConfig，不碰全局语音编辑配置。
    let client = LlmClient::new(LlmConfig {
        backend: LlmBackend::Custom,
        endpoint: driver.endpoint.clone(),
        model: driver.model.clone(),
        api_key: driver.api_key.clone(),
        timeout_secs,
    });

    log_buffer::info(format!(
        "任务台 Api driver 发起: id={} model={} endpoint={} slot={} task={}",
        driver.id, driver.model, driver.endpoint, req.slot, req.task_id
    ));

    let messages = vec![ChatMessage::user(req.prompt.clone())];
    let opts = ChatOptions::default();

    // ⚠️ 取消必须**真的中断请求**，不能只在跑完之后检查一下标志位。
    //
    // `chat_stream` 的 future 一被 drop，reqwest 就会关掉这条连接 ——
    // 于是 select 的取消分支天然就是"中断 HTTP"，不需要给 `llm_client` 加参数
    // （那会连带改动语音编辑那条链路，为一个只有任务台需要的功能）。
    //
    // 此前这个参数叫 `_cancel`、被完全忽略：Api 执行器的任务**根本取消不掉**，
    // 只能等模型把话说完。用户按"取消"时看到的是毫无反应。
    let result = tokio::select! {
        r = client.chat_stream(messages, opts, |delta| {
            // 增量直接推给出口——编排器据此发进度事件（前端能看到字在长）。
            sink.stdout_line(delta);
        }) => r,
        _ = cancel.cancelled() => {
            log_buffer::info(format!(
                "任务台 Api driver 已取消: id={} slot={} task={}",
                driver.id, req.slot, req.task_id
            ));
            return RunOutcome {
                status: RunStatus::Cancelled,
                reason: Some("任务被取消".into()),
                // 刻意**不**回传半截正文：一个"说到一半被打断"的回答，
                // 被当成任务结果四处留存（历史、未读、播报）只会造成误解。
                output: String::new(),
                duration_ms: started.elapsed().as_millis() as u64,
                confidence: driver.completion.confidence(),
                evidence: None,
                session_id: None,
            };
        }
    };

    let duration_ms = started.elapsed().as_millis() as u64;

    match result {
        Ok(text) => {
            // HttpStreamEnd：流读到结束即完成。
            // LogPattern：还要在全文里找到权威串才算完成（否则是"跑完了但没做成"）。
            match driver.completion.kind {
                CompletionKind::HttpStreamEnd => RunOutcome {
                    status: RunStatus::Completed,
                    reason: None,
                    output: text,
                    duration_ms,
                    confidence: driver.completion.confidence(),
                    evidence: None,
                session_id: None,
                },
                CompletionKind::LogPattern => {
                    let pat = driver.completion.pattern.trim();
                    if text.contains(pat) {
                        RunOutcome {
                            status: RunStatus::Completed,
                            reason: None,
                            output: text,
                            duration_ms,
                            confidence: driver.completion.confidence(),
                            evidence: None,
                session_id: None,
                        }
                    } else {
                        RunOutcome {
                            status: RunStatus::Failed,
                            reason: Some(format!(
                                "响应里没有出现完成标记「{pat}」——流虽然结束了，但上游没说它做完了"
                            )),
                            output: text,
                            duration_ms,
                            confidence: driver.completion.confidence(),
                            evidence: None,
                session_id: None,
                        }
                    }
                }
                // validate_driver 已拦；这里是纵深防御，绝不静默当成完成。
                other => RunOutcome::failed(format!(
                    "Api 执行器不支持完成信号「{}」——请在执行器设置里改成 HTTP 流结束",
                    other.label()
                )),
            }
        }
        Err(e) => {
            log_buffer::err(format!(
                "任务台 Api driver 失败: id={} slot={} task={} err={}",
                driver.id, req.slot, req.task_id, e
            ));
            RunOutcome {
                status: RunStatus::Failed,
                reason: Some(truncate(&e, 400)),
                output: String::new(),
                duration_ms,
                confidence: driver.completion.confidence(),
                evidence: None,
                session_id: None,
            }
        }
    }
}

/// 取 driver 的超时（秒）。三者取小：请求给定、driver 配置默认、全局默认。
///
/// 取小的理由：超时的目的是"别让槽位被永久占住"。两个上限都存在时，
/// 更严格的那个才是真正想表达的约束。
fn driver_timeout_secs(driver: &DriverConfig, req: &RunRequest) -> u32 {
    let from_req = (req.effective_timeout_ms() / 1000) as u32;
    let from_cfg = driver_default_timeout_secs(driver);
    from_req.min(from_cfg).max(1)
}

/// 从 driver 配置里取超时（秒）。**0 = 用全局默认**。
///
/// ⚠️ 这里**必须读 `driver.timeout_ms`**。`pump_slot` 派发时会把该值原样抄进
/// `RunRequest`，而 [`driver_timeout_secs`] 取的是 `min(req, cfg)` —— 如果本函数
/// 仍然返回全局默认，那个 `min` 就会把用户配置**永远压回 180 秒**：
/// 配了 15 分钟也不生效，而且完全不报错。这正是"配置看起来存在、实则无效"的形态。
fn driver_default_timeout_secs(driver: &DriverConfig) -> u32 {
    if driver.timeout_ms == 0 {
        (super::DEFAULT_TIMEOUT_MS / 1000) as u32
    } else {
        // 秒级取整**向上**：配 1500ms 不该被截成 1 秒（那会让超时比意图更早触发）。
        ((driver.timeout_ms + 999) / 1000).max(1) as u32
    }
}

/// 供测试：构造一个指向本地 mock 端点的 driver。
#[cfg(test)]
pub(crate) fn mock_driver(endpoint: &str) -> DriverConfig {
    use crate::agent_desk::config::{Capability, CompletionSignal};

    DriverConfig {
        id: "mock-api".into(),
        label: "Mock".into(),
        kind: crate::agent_desk::config::DriverKind::Api,
        enabled: true,
        endpoint: endpoint.into(),
        model: "mock-model".into(),
        api_key: "sk-mock".into(),
        completion: CompletionSignal {
            kind: CompletionKind::HttpStreamEnd,
            pattern: String::new(),
            evidence_cmd: String::new(),
        },
        capabilities: vec![Capability::Text],
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_desk::driver::{CollectSink, RunRequest, DEFAULT_TIMEOUT_MS};

    #[test]
    fn timeout_is_min_of_request_and_default() {
        let d = mock_driver("http://127.0.0.1:1/v1/chat/completions");
        let mut req = RunRequest::new(0, 1, "x");
        req.timeout_ms = 5_000;
        assert_eq!(driver_timeout_secs(&d, &req), 5);

        req.timeout_ms = DEFAULT_TIMEOUT_MS * 4;
        assert_eq!(
            driver_timeout_secs(&d, &req),
            (DEFAULT_TIMEOUT_MS / 1000) as u32,
            "更大的请求超时不该突破默认上限"
        );

        // 0 表示"用默认"。
        req.timeout_ms = 0;
        assert_eq!(
            driver_timeout_secs(&d, &req),
            (DEFAULT_TIMEOUT_MS / 1000) as u32
        );
    }

    #[tokio::test]
    async fn unreachable_endpoint_fails_loudly_with_reason() {
        // 指向一个必然连不上的端口：必须返回失败 + 可读原因，绝不当成完成。
        let d = mock_driver("http://127.0.0.1:9/v1/chat/completions");
        let mut req = RunRequest::new(0, 7, "你好");
        req.timeout_ms = 3_000;
        let mut sink = CollectSink::default();

        let out = run(&d, &req, &mut sink, crate::runner::CancelToken::new()).await;

        assert_eq!(out.status, RunStatus::Failed);
        assert!(
            out.reason.as_ref().map(|r| !r.is_empty()).unwrap_or(false),
            "失败必须带原因"
        );
        assert!(out.output.is_empty());
    }

    /// 回归：取消必须**中断在飞的 HTTP 请求**，而不是等它自己收完。
    ///
    /// 此前 `api_driver::run` 的取消参数叫 `_cancel`、被完全忽略 ——
    /// Api 执行器的任务**根本取消不掉**，按了取消毫无反应，只能等模型把话说完。
    ///
    /// 造一个"只接受连接、永不回包"的端点，请求会一直挂在那里；取消若真的
    /// 中断了连接，`select` 的取消分支就会赢。
    #[tokio::test]
    async fn cancel_interrupts_in_flight_http_request() {
        // 假端点用 std 的 TcpListener 在独立线程里 accept，`mem::forget` 保持连接
        // 不关闭 ⇒ 客户端永远等不到响应。（改用 `tokio::net` 要给 tokio 加 `net`
        // feature，为一个测试动依赖不划算。）
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for s in listener.incoming() {
                if let Ok(s) = s {
                    std::mem::forget(s);
                }
            }
        });

        let d = mock_driver(&format!("http://{addr}/v1/chat/completions"));
        let mut req = RunRequest::new(0, 11, "x");
        // 远大于本测试时长：确保赢的是取消，不是超时。
        req.timeout_ms = 60_000;

        let cancel = crate::runner::CancelToken::new();
        let c2 = cancel.clone();
        let handle = tokio::spawn(async move {
            let mut sink = CollectSink::default();
            run(&d, &req, &mut sink, c2).await
        });

        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let t0 = std::time::Instant::now();
        cancel.cancel();
        let out = handle.await.unwrap();
        let el = t0.elapsed();

        assert_eq!(out.status, RunStatus::Cancelled, "取消应当报 Cancelled");
        assert!(out.output.is_empty(), "取消不该回传半截正文");
        assert!(
            el < std::time::Duration::from_secs(2),
            "取消没有中断在飞的请求：{el:?}"
        );
    }

    #[test]
    fn log_pattern_matching_is_exact_substring() {
        // 这段逻辑内联在 run 里，这里用同样的判定式验证语义（避免依赖网络）。
        let text = "过程日志……\nTASK_COMPLETE\n更多日志";
        assert!(text.contains("TASK_COMPLETE"));
        assert!(!text.contains("task_complete"), "匹配是大小写敏感的显式子串");
    }
}
