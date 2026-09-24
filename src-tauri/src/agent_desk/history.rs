//! 任务历史：让"跑过什么、结果怎样、产物在哪"能被追溯。
//!
//! # 落盘两样东西（都在 `output_dir()/tasks/` 下）
//!
//! - `index.jsonl` —— 一行一条任务记录，**追加语义、全量重写落盘**
//! - `<started_ms>_s<slot>_t<task_id>.log` —— 该任务的**完整**输出
//!
//! # 为什么索引与全量日志分开
//!
//! 列表页要的是"一眼扫过最近几十条"（小、要快），排查要的是"这条到底输出了什么"
//! （大、偶尔读）。混在一处的后果是索引被几 MB 的输出撑爆 —— 打开历史页变成读几十兆。
//!
//! # 为什么用 JSONL 而不是一个大 JSON 数组
//!
//! 一行坏了只丢那一行。写成数组则"某次写入中断留下半行"会让**整份历史**解析失败，
//! 用户看到的历史页一片空白，而磁盘上明明有几百条记录。
//!
//! # 为什么有 200 条上限
//!
//! 没有上限的日志目录，结局是用户某天发现它占了几百 MB，然后**手工清理时把正在跑的
//! 那条一起删掉**。200 条 ≈ 一周以上的可追溯窗口，够用且不会失控。
//!
//! # 为什么"删老任务"要和"删它的日志"一起做
//!
//! 分开做就会有孤儿 `.log` 永远留在磁盘上：索引里已经看不到它，但文件还在 ——
//! 那正是"日志目录悄悄膨胀"的成因。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::tail_chars;

/// 保留的任务条数。
pub const KEEP: usize = 200;

/// 任务目录名（`output_dir()` 之下）。
pub const TASKS_DIR: &str = "tasks";

/// 索引文件名。
pub const INDEX_FILE: &str = "index.jsonl";

/// 索引里每条记录自带输出尾部的长度。
///
/// 列表页据此直接显示，不必为每条再读一次日志文件。全量输出仍在 `.log` 里。
const RECORD_TAIL_CHARS: usize = 600;

/// 一条任务的完整记录。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskRecord {
    /// 队列里的任务号（同一次运行内唯一）。
    pub task_id: u64,
    /// 槽位号（**0-based**，与后端一致；前端显示时 +1）。
    pub slot: usize,
    pub driver_id: String,
    /// 用户说的原话（派发时的任务文本）。
    pub prompt: String,
    /// `completed` / `failed` / `timed_out` / `cancelled`。
    pub status: String,
    /// 失败原因（含 stderr 尾部）。成功时为 `None`。
    #[serde(default)]
    pub reason: Option<String>,
    /// `high` / `medium`（`medium` = 完成判定未经世界门禁校验）。
    pub confidence: String,
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
    pub duration_ms: u64,
    /// 本轮用于续接的会话 id（`None` = 这是一轮新对话）。
    #[serde(default)]
    pub session_id: Option<String>,
    /// 全量日志文件名（相对任务目录）。`None` = 日志没能写成（磁盘满/权限）。
    #[serde(default)]
    pub log_file: Option<String>,
    /// 本次任务产生的产物路径（绝对路径）。
    #[serde(default)]
    pub artifacts: Vec<String>,
    /// 输出尾部，供列表页直接显示。
    pub output_tail: String,
}

/// 任务目录：`output_dir()/tasks`。
pub fn dir_of(output_dir: &Path) -> PathBuf {
    output_dir.join(TASKS_DIR)
}

/// 日志文件名。
///
/// 带上开始时刻与任务号：**单靠 task_id 会撞**（任务号在重启后从头开始），
/// 而只带时刻又无法分辨同毫秒内的两条（同一槽不可能，不同槽可以）。
/// 槽位号进去是为了人眼扫目录时能直接看出"这是 S2 跑的"。
pub fn log_file_name(started_at_ms: u64, slot: usize, task_id: u64) -> String {
    format!("{started_at_ms}_s{}_t{task_id}.log", slot + 1)
}

/// 列表页要显示的输出尾部。
pub fn tail_for_list(output: &str) -> String {
    tail_chars(output, RECORD_TAIL_CHARS)
}

