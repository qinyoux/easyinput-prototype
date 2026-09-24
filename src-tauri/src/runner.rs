//! 可观测子进程执行器（runner）—— 内容台调 Python 脚本的统一通道。
//!
//! 现有 `launcher.rs` / `ss_palette.rs` / `everything.rs` 里的 `std::process::Command`
//! 都是同步阻塞调用，适合"启动即忘"或"读一次 stdout 就走"的场景。内容台需要调
//! `pub.py export/check`、`gen_figs.py`、`qa_check.py` 等长任务脚本，要求：
//!
//! - **非阻塞**：不卡事件循环（spawn 后立即返回句柄）
//! - **行读**：实时拿到 stdout/stderr 每一行，推进度
//! - **UTF-8**：自动注入 `PYTHONIOENCODING=utf-8` + `PYTHONUTF8=1`，防管道下中文乱码
//! - **超时**：到点杀子进程
//! - **取消**：调用方主动杀
//! - **退出码**：区分成功 / 非零码 / 被杀
//!
//! 用 `tokio::process::Command`（需 tokio `process` + `io-util` feature）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::log_buffer;

/// 取消轮询间隔。无超时模式下用此间隔轮询取消信号 + 转发行事件。
const POLL_INTERVAL: Duration = Duration::from_millis(150);

/// 取消信号的等待轮询间隔（供 `select!` 用的 [`CancelToken::cancelled`]）。
const CANCEL_POLL: Duration = Duration::from_millis(80);

/// 进程**正常退出**后的收尾宽限：把管道里剩下的行读完。
///
/// 正常退出时管道早已关闭（写端进程没了），这里几乎瞬间返回；两秒只是
/// "万一上游写了一大坨还没冲干净"的余量。
const DRAIN_GRACE_NORMAL: Duration = Duration::from_secs(2);

/// 进程**被终止**（取消 / 超时）后的收尾宽限。刻意比上面短得多。
///
/// 此时整棵进程树已被 `terminate_tree` 杀掉，管道理论上已经关闭。留这一小段
/// 只是为了把"已经躺在管道缓冲区里的最后几行"读出来。留长了就等于让"取消"
/// 这个动作**变慢** —— 而它唯一的意义就是立刻停下来。
const DRAIN_GRACE_KILLED: Duration = Duration::from_millis(300);

// ---------------------------------------------------------------------------
// 选项
// ---------------------------------------------------------------------------

/// 执行器选项。
#[derive(Clone, Debug, Default)]
pub struct RunnerOptions {
    /// 超时。`None` = 不超时（慎用，长任务应始终给上限）。
    pub timeout: Option<Duration>,
    /// 工作目录。`None` = 继承当前进程。
    pub cwd: Option<PathBuf>,
    /// 额外环境变量（在继承当前环境的基础上叠加）。
    pub env: HashMap<String, String>,
    /// 是否自动注入 UTF-8 环境变量（`PYTHONIOENCODING=utf-8` + `PYTHONUTF8=1`）。
    /// 缺省 true——内容台调 Python 脚本必须开。
    pub utf8_env: bool,
}

impl RunnerOptions {
    /// 便捷构造：默认开 UTF-8 环境，无超时。
    pub fn new() -> Self {
        Self {
            utf8_env: true,
            ..Default::default()
        }
    }

    /// 设超时。
    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = Some(d);
        self
    }

    /// 设工作目录。
    pub fn cwd(mut self, p: impl Into<PathBuf>) -> Self {
        self.cwd = Some(p.into());
        self
    }

    /// 追加一个环境变量。
    pub fn env(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.env.insert(k.into(), v.into());
        self
    }
}

// ---------------------------------------------------------------------------
// 取消令牌
// ---------------------------------------------------------------------------

/// 取消令牌。clone 后共享同一取消信号（`Arc<AtomicBool>`）。
///
/// 用原子布尔而非 `watch` channel：取消是"发一次就够"的信号，
/// 不需要 async 等待——主循环每 `POLL_INTERVAL` 轮询一次即可。
#[derive(Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// 发出取消信号。已结束的进程忽略；运行中的会被 kill。
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    /// 是否已发出取消信号。
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// 等到取消信号到来（供 `tokio::select!` 用）。
    ///
    /// 轮询而非 `Notify`：取消是"发一次就够"的信号，80ms 粒度足够，
    /// 而且不用给 `CancelToken` 再加一个 `Arc` 字段 —— 它的 clone 语义
    /// （clone 即共享同一信号）必须保持简单，别在这里引入新的共享状态。
    ///
    /// 已取消时**立即返回**，不睡那 80ms。
    pub async fn cancelled(&self) {
        while !self.is_cancelled() {
            tokio::time::sleep(CANCEL_POLL).await;
        }
    }
}

