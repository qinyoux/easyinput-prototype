//! 产物发现：一次任务跑完，"它到底产出了什么"。
//!
//! # 第一性原理
//!
//! Agent 干完活之后，"产物"的**外部可观测量只有一个：文件系统的时间戳变化**。
//!
//! 所以做法是：任务开始时记下时刻 → 结束后扫它的工作目录 → 时间窗内被创建或被改动的
//! 文件就是产物。这条路的两个替代方案都被否掉了：
//!
//! - **解析 Agent 的输出**：它爱写不写。写了不一定带完整路径，带了不一定是产物
//!   （可能只是它复述了一句需求）。要让它可靠，就得给每个 CLI 定义格式 —— 那是维护债。
//! - **让用户在配置里手写固定产物路径**：那等于要求他预先知道 Agent 会产出什么。
//!
//! 扫盘不需要任何人配合，且**它就是事实**。
//!
//! # 代价（必须收着的两个风险）
//!
//! 1. **工作目录可能很大。** 所以有深度上限、目录黑名单、条目上限（见下方常量）。
//! 2. **可能误报。** 用户自己在同一目录里改了文件，也会落进时间窗。这是可接受的：
//!    他看到的是"这次任务期间动过的文件"，这个语义本身没错，只是宽了一点。
//!
//! ⚠️ **`cwd` 没配就不扫。** 没有工作目录时不拿"进程当前目录"去猜 —— 那在开发机上
//! 常常是 `C:\Users\<name>`，一扫描就是几十万个文件，且产物判定毫无意义。

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// 递归深度上限（相对工作目录）。
///
/// 4 层够覆盖 `out/reports/2026/xxx.md` 这类常见结构，又不至于把整棵 node_modules
/// 级别的树走一遍。
const MAX_DEPTH: usize = 4;

/// 一次扫描最多看多少个目录项。超了就收手并**如实报告**（不假装扫完了）。
const MAX_SCANNED: usize = 4_000;

/// 扫描时跳过的目录名（大小写不敏感）。
///
/// 这些是"版本库 / 依赖 / 缓存"的通用位置 —— 它们**每天都会变**，把它们的变动
/// 当成"本次任务的产物"是最典型的噪声来源；而且它们动不动上万文件，会吃光扫描预算。
///
/// # ⚠️ 这份名单为什么**不含** `dist` / `build` / `out`
///
/// 它们曾经在名单里，理由是"那也是构建产物目录"。但这是**用别人的习惯替用户做决定**：
/// 大量工具（Go、Julia、推理导出、报告生成脚本）恰恰把成果写进 `out/` ——
/// 那正是用户想让它打开的目录。
///
/// 把它们排除的代价是**静默的**：用户写了 `artifact_glob = "out/*.png"`，
/// 却永远扫不到东西，而日志里一个字都没有。这种"配了规则却什么都没发生"最难查。
/// 反过来，把 `dist` 扫进来最坏只是多几条噪声 —— 而且 `MAX_DEPTH` + `MAX_SCANNED`
/// 两道上限已经把代价封住了，超限还会被**如实报告**（见 [`Found::truncated`]）。
///
/// 判断标准只有一条：**这个目录里的新文件，有没有可能是用户要的东西？**
/// 版本库、依赖、编译中间产物——不可能；构建输出目录——可能。
const EXCLUDED_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    ".svn",
    ".hg",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    ".nuxt",
    ".cache",
    ".idea",
    ".vscode",
    "$recycle.bin",
    "system volume information",
    "windows",
    "appdata",
];