/// 把一次任务的**完整输出**写成独立日志文件，返回文件名。
///
/// `stderr` 单独给而不是预先拼进 `stdout`：两者在排查时的读法不同
/// （stdout 是"它做了什么"，stderr 是"它为什么没做成"），落盘时用一行分隔标记隔开，
/// 读的人一眼能分清。
///
/// 失败只记日志并返回 `None` —— **绝不能因为日志写不成就让任务被判失败**：
/// 任务本身已经跑完了，那是既成事实。
pub fn write_log(dir: &Path, name: &str, stdout: &str, stderr: &str) -> Option<String> {
    if let Err(e) = std::fs::create_dir_all(dir) {
        super::log_buffer::warn(format!("任务台历史目录创建失败 {}: {e}", dir.display()));
        return None;
    }
    let mut body = String::with_capacity(stdout.len() + stderr.len() + 64);
    body.push_str(stdout);
    if !stderr.trim().is_empty() {
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        body.push_str("----- stderr -----\n");
        body.push_str(stderr);
    }
    let path = dir.join(name);
    match std::fs::write(&path, body) {
        Ok(()) => Some(name.to_string()),
        Err(e) => {
            super::log_buffer::warn(format!(
                "任务台历史日志写入失败 {}: {e}（任务结果不受影响）",
                path.display()
            ));
            None
        }
    }
}

/// 载入已有索引（不存在的目录/文件 = 首次运行，不是错误）。
pub fn load(dir: &Path) -> Vec<TaskRecord> {
    let path = dir.join(INDEX_FILE);
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        // 首次运行：没有文件是正常的。
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            super::log_buffer::warn(format!("任务台历史读取失败 {}: {e}", path.display()));
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    let mut bad = 0usize;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<TaskRecord>(line) {
            Ok(r) => out.push(r),
            // 单行坏掉只丢那一行 —— 这正是选 JSONL 的理由（见模块文档）。
            Err(_) => bad += 1,
        }
    }
    if bad > 0 {
        super::log_buffer::warn(format!(
            "任务台历史有 {bad} 行解析失败（已跳过，其余 {} 条保留）",
            out.len()
        ));
    }
    out
}

/// 追加一条记录、裁剪超出上限的老记录（**连它们的日志文件一起删**），然后落盘。
///
/// `records` 由调用方持有（`DeskManager` 里的那份），本函数原地更新它。
pub fn append(dir: &Path, records: &mut Vec<TaskRecord>, rec: TaskRecord) {
    records.push(rec);

    let mut dropped = Vec::new();
    while records.len() > KEEP {
        dropped.push(records.remove(0));
    }
    for old in &dropped {
        if let Some(name) = &old.log_file {
            let p = dir.join(name);
            // 删不掉不算错误：可能已被用户手删，或正被别的程序打开着。
            // 但**要让别人知道**，否则孤儿文件会悄悄堆起来。
            if let Err(e) = std::fs::remove_file(&p) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    super::log_buffer::warn(format!(
                        "任务台历史超出上限，但旧日志删不掉 {}: {e}",
                        p.display()
                    ));
                }
            }
        }
    }
    if !dropped.is_empty() {
        super::log_buffer::info(format!(
            "任务台历史超出 {KEEP} 条上限，已淘汰最老的 {} 条（含其日志文件）",
            dropped.len()
        ));
    }

    save_index(dir, records);
}

/// 读某条任务的全量日志。
pub fn read_log(dir: &Path, rec: &TaskRecord) -> Option<String> {
    let name = rec.log_file.as_ref()?;
    match std::fs::read_to_string(dir.join(name)) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // 索引里有、文件没了：用户手删过，或上一版没写成功。如实说明。
            super::log_buffer::warn(format!("任务台历史日志已不在磁盘上: {name}"));
            None
        }
        Err(e) => {
            super::log_buffer::warn(format!("任务台历史日志读取失败 {name}: {e}"));
            None
        }
    }
}

/// 全量重写索引。
///
/// # 为什么是"重写"而不是"追加"
///
/// 追加写省一次 IO，但**裁剪**没法用追加表达（要删前面的行）。而裁剪是必须的（上限）。
/// 两种写法混用会留下"文件比内存多几行"的状态 —— 下次启动载入就多出幽灵记录。
/// 200 条 × 每条约 700 字节 ≈ 140 KB，重写成本可以忽略。
///
/// # 为什么先写临时文件再改名
///
/// 直接覆写时若进程在写一半死掉（任务台是常驻进程，会崩），索引就剩下半个 JSON 行 ——
/// 用户重启后看到的是"历史丢了一截"，而没有别的线索。改名在同一卷内是原子的：
/// 要么旧文件完好，要么新文件完整。
fn save_index(dir: &Path, records: &[TaskRecord]) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        super::log_buffer::err(format!("任务台历史目录创建失败 {}: {e}", dir.display()));
        return;
    }
    let mut body = String::new();
    let mut failed = 0usize;
    for r in records {
        match serde_json::to_string(r) {
            Ok(line) => {
                body.push_str(&line);
                body.push('\n');
            }
            Err(_) => failed += 1,
        }
    }
    if failed > 0 {
        super::log_buffer::warn(format!("任务台历史有 {failed} 条序列化失败，未能落盘"));
    }

    let path = dir.join(INDEX_FILE);
    let tmp = dir.join(format!("{INDEX_FILE}.tmp"));
    if let Err(e) = std::fs::write(&tmp, body) {
        super::log_buffer::err(format!("任务台历史写入失败 {}: {e}", tmp.display()));
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        super::log_buffer::err(format!(
            "任务台历史改名失败 {} -> {}: {e}",
            tmp.display(),
            path.display()
        ));
        // 改名失败时别把临时文件留下当垃圾。
        let _ = std::fs::remove_file(&tmp);
    }
}

