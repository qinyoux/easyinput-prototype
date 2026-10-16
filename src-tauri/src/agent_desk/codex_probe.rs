//! C · 外部会话监工 —— 监视 codex 的 rollout 日志，把"任务完成了"这件事接进通知层。
//!
//! # 为什么要有这一层
//!
//! 用户实际的工作方式常常是：**自己开一个终端跑 codex**（能看到思考过程、能看到
//! 输出文档），而不是把它交给任务台在后台 headless 地跑。这条路的唯一缺口是
//! **用户走开的那些时刻** —— 结果就在屏幕上，但没人告诉他"可以回来看了"。
//!
//! 本模块补的正是这一个缺口：盯 `~/.codex/sessions/**/rollout-*.jsonl`，
//! 见到 `task_complete` 就回一个"命中"。
//!
//! # 它**不做什么**（刻意）
//!
//! - **不产生未读**。未读是为"结果在一个你看不见的地方"设计的；这里的结果
//!   **就在你正开着的终端里**，永久驻留。再给一颗"有结果没听"的灯，是重复记账。
//! - **不解析完整会话**。只认 `task_complete` 一种事件，其它一概不看。
//! - **不做任何 IO 之外的决定**。它只回答"有哪些新命中"，播不播、播什么由 `mod.rs` 定。
//!
//! # 三个必须做对的点（都曾是想错的）
//!
//! 1. **首见一律按 EOF 登记、不读内容**。否则用户重启一次 easyop，就会把历史上
//!    几十个 rollout 文件里的 `task_complete` 全部当成"刚刚完成"，喇叭连播几十句。
//!    更毒的是**文件复活**：一个 23 天前写完的旧 rollout 跌出 `seen` 回收表后，
//!    被任务台自己的 `codex exec resume` 追加写一笔 → mtime 变新 → 重新进入窗口 →
//!    被当成"全新文件"从偏移 0 整读 → 18 个历史 `task_complete` 全变成新命中，
//!    任务**刚开始**就播"任务已完成"（v0.41.15 实锤误报）。
//!    代价是"开 easyop 之前 1 秒跑完的那个"会被漏掉 —— 漏掉远比误报好。
//! 2. **偏移只推进到最后一个完整行的末尾**。rollout 是被 codex **持续追加**的，
//!    扫到的最后一行常常只写了一半。若把半行也计进偏移，那个事件就永远读不到了。
//! 3. **先用字符串预筛，再 JSON 解析**。**不预筛的话要对几千行逐行 `serde_json`**：
//!    一个 rollout 文件动辄数千行，5 秒扫一次会白烧 CPU。

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// 只监控"最近这段时间里被写过"的会话文件。
///
/// 理由：用户可能积累了成千上万个历史 rollout 文件，每次心跳都遍历一遍既慢
/// 又无意义 —— 一个三个月前写完的文件**不可能**再冒出新事件。
/// 代价：会话连续跑超过这个窗口才会被漏掉（极少，且漏掉的表现是"没播报"，不是误报）。
pub const PROBE_WINDOW_MS: u64 = 24 * 60 * 60 * 1000;

/// 摘要最多取末尾多少字符。
///
/// `last_agent_message` 可能有几万字，而它只用于日志与界面字幕，
/// 全量搬进内存没有收益。取**末尾**：agent 的最终回复通常把结论写在最后。
const MAX_SUMMARY_CHARS: usize = 600;

/// 一次扫描最多读多少字节的新内容（防止某个文件被异常撑大时卡住心跳）。
const MAX_SCAN_BYTES: u64 = 4 * 1024 * 1024;

/// 一次命中。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeHit {
    /// 摘要（取自 `last_agent_message` 的末尾）。**可能为空** —— 事件里没有就如实为空。
    pub summary: String,
    /// 会话轮次 ID，用于日志里把同一轮对上。
    pub turn_id: String,
    /// 来自哪个文件（只留文件名，不含完整路径 —— 完整路径没有信息量且不便读）。
    pub source: String,
    /// 发现时刻（**不是**事件发生时刻，见 `scan` 的说明）。
    pub at_ms: u64,
}

/// 增量扫描器。
///
/// 持有每个已见文件的读取偏移，因此**必须长期存活**（放在 `DeskManager` 上）；
/// 每次新建实例就等于把历史全部重新播报一遍。
#[derive(Debug, Default)]
pub struct CodexProbe {
    /// `文件路径 → 已读到的字节偏移`。
    seen: HashMap<PathBuf, u64>,
}

impl CodexProbe {
    pub fn new() -> Self {
        Self::default()
    }

