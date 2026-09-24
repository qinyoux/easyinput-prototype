//! 固定语料的预合成缓存 —— 方案 §2.6.2 / Phase 3 的 3.7 在 **PC 侧的等价物**。
//!
//! # 为什么需要这一层
//!
//! 状态播报的文本是**有限闭集**（[`NoticeKind`] 那 10 句）。如果每次播报都现调 TTS：
//!
//! 1. **有 token 成本** —— 而它每次说的都是同一句话；
//! 2. **有网络延迟** —— 状态播报的价值在于"及时"，延迟几百毫秒就失去意义；
//! 3. **断网就哑了** —— 这是最致命的：**"任务失联"这条告警恰恰出现在网络出问题的时候**。
//!    播报必须现调 TTS，等于"最重要的那条通知在最需要它的时候发不出来"。
//!
//! ➡️ 所以启动时一次性把 10 句全合成成音频落盘，之后播报**直读缓存**：
//! 零 token、零延迟、拔网线照样出声。离线可用性是**硬需求，不是优化**。
//!
//! # 板子路（固件）与 PC 路的同一个设计
//!
//! 固件的做法是把语料打成资产包走 `0x14` 落 Flash；这里的做法是落**本地磁盘**。
//! 两者是同一个第一性原理在不同载体上的落地：**把"固定内容"从"每次都要现算的东西"
//! 里剥离出来，变成一次性的资产**。所以本模块的接口设计刻意与"资产"同构
//! （`cached` / `missing` / `store`），将来换载体不必重画边界。
//!
//! # 为什么本模块是同步的
//!
//! 它只做两件不需要 network 的事：**判定缓存是否可用**、**把拿到的 wav 写盘**。
//! 真正的合成（`TtsClient::synthesize`，是 async）由命令层编排。
//! 这样切的好处是：本模块可以被纯文件操作的单测钉死，不需要 mock 任何网络。

use std::path::{Path, PathBuf};

use super::notice::NoticeKind;

/// 缓存子目录名（挂在 `output_dir` 下）。
///
/// 与产物目录同源的好处：用户已经知道那个目录在哪（任务台的输出本来就在那儿），
/// 不需要再记一个新路径；而且清理产物时顺手就能带上它。
pub const NOTICE_ASSET_DIR: &str = "notice_assets";

/// 一个合法 WAV 的最小字节数（44 字节头 + 至少 1 个采样点）。
///
/// 用它而不是"文件存在"来判定缓存有效：**中断的写入会留下一个 0 字节文件**，
/// 而"文件存在"会把那种残骸当成可用缓存 —— 于是播报时静默无声，
/// 排查起来极难。宁可多合成一次，也不要读到一个空文件。
const MIN_VALID_WAV_BYTES: u64 = 46;

/// 固定语料的缓存管理器。
#[derive(Clone, Debug)]
pub struct NoticeAssets {
    dir: PathBuf,
}

impl NoticeAssets {
    /// 在 `output_dir` 下挂一个 `notice_assets/` 子目录。
    ///
    /// **不在这里创建目录**：构造应当是廉价的、不会失败的，否则每次读状态都要处理
    /// IO 错误。真正需要目录的是 [`Self::store`]，由它负责创建。
    pub fn new(output_dir: &Path) -> Self {
        Self {
            dir: output_dir.join(NOTICE_ASSET_DIR),
        }
    }

    /// 缓存目录。
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 某条语料对应的缓存文件路径（**不管它是否存在**）。
    ///
    /// 命名 = `notice_<序号>_<slug>.wav`：序号保证唯一且可排序，
    /// slug 让人不查代码就知道这个文件对应哪句话。
    pub fn path_for(&self, kind: NoticeKind) -> PathBuf {
        self.dir.join(format!(
            "notice_{:02}_{}.wav",
            kind.code(),
            kind.slug()
        ))
    }

    /// 这条语料的缓存是否**可用**（存在且长度合理）。
    ///
    /// 返回 `Some(path)` 才表示"可以直接播"。
    pub fn cached(&self, kind: NoticeKind) -> Option<PathBuf> {
        let path = self.path_for(kind);
        match std::fs::metadata(&path) {
            Ok(meta) if meta.is_file() && meta.len() >= MIN_VALID_WAV_BYTES => Some(path),
            _ => None,
        }
    }

    /// 还没有可用缓存的语料清单（**按闭集顺序**，因此结果是确定的、可比较的）。
    pub fn missing(&self) -> Vec<NoticeKind> {
        NoticeKind::ALL
            .iter()
            .copied()
            .filter(|k| self.cached(*k).is_none())
            .collect()
    }

    /// 是否全部语料都已缓存 ⇒ **离线播报可用**。
    ///
    /// 这是"离线可用"这个承诺的**唯一判据**：它必须是可查询的，
    /// 而不是一句口头保证。设置页拿它显示状态。
    pub fn all_cached(&self) -> bool {
        self.missing().is_empty()
    }

    /// 已缓存条数 / 总条数。
    pub fn cached_count(&self) -> (usize, usize) {
        let total = NoticeKind::ALL.len();
        (total - self.missing().len(), total)
    }