/// 扫描结果。
#[derive(Clone, Debug, Default)]
pub struct Found {
    /// 命中的文件，**最新的在前**。
    ///
    /// ⚠️ mtime 并列时（Windows 时间戳粒度约 1ms，连着写下的文件会完全相同）
    /// 按**路径升序**兜底 —— 顺序不承诺更准，但承诺**可复现**。
    pub files: Vec<PathBuf>,
    /// 是否因为条目上限提前收手（`true` = 可能还有更多没看到）。
    pub truncated: bool,
    /// 实际看了多少个目录项（排查"为什么没找到"时的第一手数据）。
    pub scanned: usize,
    /// 因为黑名单或深度上限**跳过**的目录数。
    ///
    /// 记它出来是为了把"跳过"从我自己的一个内部决定，变成用户能看到的事实：
    /// 用户问"我的产物在 `node_modules` 里怎么没扫到"时，日志里的数字就是答案。
    /// 没有这个数，跳过就是静默的 —— 而静默的跳过与"真的没有产物"外观完全相同。
    pub skipped: usize,
}

/// 在工作目录里找出**任务期间被动过**的文件。
///
/// - `since_ms`：任务开始的时刻（unix 毫秒）。只收 `mtime >= since_ms` 的文件。
/// - `glob`：可选通配（空 = 不过滤）。**相对工作目录**匹配，分隔符统一按 `/`：
///   `*.md` 只命中工作目录**根下**的 markdown，`out/*.png` 命中 `out` 目录里的图，
///   `**/*.md` 才命中任意深度。
///
/// # ⚠️ 为什么必须用"相对路径"匹配，不能用绝对路径
///
/// 用绝对路径匹配的话，`*.md` 永远匹配不上任何文件 —— 因为绝对路径里必然有
/// 分隔符（`C:/Users/.../out/a.md`），而 `*` 不跨分隔符。
/// 这个 bug 的表现极其误导：**用户配了过滤规则，于是什么都扫不到**，
/// 看起来像"功能没生效"，实际是规则被拿去跟一条更长的字符串比了。
pub fn discover(root: &Path, since_ms: u64, glob: &str) -> Found {
    let mut hits: Vec<(std::time::Duration, PathBuf)> = Vec::new();
    let mut found = Found::default();

    if !root.is_dir() {
        return found;
    }

    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            found.skipped += 1;
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            found.scanned += 1;
            if found.scanned > MAX_SCANNED {
                found.truncated = true;
                break;
            }
            let Ok(ft) = entry.file_type() else { continue };
            let path = entry.path();

            if ft.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if is_excluded_dir(&name) {
                    found.skipped += 1;
                    continue;
                }
                // 符号链接/联接点不跟进去：跟着走容易绕进别的盘或形成环。
                stack.push((path, depth + 1));
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            // 隐藏文件（`.DS_Store` / `.gitignore` 一类）不是"产物"。
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let Some(mtime) = mtime_since_epoch(&meta) else {
                continue;
            };
            // 时间窗用**毫秒**比（`since_ms` 就是毫秒口径），排序用全精度 —— 见下方排序处。
            if (mtime.as_millis() as u64) < since_ms {
                continue;
            }
            if !glob.is_empty() {
                // 用**相对 root 的路径**匹配（理由见 `discover` 的文档）。
                // 取不到相对路径（理论上不会发生，root 是 path 的前缀）时退回文件名：
                // 退化成"只按名字过滤"总比"一条都匹配不上"好 —— 后者是无提示的空结果。
                let rel = path
                    .strip_prefix(root)
                    .map(|r| r.to_string_lossy().to_string())
                    .unwrap_or_else(|_| entry.file_name().to_string_lossy().to_string());
                if !glob_match(glob, &rel) {
                    continue;
                }
            }
            hits.push((mtime, path));
        }
        if found.truncated {
            break;
        }
    }

    // 最新的在前 —— 调用方（打开逻辑）永远只关心"最近那个"。
    //
    // ⚠️ **并列是常态，必须定序**。实测结论（踩过两次才搞清）：
    // Windows 给文件打的时间戳**粒度约 1ms** —— 连着写的 `b.md` 与 `c.png`
    // 会拿到**一模一样**的 mtime（精确到最后一位）。所以：
    //
    //   · "同一毫秒内谁更新"这个问题**没有答案**，别试图用精度去解决它
    //     （先按毫秒截断、再改成全精度，两次都无效 —— 时间戳本身就是并列的）；
    //   · 但并列**不能就这么算了**：`sort_by` 是稳定排序 ⇒ 并列项保持"目录读取顺序"，
    //     而目录读取顺序会随文件系统状态漂移 ⇒ 用户两次打开同一条历史，看到的
    //     产物顺序可能不同，也没法写稳定的测试。
    //
    // 所以并列时按**路径升序**兜底：顺序不承诺"更准"，但承诺**可复现**。
    hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found.files = hits.into_iter().map(|(_, p)| p).collect();
    found
}