    /// 扫一遍，返回**新增**的命中。
    ///
    /// `now_ms` 用作命中的时间戳。刻意**不去解析**事件里的 ISO8601 `timestamp`：
    /// 那需要引入日期解析依赖，而这里唯一用到的语义是"我们刚刚发现它"，
    /// `now_ms` 就是这个语义的准确值。
    pub fn scan(&mut self, root: &Path, now_ms: u64) -> Vec<ProbeHit> {
        if !root.is_dir() {
            return Vec::new();
        }
        let files = collect_recent_rollouts(root, now_ms);
        let mut hits = Vec::new();

        // 借引用迭代：下面回收阶段还要再用一次 `files`。
        for (path, len) in &files {
            match self.seen.get_mut(path) {
                Some(offset) => {
                    if *len > *offset {
                        let (found, consumed) = read_since(path, *offset, now_ms);
                        hits.extend(found);
                        *offset = consumed;
                    }
                }
                None => {
                    // 首见一律按 EOF 登记、**不读内容**（见模块文档第 1 点）：
                    // 不管它是重启前的历史文件、窗口外复活的旧 rollout、还是
                    // 用户新开的会话，它"被我们看到之前"写的内容都不是新事件。
                    // 登记之后追加的内容仍会正常命中 —— 怕误报不等于把耳朵堵了。
                    self.seen.insert(path.clone(), *len);
                }
            }
        }
        // 回收：本次没被扫到的条目，说明它已超出时间窗口（不再被写），
        // 不可能再冒出新事件。不清掉的话 `seen` 会随会话数**无限增长** ——
        // 用户跑一年，这张表就有上万个条目，而它们全都再也用不上。
        let live: std::collections::HashSet<&PathBuf> = files.iter().map(|(p, _)| p).collect();
        self.seen.retain(|p, _| live.contains(p));

        hits
    }
}

/// 收集"最近被写过"的 rollout 文件，返回 `(路径, 当前长度)`。
///
/// 按路径排序：让扫描顺序**确定**，这样"两个文件同时有命中"时返回的次序稳定，
/// 便于单测断言，也便于日志比对。
fn collect_recent_rollouts(root: &Path, now_ms: u64) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    walk(root, now_ms, 0, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// 递归收集。**限深度**，避免目录结构意外变化时出现无界遍历。
fn walk(dir: &Path, now_ms: u64, depth: u32, out: &mut Vec<(PathBuf, u64)>) {
    if depth > 6 || out.len() > 512 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, now_ms, depth + 1, out);
            continue;
        }
        if !is_rollout_name(&path) {
            continue;
        }
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        // 只关心"最近被写过"的文件（见 `PROBE_WINDOW_MS` 的理由）。
        // 写成 `mtime < now - 窗口` 而不是 `now - mtime > 窗口`：
        // 两者数学等价，但这样读起来是"太旧了就不要"，且时钟异常时
        // （now 小于 mtime）会走 `saturating_sub` 的安全侧，不至于误判为过期。
        if mtime_ms(&meta) < now_ms.saturating_sub(PROBE_WINDOW_MS) {
            continue;
        }
        out.push((path, meta.len()));
    }
}

fn is_rollout_name(path: &Path) -> bool {
    match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name.starts_with("rollout-") && name.ends_with(".jsonl"),
        None => false,
    }
}

fn mtime_ms(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 从 `from` 字节处读到文件末尾，返回 `(命中, 应当推进到的偏移)`。
///
/// ⚠️ 第二个返回值**只到最后一个完整行为止**（见模块文档第 2 点）：
/// 末尾没有 `\n` 的半行不计入，留待下次重读。
fn read_since(path: &Path, from: u64, now_ms: u64) -> (Vec<ProbeHit>, u64) {
    let source = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string();

    let mut file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return (Vec::new(), from), // 打不开就保持原偏移，下次再试
    };
    if file.seek(SeekFrom::Start(from)).is_err() {
        return (Vec::new(), from);
    }

    let mut buf = Vec::new();
    if let Ok(size) = file.read_to_end(&mut buf) {
        if size as u64 > MAX_SCAN_BYTES {
            // 异常大的增量：只处理末尾这一段，避免这一次心跳被拖死。
            // 丢弃中间内容是安全的 —— 那里最多少几条播报，不会误报。
            let cut = buf.len() - MAX_SCAN_BYTES as usize;
            buf = buf[cut..].to_vec();
        }
    }

    let mut hits = Vec::new();
    let mut start = 0usize;
    let mut consumed = 0usize;
    loop {
        match buf[start..].iter().position(|&b| b == b'\n') {
            Some(i) => {
                let end = start + i; // 不含 '\n'
                if let Some(hit) = parse_line(&buf[start..end], &source, now_ms) {
                    hits.push(hit);
                }
                start = end + 1;
                consumed = start; // 完整推进到这一行之后
            }
            None => break, // 剩下的没有 '\n' ⇒ 半行，不消费
        }
    }
    (hits, from.saturating_add(consumed as u64))
}

