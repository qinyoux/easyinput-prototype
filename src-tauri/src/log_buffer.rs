//! 全局环形日志缓冲：供前端「日志」面板实时拉取调试信息。
//!
//! 设计取舍：不引入 `log` crate + 自定义 logger 的复杂度，而是用一块固定容量
//! 的 `VecDeque` 做环形缓冲。各模块在关键节点调用 `push`，前端通过 `get_logs`
//! 命令按序号增量轮询（500ms），实现「实时调试」而无需事件总线的依赖。
//!
//! 时间戳：用 `SystemTime` 手算 UTC+8 时分秒（项目仅面向 Windows / Asia/Shanghai），
//! 避免引入 chrono 依赖。精度到毫秒，调试足够。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// 缓冲区容量；超出后丢弃最早条目，避免内存无界增长。
const CAP: usize = 1000;

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

fn logs() -> &'static RwLock<VecDeque<LogEntry>> {
    LOGS.get_or_init(|| RwLock::new(VecDeque::with_capacity(CAP)))
}

/// 写一条日志。级别约定：`info` 普通流程、`warn` 可疑/降级、`err` 失败。
pub fn push(level: &str, msg: impl Into<String>) {
    let msg: String = msg.into();
    // 镜像到 stderr，便于在 `tauri dev` 终端直接观察（前端面板走增量轮询）。
    eprintln!("[{level}] {msg}");
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let entry = LogEntry {
        seq,
        ts: now_str(),
        level: level.into(),
        msg,
    };
    let mut g = logs().write().unwrap();
    if g.len() >= CAP {
        g.pop_front();
    }
    g.push_back(entry);
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

/// 清空缓冲区（前端「清空」按钮调用）。
pub fn clear() {
    let mut g = logs().write().unwrap();
    g.clear();
}

/// 本地时间戳（UTC+8 / Asia/Shanghai）。不含日期，调试够用。
fn now_str() -> String {
    let dur = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs() as i64 + 8 * 3600;
    let ms = dur.subsec_millis();
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}