// ---------------------------------------------------------------------------
// 结果与事件
// ---------------------------------------------------------------------------

/// 执行结果。
#[derive(Clone, Debug, serde::Serialize)]
pub struct RunnerOutput {
    /// 退出码。`None` = 被信号杀（超时 / 取消）。
    pub exit_code: Option<i32>,
    /// stdout 全文（逐行拼接，`\n` 分隔）。
    pub stdout: String,
    /// stderr 全文。
    pub stderr: String,
    /// 是否因超时被杀。
    pub timed_out: bool,
    /// 是否被调用方取消。
    pub cancelled: bool,
}

impl RunnerOutput {
    /// 是否成功（退出码 0）。
    pub fn is_success(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// 执行事件——流式接口的增量通知。
#[derive(Clone, Debug)]
pub enum RunnerEvent {
    /// stdout 的一行（不含换行）。
    StdoutLine(String),
    /// stderr 的一行。
    StderrLine(String),
    /// 进程结束。总是最后一条事件。
    Done(RunnerOutput),
}

// ---------------------------------------------------------------------------
// 执行
// ---------------------------------------------------------------------------

/// 运行子进程，收集全部输出后返回。适合短任务或不需要逐行进度的场景。
///
/// 长任务（生成正文、配图等）应改用 [`run_stream`]，以便实时推进度。
pub async fn run(
    program: &str,
    args: &[&str],
    opts: &RunnerOptions,
) -> Result<RunnerOutput, String> {
    run_stream(program, args, opts, CancelToken::new(), |_| {}).await
}

/// 运行子进程，逐行推送事件。返回最终汇总结果。
///
/// `on_event` 在每个 stdout/stderr 行和进程结束时同步调用——
/// 调用方可在此 emit Tauri 事件或更新进度条。
///
/// # 取消与超时
///
/// 主循环每 `POLL_INTERVAL`（150ms）轮询一次取消信号。超时到点立即 kill。
/// kill 后仍会 drain stdout/stderr 剩余行（pipe 关闭后行读任务自动结束）。
pub async fn run_stream<F>(
    program: &str,
    args: &[&str],
    opts: &RunnerOptions,
    cancel: CancelToken,
    mut on_event: F,
) -> Result<RunnerOutput, String>
where
    F: FnMut(RunnerEvent),
{
    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    // ⚠️ stdin 必须显式"关掉"，不能继承。
    //
    // headless 调用不需要输入 —— 但不少 CLI（codex 就是）发现 stdin 可读时会打印
    // "Reading additional input from stdin..." 然后**真的等在那里**，跑到超时为止。
    // 继承时它读到什么取决于宿主的运行方式（控制台 / 被谁拉起），于是同一个任务
    // 会表现出"这次 20 秒、下次永远挂着"这种最难复现的故障。
    cmd.stdin(std::process::Stdio::null());
    cmd.kill_on_drop(true);

    // ⚠️ 子进程一律**不建控制台窗口**。
    //
    // 这些执行器（codex / dsh / bl…）是命令行程序，Windows 上给控制台程序起进程
    // 默认会分配一个控制台 ⇒ 任务一跑就闪一个黑框。它没有任何信息量（输出我们
    // 全从管道里读），却会在用户正在打字/演示的时候抢一下焦点。
    // 这里用的是 `tokio::process::Command` **自带的** `creation_flags`（不是
    // `std::os::windows::process::CommandExt`）—— import 那个 trait 会得到一条
    // "unused import" 警告，因为 tokio 的固有方法优先，trait 根本没被用到。
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    if let Some(cwd) = opts.cwd.as_ref() {
        cmd.current_dir(cwd);
    }
    if opts.utf8_env {
        cmd.env("PYTHONIOENCODING", "utf-8");
        cmd.env("PYTHONUTF8", "1");
    }
    for (k, v) in &opts.env {
        cmd.env(k, v);
    }

    log_buffer::info(format!(
        "runner 启动: {} {} (timeout={:?})",
        program,
        args.join(" "),
        opts.timeout
    ));

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动子进程失败 {program}: {e}"))?;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    // 行读任务：逐行读 stdout/stderr，通过 channel 推给主循环。
    // channel unbounded——行速率远低于内存上限，不会背压。
    let (line_tx, mut line_rx) = tokio::sync::mpsc::unbounded_channel::<RunnerEvent>();
    let line_tx_err = line_tx.clone();

    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let _ = line_tx.send(RunnerEvent::StdoutLine(line));
        }
    });
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let _ = line_tx_err.send(RunnerEvent::StderrLine(line));
        }
    });

    let start = Instant::now();
    let mut stdout_lines: Vec<String> = Vec::new();
    let mut stderr_lines: Vec<String> = Vec::new();

    // 转发一行到调用方 + 收集。
    fn handle_event<F: FnMut(RunnerEvent)>(
        ev: RunnerEvent,
        on_event: &mut F,
        stdout_lines: &mut Vec<String>,
        stderr_lines: &mut Vec<String>,
    ) {
        match &ev {
            RunnerEvent::StdoutLine(l) => stdout_lines.push(l.clone()),
            RunnerEvent::StderrLine(l) => stderr_lines.push(l.clone()),
            RunnerEvent::Done(_) => {}
        }
        on_event(ev);
    }

    // 主循环：等进程结束 / 超时 / 取消，同时转发行事件。
    let (exit_code, timed_out, cancelled) = loop {
        // 非阻塞转发已到的行。
        while let Ok(ev) = line_rx.try_recv() {
            handle_event(ev, &mut on_event, &mut stdout_lines, &mut stderr_lines);
        }

        if cancel.is_cancelled() {
            // ⚠️ 用 terminate_tree 而不是 child.kill()：后者只杀直接子进程，
            // `.cmd` 包出来的 node 会活下来继续攥着管道（详见函数文档）。
            terminate_tree(&mut child).await;
            break (None, false, true);
        }

        let step = match opts.timeout {
            Some(t) => {
                let elapsed = start.elapsed();
                if elapsed >= t {
                    terminate_tree(&mut child).await;
                    break (None, true, false);
                }
                (t - elapsed).min(POLL_INTERVAL)
            }
            None => POLL_INTERVAL,
        };

        tokio::select! {
            // 进程结束
            res = child.wait() => match res {
                Ok(status) => break (status.code(), false, false),
                Err(e) => return Err(format!("等待子进程失败: {e}")),
            },
            // 到点轮询
            _ = tokio::time::sleep(step) => {}
            // 新行到达
            ev = line_rx.recv() => {
                if let Some(ev) = ev {
                    handle_event(ev, &mut on_event, &mut stdout_lines, &mut stderr_lines);
                }
            }
        }
    };

    // ---- 收尾：把行读任务里剩下的行排干 ----
    //
    // ⚠️ **这一步必须带超时。** 行读任务只在管道 EOF 时结束，而管道 EOF 要求
    // **所有**持有写端的进程都退出 —— 包括我们没直接管的孙进程。
    // Windows 上 `cmd /c xxx.cmd` 的真实结构是 `cmd.exe → node.exe`：
    // 杀掉 cmd 而 node 还活着时，管道**永不** EOF，这个循环就会一直挂着，
    // 任务看起来"取消不掉"，其实是在等 CLI 自己跑完。
    //
    // 实测（2026-09-23）：`cmd /c start /b ping -n 12`，父进程 exit=0 之后
    // `run()` 仍被拖住 11.14 秒 —— 正好等于孙进程的寿命。
    let grace = if cancelled || timed_out {
        DRAIN_GRACE_KILLED
    } else {
        DRAIN_GRACE_NORMAL
    };
    let deadline = Instant::now() + grace;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            log_buffer::warn(format!(
                "runner 收尾超时（{}ms）：仍有进程持有输出管道，剩余输出已丢弃（program={program}）",
                grace.as_millis()
            ));
            break;
        }
        match tokio::time::timeout(left, line_rx.recv()).await {
            // 有新行：照常转发
            Ok(Some(ev)) => handle_event(ev, &mut on_event, &mut stdout_lines, &mut stderr_lines),
            // 所有发送端已丢弃 = 管道正常关闭，收尾完成
            Ok(None) => break,
            // 超时：回到循环头，由上面那段统一记日志后退出
            Err(_) => continue,
        }
    }

    let output = RunnerOutput {
        exit_code,
        stdout: stdout_lines.join("\n"),
        stderr: stderr_lines.join("\n"),
        timed_out,
        cancelled,
    };

    on_event(RunnerEvent::Done(output.clone()));

    log_buffer::info(format!(
        "runner 结束: {} exit={:?} timed_out={} cancelled={} stdout_len={} stderr_len={}",
        program,
        output.exit_code,
        output.timed_out,
        output.cancelled,
        output.stdout.len(),
        output.stderr.len()
    ));

    Ok(output)
}

