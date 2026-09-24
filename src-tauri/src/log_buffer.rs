//! 全局日志：内存环形缓冲（供前端「日志」面板实时拉取）+ **按天落盘**（供事后复盘）。
//!
//! # 为什么必须有落盘这一半
//!
//! 从前这里只存内存（容量 1000 条），关掉应用就全没了。代价在一次真实故障里付清了：
//! 语音识别整条链路失效，而**事故当时一行日志都拿不到** —— 只能靠推理和外部探测，
//! 排查成本成倍上升。日志的价值恰恰在"出事的那个时候"，那一份拿不到，等于没有日志。
//!
//! 落盘策略（有意选保守的一组）：
//!   - 路径：`app_config_dir()/logs/easyop-YYYY-MM-DD.log`（按天分文件，方便只发当天的一份）
//!   - 写入：**每条 flush**。宁可多几次系统调用，也不接受崩溃时丢掉最后几行 ——
//!     而最后几行通常正是崩溃原因。
//!   - 轮转：跨天自动换文件；启动时清理 14 天前的旧文件（避免无限增长）。
//!   - 失败：写不进去就静默降级（stderr 照旧），绝不因为日志问题影响主流程。
//!
//! 时间戳：用 `SystemTime` 手算 UTC+8（项目仅面向 Windows / Asia/Shanghai），
//! 不引入 chrono。内存条目沿用 `HH:MM:SS.mmm`，文件里写完整的 `YYYY-MM-DD HH:MM:SS.mmm`。

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// 内存缓冲容量；超出后丢弃最早条目，避免内存无界增长。
///
/// 从 1000 提到 4000：一次完整的语音故障（建连 + 上行 + 回包 + 收尾）就有几十条，
/// 1000 条只够装十来次；排查时常用的动作是"复现一次，然后从头看"，容量不够会把开头挤掉。
const CAP: usize = 4000;

/// 日志目录名（位于 `app_config_dir()` 下）。
const LOG_DIR_NAME: &str = "logs";

/// 日志文件保留天数。
const KEEP_DAYS: u64 = 14;

static SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Clone, Debug)]
pub struct LogEntry {
    /// 单调递增序号；前端据此增量拉取，避免重复。
    pub seq: u64,
    /// 本地时间戳 `HH:MM:SS.mmm`。
    pub ts: String,
    /// 级别：`info` / `warn` / `err`。
    pub level: String,
    /// 日志正文。
    pub msg: String,
}

static LOGS: OnceLock<RwLock<VecDeque<LogEntry>>> = OnceLock::new();
static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
/// 当前打开的日志文件 + 它对应的日期（跨天要换文件）。
static SINK: OnceLock<Mutex<Option<(String, File)>>> = OnceLock::new();

fn logs() -> &'static RwLock<VecDeque<LogEntry>> {
    LOGS.get_or_init(|| RwLock::new(VecDeque::with_capacity(CAP)))
}

fn sink() -> &'static Mutex<Option<(String, File)>> {
    SINK.get_or_init(|| Mutex::new(None))
}

/// 初始化日志目录（启动时调用一次）。顺带清理过期文件。
pub fn init(config_dir: &Path) {
    let dir = config_dir.join(LOG_DIR_NAME);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        // 落盘失败不影响主流程；stderr 上还能看到。
        eprintln!("[warn] 无法创建日志目录 {}: {e}", dir.display());
        return;
    }
    prune_old(&dir);
    let _ = LOG_DIR.set(dir);
}

/// 日志目录（供"打开日志目录"用）。未初始化时返回 None。
pub fn dir() -> Option<PathBuf> {
    LOG_DIR.get().cloned()
}

