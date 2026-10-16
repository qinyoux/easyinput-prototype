//! 状态播报（Notice）通道 —— 与未读账本**完全分离**。
//!
//! # 第一性原理：为什么 Notice 与 Result 必须走两条路
//!
//! 这台设备的输出只有 **5 颗灯 + 1 个喇叭**，但两者的本征属性完全不同：
//!
//! | 通道 | 本征属性 | 适合承载 |
//! |---|---|---|
//! | 5 颗灯 | 多路、可并行、零成本、常亮、不打断 | **状态**（"有几件事在跑""哪一槽有结果"） |
//! | 1 个喇叭 | 单路、独占、有成本（TTS 调用）、瞬时、会打断 | **事件**（需要人注意或决策的里程碑） |
//!
//! 于是同一句话在两处出现时，语义必须分开：
//!
//! - **Result（结果）**：任务终态 + 产物已交付。**产生未读**（听完才消费），灯亮，优先级高。
//! - **Notice（通知）**：状态跃迁、里程碑、需要决策。**不产生未读**，可被抢占、可静默丢弃，优先级低。
//!
//! ➡️ 少了这个区分，就会出现最伤信任的失败模式：**用户看到"未读"灯亮，按下去只听到
//! 一句"正在推进"**。灯一旦骗人一次，"灯 = 有东西等你听"这条契约就作废了 ——
//! 而"灯可信"是整台设备的根。
//!
//! # 本模块只做一件事：决定"这次状态变化该不该出声、出哪一句"
//!
//! 它**不碰**未读账本（不 `push_result`、不 `grant`），也**不碰**物理播放
//! （PC 侧的播放器在 `audio_out.rs`，用 cpal 直接写系统默认输出设备 ——
//! **刻意不放前端**：因为"听完才消费未读"的判定点不能活在 webview 里，刷一次页面
//! 或渲染进程崩一次，这条承诺就断了；将来换板子喇叭只是换载体，判定点不动）。
//! 所有函数都是纯逻辑，无 IO、无全局可变状态，因此可以被单测钉死。
//!
//! # 两条驱动源（事件路 + 状态路）
//!
//! Notice 的触发源**不是同一种东西**，混在一起写必然出错：
//!
//! | 类别 | 例子 | 驱动方式 | 为什么 |
//! |---|---|---|---|
//! | **终态 / 起点** | 已完成、失败、失联、任务已开始 | **事件驱动**：`complete_task` / `dispatch_into` 处直接发 | 终态是**瞬时事件**，"刚完成"这个事实在状态快照里根本看不到（快照只描述"现在是什么样"） |
//! | **进度档位** | 正在推进、已运行 1 分钟、已运行 5 分钟 | **状态驱动**：扫 `RuntimeSnapshot` 推 `DeskPulse` | 进度是**持续状态的函数**，没有独立的"事件"可言 |
//!
//! 两条路都收敛到同一个 [`NoticeGate::should_emit_for`]，由它统一执行去抖与跃迁判定
//! （事件类按 `(槽位, kind)` 记账，进度类走全局账本 —— 理由见 [`NoticeGate`] 的文档）。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::state::RuntimeSnapshot;

/// 同一 `(槽位, kind)` 的事件类通知，间隔多短才算"重复送达"。
///
/// # 为什么事件类也有一个 guard
///
/// 防的是**同一件事被说两遍**：派发那一刻「任务已开始」（事件源）与
/// 「正在推进」（状态源）会同拍触发，旧版只靠"后到抢占"兜底，
/// 结果是两条都只播了个开头。3 秒足够覆盖这种同拍双送达，
/// 又不足以吞掉真实场景 —— 而即使真吞了，未读灯与历史仍然如实记录，丢的只是一句话。
///
/// ⚠️ 它**不是**"同类不重复播"：旧版用全局 `last_kind` 做后者，
/// 把 S2 的完成播报整个吞掉（v0.41.15 实锤）。两件事的代价完全不同：
/// 重复播一句话是噪音，吞掉一条终态是失信。
pub const EVENT_REPEAT_GUARD_MS: u64 = 3_000;

/// 同类 Notice 最小间隔的**出厂默认**（毫秒）。
///
/// **只约束"可去抖"的那几条**（进度类）。终态与起点类不受它约束 ——
/// 详见 [`NoticeKind::is_debounceable`] 的理由：把"任务已完成"因为"离上一条太近"
/// 而吞掉，是这套系统最不能犯的错。
///
/// ⚠️ 实际生效值来自配置（`NoticeConfig.min_interval_secs`，见 [`NoticePolicy`]）。
/// 这里保留常量只为一件事：让"没有配置时是什么行为"有一个可引用的真源，
/// 并且让测试能钉住"默认行为没有被悄悄改掉"。
pub const DEFAULT_MIN_INTERVAL_MS: u64 = 8_000;

/// 进度档位的**出厂默认**时刻：1 分钟 / 5 分钟 / 15 分钟（毫秒）。
///
/// # 为什么档位"数量"是闭集的一部分，而不是随便配
///
/// 每一档都要对应一句**固定语料**（[`NoticeKind::text`]），而语料必须在启动时
/// 全部预合成成音频资产，才能满足"拔网线后仍能出声"（方案 §2.6.5 第 4 条）。
/// 于是档位**不是配置的自由列表**，而是受 [`NoticeKind::PROGRESS_MARKS`] 这个闭集
/// 约束的：配置给几个时刻，只是决定"这几档分别落在第几分钟"，**造不出第 N+1 档** ——
/// 一旦允许无界，语料集就无界，"预合成一次、长期复用、可离线"随之作废。
pub const DEFAULT_PROGRESS_MARKS_MS: [u64; 3] = [60_000, 300_000, 900_000];

/// 状态播报的**有限闭集**。
///
/// 这是本模块最重要的约束：**能播的句子必须是一个可以枚举完的集合**。
/// 因为每一句都要预合成成音频资产（[`super`] 的缓存层），而这个集合必须小到
/// "启动时全部合成一遍"是可接受的。
///
/// `code()` 即枚举序号，直接用作缓存文件名 —— 因此**顺序是契约的一部分，
/// 只能在末尾追加，不能重排**（重排会让旧缓存张冠李戴地读到另一句音频）。
///
/// ⚠️ `Hash` 不是摆设：`NoticeGate` 的事件类账本以 `(槽位, NoticeKind)` 为
/// HashMap 的键（见 [`NoticeGate`]）。删掉它编译不过，那是故意的。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    /// 任务已进入队列（`dispatch_into` 成功）。
    TaskStarted = 0,
    /// 正在推进（运行中、未跨过任何时长档位）。
    Progressing = 1,
    /// 已运行 1 分钟。
    Running1Min = 2,
    /// 已运行 5 分钟。
    Running5Min = 3,
    /// 任务成功**且产物已交付**。
    Completed = 4,
    /// 任务成功但**产物交付失败**。刻意与 [`Self::Completed`] 分开。
    CompletedArtifactFailed = 5,
    /// 任务失败。
    Failed = 6,
    /// 需要用户确认（headless Agent 的 fail-closed 分支）。
    NeedsConfirmation = 7,
    /// 已超时。
    TimedOut = 8,
    /// 失联（轮询超时 / 断网）。离线场景下最该被听到的一句，也是必须离线的理由。
    LostContact = 9,
    /// 已运行 15 分钟（档位表的第 3 档）。
    ///
    /// ⚠️ **刻意追加在末尾，而不是插在 `Running5Min` 后面**：`code()` 同时是缓存
    /// 文件名的一部分，插在中间会把磁盘上已有的缓存**整体错位**（读到另一句音频，
    /// 而且不报错）。顺序是契约，只能追加。
    Running15Min = 10,
    /// 合并提醒：排队期间被丢弃的终态播报，凑成一句短的。
    ///
    /// 它是** meta 通知**——不描述任何单个任务的终态，只说"还有几条没念"。
    /// 因此它**不是**终态（`is_terminal() = false`）：它不决定任何任务的结束，
    /// 排队机制也刻意不把它自己排进队（否则"提醒还有提醒"会无穷尽）。
    /// 同样**追加在末尾**（`code()` 契约，见 `Running15Min` 的说明）。
    ResultsWaiting = 11,
}