// ---------------------------------------------------------------------------
// 终止进程树
// ---------------------------------------------------------------------------

/// 终止指定 pid 的**整棵进程树**（含孙进程）。仅 Windows 需要/可实现。
///
/// # 为什么不能只用 `Child::kill()`
///
/// `Child::kill()` 只杀**直接子进程**。而 Windows 上 CLI Agent 的真实结构几乎
/// 都是两层：
///
/// ```text
/// runner 的 child = cmd.exe        ← Rust 标准库对 `.cmd`/`.bat` 自动包的一层
///                     └─ node.exe  ← 干活的那个（codex / dsh / …）
/// ```
///
/// 杀掉 `cmd.exe` 之后 `node.exe` **照常活着**：继续改文件、继续烧 API 额度，
/// 而且**继承着 stdout 管道的写端** ⇒ 管道永不 EOF ⇒ 收尾循环永远等不到头。
/// 表现就是"取消按了没用，只能等它自己跑完"。
///
/// `taskkill /T` 按 `ParentProcessId` 链递归找子孙，所以**必须在父进程还活着时调用**：
/// 父进程一死链就断了，剩下的孤儿再也找不到从属关系。
#[cfg(windows)]
async fn kill_process_tree(pid: u32) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // 用绝对路径找 taskkill：`PATH` 在用户机器上可能被改得面目全非，而这是
    // "停住任务"的最后手段，不该因为一个 PATH 问题而静默失效。
    let exe = std::env::var("SystemRoot")
        .map(|r| format!("{r}\\System32\\taskkill.exe"))
        .unwrap_or_else(|_| "taskkill.exe".to_string());

    let mut cmd = Command::new(&exe);
    cmd.args(["/T", "/F", "/PID", &pid.to_string()]);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    cmd.creation_flags(CREATE_NO_WINDOW);

    match cmd.status().await {
        Ok(s) if s.success() => {
            log_buffer::info(format!("runner 已终止整棵进程树 pid={pid}"));
        }
        // 非 0 最常见的原因是"进程已经不在了"（128）。这是正常竞态
        // （用户点取消的同时任务刚好自己结束），不必当成故障。
        Ok(s) => log_buffer::warn(format!(
            "runner taskkill 退出码 {:?}（pid={pid} 可能已自行退出）",
            s.code()
        )),
        Err(e) => log_buffer::warn(format!("runner 调用 taskkill 失败（pid={pid}）: {e}")),
    }
}

