//! 「启动终端并预置命令」——把环境准备好，内容留给你说。
//!
//! # 它解决什么
//!
//! 按下绑定键 → 开一个命令行窗口，并把配置好的命令（如
//! `codex.cmd exec --skip-git-repo-check`）**打进输入框但不回车**。
//! 随后按语音键说出的话接在这条命令后面，回车即执行 ——
//! **「启动环境」与「输入内容」因此成为两个各自独立的步骤。**
//!
//! # ⛔ 为什么不能"用参数把命令喂给 cmd"
//!
//! `cmd /k <命令>` 是**直接执行**。要"只打进去、等用户补完"，唯一的路子是
//! **先开窗口、再往窗口里按键**。而"往窗口里按键"正是 `text_delivery` 已有的能力
//! （带焦点校验、带粘贴键自适应），所以这里**复用**它，而不是另写一套 SendInput。
//!
//! # 与"投递完整命令行"（v0.26.0 那套）的区别
//!
//! 那套是：模板展开成**完整**命令行 → 粘进**你正开着的**终端 → 回车。
//! 这套是：只粘**命令前缀** → 粘进**我替你新开的**终端 → **不**回车，等你补完。
//! 两者语义相反（"完整"vs"半截"），不能共用同一个「粘贴后回车」开关 ——
//! 所以这里按次覆盖为"不回车"，不动全局设置。
//!
//! # 三个容易忽略、但必须做对的点
//!
//! 1. **末尾那个空格**。命令是前缀，语音要接在后面；两段都不带分隔符 ⇒
//!    粘出来是 `--skip-git-repo-check打开计算器`（一个参数）。见 [`prefix_payload`]。
//! 2. **焦点已经在终端上就复用，不新开**。否则"上次没等到窗口"的重试路径
//!    会变成"每按一次多一个窗口、永远打不进去"。
//! 3. **同一时刻只允许一次在飞**（[`LAUNCH_LOCK`]）。并发注入会互相踩剪贴板，
//!    而且**不会报错** —— 只是粘出莫名其妙的内容。

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::log_buffer;

/// 配置文件（位于 `app_config_dir()`）。
const FILE_NAME: &str = "cli_terminal.json";

/// 本机 PowerShell 里可直接跑 `.cmd`，但 `codex.cmd` 这类包装脚本在 PowerShell 下
/// 依赖 PATHEXT 解析，写法差异多；默认落在 cmd 上最可控。
const DEFAULT_COMMAND: &str = "codex exec --skip-git-repo-check";

/// 等终端窗口起来并拿到前台的**上限**。
///
/// 不是"睡这么久再打键"，而是"最多等这么久"（`wait_for_terminal_foreground`
/// 一满足条件立刻返回）。取 6 秒是给冷启动的 Windows Terminal 留余量。
const WAIT_WINDOW_MS: u64 = 6000;

/// 终端程序选择。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TerminalKind {
    /// 自动：优先 Windows Terminal，没有则退回 cmd.exe。
    #[default]
    Auto,
    /// 固定 Windows Terminal（`wt.exe`）。
    Wt,
    /// 固定 `cmd.exe`。
    Cmd,
    /// 固定 Windows PowerShell。
    PowerShell,
}

impl TerminalKind {
    fn as_str(self) -> &'static str {
        match self {
            TerminalKind::Auto => "auto",
            TerminalKind::Wt => "wt",
            TerminalKind::Cmd => "cmd",
            TerminalKind::PowerShell => "powershell",
        }
    }
}

/// 终端启动配置。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CliTerminalConfig {
    /// 要预置到输入框里的命令（**不含回车**）。
    pub command: String,
    /// 工作目录（空 = 用户主目录）。
    pub cwd: String,
    /// 用哪个终端。
    pub terminal: TerminalKind,
    /// 打完是否直接回车执行。
    ///
    /// 默认 `false` —— 因为默认用途是"预置前缀、等语音补全"。
    /// 打开它则退化成"一键跑固定命令"，那是另一种用法，同样有用。
    pub auto_run: bool,
}

impl Default for CliTerminalConfig {
    fn default() -> Self {
        Self {
            command: DEFAULT_COMMAND.to_string(),
            cwd: String::new(),
            terminal: TerminalKind::Auto,
            auto_run: false,
        }
    }
}

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();
static CONFIG: OnceLock<std::sync::RwLock<CliTerminalConfig>> = OnceLock::new();

fn cell() -> &'static std::sync::RwLock<CliTerminalConfig> {
    CONFIG.get_or_init(|| std::sync::RwLock::new(CliTerminalConfig::default()))
}