/// 这次的产物该怎么给用户。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "how", rename_all = "snake_case")]
pub enum OpenOutcome {
    /// 只有一个产物 ⇒ 直接打开它。
    File { path: String },
    /// 有多个 ⇒ 打开所在文件夹并选中**最新**那个（不逐个弹程序）。
    ///
    /// ⚠️ "最新"在 mtime 并列时是路径序的第一个 —— 那种情况意味着几个文件
    /// 是**同一瞬间**写下的（Windows 时间戳粒度约 1ms），本来就没有更"对"的那个；
    /// 反正文件夹已经打开，用户一眼能看到全部。
    Folder { path: String, count: usize },
}

/// 按"少的直接开、多的开文件夹"打开产物。
///
/// # 为什么不是"全都打开"
///
/// 一个任务产出十几个文件时逐个启动关联程序，结果是用户桌面被弹满，
/// 而他真正想看的往往只有最新那一个。开文件夹 + 选中，把选择权还给他。
pub fn open_artifacts(files: &[PathBuf]) -> Result<OpenOutcome, String> {
    let Some(newest) = files.first() else {
        return Err("没有产物可打开".into());
    };
    let p = newest.to_string_lossy().to_string();
    if files.len() == 1 {
        crate::launcher::shell_open(&p)?;
        Ok(OpenOutcome::File { path: p })
    } else {
        crate::ss_palette::open_containing_folder(&p)?;
        Ok(OpenOutcome::Folder {
            path: p,
            count: files.len(),
        })
    }
}

/// 目录名是否在黑名单里。
pub fn is_excluded_dir(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    EXCLUDED_DIRS.iter().any(|d| *d == lower)
}

/// 文件 mtime 距 UNIX 纪元的时长。
///
/// ⚠️ 刻意返回 `Duration` 而**不是**毫秒，别在入口就截断精度：
/// 调用方两处口径不同 —— **时间窗**用毫秒（`since_ms` 是毫秒口径），
/// **排序**要全精度。早先这里直接 `.as_millis()`，等于在源头就把信息丢了。
///
/// 但别指望全精度能解决"同毫秒谁更新" —— 那是**文件系统给不出**的答案
/// （实测 Windows 时间戳粒度约 1ms，连着写的两个文件会完全相同）。
/// 全精度只是"有多少用多少"，并列的兜底在排序处（按路径定序）。
fn mtime_since_epoch(meta: &std::fs::Metadata) -> Option<std::time::Duration> {
    meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()
}

/// 简单 glob：`*` 不跨 `/`，`**` 跨，`?` 匹配单个非 `/` 字符。
///
/// # 为什么 `*` 不跨分隔符
///
/// 用户写 `*.md` 时想的是"当前目录下的 markdown"，而不是"任意深度下的所有 markdown"。
/// 让 `*` 跨 `/` 会让规则比它看起来的宽得多，而**宽了的规则不会报错** ——
/// 用户只会觉得"它打开了些莫名其妙的东西"。想要跨层就写 `**`。
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = normalize(pattern).chars().collect();
    let t: Vec<char> = normalize(text).chars().collect();
    // 记忆化"已确认失败"的 (模式位置, 文本位置)。
    //
    // ⚠️ 必须有它：朴素的回溯实现在 `a*a*a*a*b` 这类模式上是**指数级**的。
    // 这里的模式来自用户配置、文本是路径（都很短），但仍然不该留一个会突然卡住的算法 ——
    // 卡住发生在任务完成的回调里，用户看到的是"任务台不动了"。
    let mut dead: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    go(&p, &t, 0, 0, &mut dead)
}