/// 清理超过 `KEEP_DAYS` 天的日志文件。
fn prune_old(dir: &Path) {
    let cutoff = SystemTime::now() - Duration::from_secs(KEEP_DAYS * 86400);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with("easyop-") || !name.ends_with(".log") {
            continue;
        }
        let too_old = e
            .metadata()
            .and_then(|m| m.modified())
            .map(|t| t < cutoff)
            .unwrap_or(false);
        if too_old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// 写一条日志。级别约定：`info` 普通流程、`warn` 可疑/降级、`err` 失败。
pub fn push(level: &str, msg: impl Into<String>) {
    let msg: String = msg.into();
    // 镜像到 stderr，便于在 `tauri dev` 终端直接观察（前端面板走增量轮询）。
    eprintln!("[{level}] {msg}");

    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let entry = LogEntry {
        seq,
        ts: now_local(false),
        level: level.into(),
        msg: msg.clone(),
    };
    {
        let mut g = logs().write().unwrap();
        if g.len() >= CAP {
            g.pop_front();
        }
        g.push_back(entry);
    }
    append_disk(level, &msg);
}

/// 追加到当天日志文件。任何失败都静默降级 —— 日志不该有能力搞坏主流程。
fn append_disk(level: &str, msg: &str) {
    let Some(dir) = LOG_DIR.get() else {
        return; // 未初始化（如单测环境）⇒ 只留内存
    };
    let full = now_local(true);
    let date = full[..10].to_string();

    let mut guard = sink().lock().unwrap();
    // 跨天（或首次）换文件。
    let need_open = !matches!(guard.as_ref(), Some((d, _)) if *d == date);
    if need_open {
        let path = dir.join(format!("easyop-{date}.log"));
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(f) => *guard = Some((date.clone(), f)),
            Err(e) => {
                eprintln!("[warn] 无法打开日志文件 {}: {e}", path.display());
                *guard = None;
                return;
            }
        }
    }
    if let Some((_, f)) = guard.as_mut() {
        // 每条 flush：崩在下一秒时，"最后一句话"就是原因，不能丢在缓冲区里。
        let _ = writeln!(f, "{full} [{level}] {msg}");
        let _ = f.flush();
    }
}

pub fn info(msg: impl Into<String>) {
    push("info", msg);
}
pub fn warn(msg: impl Into<String>) {
    push("warn", msg);
}
pub fn err(msg: impl Into<String>) {
    push("err", msg);
}

/// 返回序号严格大于 `since` 的所有条目（按写入顺序）。
pub fn since(seq: u64) -> Vec<LogEntry> {
    let g = logs().read().unwrap();
    g.iter().filter(|e| e.seq > seq).cloned().collect()
}

/// 清空**内存**缓冲（前端「清空」按钮调用）。磁盘文件不动 —— 那是复盘用的档案。
pub fn clear() {
    let mut g = logs().write().unwrap();
    g.clear();
}

/// 本地时间戳（UTC+8 / Asia/Shanghai）。`full=true` 带日期，用于落盘。
fn now_local(full: bool) -> String {
    let dur = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs() as i64 + 8 * 3600;
    let ms = dur.subsec_millis();
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (h, m, s) = (rem / 3600, (rem / 60) % 60, rem % 60);
    if !full {
        return format!("{h:02}:{m:02}:{s:02}.{ms:03}");
    }
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}.{ms:03}")
}

/// 由"1970-01-01 起的天数"求公历日期。
///
/// 手算而不引 chrono：项目只需要"今天几号"用来给日志分文件，为此拉一个时间库不值得。
/// 算法是 Howard Hinnant 的 `civil_from_days`（era 分段，对负数天数同样正确）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 日期换算错了不会报错，只会把日志写到 `easyop-1970-01-01.log` 这种地方 ——
    /// 于是"打开日志目录看不到今天的内容"。所以钉住几个已知日期。
    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_000), (2022, 1, 8));
        // 2026-09-21 = 1970-01-01 起第 20717 天
        assert_eq!(civil_from_days(20_717), (2026, 9, 21));
        // 闰日
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn full_timestamp_has_date_and_time_in_order() {
        let s = now_local(true);
        assert_eq!(s.len(), 23, "YYYY-MM-DD HH:MM:SS.mmm 共 23 字符: {s}");
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[10..11], " ");
        assert_eq!(&s[13..14], ":");
        // 落盘时用 s[..10] 当文件名日期，格式必须稳定。
        let d = &s[..10];
        assert!(d.starts_with("20"), "日期段应形如 2026-09-21: {d}");
    }

    #[test]
    fn short_timestamp_stays_compact_for_the_ui() {
        let s = now_local(false);
        assert_eq!(s.len(), 12, "HH:MM:SS.mmm 共 12 字符: {s}");
    }
}