    /// 把一条语料的 wav 字节写进缓存。
    ///
    /// **先写临时文件再改名**：直接往目标路径写的话，写到一半被打断（崩溃、断电、
    /// 用户关软件）会留下半个文件；而 `cached()` 靠长度判定，半个文件可能恰好
    /// 越过阈值被当成有效缓存 —— 播出来是一段断裂的杂音。改名是原子的，
    /// 所以目标路径上要么是完整的、要么是旧的，不存在中间态。
    pub fn store(&self, kind: NoticeKind, wav: &[u8]) -> Result<PathBuf, String> {
        if (wav.len() as u64) < MIN_VALID_WAV_BYTES {
            return Err(format!(
                "合成结果过短（{} 字节），拒绝写入缓存",
                wav.len()
            ));
        }
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| format!("创建语料缓存目录 {} 失败：{e}", self.dir.display()))?;

        let final_path = self.path_for(kind);
        let temp_path = self.dir.join(format!(
            "notice_{:02}_{}.wav.tmp",
            kind.code(),
            kind.slug()
        ));
        std::fs::write(&temp_path, wav)
            .map_err(|e| format!("写入临时文件 {} 失败：{e}", temp_path.display()))?;
        std::fs::rename(&temp_path, &final_path).map_err(|e| {
            // 改名失败要把临时文件收掉，否则缓存目录里会慢慢堆垃圾。
            let _ = std::fs::remove_file(&temp_path);
            format!("落盘 {} 失败：{e}", final_path.display())
        })?;
        Ok(final_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "easyop_notice_assets_{tag}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    /// 造一段长度合法的假 wav（内容不重要，只要过长度阈值）。
    fn fake_wav() -> Vec<u8> {
        vec![0u8; 200]
    }

    #[test]
    fn empty_cache_reports_every_notice_missing() {
        let out = temp_dir("empty");
        let assets = NoticeAssets::new(&out);
        assert_eq!(assets.missing().len(), NoticeKind::ALL.len());
        assert!(!assets.all_cached());
        assert_eq!(assets.cached_count(), (0, NoticeKind::ALL.len()));
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn storing_one_removes_it_from_missing() {
        let out = temp_dir("one");
        let assets = NoticeAssets::new(&out);
        let path = assets
            .store(NoticeKind::Completed, &fake_wav())
            .expect("写缓存应当成功");

        assert!(path.exists(), "缓存文件应当真的落盘");
        assert_eq!(assets.cached(NoticeKind::Completed), Some(path));
        assert_eq!(assets.missing().len(), NoticeKind::ALL.len() - 1);
        assert!(!assets.all_cached());
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn storing_every_notice_makes_the_cache_offline_ready() {
        let out = temp_dir("all");
        let assets = NoticeAssets::new(&out);
        for kind in NoticeKind::ALL {
            assets.store(kind, &fake_wav()).expect("写缓存");
        }
        assert!(assets.all_cached(), "全部落盘后应判定为离线可用");
        assert!(assets.missing().is_empty());
        assert_eq!(
            assets.cached_count(),
            (NoticeKind::ALL.len(), NoticeKind::ALL.len())
        );
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn truncated_file_is_not_treated_as_cache() {
        // 中断写入留下的残骸（0 字节 / 半个头）绝不能被当成可用缓存 ——
        // 否则播报无声且极难排查。
        let out = temp_dir("truncated");
        let assets = NoticeAssets::new(&out);
        std::fs::create_dir_all(assets.dir()).unwrap();
        std::fs::write(assets.path_for(NoticeKind::Failed), b"").unwrap();

        assert_eq!(assets.cached(NoticeKind::Failed), None);
        assert!(assets.missing().contains(&NoticeKind::Failed));
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn too_short_synthesis_result_is_rejected_instead_of_stored() {
        let out = temp_dir("short");
        let assets = NoticeAssets::new(&out);
        let err = assets
            .store(NoticeKind::Completed, &[0u8; 10])
            .expect_err("过短的结果必须被拒绝，不能静默落盘");
        assert!(err.contains("过短"), "错误信息要说清原因：{err}");
        assert!(!assets.path_for(NoticeKind::Completed).exists());
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn store_leaves_no_temp_files_behind() {
        let out = temp_dir("notmp");
        let assets = NoticeAssets::new(&out);
        assets.store(NoticeKind::TimedOut, &fake_wav()).unwrap();

        let leftovers: Vec<_> = std::fs::read_dir(assets.dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "不应残留临时文件：{leftovers:?}");
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn path_is_stable_and_readable() {
        let assets = NoticeAssets::new(Path::new("X:\\out"));
        let path = assets.path_for(NoticeKind::Running1Min);
        assert!(
            path.to_string_lossy().ends_with("notice_02_running_1min.wav"),
            "命名应当可读且带序号：{}",
            path.display()
        );
    }

    #[test]
    fn every_notice_maps_to_a_distinct_path() {
        let assets = NoticeAssets::new(Path::new("X:\\out"));
        let mut paths: Vec<String> = NoticeKind::ALL
            .iter()
            .map(|k| assets.path_for(*k).to_string_lossy().to_string())
            .collect();
        paths.sort();
        paths.dedup();
        assert_eq!(
            paths.len(),
            NoticeKind::ALL.len(),
            "每条语料必须有唯一的缓存路径"
        );
    }

    #[test]
    fn missing_order_follows_the_closure_order() {
        // 顺序确定 ⇒ 两次调用结果可比较，也便于 UI 稳定显示。
        let out = temp_dir("order");
        let assets = NoticeAssets::new(&out);
        assets.store(NoticeKind::Running5Min, &fake_wav()).unwrap();
        let missing = assets.missing();
        assert_eq!(missing.first(), Some(&NoticeKind::TaskStarted));
        assert!(!missing.contains(&NoticeKind::Running5Min));
        let _ = std::fs::remove_dir_all(&out);
    }
}