/// 终止子进程**及其整棵进程树**，并回收进程句柄。
///
/// 顺序是硬性的：**先杀树，后杀子进程**。反过来（先 `kill()` 掉子进程）
/// 会让 `taskkill /T` 找不到子孙 —— 它会安静地只杀掉一个已经死掉的 pid。
async fn terminate_tree(child: &mut tokio::process::Child) {
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        kill_process_tree(pid).await;
    }
    // 兜底：taskkill 没覆盖到的情形（以及非 Windows 平台）由这里收掉。
    let _ = child.kill().await;
    // ⚠️ kill 之后必须 wait 一次：不回收的话子进程句柄一直留着，
    // 并且 `Child::id()` 之后仍会返回那个已经不存在的 pid。
    let _ = child.wait().await;
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn echo_success() {
        let out = run("cmd.exe", &["/c", "echo", "hello"], &RunnerOptions::new())
            .await
            .unwrap();
        assert!(out.is_success());
        assert_eq!(out.stdout.trim(), "hello");
        assert!(!out.timed_out && !out.cancelled);
    }

    #[tokio::test]
    async fn nonzero_exit_code() {
        let out = run("cmd.exe", &["/c", "exit", "42"], &RunnerOptions::new())
            .await
            .unwrap();
        assert_eq!(out.exit_code, Some(42));
        assert!(!out.is_success());
    }

    #[tokio::test]
    async fn timeout_kills() {
        let opts = RunnerOptions::new().timeout(Duration::from_millis(100));
        let out = run(
            "cmd.exe",
            &["/c", "ping", "-n", "10", "127.0.0.1"],
            &opts,
        )
        .await
        .unwrap();
        assert!(out.timed_out);
        assert!(out.exit_code.is_none());
    }

    #[tokio::test]
    async fn cancel_kills() {
        let cancel = CancelToken::new();
        let cancel2 = cancel.clone();
        let opts = RunnerOptions::new();
        let handle = tokio::spawn(async move {
            run_stream(
                "cmd.exe",
                &["/c", "ping", "-n", "10", "127.0.0.1"],
                &opts,
                cancel2,
                |_| {},
            )
            .await
            .unwrap()
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        cancel.cancel();
        let out = handle.await.unwrap();
        assert!(out.cancelled);
        assert!(out.exit_code.is_none());
    }

    #[tokio::test]
    async fn stream_lines() {
        let opts = RunnerOptions::new();
        let mut lines = Vec::new();
        let out = run_stream(
            "cmd.exe",
            &["/c", "echo", "line1", "&", "echo", "line2"],
            &opts,
            CancelToken::new(),
            |ev| {
                if let RunnerEvent::StdoutLine(l) = ev {
                    lines.push(l);
                }
            },
        )
        .await
        .unwrap();
        assert!(out.is_success());
        assert!(lines.iter().any(|l| l.trim() == "line1"));
        assert!(lines.iter().any(|l| l.trim() == "line2"));
    }

    #[tokio::test]
    async fn stderr_capture() {
        let out = run(
            "cmd.exe",
            &["/c", "echo", "err", "1>&2"],
            &RunnerOptions::new(),
        )
        .await
        .unwrap();
        assert!(out.is_success());
        assert_eq!(out.stderr.trim(), "err");
    }

    /// 回归：父进程退出、孙进程仍攥着 stdout 管道时，收尾**不得被拖住**。
    ///
    /// 这就是"取消按了没用"的真实形状。修之前 `run()` 会被拖到孙进程自然死亡
    /// （实测 11.14s，父进程 exit=0 之后）；修之后应在 `DRAIN_GRACE_NORMAL` 附近返回。
    ///
    /// ⚠️ 故意用 `ping -n 12`（约 11 秒）而不是更长的命令：万一哪天回归了，
    /// 这个测试也只是慢十几秒，而不会把整个测试套件永久挂住。
    #[tokio::test]
    async fn grandchild_holding_pipe_cannot_hang_drain() {
        let t0 = Instant::now();
        let out = run(
            "cmd.exe",
            &["/c", "start", "/b", "ping", "-n", "12", "127.0.0.1"],
            &RunnerOptions::new(),
        )
        .await
        .unwrap();
        let el = t0.elapsed();

        // 父进程自己是正常退出的 —— 与本次修复无关，顺手钉住。
        assert_eq!(out.exit_code, Some(0));
        assert!(!out.cancelled && !out.timed_out);

        assert!(
            el < DRAIN_GRACE_NORMAL + Duration::from_secs(3),
            "收尾被孙进程拖住：elapsed={el:?}（宽限 {DRAIN_GRACE_NORMAL:?}）"
        );
    }

    /// 回归：取消时即使还挂着孙进程，也必须在**亚秒级**返回。
    ///
    /// 这条钉的是"用户按下取消"这个动作的可感知契约 —— 它以前会被拖到
    /// CLI 自己跑完（几分钟）。命令形状刻意与真实 CLI 一致：父进程活着
    /// （`cmd /c` 会等它），同时另起一个后台孙进程持续输出。
    #[tokio::test]
    async fn cancel_returns_promptly_while_grandchild_alive() {
        let cancel = CancelToken::new();
        let c2 = cancel.clone();
        let handle = tokio::spawn(async move {
            run_stream(
                "cmd.exe",
                // 父：`ping -n 20`（约 19 秒，保证取消时父进程还活着）
                // 孙：`start /b ping -n 20`，并行跑满同样的时长
                &[
                    "/c",
                    "start /b ping -n 20 127.0.0.1 & ping -n 20 127.0.0.1",
                ],
                &RunnerOptions::new(),
                c2,
                |_| {},
            )
            .await
            .unwrap()
        });

        tokio::time::sleep(Duration::from_millis(400)).await;
        let t0 = Instant::now();
        cancel.cancel();
        let out = handle.await.unwrap();
        let el = t0.elapsed();

        assert!(out.cancelled, "取消路径必须报 Cancelled");
        assert!(out.exit_code.is_none(), "被取消的进程不该有正常退出码");
        assert!(
            el < Duration::from_secs(3),
            "取消后仍被拖住：{el:?}（说明进程树没被杀干净）"
        );
    }
}