/// 解析一行 JSONL；不是 `task_complete` 就返回 `None`。
fn parse_line(bytes: &[u8], source: &str, now_ms: u64) -> Option<ProbeHit> {
    // 预筛（模块文档第 3 点）：绝大多数行都没有这个字符串，
    // 不预筛就要对几千行做 serde_json 解析。
    let text = std::str::from_utf8(bytes).ok()?;
    if !text.contains("\"task_complete\"") {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    if v.get("payload").and_then(|p| p.get("type")).and_then(|t| t.as_str()) != Some("task_complete")
    {
        return None;
    }
    let msg = v
        .get("payload")
        .and_then(|p| p.get("last_agent_message"))
        .and_then(|m| m.as_str())
        .unwrap_or("");
    let turn_id = v
        .get("payload")
        .and_then(|p| p.get("turn_id"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    Some(ProbeHit {
        summary: tail_chars(msg, MAX_SUMMARY_CHARS),
        turn_id,
        source: source.to_string(),
        at_ms: now_ms,
    })
}

/// 取末尾 `max` 个字符（**按字符边界**，不按字节 —— 中文被切断会产生乱字符）。
pub fn tail_chars(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    s.chars().skip(n - max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("easyop_probe_test_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    /// 造一行真实的 task_complete（取自本机 rollout 的实测形状）。
    fn complete_line(turn: &str, msg: &str) -> String {
        format!(
            r#"{{"timestamp":"2026-09-20T08:00:00.000Z","type":"event_msg","payload":{{"type":"task_complete","turn_id":"{turn}","last_agent_message":"{msg}"}}}}"#
        )
    }

    fn noise_line() -> String {
        r#"{"timestamp":"2026-09-20T08:00:01.000Z","type":"event_msg","payload":{"type":"agent_message","message":"正在读取文件…"}}"#.to_string()
    }

    fn write(path: &Path, lines: &[String]) {
        let mut s = String::new();
        for l in lines {
            s.push_str(l);
            s.push('\n');
        }
        fs::write(path, s).expect("写测试文件");
    }

    #[test]
    fn first_pass_registers_offset_without_emitting() {
        // 这是本模块最重要的一条：首见**绝不能**把历史内容当成新事件。
        let dir = root("first_pass");
        let f = dir.join("rollout-a.jsonl");
        write(&f, &[complete_line("t1", "历史任务"), noise_line()]);

        let mut p = CodexProbe::new();
        let hits = p.scan(&dir, 1_000);
        assert!(hits.is_empty(), "首见必须静默：历史文件不是新事件");
        assert_eq!(p.seen.len(), 1);
    }

    #[test]
    fn a_file_first_seen_is_registered_at_eof_and_history_is_ignored() {
        // v0.41.15 实锤误报的场景：任务台 `codex exec resume` 追加写一个 23 天前的
        // 旧 rollout，文件复活后被当成"全新文件"从 0 整读，18 个历史 task_complete
        // 全变成新命中，任务刚开始就播"任务已完成"。
        // 新语义：首见一律按 EOF 登记；登记之后追加的事件仍然要能听到。
        let dir = root("first_seen_eof");
        let f = dir.join("rollout-a.jsonl");
        write(
            &f,
            &[
                complete_line("t1", "二十三天的旧结论"),
                complete_line("t2", "又一条旧结论"),
                noise_line(),
            ],
        );

        let mut p = CodexProbe::new();
        assert!(p.scan(&dir, 1_000).is_empty(), "首见不得回放历史");

        let mut s = fs::read_to_string(&f).unwrap();
        s.push_str(&complete_line("t3", "这次的结论"));
        s.push('\n');
        fs::write(&f, s).unwrap();

        let hits = p.scan(&dir, 2_000);
        assert_eq!(hits.len(), 1, "登记 EOF 之后追加的事件必须能听到");
        assert!(hits[0].summary.contains("这次的结论"));
    }

    #[test]
    fn appended_content_is_picked_up_exactly_once() {
        let dir = root("append_once");
        let f = dir.join("rollout-a.jsonl");
        write(&f, &[noise_line()]);

        let mut p = CodexProbe::new();
        p.scan(&dir, 1_000);

        // 追加一条完成
        let mut s = fs::read_to_string(&f).unwrap();
        s.push_str(&complete_line("t3", "追加的结论"));
        s.push('\n');
        fs::write(&f, s).unwrap();

        assert_eq!(p.scan(&dir, 2_000).len(), 1);
        // 再来一次必须为空 —— 同一个事件不能播两遍。
        assert!(p.scan(&dir, 3_000).is_empty(), "同一事件不得重复命中");
    }

    #[test]
    fn a_half_written_last_line_is_not_consumed() {
        // 模块文档第 2 点：文件正在被追加，末尾常常是半行。
        let dir = root("half_line");
        let f = dir.join("rollout-a.jsonl");
        write(&f, &[noise_line()]);

        let mut p = CodexProbe::new();
        p.scan(&dir, 1_000);

        // 故意**不加结尾换行**：模拟写到一半
        let mut s = fs::read_to_string(&f).unwrap();
        s.push_str(&complete_line("t4", "半行时的结论"));
        fs::write(&f, s).unwrap();

        assert!(p.scan(&dir, 2_000).is_empty(), "半行不该被当成完整行消费");

        // 写完那个换行后，它必须能出声 —— 否则这条事件就永久丢了。
        let mut s2 = fs::read_to_string(&f).unwrap();
        s2.push('\n');
        fs::write(&f, s2).unwrap();
        let hits = p.scan(&dir, 3_000);
        assert_eq!(hits.len(), 1, "补齐换行后必须补上");
        assert!(hits[0].summary.contains("半行时的结论"));
    }

    #[test]
    fn noise_lines_never_produce_hits() {
        let dir = root("noise");
        write(
            &dir.join("rollout-a.jsonl"),
            &[noise_line(), noise_line(), noise_line()],
        );
        let mut p = CodexProbe::new();
        p.scan(&dir, 1_000);
        assert!(p.scan(&dir, 2_000).is_empty());
    }

    #[test]
    fn non_rollout_files_are_ignored() {
        let dir = root("names");
        write(&dir.join("session-meta.json"), &[complete_line("t5", "x")]);
        write(&dir.join("rollout-a.txt"), &[complete_line("t6", "y")]);
        let mut p = CodexProbe::new();
        p.scan(&dir, 1_000);
        assert_eq!(p.seen.len(), 0, "只有 rollout-*.jsonl 该被监控");
    }

    #[test]
    fn stale_files_are_outside_the_probe_window() {
        // 很久没被写过的文件不可能是"刚刚完成"，不该进监控表。
        let dir = root("stale");
        let f = dir.join("rollout-old.jsonl");
        write(&f, &[complete_line("t7", "很久以前")]);

        // 不真的去改文件 mtime（那要额外依赖），改为把"现在"拨到 10 天之后 ——
        // 窗口判定算的是 `now - mtime`，拨时钟与改 mtime 在这条判据上等价。
        let future = real_now_ms() + 10 * 24 * 3600 * 1000;
        let mut p = CodexProbe::new();
        p.scan(&dir, future);
        assert_eq!(p.seen.len(), 0, "超出时间窗口的文件不该被监控");
    }

    #[test]
    fn tracked_entries_are_reclaimed_once_out_of_window() {
        // seen 表必须会缩，否则用户跑一年就攒上万个永远用不上的条目。
        let dir = root("reclaim");
        write(&dir.join("rollout-a.jsonl"), &[noise_line()]);

        let mut p = CodexProbe::new();
        p.scan(&dir, real_now_ms());
        assert_eq!(p.seen.len(), 1);

        let future = real_now_ms() + 10 * 24 * 3600 * 1000;
        p.scan(&dir, future);
        assert_eq!(p.seen.len(), 0, "超出窗口的条目必须被回收");
    }

    fn real_now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    #[test]
    fn summary_is_truncated_at_a_char_boundary() {
        // 中文按字节切断会产生乱字符，必须按字符切。
        let long: String = "结论".repeat(1000);
        let got = tail_chars(&long, 10);
        assert_eq!(got.chars().count(), 10);
        assert_eq!(got, "结论结论结论结论结论");
    }

    #[test]
    fn short_text_is_returned_whole() {
        assert_eq!(tail_chars("abc", 10), "abc");
    }

    #[test]
    fn scanning_a_missing_root_is_not_an_error() {
        let mut p = CodexProbe::new();
        assert!(p.scan(Path::new("E:/绝不存在/的目录"), 1_000).is_empty());
    }
}
