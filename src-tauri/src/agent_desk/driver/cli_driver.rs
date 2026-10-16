//! 通用 CLI 执行器 —— **一个 driver 覆盖所有 headless Agent CLI**。
//!
//! bl / qwen / codex / claude / opencode / cursor-agent …… 它们的 headless 调用形态
//! 高度一致，差异只在**命令行模板**与**完成信号**。所以这里是「1 个 driver + N 条配置」，
//! 而不是 N 个 driver。**加新 Agent = 加一行 JSON，不写 Rust 代码。**
//!
//! # 安全默认：不经 shell
//!
//! 任务文本作为**独立 argv 元素**传给子进程（`Command::args`），**不拼进 shell 命令行**。
//! 这意味着 prompt 里的 `&&`、`|`、`$(...)` 不会被解释成命令 —— 语音识别出来的任意
//! 一句话都不构成注入面。
//!
//! 需要 shell 特性（管道 / 重定向）时，由用户在配置里显式用
//! `command="cmd"` + `args=["/C", "…{prompt}…"]` 表达 —— **显式开启，而不是默认敞开**。
//!
//! # 完成判定
//!
//! - `ProcessExitZero`：退出码 0。**可信度中**（铁律：进程退出 ≠ 完成）。
//!   配了 `evidence_cmd` 世界门禁时会被升级，见 [`super::run_driver`]。
//! - `LogPattern`：stdout 里出现权威串。可信度高（上游明确说了"做完了"）。
//!
//! 两种都判不出时返回 `Failed` 并说清是哪种判不出，**绝不回落成"应该算成功吧"**。

use std::time::Duration;

use crate::agent_desk::config::{CompletionKind, ContinuationSpec, DriverConfig};
use crate::log_buffer;
use crate::runner::{self, CancelToken, RunnerEvent, RunnerOptions};

use super::{tail, truncate, EventSink, RunOutcome, RunRequest, RunStatus};

/// stdout 是否命中完成标记。
///
/// `pattern` 支持 **`;` 分隔的多候选串，任一命中即完成**（没有分隔符就是单串，
/// 与旧配置完全兼容）。
///
/// 为什么要多候选：上游工具的日志格式会随版本变。把它拆成几个版本号各写一个 driver
/// 是错的方向（形态是封闭集、差异只在串本身），而"只有一个串"意味着上游一改名，
/// 用户手上的配置就集体失效，且表现为**每个任务都失败**。
///
/// 纯函数：这是完成判定的唯一依据，必须能被单测直接钉住。
fn log_pattern_hit(stdout: &str, pattern: &str) -> bool {
    pattern
        .split(';')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .any(|p| stdout.contains(p))
}