/// `glob_match` 的实际递归体（分离出来，便于把记忆化收在一处）。
fn go(
    p: &[char],
    t: &[char],
    pi: usize,
    ti: usize,
    dead: &mut std::collections::HashSet<(usize, usize)>,
) -> bool {
    if dead.contains(&(pi, ti)) {
        return false;
    }

    let ok = if pi == p.len() {
        ti == t.len()
    } else if p[pi] == '*' && p.get(pi + 1) == Some(&'*') {
        // `**` ⇒ 跨层（可以吃掉分隔符）。
        //
        // ⚠️ 后面紧跟 `/` 时把那个 `/` 一起跳过，于是 `**` **也能匹配零层目录** ——
        // 即 `**/*.md` 能命中根目录下的 `报告.md`，与 git 的 `**/` 语义一致。
        // 少了这一条，用户按最流行的写法配了 `**/*.md`，却发现根目录的产物一个都不匹配，
        // 而失败是**静默**的（表现为"没产物"）。
        let skip = if p.get(pi + 2) == Some(&'/') {
            pi + 3
        } else {
            pi + 2
        };
        go(p, t, skip, ti, dead) || (ti < t.len() && go(p, t, pi, ti + 1, dead))
    } else if p[pi] == '*' {
        // 单星：零个或多个**非分隔符**字符。
        go(p, t, pi + 1, ti, dead)
            || (ti < t.len() && t[ti] != '/' && go(p, t, pi, ti + 1, dead))
    } else if p[pi] == '?' {
        ti < t.len() && t[ti] != '/' && go(p, t, pi + 1, ti + 1, dead)
    } else {
        ti < t.len() && t[ti] == p[pi] && go(p, t, pi + 1, ti + 1, dead)
    };

    if !ok {
        dead.insert((pi, ti));
    }
    ok
}

