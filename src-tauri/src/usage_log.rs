//! 使用记录存储层 —— 追加写 JSONL，供工作台"使用记录中心"查询。
//!
//! 每条记录：{ts, kind, text, source, profile}
//! - ts: ISO 8601 带时区（如 "2026-09-19T14:30:15+08:00"）
//! - kind: "asr" / "ss_exec" / "prompt_use" / "key_action"
//! - text: 识别文本 / 命令名 / 提示词内容 / 动作名
//! - source: "keyboard" / "palette" / "workbench" / "hardware"
//! - profile: 当前方案名（可选）
//!
//! 存储路径：app_config_dir()/usage_log.jsonl（追加写，按行 JSON）
//! 并发安全：Mutex 保护写入

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::log_buffer;

/// 使用记录条目。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageEntry {
    /// ISO 8601 时间戳，如 "2026-09-19T14:30:15+08:00"
    pub ts: String,
    /// 记录类型："asr" / "ss_exec" / "prompt_use" / "key_action"
    pub kind: String,
    /// 正文：识别文本 / 命令名 / 提示词内容 / 动作名
    pub text: String,
    /// 触发来源："keyboard" / "palette" / "workbench" / "hardware"
    pub source: String,
    /// 当前方案名（可选）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

/// 查询参数：按日期范围过滤。
#[derive(Deserialize)]
pub struct UsageQuery {
    /// 起始日期 "YYYY-MM-DD"（含）
    pub from: String,
    /// 结束日期 "YYYY-MM-DD"（含）
    pub to: String,
}

/// 日历统计：某天有多少条记录。
#[derive(Serialize)]
pub struct DailyCount {
    /// 日期 "YYYY-MM-DD"
    pub date: String,
    /// 记录数
    pub count: usize,
    /// 按类型分项计数
    pub by_kind: HashMap<String, usize>,
}

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();
static WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// 初始化使用记录存储（启动时调用）。
pub fn init(config_dir: &Path) {
    let _ = CONFIG_DIR.set(config_dir.to_path_buf());
}

fn log_path() -> Option<PathBuf> {
    CONFIG_DIR.get().map(|d| d.join("usage_log.jsonl"))
}

fn write_lock() -> &'static Mutex<()> {
    WRITE_LOCK.get_or_init(|| Mutex::new(()))
}

/// 记录一条使用日志（追加写）。
pub fn record(kind: &str, text: &str, source: &str, profile: Option<&str>) {
    let entry = UsageEntry {
        ts: now_iso8601(),
        kind: kind.to_string(),
        text: text.to_string(),
        source: source.to_string(),
        profile: profile.map(|s| s.to_string()),
    };

    let path = match log_path() {
        Some(p) => p,
        None => {
            log_buffer::warn("使用记录未初始化，跳过写入");
            return;
        }
    };

    let _guard = write_lock().lock().unwrap();
    let line = serde_json::to_string(&entry).unwrap_or_default();
    if let Err(e) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, format!("{line}\n").as_bytes()))
    {
        log_buffer::warn(format!("使用记录写入失败: {e}"));
    }
}

/// 当前时间 ISO 8601 带时区（UTC+8 / Asia/Shanghai）。
fn now_iso8601() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = dur.as_secs() as i64 + 8 * 3600;
    let ms = dur.subsec_millis();

    let secs = total_secs % 60;
    let mins = (total_secs / 60) % 60;
    let hours = (total_secs / 3600) % 24;
    let days = total_secs / 86400;

    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{mins:02}:{secs:02}.{ms:03}+08:00")
}

/// 天数 → 年月日（从 1970-01-01 起）。
fn days_to_ymd(days: i64) -> (i64, i64, i64) {
    let mut remaining = days;
    let mut year = 1970i64;

    loop {
        let yd = if is_leap(year) { 366 } else { 365 };
        if remaining < yd {
            break;
        }
        remaining -= yd;
        year += 1;
    }

    let leap = is_leap(year);
    let mdays = [
        31i64,
        if leap { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];

    let mut month = 1i64;
    for &md in &mdays {
        if remaining < md {
            break;
        }
        remaining -= md;
        month += 1;
    }

    (year, month, remaining + 1)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

// ── Tauri commands ──

/// 查询使用记录（按日期范围）。
#[tauri::command]
pub fn usage_log_query(query: UsageQuery) -> Result<Vec<UsageEntry>, String> {
    let path = log_path().ok_or("使用记录未初始化")?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("读取使用记录失败: {e}"))?;
    let entries: Vec<UsageEntry> = content
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<UsageEntry>(l).ok())
        .filter(|e: &UsageEntry| {
            if e.ts.len() < 10 {
                return false;
            }
            let date = &e.ts[..10];
            date >= query.from.as_str() && date <= query.to.as_str()
        })
        .collect();

    Ok(entries)
}

/// 日历统计：每天的记录数与类型分布。
#[tauri::command]
pub fn usage_log_daily_counts(query: UsageQuery) -> Result<Vec<DailyCount>, String> {
    let entries = usage_log_query(query)?;

    let mut map: BTreeMap<String, DailyCount> = BTreeMap::new();
    for e in entries {
        let date = e.ts[..10].to_string();
        let dc = map.entry(date.clone()).or_insert(DailyCount {
            date,
            count: 0,
            by_kind: HashMap::new(),
        });
        dc.count += 1;
        *dc.by_kind.entry(e.kind.clone()).or_insert(0) += 1;
    }

    Ok(map.into_values().collect())
}

// ── 测试 ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_to_ymd_epoch() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_2026() {
        // 2026-09-19 距 1970-01-01 的天数
        let days = (2026 - 1970) * 365 + 14; // 14 个闰年
        let (y, m, d) = days_to_ymd(days);
        assert_eq!(y, 2026);
        assert_eq!((m, d), (1, 1));
    }

    #[test]
    fn is_leap_basic() {
        assert!(is_leap(2000));
        assert!(!is_leap(1900));
        assert!(is_leap(2024));
        assert!(!is_leap(2023));
    }

    #[test]
    fn now_iso8601_format() {
        let s = now_iso8601();
        assert!(s.contains("T"));
        assert!(s.ends_with("+08:00"));
        assert_eq!(s.len(), 29);
    }
}