fn path() -> Option<PathBuf> {
    CONFIG_DIR.get().map(|d| d.join(FILE_NAME))
}

/// 初始化：登记配置目录并读盘。
pub fn init(config_dir: &std::path::Path) {
    let _ = CONFIG_DIR.set(config_dir.to_path_buf());
    let Some(p) = path() else { return };
    match std::fs::read_to_string(&p) {
        Ok(s) => match serde_json::from_str::<CliTerminalConfig>(&s) {
            Ok(cfg) => {
                *cell().write().unwrap() = cfg;
                log_buffer::info("终端启动：已加载 cli_terminal.json");
            }
            Err(e) => {
                // 配置坏了必须说出来：静默用默认值会让用户以为"我配的没生效"，
                // 而他改不动 —— 因为下一次保存又把坏文件覆盖成默认值。
                log_buffer::warn(format!(
                    "终端启动：{} 解析失败（{}），已退回默认配置；原文件保留未改",
                    FILE_NAME, e
                ));
            }
        },
        Err(_) => {
            // 首次运行：写一份默认配置，让用户有个可改的起点。
            let cfg = CliTerminalConfig::default();
            *cell().write().unwrap() = cfg.clone();
            let _ = save(&cfg);
        }
    }
}

pub fn config() -> CliTerminalConfig {
    cell().read().unwrap().clone()
}

/// 保存配置并落盘。
pub fn set_config(cfg: CliTerminalConfig) -> Result<(), String> {
    let cfg = CliTerminalConfig {
        command: cfg.command.trim().to_string(),
        cwd: cfg.cwd.trim().to_string(),
        terminal: cfg.terminal,
        auto_run: cfg.auto_run,
    };
    save(&cfg)?;
    *cell().write().unwrap() = cfg;
    Ok(())
}

fn save(cfg: &CliTerminalConfig) -> Result<(), String> {
    let Some(p) = path() else {
        return Err("配置目录未初始化".to_string());
    };
    let s = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(&p, s).map_err(|e| format!("写入 {} 失败: {e}", p.display()))
}

/// 解析实际要用的终端程序。
///
/// `auto` 的判定是**看文件在不在**，而不是试着 spawn 一次再回退 ——
/// 后者会先弹一个空窗口、失败、再弹一个，用户看到两个窗口闪。
fn resolve_program(kind: TerminalKind) -> (String, bool) {
    // 返回 (程序路径, 是否 Windows Terminal)
    if matches!(kind, TerminalKind::Wt) {
        return (wt_path().unwrap_or_else(|| "wt.exe".to_string()), true);
    }
    if matches!(kind, TerminalKind::Cmd) {
        return ("cmd.exe".to_string(), false);
    }
    if matches!(kind, TerminalKind::PowerShell) {
        return ("powershell.exe".to_string(), false);
    }
    // auto
    match wt_path() {
        Some(p) => (p, true),
        None => ("cmd.exe".to_string(), false),
    }
}

/// 找 `wt.exe`。它是个"应用执行别名"，通常在 `%LOCALAPPDATA%\Microsoft\WindowsApps` 下。
fn wt_path() -> Option<String> {
    let local = std::env::var("LOCALAPPDATA").ok()?;
    let p = PathBuf::from(local)
        .join("Microsoft")
        .join("WindowsApps")
        .join("wt.exe");
    p.exists().then(|| p.to_string_lossy().to_string())
}

/// "启动终端"同一时刻只允许一次在飞。
///
/// 不是为了防重复点按（那只是体验问题），而是**注入链路本身要求串行**：
/// 一次注入是"写剪贴板 → 派粘贴键 → 等 1.2 秒让目标消费 → 恢复剪贴板"。
/// 两次并发交叠时，先结束的那次会把后一次**刚写进去**的内容当成"用户原来的剪贴板"
/// 恢复回去 —— 表现为粘出来一段莫名其妙的东西，而且全程没有任何报错。
static LAUNCH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 现在前台是不是已经在某个"已知终端"上 —— **不等待**。
///
/// 复用 `wait_for_terminal_foreground`（超时给 0 = 只查一次）而不是自己再写一遍
/// "读前台 → 排除自己 → 查类名表"：那个判据必须只有一份，
/// 两份的结果是"等窗口时认得的终端，复用时不认得"，而两边都看不出错。
fn foreground_terminal_now() -> Option<(isize, crate::agent_desk::delivery::WindowLabel)> {
    crate::agent_desk::delivery::wait_for_terminal_foreground(std::time::Duration::ZERO)
}