impl NoticeKind {
    /// 全部取值，**按 `code()` 顺序**。预合成层靠它遍历整个闭集。
    pub const ALL: [NoticeKind; 12] = [
        NoticeKind::TaskStarted,             // 0
        NoticeKind::Progressing,             // 1
        NoticeKind::Running1Min,             // 2
        NoticeKind::Running5Min,             // 3
        NoticeKind::Completed,               // 4
        NoticeKind::CompletedArtifactFailed, // 5
        NoticeKind::Failed,                  // 6
        NoticeKind::NeedsConfirmation,       // 7
        NoticeKind::TimedOut,                // 8
        NoticeKind::LostContact,             // 9
        NoticeKind::Running15Min,            // 10 —— 追加，见变体上的注释
        NoticeKind::ResultsWaiting,          // 11 —— 追加，见变体上的注释
    ];

    /// 进度档位对应的语料，**按档位从低到高**。
    ///
    /// 长度即 [`NoticePolicy::progress_marks_ms`] 的容量上限：配置只能决定
    /// "这几档分别在第几分钟"，改变不了档数。加档 = 加一个变体 + 加一句语料，
    /// 属**代码改动**，不是配置项。
    pub const PROGRESS_MARKS: [NoticeKind; 3] = [
        NoticeKind::Running1Min,
        NoticeKind::Running5Min,
        NoticeKind::Running15Min,
    ];

    /// 枚举序号。**同时是缓存文件名的一部分**，改动即破坏旧缓存。
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// 稳定的英文标识，**只用于缓存文件名**。
    ///
    /// 刻意不用中文：缓存要落在用户磁盘上，中文文件名在跨工具链（脚本、压缩包、
    /// 其它 agent 的文件操作）时是本项目已经踩过的坑。`code()` 已经保证唯一，
    /// slug 只是为了让目录**人眼可读**——「这条 wav 对应哪句话」不该需要查代码。
    pub const fn slug(self) -> &'static str {
        match self {
            NoticeKind::TaskStarted => "task_started",
            NoticeKind::Progressing => "progressing",
            NoticeKind::Running1Min => "running_1min",
            NoticeKind::Running5Min => "running_5min",
            NoticeKind::Completed => "completed",
            NoticeKind::CompletedArtifactFailed => "completed_artifact_failed",
            NoticeKind::Failed => "failed",
            NoticeKind::NeedsConfirmation => "needs_confirmation",
            NoticeKind::TimedOut => "timed_out",
            NoticeKind::LostContact => "lost_contact",
            NoticeKind::Running15Min => "running_15min",
            NoticeKind::ResultsWaiting => "results_waiting",
        }
    }

    /// 固定语料文本。
    ///
    /// ⚠️ **措辞纪律**（方案 §2.6.3）：播报文本必须由"产物交付成功且可回读"驱动，
    /// 而不是由任务 API 的状态驱动。所以"成功但产物没落地"**绝不许**说"已完成"。
    pub const fn text(self) -> &'static str {
        match self {
            NoticeKind::TaskStarted => "任务已开始",
            NoticeKind::Progressing => "正在推进",
            NoticeKind::Running1Min => "已运行一分钟",
            NoticeKind::Running5Min => "已运行五分钟",
            NoticeKind::Completed => "任务已完成",
            NoticeKind::CompletedArtifactFailed => "任务完成，但产物交付失败",
            NoticeKind::Failed => "任务失败",
            NoticeKind::NeedsConfirmation => "需要你确认",
            NoticeKind::TimedOut => "已超时",
            NoticeKind::LostContact => "任务失联",
            NoticeKind::Running15Min => "已运行十五分钟",
            // 合并提醒的**固定语料**不带条数（静态函数给不出动态值）；
            // 实际出声用动态正文（"还有 N 条…"），这条只是离线兜底与字幕。
            NoticeKind::ResultsWaiting => "还有任务结果未播报",
        }
    }

    /// 是否可被最小间隔去抖（即进度类）。
    pub const fn is_debounceable(self) -> bool {
        matches!(
            self,
            NoticeKind::Progressing
                | NoticeKind::Running1Min
                | NoticeKind::Running5Min
                | NoticeKind::Running15Min
        )
    }

    /// 是否为终态（决定任务是否已结束的那几条）。
    ///
    /// 用于 UI 措辞与统计；**不参与**去抖判定（那是 [`Self::is_debounceable`] 的事）。
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            NoticeKind::Completed
                | NoticeKind::CompletedArtifactFailed
                | NoticeKind::Failed
                | NoticeKind::NeedsConfirmation
                | NoticeKind::TimedOut
                | NoticeKind::LostContact
        )
    }
}

/// 从状态快照里抠出来的**最小输入**。
///
/// 刻意不直接吃 `RuntimeSnapshot`：那样 notice 的依赖面会随快照字段增删而漂移
/// （快照每加一个字段，这个模块都得重新想一遍"它影响播报吗"）。这里只保留
/// 真正决定"该播哪句进度"的两个量，转换点仅 [`Self::from_snapshot`] 一处。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeskPulse {
    /// 正在执行的槽数。
    pub running_count: usize,
    /// **最早开始**的那个任务的开始时刻（unix 毫秒）。0 表示没有在跑的任务。
    ///
    /// 取"最早"而不是"最新"：进度应当由**最久没结束的那件事**驱动 ——
    /// 一件跑了 6 分钟的任务，不会因为旁边新起了一个任务而退回"正在推进"。
    pub oldest_running_ms: u64,
}

impl DeskPulse {
    /// 从运行时快照提取。
    pub fn from_snapshot(snap: &RuntimeSnapshot) -> Self {
        let mut oldest = 0u64;
        for slot in &snap.queue.slots {
            if let Some(running) = &slot.running {
                let started = running.started_at_ms;
                if started != 0 && (oldest == 0 || started < oldest) {
                    oldest = started;
                }
            }
        }
        Self {
            running_count: snap.queue.running_count,
            oldest_running_ms: oldest,
        }
    }