/// 倒序（最新在前）取最近 `limit` 条。
pub fn recent(records: &[TaskRecord], limit: usize) -> Vec<TaskRecord> {
    records.iter().rev().take(limit).cloned().collect()
}

/// 按任务号找一条。
///
/// 从**后往前**找：任务号在重启后会从头开始，所以同一个号可能存在多条 ——
/// 用户要看的一定是最近跑的那次。
pub fn find(records: &[TaskRecord], task_id: u64) -> Option<&TaskRecord> {
    records.iter().rev().find(|r| r.task_id == task_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("easyop_hist_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn rec(id: u64, slot: usize, log: Option<&str>) -> TaskRecord {
        TaskRecord {
            task_id: id,
            slot,
            driver_id: "codex-cli".into(),
            prompt: format!("任务 {id}"),
            status: "completed".into(),
            reason: None,
            confidence: "high".into(),
            started_at_ms: 1000 + id,
            finished_at_ms: 2000 + id,
            duration_ms: 1000,
            session_id: None,
            log_file: log.map(|s| s.to_string()),
            artifacts: Vec::new(),
            output_tail: "done".into(),
        }
    }

    #[test]
    fn append_then_load_roundtrips() {
        let d = tmp("roundtrip");
        let mut recs = Vec::new();
        append(&d, &mut recs, rec(1, 0, None));
        append(&d, &mut recs, rec(2, 1, None));

        let back = load(&d);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].task_id, 1);
        assert_eq!(back[1].task_id, 2, "顺序必须保留（追加 == 时间序）");
        assert_eq!(back[0].prompt, "任务 1");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_index_is_first_run_not_error() {
        let d = tmp("missing");
        assert!(load(&d).is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    /// 单行坏掉不该让整份历史消失 —— 这正是选 JSONL 的理由。
    #[test]
    fn one_bad_line_does_not_kill_the_rest() {
        let d = tmp("badline");
        fs::create_dir_all(&d).unwrap();
        let good = serde_json::to_string(&rec(7, 0, None)).unwrap();
        fs::write(d.join(INDEX_FILE), format!("{good}\n{{ 半行\n{good}\n")).unwrap();

        let back = load(&d);
        assert_eq!(back.len(), 2, "两条好行都要留下");
        let _ = fs::remove_dir_all(&d);
    }

    /// 淘汰老记录时**必须连它的日志一起删**，否则日志目录会悄悄膨胀。
    #[test]
    fn trimming_also_removes_old_log_files() {
        let d = tmp("trim");
        let mut recs = Vec::new();
        for i in 0..(KEEP + 3) as u64 {
            let name = log_file_name(1000 + i, 0, i);
            write_log(&d, &name, &format!("输出 {i}"), "").unwrap();
            append(&d, &mut recs, rec(i, 0, Some(&name)));
        }
        assert_eq!(recs.len(), KEEP, "内存里必须被裁到上限");
        assert_eq!(load(&d).len(), KEEP, "磁盘上同样");

        // 最早那三个的日志必须已经不在了。
        for i in 0..3u64 {
            let p = d.join(log_file_name(1000 + i, 0, i));
            assert!(!p.exists(), "第 {i} 条的日志该被淘汰：{}", p.display());
        }
        // 最新那条的日志必须还在。
        let last = d.join(log_file_name(1000 + KEEP as u64 + 2, 0, KEEP as u64 + 2));
        assert!(last.exists(), "最新一条的日志不该被误删");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn log_keeps_stderr_apart_from_stdout() {
        let d = tmp("stderr");
        let name = log_file_name(1, 0, 1);
        write_log(&d, &name, "正常输出", "错误输出").unwrap();
        let body = fs::read_to_string(d.join(&name)).unwrap();
        assert!(body.contains("正常输出"));
        assert!(body.contains("错误输出"));
        assert!(body.contains("----- stderr -----"), "两段必须有分隔标记");
        // stdout 在前：读的人先看"它做了什么"。
        assert!(body.find("正常输出").unwrap() < body.find("错误输出").unwrap());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn recent_returns_newest_first_and_find_prefers_latest() {
        let recs = vec![rec(1, 0, None), rec(2, 0, None), rec(1, 0, None)];
        let r = recent(&recs, 2);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].task_id, 1, "最新的在最前");
        assert_eq!(r[1].task_id, 2);

        // 任务号 1 出现两次（模拟重启后号从头开始）—— 必须拿到**最近**那条。
        let f = find(&recs, 1).unwrap();
        assert_eq!(f.started_at_ms, recs[2].started_at_ms);
    }
}