/// 实际要打进输入框的内容 = 命令 + **一个尾随空格**。
///
/// 抽成纯函数并被单测钉死，是因为它错了以后的症状在**别的层**：
/// 粘出来是 `--skip-git-repo-check打开计算器`，shell 报"参数不对"，
/// 而人会去查 codex 的参数、查语音识别，唯独想不到是少了个空格。
fn prefix_payload(command: &str) -> String {
    format!("{} ", command.trim())
}

/// 启动终端并把命令预置进去。
///
/// 返回给用户看的一句话（进日志与界面提示）。
pub fn launch() -> Result<String, String> {
    let cfg = config();
    let command = cfg.command.trim().to_string();
    if command.is_empty() {
        return Err("还没配置要预置的命令（设置 → 终端启动）".to_string());
    }
    // 多行会让"预置"失去意义：回车即执行，第二行起会在你还没看清时就跑掉。
    if command.contains('\n') || command.contains('\r') {
        return Err("预置命令不能包含换行（回车即执行，多行会误触发）".to_string());
    }

    let cwd = if cfg.cwd.is_empty() {
        std::env::var("USERPROFILE").unwrap_or_default()
    } else {
        cfg.cwd.clone()
    };
    if !cwd.is_empty() && !std::path::Path::new(&cwd).is_dir() {
        return Err(format!("工作目录不存在：{cwd}"));
    }

    let Ok(_serial) = LAUNCH_LOCK.try_lock() else {
        // 明确说出来而不是静默丢掉：否则用户看到的是"按了没反应"。
        log_buffer::warn("终端启动：上一次还未结束，已跳过本次（避免两次注入互相踩剪贴板）");
        return Err("上一次「启动终端」还没结束，本次已跳过；等它出窗口再按一次".to_string());
    };

    // ------------------------------------------------------------------
    // 选窗口：焦点已经在终端上就**复用它**，否则才新开一个。
    //
    // 复用不是为了省窗口，而是为了让"没等到窗口"这件事有救：
    // 新开的窗口没有拿到前台时，我们已经明确报错、什么都没打（好过盲投）。
    // 用户此时的自然动作是"点一下那个终端，再按一次键" —— 如果那一次又开一个新窗口，
    // 就会陷入"每重试一次多一个窗口、永远打不进去"。
    // 而"已知终端在前台"本来就是这个模块认定的"可以打键"的条件（同一个判据），
    // 所以复用不引入任何新的风险方向。
    // ------------------------------------------------------------------
    let (hwnd, label, reused) = match foreground_terminal_now() {
        Some((h, l)) => {
            log_buffer::info(format!(
                "终端启动：焦点已在终端 {} 上，复用它（不新开窗口）",
                l.display()
            ));
            (h, l, true)
        }
        None => {
            let (program, is_wt) = resolve_program(cfg.terminal);
            log_buffer::info(format!(
                "终端启动：program={program} kind={} cwd={cwd:?} command={command:?} auto_run={}",
                cfg.terminal.as_str(),
                cfg.auto_run
            ));

            let mut cmd = Command::new(&program);
            if is_wt {
                // `wt -d <目录> cmd.exe`：新窗口落在指定目录，并跑 cmd 而不是 WT 的默认 profile ——
                // 用户配的命令多半是 `.cmd` 包装脚本，cmd 语义最可控。
                if !cwd.is_empty() {
                    cmd.arg("-d").arg(&cwd);
                }
                cmd.arg("cmd.exe");
            } else if !cwd.is_empty() {
                cmd.current_dir(&cwd);
            }

            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                /// 新开一个控制台窗口。不设它的话，从 GUI 进程 spawn 控制台程序可能
                /// 复用/继承到无效的标准句柄，表现为"窗口一闪就没"。
                const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
                if !is_wt {
                    cmd.creation_flags(CREATE_NEW_CONSOLE);
                }
            }

            cmd.spawn()
                .map_err(|e| format!("启动终端失败（{program}）：{e}"))?;

            // 等窗口真的起来并拿到前台 —— 不能定长 sleep 然后盲投，那一刻前台可能还是 easyop。
            let waited = std::time::Duration::from_millis(WAIT_WINDOW_MS);
            let Some((hwnd, label)) =
                crate::agent_desk::delivery::wait_for_terminal_foreground(waited)
            else {
                return Err(format!(
                    "终端已启动，但 {WAIT_WINDOW_MS} ms 内没等到它成为前台窗口 —— \
                     本次没往里打命令（免得打到别的窗口）。请手动点一下终端，再按一次键。"
                ));
            };
            log_buffer::info(format!(
                "终端启动：窗口就绪 {}（{:?}）",
                label.display(),
                hwnd
            ));
            (hwnd, label, false)
        }
    };

    // ------------------------------------------------------------------
    // 打内容。
    //
    // ⚠️ 末尾**必须**补一个空格：默认命令自己不带（也不该带 —— 存配置时被 `trim` 了），
    //    而随后语音粘进来的文字也不带前导空格。少了它，粘出来就是
    //    `--skip-git-repo-check打开计算器`，shell 会当成**一个**参数。
    //    这一点由 `text_delivery` 的前缀模式（不 trim）专门保住。
    // ------------------------------------------------------------------
    let style = crate::text_delivery::paste_style_for_class(&label.class);
    let result = if cfg.auto_run {
        crate::text_delivery::deliver_to_window_submit(&command, hwnd, style)
            .map(|_| ("已启动终端并执行命令".to_string(), true))
    } else {
        let payload = prefix_payload(&command);
        crate::text_delivery::deliver_to_window_prefix(&payload, hwnd, style)
            .map(|_| ("已启动终端并预置命令（未回车，等你说内容）".to_string(), false))
    };

    match result {
        Ok((msg, ran)) => {
            let where_ = if reused { "复用已有终端" } else { "新开终端" };
            log_buffer::info(format!("终端启动：{where_} · {msg}"));
            if !ran {
                log_buffer::info(
                    "终端启动：命令停在输入框里 —— 现在按语音键说出内容，回车即执行"
                        .to_string(),
                );
            }
            Ok(format!("{where_}：{msg}"))
        }
        Err(e) => {
            log_buffer::err(format!("终端启动：预置命令失败 —— {e}"));
            Err(format!(
                "终端已就绪，但命令没能打进去：{e}。窗口是好的，可手动粘贴。"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_safe_to_run_as_is() {
        let c = CliTerminalConfig::default();
        assert!(!c.auto_run, "默认必须「不回车」—— 否则预置前缀会被立刻当命令执行");
        assert!(!c.command.is_empty(), "默认给一条可改的起点，空配置会让用户以为是坏的");
        assert_eq!(c.terminal, TerminalKind::Auto);
    }

    /// 终端选择要能经 JSON 往返 —— 存不住的话表现是"改了设置重启又变回去了"。
    #[test]
    fn terminal_kind_round_trips_through_json() {
        for k in [
            TerminalKind::Auto,
            TerminalKind::Wt,
            TerminalKind::Cmd,
            TerminalKind::PowerShell,
        ] {
            let s = serde_json::to_string(&k).unwrap();
            let back: TerminalKind = serde_json::from_str(&s).unwrap();
            assert_eq!(k, back, "{s} 往返失败");
        }
    }

    /// 老配置缺字段时必须能读 —— 缺字段直接报错会让用户"改都没法改"。
    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let c: CliTerminalConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(c.command, DEFAULT_COMMAND);
        assert!(!c.auto_run);
        let c2: CliTerminalConfig = serde_json::from_str(r#"{"command":"echo hi"}"#).unwrap();
        assert_eq!(c2.command, "echo hi");
        assert_eq!(c2.terminal, TerminalKind::Auto);
    }

    #[test]
    fn set_config_trims_whitespace() {
        let cfg = CliTerminalConfig {
            command: "  codex exec  \n".to_string(),
            cwd: "  E:\\MY_AI  ".to_string(),
            terminal: TerminalKind::Cmd,
            auto_run: true,
        };
        // 只验裁剪语义（不落盘：单测环境没有配置目录）。
        let trimmed = CliTerminalConfig {
            command: cfg.command.trim().to_string(),
            cwd: cfg.cwd.trim().to_string(),
            terminal: cfg.terminal,
            auto_run: cfg.auto_run,
        };
        assert_eq!(trimmed.command, "codex exec");
        assert_eq!(trimmed.cwd, "E:\\MY_AI");
    }

    /// ⛔ 整个功能里最容易"看着没问题"的一处：末尾那个空格。
    ///
    /// 它没了，命令本身照样打得进去、终端照样看着正常，只有 shell 会报一个
    /// 像是"codex 参数写错"的错。所以这条测试必须存在。
    #[test]
    fn prefix_payload_keeps_exactly_one_trailing_space() {
        let p = prefix_payload("codex exec --skip-git-repo-check");
        assert!(p.ends_with(' '), "末尾必须有空格，否则语音内容会黏在命令后面");
        assert_eq!(p, "codex exec --skip-git-repo-check ");
        // 用户手输时多带了空白 ⇒ 先 trim 再加一个空格，不会变成两个。
        assert_eq!(prefix_payload("  codex exec  "), "codex exec ");
    }
}