    /// 当前该播的进度类通知；没有任务在跑时返回 `None`。
    ///
    /// `marks_ms` = 生效档位表（升序、无 0、长度 ≤ [`NoticeKind::PROGRESS_MARKS`] 的长度），
    /// 由 [`NoticePolicy`] 从配置归一化而来。表短于闭集时**后面几档就不存在**
    /// （只配 1 个时刻 ⇒ 只会播"已运行一分钟"，不会播 5 分钟 / 15 分钟）。
    ///
    /// 档位是**单调递增**的，所以只要调用方每次都走 [`NoticeGate::should_emit`]，
    /// 档位跃迁会自然变成"每档恰好播一次"，不需要额外记录"播过哪几档"。
    pub fn progress_notice(&self, now_ms: u64, marks_ms: &[u64]) -> Option<NoticeKind> {
        if self.running_count == 0 || self.oldest_running_ms == 0 {
            return None;
        }
        let elapsed = now_ms.saturating_sub(self.oldest_running_ms);
        // 从**最高档往低档**扫，取第一个跨过的。正向扫也能得到"跨过的最低档"，
        // 但这里反向扫有一个额外好处：档位表万一比闭集长（配置被别的工具改坏），
        // `PROGRESS_MARKS.get(i)` 返回 `None` 而跳过，自然落回**还能表达的最高档**，
        // 而不是把"跑了 20 分钟"播成"已运行一分钟"。
        for (i, mark) in marks_ms.iter().enumerate().rev() {
            // ⚠️ 0 是"这一档没启用"，**必须跳过**。
            // `elapsed >= 0` 恒真，漏了这行的话：留空最后一档 ⇒ 任务刚起步就宣称
            // "已运行十五分钟"。这种谎话比不播更伤 —— 它让人对整套播报失去信任。
            if *mark == 0 {
                continue;
            }
            if elapsed >= *mark {
                if let Some(kind) = NoticeKind::PROGRESS_MARKS.get(i) {
                    return Some(*kind);
                }
            }
        }
        Some(NoticeKind::Progressing)
    }
}

/// 生效的播报策略 —— `NoticeConfig` **归一化之后**的形态。
///
/// # 为什么不直接吃配置
///
/// 配置是用户（或其他工具、旧版本）写在磁盘上的 JSON，里面有三类东西**不能直接信**：
///
/// | 不可信的东西 | 直接吃进去会怎样 | 归一化后 |
/// |---|---|---|
/// | 档位表（可能 5 项 / 乱序 / 含 0） | 下标越界，或把"跑了 20 分钟"播成"已运行一分钟" | 过滤 0 → 升序 → 去重 → 截到闭集容量 |
/// | 最小间隔（可能 0 或极大） | 0 天然等于"不限"（不必特判），极大等于"几乎不播"（用户意图） | 原样保留，语义交给 [`NoticeGate::should_emit`] |
/// | 四个分类开关 | 漏掉任一 `NoticeKind` ⇒ 那条播报**永远发不出去且不报错** | [`Self::allows`] 用 `match` 穷尽，新增变体时编译器会拦 |
///
/// # 配置里"多写一档"为什么是静默截断而不是报错
///
/// 用户的意图是"我想多一档提醒"。我们没有那句话（没有对应的固定语料），
/// 报错并不能把语料变出来，只会让配置保存失败。所以退化成"还能表达的最高档"，
/// 并由 UI 用 [`Self::MARK_CAPACITY`] 明确提示上限。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoticePolicy {
    /// 总开关。关掉 ⇒ 一句都不播。
    ///
    /// ⚠️ **只管喇叭，不管灯。** 灯是另一条信道（多路、零成本、不打断），
    /// 它的存在恰恰是为了让人不必靠声音去获知状态。把两者绑在一个开关上，
    /// 会让"我只想安静一点"变成"我连进度都看不见了"。
    pub enabled: bool,
    /// 任务开始（`TaskStarted`）。
    pub on_start: bool,
    /// 进度类（`Progressing` / `Running1Min` / `Running5Min` / `Running15Min`）。
    pub on_progress: bool,
    /// 成功类（`Completed` / `CompletedArtifactFailed`）。
    pub on_complete: bool,
    /// 异常类（`Failed` / `NeedsConfirmation` / `TimedOut` / `LostContact`）。
    pub on_fail: bool,
    /// 同类 Notice 的最小间隔（毫秒）。0 = 不限。
    pub min_interval_ms: u64,
    /// 生效档位表：升序、无 0、长度 ≤ [`Self::MARK_CAPACITY`]。
    pub progress_marks_ms: Vec<u64>,
}

impl Default for NoticePolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            on_start: true,
            on_progress: true,
            on_complete: true,
            on_fail: true,
            min_interval_ms: DEFAULT_MIN_INTERVAL_MS,
            progress_marks_ms: DEFAULT_PROGRESS_MARKS_MS.to_vec(),
        }
    }
}

impl NoticePolicy {
    /// 档位容量：语料闭集里**能表达**几档。
    ///
    /// UI 用它提示上限（"最多 N 档"）。它由 [`NoticeKind::PROGRESS_MARKS`] 推导，
    /// 因此加档时不会忘记同步文案。
    pub const MARK_CAPACITY: usize = NoticeKind::PROGRESS_MARKS.len();

    /// 从配置的原始七元组建策略，并归一化掉不可信的部分。
    ///
    /// 刻意收**裸字段**而不是 `&NoticeConfig`：这样这个模块不必依赖 `config` 模块，
    /// 单测可以直接造"一个乱序、带 0、超长的档位表"而不用去构造一份完整配置。
    pub fn normalize(
        enabled: bool,
        on_start: bool,
        on_progress: bool,
        on_complete: bool,
        on_fail: bool,
        min_interval_ms: u64,
        progress_marks_ms: &[u64],
    ) -> Self {
        // ---- 档位表必须**按位置**保留，不能压实 ----
        //
        // 界面上是固定的三个输入框「第 1 / 2 / 3 档」，第 i 个框永远对应
        // [`NoticeKind::PROGRESS_MARKS`][i] 那一句话。所以 0 的含义是
        // **"这一档我不用"**，而不是"这一项不存在" —— 压实它会让后面的档整体前移，
        // 于是界面写着「第 2 档 5 分钟」，实际却在 1 分钟的位置播了"已运行五分钟"。
        // 那种错是**说明与实现不一致**，而且只在用户留空中间某一档时才复现。
        let mut marks: Vec<u64> = progress_marks_ms
            .iter()
            .copied()
            .take(Self::MARK_CAPACITY)
            .collect();
        marks.resize(Self::MARK_CAPACITY, 0);

        // 单调修正：**后面的档不得早于前面的档**。
        // 用户把两档填反了（第 1 档 5 分钟、第 2 档 1 分钟）时，
        // 不许出现"先播五分钟、再播一分钟"这种倒着走的路标。
        // 刻意**不排序**：排序会把用户填在第 2 个框的值挪到第 1 个框上，
        // 等于替他改了文案归属。
        let mut floor = 0u64;
        for m in marks.iter_mut() {
            if *m == 0 {
                continue; // 未启用，既不参与比较也不抬别人
            }
            *m = (*m).max(floor);
            floor = floor.max(*m);
        }
        Self {
            enabled,
            on_start,
            on_progress,
            on_complete,
            on_fail,
            min_interval_ms,
            progress_marks_ms: marks,
        }
    }