/// 统一分隔符：Windows 上 `\` 与 `/` 是同一件事，模式与路径必须同口径比较。
fn normalize(s: &str) -> String {
    s.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("easyop_art_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    // ---- glob ----

    #[test]
    fn star_does_not_cross_directories() {
        assert!(glob_match("*.md", "报告.md"));
        assert!(glob_match("out/*.png", "out/a.png"));
        // 关键否定：`*.md` 不该命中深层文件 —— 那会让用户的规则比它看起来宽得多。
        assert!(!glob_match("*.md", "deep/nested/报告.md"));
        assert!(glob_match("**/*.md", "deep/nested/报告.md"));
        // `**/` 也要能匹配**零层**目录（与 git 一致）：否则最流行的写法
        // `**/*.md` 反而不命中根目录的产物，而失败是静默的。
        assert!(glob_match("**/*.md", "报告.md"));
        // 尾随 `**` 吃掉剩下的一切。
        assert!(glob_match("out/**", "out/a/b/c.txt"));
        assert!(glob_match("**", "任何/深度/名字.txt"));
    }

    #[test]
    fn question_mark_matches_one_char_but_never_a_separator() {
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(!glob_match("a?c", "a/c"));
    }

    #[test]
    fn backslashes_are_treated_as_separators() {
        // 用户从资源管理器复制来的路径是反斜杠的，规则里写的是正斜杠。
        assert!(glob_match("out/*.png", "out\\a.png"));
        assert!(glob_match("out\\*.png", "out/a.png"));
    }

    #[test]
    fn glob_handles_multiple_stars_and_literals() {
        assert!(glob_match("*e*t*", "test"));
        assert!(glob_match("*.tar.gz", "x.tar.gz"));
        assert!(!glob_match("*.tar.gz", "x.tar.gzip"));
        assert!(glob_match("**", "any/deep/path.txt"));
        assert!(glob_match("", ""));
        assert!(!glob_match("", "x"));
    }

    // ---- 扫描 ----

    #[test]
    fn only_files_touched_after_the_mark_are_found() {
        let d = tmp("window");
        let old = d.join("old.txt");
        fs::write(&old, "旧").unwrap();
        let mark = now_ms() + 5; // 之后写的一定晚于它
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(d.join("new.txt"), "新").unwrap();

        let found = discover(&d, mark, "");
        assert_eq!(found.files.len(), 1, "只该看到时间窗内的那个");
        assert!(found.files[0].ends_with("new.txt"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn excluded_dirs_are_not_walked() {
        let d = tmp("excl");
        let nm = d.join("node_modules").join("pkg");
        fs::create_dir_all(&nm).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mark = now_ms() - 5;
        fs::write(nm.join("index.js"), "x").unwrap();
        fs::write(d.join("real.txt"), "y").unwrap();

        let found = discover(&d, mark, "");
        assert_eq!(found.files.len(), 1, "node_modules 里的不该被当成产物");
        assert!(found.files[0].ends_with("real.txt"));
        // 跳过必须是**可观测的**：否则用户无法区分"真没有产物"与"全被跳过了"。
        assert!(found.skipped >= 1, "跳过的目录数要能让日志说出来");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn hidden_files_are_skipped() {
        let d = tmp("hidden");
        let mark = now_ms() - 5;
        fs::write(d.join(".DS_Store"), "x").unwrap();
        fs::write(d.join("keep.txt"), "y").unwrap();
        let found = discover(&d, mark, "");
        assert_eq!(found.files.len(), 1);
        assert!(found.files[0].ends_with("keep.txt"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn newest_is_first_and_glob_filters() {
        // ⚠️ 这条测试曾经**偶发失败**（约 4/6 的概率挂）。根因不在排序，而在
        // **Windows 文件时间戳只有约 1ms 粒度**：`b.md` 与紧随其后的 `c.png`
        // 实测会拿到**完全相同**的 mtime ⇒ "谁更新"这个问题在文件系统层面就无解。
        // 详见排序处与下面断言处的注释。
        let d = tmp("order");
        let mark = now_ms() - 5;
        fs::write(d.join("a.md"), "1").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(d.join("b.md"), "2").unwrap();
        fs::write(d.join("c.png"), "3").unwrap();

        let all = discover(&d, mark, "");
        assert_eq!(all.files.len(), 3);
        // ⚠️ 这条测试曾经**偶发失败**（约 4/6 的概率挂），根因不是排序写错，而是
        // **Windows 的文件时间戳粒度约 1ms**：`b.md` 与紧随其后的 `c.png` 实测会拿到
        // **完全相同**的 mtime（`...715775300` 一字不差）。同一时间片内谁"更新"根本不存在，
        // 所以"c.png 必须排第一"是个**测不了**的断言 —— 换成断言真正能保证的东西。
        //
        // a.md 前面睡了 20ms，与后两个不在同一时间片 ⇒ 它最旧，必须排最后。
        assert!(all.files.last().unwrap().ends_with("a.md"), "最旧的一定排最后");
        // 顺序必须按 mtime **非递增**（这是排序的真实契约）。
        let ages: Vec<_> = all
            .files
            .iter()
            .map(|f| fs::metadata(f).unwrap().modified().unwrap())
            .collect();
        assert!(
            ages.windows(2).all(|w| w[0] >= w[1]),
            "必须按 mtime 从新到旧：{ages:?}"
        );

        // 等到下一个时间片再写一个 ⇒ 这才是"最新的一定排第一"的可检定点。
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(d.join("d.png"), "4").unwrap();
        let newer = discover(&d, mark, "");
        assert!(newer.files[0].ends_with("d.png"), "最新的一定排第一");

        let only_md = discover(&d, mark, "*.md");
        assert_eq!(only_md.files.len(), 2, "glob 必须过滤掉 png");
        assert!(only_md.files.iter().all(|p| p.extension().is_some()));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn ties_are_broken_deterministically_by_path() {
        // 同一时间片内的几个文件 mtime 完全相同 ⇒ 靠 mtime 分不出先后。
        // 但**顺序必须是可复现的**：否则用户两次打开同一个历史条目、看到的产物顺序
        // 可能不一样（底层是目录读取顺序，随 fs 状态漂移），而且没法写稳定的测试。
        // 所以并列时按路径定序。这条测试同时是"并列确实存在"的证据 ——
        // 如果哪天 Windows 的时间戳细到能分开它们，这里的断言仍然成立（只是不再并列）。
        let d = tmp("ties");
        let mark = now_ms() - 5;
        for n in ["q1.txt", "q2.txt", "q3.txt", "q4.txt"] {
            fs::write(d.join(n), "x").unwrap();
        }
        let a = discover(&d, mark, "");
        let b = discover(&d, mark, "");
        assert_eq!(a.files, b.files, "两次扫描必须给出同样的顺序");
        assert_eq!(a.files.len(), 4);
        // 同一 mtime 的那一组内部必须按路径升序。
        let mut group: Vec<String> = a
            .files
            .iter()
            .filter(|p| {
                fs::metadata(p).unwrap().modified().unwrap()
                    == fs::metadata(&a.files[0]).unwrap().modified().unwrap()
            })
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        let sorted = {
            let mut g = group.clone();
            g.sort();
            g
        };
        assert_eq!(group, sorted, "并列项必须按路径升序");
        group.clear();
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn glob_is_matched_against_the_relative_path_not_the_absolute_one() {
        // 这条测试存在的唯一理由：绝对路径里必然含分隔符，而 `*` 不跨分隔符 ——
        // 一旦退回"拿绝对路径去比"，`*.md` 会一条也匹配不上，且**完全静默**。
        let d = tmp("relpath");
        let mark = now_ms() - 5;
        fs::write(d.join("root.md"), "1").unwrap();
        let sub = d.join("out");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("deep.md"), "2").unwrap();

        // `*.md` 只该命中根下那个（相对路径 = "root.md"）。
        let flat = discover(&d, mark, "*.md");
        assert_eq!(flat.files.len(), 1, "根层的 *.md 只该命中 1 个");
        assert!(flat.files[0].ends_with("root.md"));

        // 跨层要写 `**`（相对路径 = "out/deep.md"）。
        let deep = discover(&d, mark, "**/*.md");
        assert_eq!(deep.files.len(), 2, "**/*.md 该命中两层");

        // 目录限定：`out/*.md` 只命中子目录里那个。
        let only_sub = discover(&d, mark, "out/*.md");
        assert_eq!(only_sub.files.len(), 1);
        assert!(only_sub.files[0].ends_with("deep.md"));

        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_broken_pattern_returns_nothing_instead_of_panicking() {
        // 用户手写 glob 会写错。写错的结果应该是"没扫到产物"（一条安静的空结果），
        // 而**绝不能**是 panic —— 那会把任务完成的回调整个打断。
        for bad in ["[", "***", "**/**/**", "*?*?*?*?*?*?*?*?*?*?*?*?*x"] {
            let d = tmp("badglob");
            fs::write(d.join("a.md"), "1").unwrap();
            let _ = discover(&d, 0, bad);
            let _ = fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn missing_working_directory_yields_nothing_instead_of_panicking() {
        let d = std::env::temp_dir().join("easyop_art_definitely_missing_xyz");
        let _ = fs::remove_dir_all(&d);
        let found = discover(&d, 0, "");
        assert!(found.files.is_empty());
        assert_eq!(found.scanned, 0);
    }

    #[test]
    fn open_requires_at_least_one_file() {
        assert!(open_artifacts(&[]).is_err());
    }

    #[test]
    fn excluded_dir_names_are_case_insensitive() {
        assert!(is_excluded_dir("node_modules"));
        assert!(is_excluded_dir("NODE_MODULES"));
        assert!(is_excluded_dir(".Git"));
        assert!(!is_excluded_dir("src"));
    }
}