/// 从 stdout 里抽出会话 id（用于下一轮续接）。
///
/// 只认配置指定的那一种事件：一次 headless 调用会输出几十行事件，
/// 而"哪一行承载会话标识"是**工具自己决定的事实**，必须落在配置里，
/// 而"哪一行承载会话标识"是**工具自己决定的事实**，必须落在配置里。
/// 不能靠"长得像 UUID 就去认"——输出里往往还有别的 id（thread / item / call），
/// 认错一个就是把第二轮接到一个毫不相干的会话上，而且毫无征兆。
///
/// 抽不到返回 `None` —— 那是"这轮没能建立会话"的正常表达，不是错误。
fn extract_session_id(stdout: &str, cont: &ContinuationSpec) -> Option<String> {
    for line in stdout.lines() {
        let line = line.trim();
        if !line.starts_with('{') || !line.ends_with('}') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some(cont.id_event_type.as_str()) {
            continue;
        }
        if let Some(id) = v.get(cont.id_field.as_str()).and_then(|x| x.as_str()) {
            let id = id.trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    None
}

/// 组装最终 argv。
///
/// # 续接参数插在哪（这是实测定死的，不是随手选的）
///
/// codex 的 headless 续接是
/// `exec --json --skip-git-repo-check resume <id> "<prompt>"`
/// —— 子命令必须落在**全局选项之后、prompt 之前**。写成
/// `exec resume <id> --json` 会把 `--json` 当成 `resume` 的参数。
///
/// 所以规则是：**插到 `{prompt}` 那个参数之前**；找不到 `{prompt}` 才追加到末尾
/// （那种配置通常把任务文本走 stdin，此时参数顺序无所谓）。
///
/// 纯函数：`argv` 是这条链路上唯一会被真正执行的东西，必须能单测钉住。
fn build_args(
    driver: &DriverConfig,
    prompt: &str,
    cont: Option<&ContinuationSpec>,
    session: Option<&str>,
) -> Vec<String> {
    let mut args: Vec<String> = driver
        .args
        .iter()
        .map(|a| a.replace("{prompt}", prompt))
        .collect();

    let (Some(cont), Some(session)) = (cont, session) else {
        return args;
    };
    let session = session.trim();
    if session.is_empty() {
        return args;
    }

    let cont_args: Vec<String> = cont
        .args
        .iter()
        .map(|a| a.replace("{id}", session))
        .collect();
    if cont_args.is_empty() {
        return args;
    }

    match driver.args.iter().position(|a| a.contains("{prompt}")) {
        Some(i) => {
            args.splice(i..i, cont_args);
            args
        }
        None => {
            args.extend(cont_args);
            args
        }
    }
}

/// 执行一次 CLI 调用：先试着续接会话，没成立就降级为新会话重跑一次。
///
/// # 为什么不把续接逻辑塞进调用方
///
/// "怎么把会话 id 递给这个工具"只有 driver 知道（argv 位置、子命令形态都不同）。
/// 让上层去拼 argv 意味着每加一个 CLI 都要改上层；让 driver 自己看一眼
/// `RunRequest::session_id` 则天然适配所有工具。
pub async fn run(
    driver: &DriverConfig,
    req: &RunRequest,
    sink: &mut dyn EventSink,
    cancel: CancelToken,
) -> RunOutcome {
    let cont = driver.continuation.as_ref().filter(|c| c.usable());
    let session = req
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let resumed = cont.is_some() && session.is_some();

    let outcome = run_once(driver, req, cont, session, sink, cancel.clone()).await;

    // ---- 降级：续接没成立 ⇒ 丢掉会话 id 重跑一次 ----
    //
    // 必须兜这一下：会话 id 是**上游**给的，它的生命周期我们管不着
    // （rollout 被删、版本升级换了格式、用户手改了配置都可能让它失效）。
    // 一旦失效而这里不做处理，用户看到的是"每次都失败"，且没有任何线索。
    // 降级重跑让它**至少还能用**，并且返回的 `session_id = None`
    // ⇒ 上层会丢掉这个坏 id，下一轮自然回到新会话，不会一直摔在同一个坑里。
    if resumed && resume_looks_failed(&outcome) {
        log_buffer::warn(format!(
            "任务台执行器 {} 续接会话疑似失效（本轮输出为空且未完成）——降级为新会话重跑一次",
            driver.id
        ));
        let fresh = run_once(driver, req, cont, None, sink, cancel).await;
        return RunOutcome {
            session_id: None,
            ..fresh
        };
    }
    outcome
}

/// "续接显然没成立"的判据。
///
/// 刻意**不看错误文案** —— 那要猜上游措辞，版本一变就失效。
/// 只看一件硬事实：这次调用**没有任何有效输出** ⇒ 它多半在建立会话那一步就退出了。
/// 取消与超时不算：那是"人按停了 / 到点了"，不是会话坏了。
fn resume_looks_failed(o: &RunOutcome) -> bool {
    o.status == RunStatus::Failed && o.output.trim().is_empty()
}

/// 真正跑一次（不含降级重试）。
#[allow(clippy::too_many_arguments)]
async fn run_once(
    driver: &DriverConfig,
    req: &RunRequest,
    cont: Option<&ContinuationSpec>,
    session: Option<&str>,
    sink: &mut dyn EventSink,
    cancel: CancelToken,
) -> RunOutcome {
    let started = std::time::Instant::now();

    // ---- 参数模板：{prompt} / 续接参数各自替换，顺序由 build_args 保证 ----
    let args = build_args(driver, &req.prompt, cont, session);
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    // ---- 选项 ----
    let mut opts = RunnerOptions::new();
    opts.timeout = Some(Duration::from_millis(req.effective_timeout_ms()));
    if !driver.cwd.trim().is_empty() {
        opts.cwd = Some(std::path::PathBuf::from(driver.cwd.trim()));
    }
    for raw in &driver.env {
        match raw.split_once('=') {
            Some((k, v)) if !k.trim().is_empty() => {
                opts.env.insert(k.trim().to_string(), v.to_string());
            }
            // 格式错误的 env 条目**不静默跳过**：它多半意味着用户想设一个关键变量却没设上。
            _ => log_buffer::warn(format!(
                "任务台 CLI 执行器 {} 的环境变量条目格式非法（应为 KEY=VALUE）：{raw}",
                driver.id
            )),
        }
    }

    log_buffer::info(format!(
        "任务台 CLI driver 发起: id={} cmd={} args={:?} cwd={:?} slot={} task={}",
        driver.id, driver.command, arg_refs, opts.cwd, req.slot, req.task_id
    ));

    let result = runner::run_stream(&driver.command, &arg_refs, &opts, cancel, |ev| match ev {
        RunnerEvent::StdoutLine(l) => sink.stdout_line(&l),
        RunnerEvent::StderrLine(l) => sink.stderr_line(&l),
        RunnerEvent::Done(_) => {}
    })
    .await;

    let duration_ms = started.elapsed().as_millis() as u64;

    let out = match result {
        Ok(o) => o,
        Err(e) => {
            // 启动失败（命令不存在 / cwd 不存在 / 权限）。这类错误最容易被误诊成
            // "Agent 跑失败了"，所以原文照抄，并标明是"启动"阶段。
            log_buffer::err(format!(
                "任务台 CLI driver 启动失败: id={} err={}",
                driver.id, e
            ));
            return RunOutcome {
                status: RunStatus::Failed,
                reason: Some(format!("启动执行器失败：{}", truncate(&e, 400))),
                output: String::new(),
                duration_ms,
                confidence: driver.completion.confidence(),
                evidence: None,
                session_id: None,
            };
        }
    };

    // ---- 抽出本轮会话 id，交给上层给下一轮续接 ----
    //
    // 只在**成功跑出东西**时才回传：空输出多半是压根没建立会话，
    // 把一个空会话 id 存进账本只会让下一轮带着垃圾参数去 resume。
    let sid = cont.and_then(|c| extract_session_id(&out.stdout, c));
    if let Some(id) = sid.as_deref() {
        log_buffer::info(format!("任务台执行器 {} 会话 id：{}", driver.id, id));
    }

    // ---- 取消 / 超时优先于一切完成判定 ----
    if out.cancelled {
        return RunOutcome {
            status: RunStatus::Cancelled,
            reason: Some("任务被取消".into()),
            output: out.stdout,
            duration_ms,
            confidence: driver.completion.confidence(),
            evidence: None,
            session_id: sid.clone(),
        };
    }
    if out.timed_out {
        return RunOutcome {
            status: RunStatus::TimedOut,
            reason: Some(format!(
                "执行超时（{} 秒）——进程已被终止，产物可能不完整",
                req.effective_timeout_ms() / 1000
            )),
            output: out.stdout,
            duration_ms,
            confidence: driver.completion.confidence(),
            evidence: None,
            session_id: sid.clone(),
        };
    }

    let confidence = driver.completion.confidence();
    match driver.completion.kind {
        CompletionKind::ProcessExitZero => {
            if out.is_success() {
                RunOutcome {
                    status: RunStatus::Completed,
                    reason: None,
                    output: out.stdout,
                    duration_ms,
                    confidence,
                    evidence: None,
            session_id: sid.clone(),
                }
            } else {
                // ⚠️ 铁律：进程退出码非 0 只说明"进程结束了"，但这里它确实是失败信号。
                // 真正危险的是它**等于 0** 的时候——那也不代表任务做成，见模块文档。
                let tail = if out.stderr.trim().is_empty() {
                    out.stdout.clone()
                } else {
                    out.stderr.clone()
                };
                RunOutcome {
                    status: RunStatus::Failed,
                    reason: Some(format!(
                        "进程退出码 {:?}（非 0）。{}",
                        out.exit_code,
                        truncate(&tail, 400)
                    )),
                    output: out.stdout,
                    duration_ms,
                    confidence,
                    evidence: None,
            session_id: sid.clone(),
                }
            }
        }
        CompletionKind::LogPattern => {
            let pat = driver.completion.pattern.trim();
            if log_pattern_hit(&out.stdout, pat) {
                RunOutcome {
                    status: RunStatus::Completed,
                    reason: None,
                    output: out.stdout,
                    duration_ms,
                    confidence,
                    evidence: None,
            session_id: sid.clone(),
                }
            } else {
                // ⚠️ **失败必须自带证据。** 只回一句"没出现某某标记"等于把排查成本
                // 全推给用户 —— 09-20 就栽在这：codex 的每个任务都报"没出现
                // task_complete"，而 stdout 里真正写的是 `turn.completed`，
                // 光看那句话永远看不出来，非得把输出抓出来数一遍。
                // 取**尾部**：决定终态的事件总在输出末尾，取头部等于把最关键那句砍掉。
                let tail_evidence = tail(&out.stdout, 600);
                let info = format!(
                    "输出里没有出现完成标记「{pat}」；退出码 {:?}，stdout {} 字，尾部如下：\n{}",
                    out.exit_code,
                    out.stdout.chars().count(),
                    tail_evidence
                );
                log_buffer::warn(format!(
                    "任务台执行器 {} 的完成标记未命中：{}",
                    driver.id, info
                ));
                RunOutcome {
                    status: RunStatus::Failed,
                    reason: Some(info),
                    output: out.stdout,
                    duration_ms,
                    confidence,
                    evidence: None,
            session_id: sid.clone(),
                }
            }
        }
        // validate_driver 已拦；纵深防御，绝不静默当成完成。
        other => RunOutcome::failed(format!(
            "CLI 执行器不支持完成信号「{}」——请在执行器设置里改成进程退出码或日志权威串",
            other.label()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_desk::config::{Capability, CompletionSignal, DeskConfig, DriverKind};
    use crate::agent_desk::driver::CollectSink;

    /// 造一个用 `cmd /C echo` 当"Agent"的 driver —— 真实子进程、离线可测。
    fn echo_driver(completion: CompletionSignal) -> DriverConfig {
        DriverConfig {
            id: "echo-cli".into(),
            label: "Echo".into(),
            kind: DriverKind::Cli,
            enabled: true,
            command: "cmd".into(),
            args: vec!["/C".into(), "echo {prompt}".into()],
            env: vec!["HOME=C:\\Users\\test".into()],
            cwd: String::new(),
            completion,
            capabilities: vec![Capability::Text],
            ..Default::default()
        }
    }

    fn exit_completion(code: i32) -> CompletionSignal {
        CompletionSignal {
            kind: CompletionKind::ProcessExitZero,
            pattern: String::new(),
            // 用真实子进程当门禁：退出码可控、离线可测。
            evidence_cmd: format!("exit {code}"),
        }
    }

    #[tokio::test]
    async fn shell_metacharacters_are_interpreted_when_the_config_uses_a_shell() {
        // ⚠️ 这是一个**已知且刻意保留**的行为，不是 bug：
        // 当 driver 自己把参数写成 `cmd /C <...>` 时，整串会交给 cmd 解释，
        // 于是 prompt 里的 `&&` 会被当成命令分隔符。
        //
        // 为什么不去"修"它：用户配 `cmd /C` 正是为了用 shell 特性（管道、重定向、串联）。
        // 把这个能力堵掉，等于让一半的用法用不了。
        //
        // 防线在别处：**只有用户主动写出 `cmd /C` 才会进入这条路径**。
        // 预置的 7 个执行器全部直接 spawn（dsh / bl / qwen / codex），
        // prompt 作为独立 argv 元素传入，不经过 shell。
        //
        // 这条测试的作用是把"到底会发生什么"钉死，而不是假装它不会发生。
        let mut d = echo_driver(exit_completion(0));
        d.args = vec!["/C".into(), "echo {prompt}".into()];
        let req = RunRequest::new(0, 1, "a && echo INJECTED");
        let mut sink = CollectSink::default();

        let out = run(&d, &req, &mut sink, CancelToken::new()).await;

        assert!(
            out.output.contains("INJECTED"),
            "经 shell 时元字符确实会被解释（这是事实，不是期望）：{:?}",
            out.output
        );
    }

    #[tokio::test]
    async fn prompt_reaches_a_directly_spawned_program_without_shell_interpretation() {
        // 判据选取说明：不去断言"参数原样出现在输出里"——`where` 的提示信息走的是
        // cmd 的 GBK 代码页，经 Rust 的 UTF-8 读取会整段丢失（实测 stdout/stderr 长度都是 0）。
        // 依赖它会让测试变成一个"看运气"的东西。
        //
        // 改用**可判定的否定证据**：如果 prompt 被 shell 拆解，那么 `&& echo INJECTED`
        // 会真的执行并产生输出。现在输出是空的 ⇒ `&&` 被当作参数内容，没有 shell 介入。
        let mut d = echo_driver(exit_completion(0));
        d.command = "where".into();
        d.args = vec!["{prompt}".into()];
        let req = RunRequest::new(0, 2, "no-such-file && echo INJECTED");
        let mut sink = CollectSink::default();

        let out = run(&d, &req, &mut sink, CancelToken::new()).await;

        assert!(
            !out.output.contains("INJECTED"),
            "直接 spawn 时元字符不得被解释，实际输出 {:?}",
            out.output
        );
    }

    #[tokio::test]
    async fn stdout_is_streamed_to_the_sink() {
        let d = echo_driver(exit_completion(0));
        let req = RunRequest::new(0, 2, "hello");
        let mut sink = CollectSink::default();

        run(&d, &req, &mut sink, CancelToken::new()).await;
        assert!(
            sink.stdout.iter().any(|l| l.contains("hello")),
            "实时行必须推给出口（前端靠它显示进度），实际 {:?}",
            sink.stdout
        );
    }

    #[tokio::test]
    async fn process_exit_zero_completes_but_stays_unverified_without_gate() {
        // 铁律：进程退出 0 也不算"已校验"，除非门禁也过。
        let d = echo_driver(CompletionSignal {
            kind: CompletionKind::ProcessExitZero,
            pattern: String::new(),
            evidence_cmd: String::new(),
        });
        let req = RunRequest::new(0, 3, "x");
        let mut sink = CollectSink::default();

        let out = run(&d, &req, &mut sink, CancelToken::new()).await;
        assert_eq!(out.status, RunStatus::Completed);
        assert_eq!(
            out.confidence,
            crate::agent_desk::config::Confidence::Medium,
            "没有外部证据时，进程退出只能是「未校验」"
        );
    }

    #[tokio::test]
    async fn nonzero_exit_is_a_failure_with_stderr_context() {
        let mut d = echo_driver(CompletionSignal {
            kind: CompletionKind::ProcessExitZero,
            pattern: String::new(),
            evidence_cmd: String::new(),
        });
        d.args = vec!["/C".into(), "echo boom 1>&2 & exit 4".into()];
        let req = RunRequest::new(0, 4, "x");
        let mut sink = CollectSink::default();

        let out = run(&d, &req, &mut sink, CancelToken::new()).await;
        assert_eq!(out.status, RunStatus::Failed);
        let reason = out.reason.unwrap();
        assert!(reason.contains("4"), "必须报出退出码，实际 {reason}");
        assert!(reason.contains("boom"), "必须带上错误输出，实际 {reason}");
    }

    #[tokio::test]
    async fn exit_zero_without_pattern_still_fails_under_log_pattern_signal() {
        // 完成信号选了"日志权威串"，但输出里没有它 ⇒ 不能算完成。
        let d = echo_driver(CompletionSignal {
            kind: CompletionKind::LogPattern,
            pattern: "TASK_COMPLETE".into(),
            evidence_cmd: String::new(),
        });
        let req = RunRequest::new(0, 5, "hello");
        let mut sink = CollectSink::default();

        let out = run(&d, &req, &mut sink, CancelToken::new()).await;
        assert_eq!(out.status, RunStatus::Failed);
        assert!(out.reason.unwrap().contains("TASK_COMPLETE"));
    }

    #[tokio::test]
    async fn log_pattern_present_completes_with_high_confidence() {
        let d = echo_driver(CompletionSignal {
            kind: CompletionKind::LogPattern,
            pattern: "TASK_COMPLETE".into(),
            evidence_cmd: String::new(),
        });
        let req = RunRequest::new(0, 6, "TASK_COMPLETE");
        let mut sink = CollectSink::default();

        let out = run(&d, &req, &mut sink, CancelToken::new()).await;
        assert_eq!(out.status, RunStatus::Completed);
        assert_eq!(
            out.confidence,
            crate::agent_desk::config::Confidence::High,
            "权威串本身就是高可信信号"
        );
    }

    #[test]
    fn log_pattern_supports_multiple_candidates() {
        let out = r#"{"type":"thread.started"}"#;
        // 单串：与旧配置完全兼容。
        assert!(log_pattern_hit(out, "thread.started"));
        assert!(!log_pattern_hit(out, "turn.completed"));
        // 多串：任一命中即可；多余的分号/空白不能变成"空串恒命中"。
        assert!(log_pattern_hit(out, "nope; thread.started "));
        assert!(!log_pattern_hit(out, "nope; also-nope"));
        assert!(!log_pattern_hit(out, ""));
        assert!(!log_pattern_hit(out, ";;"));
    }

    /// 09-20 抓到的 codex `exec --json` 真实 stdout（缩略版）。这条的作用是把
    /// **"完成标记该取哪一个"**这个实测结论钉死 —— 上一版取 `task_complete`，
    /// 于是每个任务都被误判失败。
    #[test]
    fn codex_exec_stdout_completes_on_turn_completed() {
        const REAL_CODEX_STDOUT: &str = concat!(
            r#"{"type":"thread.started","thread_id":"01a0be3d"}"#,
            "\n",
            r#"{"type":"turn.started"}"#,
            "\n",
            r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"OK"}}"#,
            "\n",
            r#"{"type":"turn.completed","usage":{"input_tokens":11249}}"#,
        );
        // stdout 上确实**没有** task_complete —— 那是 rollout 文件里的事件。
        assert!(!REAL_CODEX_STDOUT.contains("task_complete"));
        assert!(log_pattern_hit(
            REAL_CODEX_STDOUT,
            "turn.completed;task_complete"
        ));
    }

    #[tokio::test]
    async fn pattern_miss_reports_the_actual_output_tail() {
        // 失败原因必须自带证据：只说"没命中某某串"等于让用户瞎猜（09-20 实测踩过）。
        let d = echo_driver(CompletionSignal {
            kind: CompletionKind::LogPattern,
            pattern: "turn.completed".into(),
            evidence_cmd: String::new(),
        });
        let req = RunRequest::new(0, 11, "THE_TAIL_MARKER");
        let mut sink = CollectSink::default();

        let outcome = run(&d, &req, &mut sink, CancelToken::new()).await;
        let reason = outcome.reason.unwrap();
        assert!(reason.contains("退出码"), "必须带上退出码：{reason}");
        assert!(
            reason.contains("THE_TAIL_MARKER"),
            "必须附上输出尾部，否则无从判断该改成哪个串：{reason}"
        );
    }

    #[tokio::test]
    async fn cancellation_is_reported_as_cancelled_not_failed() {
        // 取消 ≠ 失败：用户主动中止不该在日志里记成"任务出错"。
        let mut d = echo_driver(exit_completion(0));
        // ping 一个不存在的地址并等待，保证进程活着足够久。
        d.args = vec!["/C".into(), "ping -n 10 127.0.0.1 >nul".into()];
        let req = RunRequest::new(0, 8, "x");
        let mut sink = CollectSink::default();

        let cancel = CancelToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(400)).await;
            c2.cancel();
        });

        let out = run(&d, &req, &mut sink, cancel).await;
        assert_eq!(out.status, RunStatus::Cancelled);
    }

    #[tokio::test]
    async fn missing_command_fails_at_launch_with_clear_wording() {
        let mut d = echo_driver(exit_completion(0));
        d.command = "definitely-not-a-real-binary-xyz".into();
        d.args = vec!["{prompt}".into()];
        let req = RunRequest::new(0, 9, "x");
        let mut sink = CollectSink::default();

        let out = run(&d, &req, &mut sink, CancelToken::new()).await;
        assert_eq!(out.status, RunStatus::Failed);
        assert!(
            out.reason.unwrap().contains("启动"),
            "必须区分「启动失败」与「跑失败」——误诊方向完全不同"
        );
    }

    #[tokio::test]
    async fn timeout_is_reported_as_timeout() {
        let mut d = echo_driver(exit_completion(0));
        d.args = vec!["/C".into(), "ping -n 10 127.0.0.1 >nul".into()];
        let mut req = RunRequest::new(0, 10, "x");
        req.timeout_ms = 300;
        let mut sink = CollectSink::default();

        let out = run(&d, &req, &mut sink, CancelToken::new()).await;
        assert_eq!(out.status, RunStatus::TimedOut);
        assert!(out.reason.unwrap().contains("超时"));
    }

    // ---- 会话续接（多轮对话，09-20 实测定型）----

    fn cont_codex() -> ContinuationSpec {
        ContinuationSpec {
            id_event_type: "thread.started".into(),
            id_field: "thread_id".into(),
            args: vec!["resume".into(), "{id}".into()],
        }
    }

    /// 本机实跑抓到的 codex stdout（结构未改，仅缩行）。
    const CODEX_STDOUT: &str = concat!(
        r#"{"type":"thread.started","thread_id":"01a0bf1e-2e5e-7810-8e94-18005a8acacc"}"#,
        "\n",
        r#"{"type":"turn.started"}"#,
        "\n",
        r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"7391"}}"#,
        "\n",
        r#"{"type":"turn.completed","usage":{"input_tokens":31305}}"#,
    );

    #[test]
    fn session_id_comes_from_the_configured_event_only() {
        // 输出里有多个 id（thread / item / call），只认配置指定的那一个。
        assert_eq!(
            extract_session_id(CODEX_STDOUT, &cont_codex()).as_deref(),
            Some("01a0bf1e-2e5e-7810-8e94-18005a8acacc")
        );
        // 换个事件名 ⇒ 什么都抽不到，**而不是抽一个"看起来也像 id"的东西**。
        let wrong = ContinuationSpec {
            id_event_type: "turn.started".into(),
            ..cont_codex()
        };
        assert_eq!(extract_session_id(CODEX_STDOUT, &wrong), None);
    }

    #[test]
    fn resume_args_land_after_global_flags_and_before_the_prompt() {
        // 这是实测这条亲身踩出来的顺序：`exec resume <id> --json` 会把 --json
        // 当成 resume 的参数；必须是 `exec --json … resume <id> <prompt>`。
        let mut d = echo_driver(exit_completion(0));
        d.args = vec![
            "exec".into(),
            "--json".into(),
            "--skip-git-repo-check".into(),
            "{prompt}".into(),
        ];
        let args = build_args(&d, "打开微信", Some(&cont_codex()), Some("abc123"));
        assert_eq!(
            args,
            vec![
                "exec",
                "--json",
                "--skip-git-repo-check",
                "resume",
                "abc123",
                "打开微信",
            ]
        );
    }

    /// v0.42.1：拿**真实预置参数**（含 `--sandbox danger-full-access`）叠续接，
    /// 验证子命令 `resume` 仍落在所有全局选项之后。
    ///
    /// 上面那条用的是手写 args；这条钉的是预置本体 —— 将来谁再往预置里加全局旗标，
    /// 如果把它插到了 `{prompt}` 之后，这条会红。
    #[test]
    fn the_real_preset_keeps_resume_after_every_global_flag() {
        let codex = DeskConfig::default()
            .driver("codex-cli")
            .expect("预置必须有 codex-cli")
            .clone();
        let cont = codex
            .continuation
            .as_ref()
            .expect("codex 预置必须配续接");
        let args = build_args(&codex, "打开微信", Some(cont), Some("abc123"));
        assert_eq!(
            args,
            vec![
                "exec",
                "--json",
                "--skip-git-repo-check",
                "--sandbox",
                "danger-full-access",
                "resume",
                "abc123",
                "打开微信",
            ]
        );
    }

    #[test]
    fn without_a_session_the_argv_is_untouched() {
        // 续接是增强：没有会话时必须**退化成原本那条命令**，而不是塞个空参数进去。
        let mut d = echo_driver(exit_completion(0));
        d.args = vec!["exec".into(), "{prompt}".into()];
        let args = build_args(&d, "hi", Some(&cont_codex()), None);
        assert_eq!(args, vec!["exec", "hi"]);
        // 配空 configs（不支持续接的 CLI）也一样：原样返回。
        let args2 = build_args(&d, "hi", None, Some("abc"));
        assert_eq!(args2, vec!["exec", "hi"]);
    }

    #[test]
    fn empty_session_id_is_not_a_session() {
        // 空白 id 一旦被当真，就是"每次都带着空字符串去 resume"。
        let mut d = echo_driver(exit_completion(0));
        d.args = vec!["exec".into(), "{prompt}".into()];
        let args = build_args(&d, "hi", Some(&cont_codex()), Some("   "));
        assert_eq!(args, vec!["exec", "hi"]);
    }

    #[test]
    fn when_the_driver_has_no_prompt_placeholder_args_append_at_the_end() {
        // prompt 走 stdin 的配置：没有 {prompt}，续接参数只能笨拙地追加到末尾
        // —— 没有更好位置可选，但至少不能丢。
        let mut d = echo_driver(exit_completion(0));
        d.args = vec!["exec".into(), "--json".into()];
        let args = build_args(&d, "", Some(&cont_codex()), Some("zz"));
        assert_eq!(args, vec!["exec", "--json", "resume", "zz"]);
    }

    #[test]
    fn degraded_retry_only_triggers_on_failure_with_no_output() {
        // 判据刻意不用错误文案（那是猜上游措辞）。
        let base = RunOutcome {
            status: RunStatus::Failed,
            reason: Some("boom".into()),
            output: String::new(),
            duration_ms: 1,
            confidence: crate::agent_desk::config::Confidence::Medium,
            evidence: None,
            session_id: None,
        };
        assert!(resume_looks_failed(&base));
        // 有输出 ⇒ 它真的跑了，只是没成功 —— 不该再重跑一次（那是双倍烧钱 + 双倍时间）。
        let with_output = RunOutcome {
            output: "something happened".into(),
            ..base.clone()
        };
        assert!(!resume_looks_failed(&with_output));
        // 超时/取消不是"会话坏了"，是"人按停了/到点了"。
        let timed = RunOutcome {
            status: RunStatus::TimedOut,
            ..base
        };
        assert!(!resume_looks_failed(&timed));
    }

}