    /// 这一条在不在允许范围内。
    ///
    /// # 用 `match` 而不是 `matches!` 链是刻意的
    ///
    /// `match` 对 `NoticeKind` 是穷尽的：将来加一个变体却忘了归组，
    /// **编译不过**。若写成 `matches!` 白名单，新增变体会静默落到"不在任何组里" ⇒
    /// 那条播报永远发不出去，且没有任何征兆。这正是本模块最想防的失效模式。
    pub fn allows(&self, kind: NoticeKind) -> bool {
        if !self.enabled {
            return false;
        }
        match kind {
            NoticeKind::TaskStarted => self.on_start,
            NoticeKind::Progressing
            | NoticeKind::Running1Min
            | NoticeKind::Running5Min
            | NoticeKind::Running15Min => self.on_progress,
            // "成功但产物交付失败"归**完成组**：它是成功路径上的异常。
            // 用户用"完成"这个开关来管它，比塞进"失败"组更符合直觉 ——
            // 任务确实跑完了，只是东西没落地。
            NoticeKind::Completed | NoticeKind::CompletedArtifactFailed => self.on_complete,
            NoticeKind::Failed
            | NoticeKind::NeedsConfirmation
            | NoticeKind::TimedOut
            | NoticeKind::LostContact => self.on_fail,
            // 合并提醒归**完成组**：它说的是"还有几条完成的结果没念"。
            NoticeKind::ResultsWaiting => self.on_complete,
        }
    }
}

/// 播报去抖器。
///
/// # 两套账本，刻意分开
///
/// | 类别 | 账本 | 规则 |
/// |---|---|---|
/// | **进度类**（`is_debounceable`） | 全局一本（`last_kind` + `last_spoken_at_ms`） | 跃迁才播 + 最小间隔 |
/// | **事件类**（起点 / 终态） | **按 `(slot, kind)` 一本** | 3 秒内重复送达才沉默 |
///
/// # 为什么事件类必须按槽记账（v0.41.15 实锤的 BUG）
///
/// 旧版两段共用全局 `last_kind`："S1 完成 ⇒ last_kind = Completed"，
/// 于是 **S2 完成被判定为"与上一条相同"而吞掉** —— 日志里连播报行都没有，
/// 用户只看到未读 +1，没有任何语音。四槽并行时，"哪一槽发生了什么"
/// **必须能分别说出来**：S1 完成与 S2 完成是两件事，不是一件事说了两遍。
///
/// 3 秒重复 guard 保留的是它真正该防的东西：**同一件事被同拍双重送达**
/// （派发那一刻「任务已开始」与「正在推进」两个通知源同时触发，见 `dispatch_task`）。
/// 同一槽同一 kind 在 3 秒后再出现，允许再播 —— 那更可能是两条真实任务。
///
/// # 进度类为什么保持全局
///
/// 进度本身就是**全局量**：`DeskPulse` 取"最老的那个任务"驱动档位，
/// 没有"这一槽的进度"可言。全局跃迁同时承担"轮询不刷屏"与"档位恰好各播一次"。
///
/// # 被拦下时**不**更新账本（两类都是刻意的）
///
/// - 进度类被间隔拦下 ⇒ 不更新 `last_kind`：下次轮询还会再试（进度迟到几秒无所谓，
///   但状态记录要诚实）。
/// - 事件类被 3 秒 guard 拦下 ⇒ 不刷新时间戳：否则持续重复送达会把守卫一直续期，
///   让真正的新事件永远等不到头。
#[derive(Clone, Debug, Default)]
pub struct NoticeGate {
    /// 生效策略。默认 = 出厂默认（见 [`NoticePolicy::default`]）。
    policy: NoticePolicy,
    /// 上一条**真的出声过**的通知。`None` = 还没播过任何一条。
    last_kind: Option<NoticeKind>,
    /// 上一条真的出声的时刻（unix 毫秒）。0 = 从未出声（不是"1970 年"）。
    last_spoken_at_ms: u64,
    /// 事件类的去重账本：`(槽位, kind) → 上次出声时刻`。
    ///
    /// `None` 槽位 = 无主通知（外部会话监工、合并提醒）。
    last_event: HashMap<(Option<usize>, NoticeKind), u64>,
}

impl NoticeGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// 换策略（配置保存时调用）。
    ///
    /// ⚠️ **刻意不动去抖账本。** 只改个间隔就"当作从没播过"，会让下一次状态变化
    /// 把上一条重复播一遍 —— 那是**假信息**（"任务已完成"说了两次，用户会以为
    /// 完成了两件事）。策略是"以后怎么播"，账本是"刚才播过什么"，两件事。
    pub fn set_policy(&mut self, policy: NoticePolicy) {
        self.policy = policy;
    }

    /// 当前生效策略（只读）。进度档位表由此取用。
    pub fn policy(&self) -> &NoticePolicy {
        &self.policy
    }

    /// 判定这条通知现在该不该出声。**无槽通知**（监工 / 合并提醒）走这支。
    ///
    /// 等价于 `should_emit_for(kind, None, now_ms)`，保留它是为了让不关心槽位的
    /// 调用方（探针、试听）不必每次传一个没有信息的 `None`。
    pub fn should_emit(&mut self, kind: NoticeKind, now_ms: u64) -> bool {
        self.should_emit_for(kind, None, now_ms)
    }

    /// 判定这条通知现在该不该出声，**按槽位分开记账**。
    ///
    /// 返回 `true` 即表示"已经记账，调用方应当把它播出去"。
    ///
    /// `slot` 只对**事件类**（起点 / 终态）有意义：进度类是全局量（见结构体文档），
    /// 传什么都会被忽略。
    pub fn should_emit_for(&mut self, kind: NoticeKind, slot: Option<usize>, now_ms: u64) -> bool {
        // 规则 0：分类开关（含总开关）。关掉的组**不记账**（与"被间隔拦下"同理）——
        // 用户后来把开关打开时，当前状态该说的那句话还应该说得出来。
        if !self.policy.allows(kind) {
            return false;
        }

        if kind.is_debounceable() {
            return self.emit_debounceable(kind, now_ms);
        }

        // ---- 事件类：按 (slot, kind) 记账 ----
        //
        // 3 秒 guard 只挡"同一件事被同拍双重送达"（派发那一刻两个通知源同时触发）。
        // 它**不**承担"同类不重复播"——那是旧版全局账本做的事，也正是它吞掉了
        // S2 的完成播报。同一槽同一 kind 隔 3 秒再来，允许再播。
        let key = (slot, kind);
        if let Some(&at) = self.last_event.get(&key) {
            if now_ms.saturating_sub(at) < EVENT_REPEAT_GUARD_MS {
                return false;
            }
        }
        self.last_event.insert(key, now_ms);
        // 全局账本照记：进度类的最小间隔要参考"上一条真的出声"。
        self.last_kind = Some(kind);
        self.last_spoken_at_ms = now_ms;
        true
    }

    /// 进度类：全局跃迁 + 最小间隔（旧 `should_emit` 的主体，原样搬来）。
    fn emit_debounceable(&mut self, kind: NoticeKind, now_ms: u64) -> bool {
        // 规则 1：跃迁才播。
        if self.last_kind == Some(kind) {
            return false;
        }

        // 规则 2：可去抖的那几条受最小间隔约束。间隔为 0 时 `elapsed < 0` 恒假 ⇒
        // 天然等于"不限"，不需要给 0 写特判分支。
        if self.last_spoken_at_ms != 0 {
            let elapsed = now_ms.saturating_sub(self.last_spoken_at_ms);
            if elapsed < self.policy.min_interval_ms {
                // 拦下这条，且**不更新 last_kind** ⇒ 下次轮询还会再试。
                return false;
            }
        }

        self.last_kind = Some(kind);
        self.last_spoken_at_ms = now_ms;
        true
    }

    /// 上一条真的出声过的通知（诊断用）。
    pub fn last_kind(&self) -> Option<NoticeKind> {
        self.last_kind
    }

    /// 复位（任务台重载 / 测试用）。
    ///
    /// ⚠️ 会把 `last_spoken_at_ms` 归零，于是**下一条通知必定能出声** ——
    /// 这是复位该有的语义（"当作从没播过"），不是漏洞。
    pub fn reset(&mut self) {
        self.last_kind = None;
        self.last_spoken_at_ms = 0;
        self.last_event.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_desk::queue::QueuedTask;
    use crate::agent_desk::queue::RunningTask;
    use crate::agent_desk::state::DeskRuntime;

    /// 造一个"某槽正在跑、已跑了 `elapsed` 毫秒"的快照。
    fn snapshot_with_running(elapsed_ms: u64, now_ms: u64) -> RuntimeSnapshot {
        let mut rt = DeskRuntime::new(4);
        rt.queue_mut()
            .enqueue(0, "跑个任务", "cli", now_ms.saturating_sub(elapsed_ms))
            .expect("入队应当成功");
        rt.queue_mut()
            .start_next(0, now_ms.saturating_sub(elapsed_ms))
            .expect("应当能启动");
        rt.snapshot()
    }

    fn snapshot_idle() -> RuntimeSnapshot {
        DeskRuntime::new(4).snapshot()
    }

    /// 出厂档位表。大多数档位测试用的就是它。
    fn marks() -> Vec<u64> {
        DEFAULT_PROGRESS_MARKS_MS.to_vec()
    }

    // ---------- 闭集本身 ----------

    #[test]
    fn all_covers_every_variant_in_code_order() {
        for (index, kind) in NoticeKind::ALL.iter().enumerate() {
            assert_eq!(
                kind.code() as usize,
                index,
                "ALL 的顺序必须是 code 顺序 —— 缓存文件名依赖它"
            );
        }
    }

    #[test]
    fn every_notice_has_non_empty_text() {
        for kind in NoticeKind::ALL {
            assert!(!kind.text().is_empty(), "{kind:?} 的语料不能为空");
        }
    }

    #[test]
    fn slugs_are_unique_and_ascii_only() {
        // slug 直接进文件名。中文/空格在跨工具链时是本项目踩过的坑（脚本、压缩包、
        // 其它 agent 的文件操作），所以这里把它钉死。
        for kind in NoticeKind::ALL {
            let slug = kind.slug();
            assert!(!slug.is_empty(), "{kind:?} 的 slug 不能为空");
            assert!(
                slug.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()),
                "{kind:?} 的 slug 只能是小写字母/数字/下划线：{slug}"
            );
        }
        let mut slugs: Vec<&str> = NoticeKind::ALL.iter().map(|k| k.slug()).collect();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), NoticeKind::ALL.len(), "slug 必须唯一");
    }

    #[test]
    fn terminal_and_progress_partition_the_closure() {
        for kind in NoticeKind::ALL {
            let non_terminal = matches!(
                kind,
                NoticeKind::TaskStarted
                    | NoticeKind::Progressing
                    | NoticeKind::Running1Min
                    | NoticeKind::Running5Min
                    | NoticeKind::Running15Min
                    // 合并提醒是 meta 通知：不决定任何任务的结束，因此不是终态。
                    | NoticeKind::ResultsWaiting
            );
            assert_eq!(
                kind.is_terminal(),
                !non_terminal,
                "{kind:?} 的终态分类与非终态分类必须互补"
            );
        }
    }

    #[test]
    fn completed_and_artifact_failed_are_distinct_wording() {
        // 措辞纪律：产物没落地时绝不许说"已完成"。
        assert_ne!(
            NoticeKind::Completed.text(),
            NoticeKind::CompletedArtifactFailed.text()
        );
        assert!(NoticeKind::CompletedArtifactFailed
            .text()
            .contains("失败"));
    }

    // ---------- 档位推导 ----------

    #[test]
    fn progress_ladder_advances_monotonically() {
        let pulse = DeskPulse {
            running_count: 1,
            oldest_running_ms: 1_000,
        };
        let m = marks();
        assert_eq!(
            pulse.progress_notice(10_000, &m),
            Some(NoticeKind::Progressing)
        );
        assert_eq!(
            pulse.progress_notice(1_000 + m[0], &m),
            Some(NoticeKind::Running1Min)
        );
        assert_eq!(
            pulse.progress_notice(1_000 + m[1], &m),
            Some(NoticeKind::Running5Min)
        );
        assert_eq!(
            pulse.progress_notice(1_000 + m[2], &m),
            Some(NoticeKind::Running15Min)
        );
        // 再久也不会有第四档 —— 档位是闭集，不是连续值。
        assert_eq!(
            pulse.progress_notice(1_000 + m[2] * 10, &m),
            Some(NoticeKind::Running15Min)
        );
    }

    #[test]
    fn progress_ladder_length_follows_the_mark_table() {
        // 档数由**配置给几个时刻**决定，上限由语料闭集决定。
        // `oldest_running_ms` 必须是真实时刻 —— 0 是"没有任务在跑"的哨兵值。
        let pulse = DeskPulse {
            running_count: 1,
            oldest_running_ms: 1_000,
        };
        let now = 1_000 + 10 * 60_000; // 已跑 10 分钟

        // 只配一档 ⇒ 就算跑了 10 分钟，也只能说"已运行一分钟"（没有第 2 句语料）。
        assert_eq!(
            pulse.progress_notice(now, &[60_000]),
            Some(NoticeKind::Running1Min)
        );
        // 空表 ⇒ 连时长档都没有，只剩"正在推进"。
        assert_eq!(
            pulse.progress_notice(now, &[]),
            Some(NoticeKind::Progressing)
        );
        // 出厂表 ⇒ 10 分钟落在第 2 档。
        assert_eq!(
            pulse.progress_notice(now, &marks()),
            Some(NoticeKind::Running5Min)
        );
    }

    #[test]
    fn unused_marks_keep_their_position() {
        // 第 i 个输入框永远对应 PROGRESS_MARKS[i] 那句话。
        // 旧实现把 0 压实（filter），于是"第 1 档留空"会把第 2 档推到第 1 位
        // ⇒ 界面写着「第 2 档 5 分钟」，实际却在 1 分钟的位置播"已运行五分钟"。
        let p = NoticePolicy::normalize(true, true, true, true, true, 0, &[0, 300_000, 0]);
        assert_eq!(
            p.progress_marks_ms,
            vec![0, 300_000, 0],
            "0 必须留在原位表示这一档不用"
        );
        assert_eq!(p.min_interval_ms, 0, "0 是合法的'不限'，不该被改掉");
    }

    #[test]
    fn disabled_marks_never_fire_immediately() {
        // ⛔ 这是"留空一档"时最容易踩的坑：`elapsed >= 0` 恒真，
        // 不跳过 0 的话，任务刚起步就会宣称"已运行十五分钟"。
        let pulse = DeskPulse {
            running_count: 1,
            oldest_running_ms: 1_000,
        };
        assert_eq!(
            pulse.progress_notice(1_500, &[0, 0, 0]),
            Some(NoticeKind::Progressing),
            "全部留空 ⇒ 永远只说「正在推进」"
        );
        assert_eq!(
            pulse.progress_notice(1_500, &[0, 0, 900_000]),
            Some(NoticeKind::Progressing),
            "只配第 3 档时，没到点不许播第 3 档"
        );
    }

    #[test]
    fn a_higher_mark_is_never_reached_before_a_lower_one() {
        // 用户把两档填反了 ⇒ 不许出现"先播五分钟、再播一分钟"。
        let p = NoticePolicy::normalize(true, true, true, true, true, 0, &[300_000, 60_000]);
        assert!(p.progress_marks_ms[1] >= p.progress_marks_ms[0]);
    }

    #[test]
    fn overtime_table_is_truncated_without_an_error() {
        // 超长表被截到闭集容量，且**不报错**（报错也变不出第 4 句语料）。
        let long = NoticePolicy::normalize(true, true, true, true, true, 1_000, &[1, 2, 3, 4, 5, 6]);
        assert_eq!(long.progress_marks_ms.len(), NoticePolicy::MARK_CAPACITY);
    }

    #[test]
    fn mark_capacity_matches_the_corpus_closure() {
        // 这条是给"将来加档"的人看的：加一句语料就必须同步进 PROGRESS_MARKS，
        // 否则配置里多写的档会被静默截断，而 UI 提示的上限是错的。
        assert_eq!(
            NoticePolicy::MARK_CAPACITY,
            NoticeKind::PROGRESS_MARKS.len()
        );
        for kind in NoticeKind::PROGRESS_MARKS {
            assert!(
                kind.is_debounceable(),
                "{kind:?} 属于进度档位表，就该是可去抖的"
            );
        }
    }

    #[test]
    fn idle_pulse_has_no_progress_notice() {
        let pulse = DeskPulse::from_snapshot(&snapshot_idle());
        assert_eq!(pulse.running_count, 0);
        assert_eq!(pulse.progress_notice(999_999, &marks()), None);
    }

    #[test]
    fn pulse_picks_the_oldest_running_task() {
        // 槽 0 跑了 10 分钟，槽 1 刚起来 2 秒 ⇒ 进度应由"最久的那件"驱动。
        let mut rt = DeskRuntime::new(4);
        rt.queue_mut().enqueue(0, "老任务", "cli", 1_000).unwrap();
        rt.queue_mut().start_next(0, 1_000).unwrap();
        rt.queue_mut().enqueue(1, "新任务", "cli", 598_000).unwrap();
        rt.queue_mut().start_next(1, 598_000).unwrap();

        let pulse = DeskPulse::from_snapshot(&rt.snapshot());
        assert_eq!(pulse.running_count, 2);
        assert_eq!(pulse.oldest_running_ms, 1_000, "应当取最早开始的那个");
        assert_eq!(
            pulse.progress_notice(601_000, &marks()),
            Some(NoticeKind::Running5Min),
            "老任务已跑 600 秒 ⇒ 5 分钟档，不受新任务影响"
        );
    }

    #[test]
    fn pulse_ignores_slots_that_are_not_running() {
        let mut rt = DeskRuntime::new(4);
        // 只入队、不启动 ⇒ 有积压但没在跑 ⇒ 没有进度可播。
        rt.queue_mut().enqueue(2, "排队中", "cli", 1_000).unwrap();
        let pulse = DeskPulse::from_snapshot(&rt.snapshot());
        assert_eq!(pulse.running_count, 0);
        assert_eq!(pulse.progress_notice(999_999, &marks()), None);
    }

    // ---------- 3.8 的验收：去抖 ----------

    #[test]
    fn ten_polls_produce_at_most_one_notice() {
        // 方案 §Phase 3 3.8 的原始验收：连续 10 次轮询只产生 ≤1 次播报。
        let mut gate = NoticeGate::new();
        let pulse = DeskPulse {
            running_count: 1,
            oldest_running_ms: 1_000,
        };

        let mut spoken = 0;
        for tick in 0..10u64 {
            let now = 2_000 + tick * 100; // 10 次轮询跨 900 毫秒
            if let Some(kind) = pulse.progress_notice(now, &marks()) {
                if gate.should_emit(kind, now) {
                    spoken += 1;
                }
            }
        }
        assert_eq!(spoken, 1, "10 次轮询最多出声一次");
    }

    #[test]
    fn repeating_the_same_kind_does_not_refresh_the_interval() {
        // 反面教材：若重复送达也刷新时间戳，持续运行的轮询会把间隔一直往后推，
        // 下一条真通知就永远等不到间隔。
        let mut gate = NoticeGate::new();
        let start = 1_000u64;
        assert!(gate.should_emit(NoticeKind::Progressing, start));

        // 把整个最小间隔走完：这段时间内每 1 毫秒轮询一次，全部必须沉默。
        // 注意最后一条恰好落在 elapsed == 间隔 处，仍未出声（同 kind）。
        for tick in 1..=DEFAULT_MIN_INTERVAL_MS {
            assert!(
                !gate.should_emit(NoticeKind::Progressing, start + tick),
                "同 kind 重复送达必须沉默（第 {tick} 毫秒）"
            );
        }

        // 关键断言：上面 8000 次重复送达**没有**刷新 last_spoken_at_ms，
        // 所以此刻（距首次出声恰好一个间隔）换一条进度必须立刻能出声。
        // 若实现里误把重复送达也记账，这里的 elapsed 会被算成 0 而被拦下。
        assert!(
            gate.should_emit(NoticeKind::Running1Min, start + DEFAULT_MIN_INTERVAL_MS),
            "重复送达不得刷新间隔时间戳"
        );
    }

    #[test]
    fn terminal_is_never_swallowed_by_the_min_interval() {
        // 这是整套系统最不能犯的错：进度刚播过 2 秒，任务就完成了。
        // 若终态也受间隔约束，"已完成"会被吞掉。
        let mut gate = NoticeGate::new();
        assert!(gate.should_emit(NoticeKind::Progressing, 1_000));
        assert!(
            gate.should_emit(NoticeKind::Completed, 3_000),
            "终态必须无视最小间隔 —— 被吞掉就等于用户永远不知道它结束了"
        );
    }

    #[test]
    fn task_started_is_never_swallowed_either() {
        // 用户刚按了键。此时"没有反应"本身就是坏消息 ⇒ 起点类同样必须送达。
        let mut gate = NoticeGate::new();
        assert!(gate.should_emit(NoticeKind::Progressing, 1_000));
        assert!(gate.should_emit(NoticeKind::TaskStarted, 1_500));
    }

    #[test]
    fn debounced_progress_is_retried_not_forgotten() {
        let mut gate = NoticeGate::new();
        assert!(gate.should_emit(NoticeKind::TaskStarted, 1_000));
        // 1.5 秒后跨过 1 分钟档 —— 被间隔拦下。
        assert!(!gate.should_emit(NoticeKind::Running1Min, 2_500));
        // 但 last_kind 未被推进 ⇒ 下一次轮询（间隔已过）必须还能出声。
        assert!(
            gate.should_emit(NoticeKind::Running1Min, 1_000 + DEFAULT_MIN_INTERVAL_MS),
            "被拦下的进度跃迁必须被重试，而不是静默丢弃"
        );
    }

    #[test]
    fn only_one_notice_per_ladder_step() {
        let mut gate = NoticeGate::new();
        let base = 1_000u64;
        let pulse = DeskPulse {
            running_count: 1,
            oldest_running_ms: base,
        };

        let mut spoken = Vec::new();
        // 每分钟轮询一次，跑 7 分钟。
        for minute in 0..=7u64 {
            let now = base + minute * 60_000;
            if let Some(kind) = pulse.progress_notice(now, &marks()) {
                if gate.should_emit(kind, now) {
                    spoken.push(kind);
                }
            }
        }

        assert_eq!(
            spoken,
            vec![
                NoticeKind::Progressing,
                NoticeKind::Running1Min,
                NoticeKind::Running5Min
            ],
            "三个档位各恰好播一次"
        );
    }

    #[test]
    fn a_new_kind_after_a_long_silence_always_speaks() {
        let mut gate = NoticeGate::new();
        assert!(gate.should_emit(NoticeKind::Completed, 1_000));
        assert!(
            gate.should_emit(NoticeKind::Failed, 10_000_000),
            "间隔远超过最小间隔后，换一条必须能出声"
        );
    }

    #[test]
    fn clock_going_backwards_does_not_panic_or_speak() {
        let mut gate = NoticeGate::new();
        assert!(gate.should_emit(NoticeKind::Progressing, 500_000));
        // 时间倒流 ⇒ saturating_sub 得 0 ⇒ 视为"间隔未到"。
        assert!(!gate.should_emit(NoticeKind::Running1Min, 1_000));
    }

    #[test]
    fn reset_lets_the_next_notice_speak() {
        let mut gate = NoticeGate::new();
        assert!(gate.should_emit(NoticeKind::Completed, 1_000));
        assert!(!gate.should_emit(NoticeKind::Completed, 1_100));
        gate.reset();
        assert!(gate.should_emit(NoticeKind::Completed, 1_200));
        assert_eq!(gate.last_kind(), Some(NoticeKind::Completed));
    }

    // ---------- 事件类按 (槽位, kind) 记账（修「S2 完成被吞」的回归测试） ----------

    #[test]
    fn two_slots_completing_both_get_to_speak() {
        // v0.41.15 实锤 BUG：S1 完成 ⇒ 全局 last_kind = Completed ⇒
        // S2 完成被判"与上一条相同"吞掉，日志里连播报行都没有，用户只看到未读 +1。
        let mut gate = NoticeGate::new();
        assert!(gate.should_emit_for(NoticeKind::Completed, Some(0), 1_000));
        assert!(
            gate.should_emit_for(NoticeKind::Completed, Some(1), 1_000),
            "S1 说完不该堵住 S2 的嘴 —— 四槽并行时每槽的终态都必须能分别说出来"
        );
    }

    #[test]
    fn the_same_slot_repeating_within_the_guard_is_silent() {
        // guard 防的是"同一件事被同拍双重送达"（派发那一刻两个通知源同时触发）。
        let mut gate = NoticeGate::new();
        assert!(gate.should_emit_for(NoticeKind::TaskStarted, Some(0), 1_000));
        assert!(!gate.should_emit_for(NoticeKind::TaskStarted, Some(0), 1_500));
        assert!(!gate.should_emit_for(NoticeKind::TaskStarted, Some(0), 1_000 + EVENT_REPEAT_GUARD_MS - 1));
    }

    #[test]
    fn the_same_slot_speaks_again_after_the_guard() {
        // guard 到期后允许再播：同一槽紧接着完成的第二条任务是真实事件，
        // 未读与历史都记着，没道理连一句话都不给。
        let mut gate = NoticeGate::new();
        assert!(gate.should_emit_for(NoticeKind::Completed, Some(0), 1_000));
        assert!(gate.should_emit_for(
            NoticeKind::Completed,
            Some(0),
            1_000 + EVENT_REPEAT_GUARD_MS
        ));
    }

    #[test]
    fn slotless_events_use_the_none_key() {
        // 无主通知（外部会话监工）之间也按同一个 key 去重。
        let mut gate = NoticeGate::new();
        assert!(gate.should_emit_for(NoticeKind::Completed, None, 1_000));
        assert!(!gate.should_emit_for(NoticeKind::Completed, None, 1_500));
        assert!(
            gate.should_emit_for(NoticeKind::Completed, Some(0), 1_500),
            "槽位通知与无主通知互不遮挡"
        );
    }

    #[test]
    fn progress_stays_on_the_global_ledger_regardless_of_slot() {
        // 进度是全局量（由最老的任务驱动），没有"这一槽的进度"。
        let mut gate = NoticeGate::new();
        assert!(gate.should_emit_for(NoticeKind::Progressing, Some(0), 1_000));
        assert!(
            !gate.should_emit_for(NoticeKind::Progressing, Some(1), 2_000),
            "另一槽的进度不该让全局再播一次「正在推进」"
        );
    }

    #[test]
    fn reset_also_clears_the_per_slot_ledger() {
        let mut gate = NoticeGate::new();
        assert!(gate.should_emit_for(NoticeKind::Completed, Some(0), 1_000));
        assert!(!gate.should_emit_for(NoticeKind::Completed, Some(0), 1_500));
        gate.reset();
        assert!(gate.should_emit_for(NoticeKind::Completed, Some(0), 1_600));
    }

    #[test]
    fn a_terminal_still_updates_the_global_ledger_for_progress_spacing() {
        // 事件类出声后，进度类仍要受最小间隔约束 ——
        // 否则"刚说完完成"立刻又来一句"正在推进"（同一槽的下一个任务）。
        let mut gate = NoticeGate::new();
        assert!(gate.should_emit_for(NoticeKind::Completed, Some(0), 1_000));
        assert!(!gate.should_emit_for(NoticeKind::Progressing, Some(0), 1_500));
        assert!(gate.should_emit_for(
            NoticeKind::Progressing,
            Some(0),
            1_000 + DEFAULT_MIN_INTERVAL_MS
        ));
    }

    #[test]
    fn debounceable_set_is_exactly_the_progress_kinds() {
        // "必须送达"就是"不可去抖"—— 两者是同一件事的两面，不是两套规则。
        for kind in NoticeKind::ALL {
            assert_eq!(kind.is_terminal(), !matches!(
                kind,
                NoticeKind::TaskStarted
                    | NoticeKind::Progressing
                    | NoticeKind::Running1Min
                    | NoticeKind::Running5Min
                    | NoticeKind::Running15Min
                    | NoticeKind::ResultsWaiting
            ));
        }
        assert!(NoticeKind::Progressing.is_debounceable());
        assert!(NoticeKind::Running1Min.is_debounceable());
        assert!(NoticeKind::Running5Min.is_debounceable());
        assert!(NoticeKind::Running15Min.is_debounceable());
        assert!(!NoticeKind::Completed.is_debounceable());
        assert!(!NoticeKind::Failed.is_debounceable());
        assert!(!NoticeKind::LostContact.is_debounceable());
    }

    // ---------- 配置真的生效（修「NoticeConfig 是死配置」的回归测试） ----------

    #[test]
    fn policy_switch_actually_mutes_that_group() {
        // 关掉进度组：进度一条都不许出声，但主线提醒仍须送达。
        let mut gate = NoticeGate::new();
        gate.set_policy(NoticePolicy::normalize(true, true, false, true, true, 8_000, &marks()));
        assert!(!gate.should_emit(NoticeKind::Running1Min, 1_000));
        assert!(!gate.should_emit(NoticeKind::Progressing, 1_000));
        assert!(
            gate.should_emit(NoticeKind::Completed, 1_000),
            "关掉进度不该连完成都一起关掉"
        );
    }

    #[test]
    fn master_switch_mutes_everything() {
        let mut gate = NoticeGate::new();
        gate.set_policy(NoticePolicy::normalize(false, true, true, true, true, 8_000, &marks()));
        for kind in NoticeKind::ALL {
            assert!(!gate.should_emit(kind, 1_000 + kind.code() as u64), "{kind:?} 应被总开关关掉");
        }
    }

    #[test]
    fn muted_kinds_are_not_recorded_so_they_can_speak_later() {
        // 关着的时候"没播"不算播过 —— 否则用户把开关打开后，当前该说的那句话
        // 会因为 last_kind 已经是它而永远不再出声。
        let mut gate = NoticeGate::new();
        gate.set_policy(NoticePolicy::normalize(true, true, false, true, true, 8_000, &marks()));
        assert!(!gate.should_emit(NoticeKind::Progressing, 1_000));
        // 打开进度组：同一个 kind 必须还能出声。
        gate.set_policy(NoticePolicy::normalize(true, true, true, true, true, 8_000, &marks()));
        assert!(gate.should_emit(NoticeKind::Progressing, 1_000));
    }

    #[test]
    fn configured_min_interval_is_the_one_that_applies() {
        // 配 30 秒：8 秒后必须仍然沉默（出厂默认是 8 秒，若配置没生效这条会挂）。
        let mut gate = NoticeGate::new();
        gate.set_policy(NoticePolicy::normalize(true, true, true, true, true, 30_000, &marks()));
        assert!(gate.should_emit(NoticeKind::Progressing, 1_000));
        assert!(!gate.should_emit(NoticeKind::Running1Min, 9_000), "9 秒 < 配的 30 秒");
        assert!(gate.should_emit(NoticeKind::Running1Min, 31_000));
    }

    #[test]
    fn configured_marks_move_the_ladder() {
        // 把 1 分钟档挪到 10 秒 —— 配置生效的话，10 秒就该是第 1 档。
        let mut gate = NoticeGate::new();
        gate.set_policy(NoticePolicy::normalize(true, true, true, true, true, 0, &[10_000]));
        let pulse = DeskPulse {
            running_count: 1,
            oldest_running_ms: 1_000,
        };
        let m = gate.policy().progress_marks_ms.clone();
        assert_eq!(
            pulse.progress_notice(11_000, &m),
            Some(NoticeKind::Running1Min)
        );
    }

    #[test]
    fn changing_policy_does_not_rewind_the_debounce_ledger() {
        // 只改间隔不该让上一条重复播一遍 —— 那是假信息（"已完成"说两次）。
        let mut gate = NoticeGate::new();
        assert!(gate.should_emit(NoticeKind::Completed, 1_000));
        gate.set_policy(NoticePolicy::normalize(true, true, true, true, true, 1, &marks()));
        assert!(
            !gate.should_emit(NoticeKind::Completed, 1_100),
            "策略变更不得清空去抖账本"
        );
    }

    #[test]
    fn default_policy_matches_the_factory_defaults() {
        // 防止"引入策略"这一步悄悄改掉出厂行为。
        let p = NoticePolicy::default();
        assert!(p.enabled && p.on_start && p.on_progress && p.on_complete && p.on_fail);
        assert_eq!(p.min_interval_ms, DEFAULT_MIN_INTERVAL_MS);
        assert_eq!(p.progress_marks_ms, DEFAULT_PROGRESS_MARKS_MS.to_vec());
    }

    #[test]
    fn pulse_from_snapshot_reflects_real_queue_state() {
        let snap = snapshot_with_running(5_000, 100_000);
        let pulse = DeskPulse::from_snapshot(&snap);
        assert_eq!(pulse.running_count, 1);
        assert_eq!(pulse.oldest_running_ms, 95_000);
        assert_eq!(
            pulse.progress_notice(100_000, &marks()),
            Some(NoticeKind::Progressing)
        );
    }

    #[test]
    fn running_task_and_queued_task_shapes_are_usable() {
        // 这条只是把"测试用到的两个队列类型确实可用"钉住，
        // 免得将来 QueueSnapshot 改名时测试悄悄失联。
        let task = QueuedTask {
            id: 1,
            slot: 0,
            prompt: "x".into(),
            driver_id: "cli".into(),
            enqueued_at_ms: 10,
        };
        let running = RunningTask {
            task,
            started_at_ms: 20,
        };
        assert_eq!(running.started_at_ms, 20);
    }
}
