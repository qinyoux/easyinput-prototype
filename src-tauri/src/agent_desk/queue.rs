//! 槽位任务队列 —— 每槽一条 FIFO。
//!
//! # 为什么"同槽串行"不是策略，而是物理约束
//!
//! 第一性原理：一个槽位 = **一个按键 + 一颗灯 + 一段播放**。这三样都是单份的。
//!
//! - 播放通道只有一个喇叭。同槽并行跑两个任务，完成后会有两段总结，而用户按一次键
//!   只能听一段——另一段就变成了"幽灵未读"（用户根本不知道它存在）。
//! - 灯的语义是"这个槽有未读"。两个任务抢一颗灯，灯灭的时机就不确定了，
//!   "灯亮 = 有东西听"这条最核心的契约立刻失效。
//!
//! ⇒ **同槽必须严格串行。** 不这么做的话，坏掉的不是性能，是可解释性。
//!
//! 反过来，跨槽**可以且应该并行**：4 个槽各有各的灯、各有各的按键，输出通道可区分；
//! 而执行器（HTTP 请求 / 子进程）本身是 I/O 密集，天然可并行。
//!
//! # 本文件只做状态机
//!
//! 不含任何 I/O、不持有 `Instant`、不 spawn 任务——所有时间戳由调用方注入（`now_ms`）。
//! 这样它是**可完全确定性地单测**的：同样的调用序列，永远得到同样的结果。
//! 真正的执行（调 driver、发事件）在 `mod.rs` 的编排器里。

use std::collections::VecDeque;

use serde::Serialize;

/// 每槽待执行任务上限。
///
/// 不是性能考虑——是**防无界增长**：任务文本来自 ASR，可能几百字；如果某个槽的
/// driver 一直失败而用户一直按，无上限的队列会一直吃内存。8 条已经远超实际使用
/// （正常人不会为一个槽排 8 个任务），到顶就**明确报错**，绝不静默丢弃。
pub const MAX_DEPTH_PER_SLOT: usize = 8;

/// 全局任务号。跨槽唯一、单调递增。
pub type TaskId = u64;

/// 队列里的一条任务。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct QueuedTask {
    /// 全局唯一任务号。
    pub id: TaskId,
    /// 归属槽位（0 基，对外显示时 +1 成 S1–S4）。
    pub slot: usize,
    /// 任务文本（来自 ASR）。
    pub prompt: String,
    /// 路由后确定的执行器 id。**入队时就已经定死**——路由发生在入队之前，
    /// 不在这里做，因为队列的职责是"排队"，不是"决策"。
    pub driver_id: String,
    /// 入队时刻（unix 毫秒，调用方注入）。
    pub enqueued_at_ms: u64,
}

/// 正在执行的某条任务。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RunningTask {
    pub task: QueuedTask,
    /// 开始执行的时刻（unix 毫秒，调用方注入）。
    pub started_at_ms: u64,
}

/// 队列操作失败的原因。
///
/// **每一个变体都是一个不同的病因**，合并成一句"队列错误"会让排错无从下手。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueueError {
    /// 槽位号越界。
    SlotOutOfRange { slot: usize, slots: usize },
    /// 该槽待执行数已达上限。
    QueueFull { slot: usize, max: usize },
    /// 同一槽已有**待执行**的同文本任务（重复触发）。
    DuplicatePending { slot: usize, existing_id: TaskId },
    /// 任务文本为空。
    EmptyPrompt { slot: usize },
    /// 该槽上没有正在执行的任务。
    NotRunning { slot: usize },
    /// 回执的任务号与当前正在执行的不符（迟到回执）。
    TaskIdMismatch {
        slot: usize,
        expected: TaskId,
        actual: TaskId,
    },
}

impl QueueError {
    /// 面向用户的中文说明。
    pub fn message(&self) -> String {
        match self {
            QueueError::SlotOutOfRange { slot, slots } => {
                format!("槽位号 {slot} 越界（有效范围 0..{slots}）")
            }
            QueueError::QueueFull { slot, max } => {
                format!("槽位 S{} 的待执行任务已达上限 {max} 条，请等前面的跑完", slot + 1)
            }
            QueueError::DuplicatePending { slot, existing_id } => format!(
                "同一任务已在槽位 S{} 排队中（任务号 {existing_id}），未重复入队",
                slot + 1
            ),
            QueueError::EmptyPrompt { slot } => {
                format!("槽位 S{} 的任务文本为空，拒绝入队", slot + 1)
            }
            QueueError::NotRunning { slot } => {
                format!("槽位 S{} 当前没有正在执行的任务", slot + 1)
            }
            QueueError::TaskIdMismatch {
                slot,
                expected,
                actual,
            } => format!(
                "槽位 S{} 的完成回执指向任务 {actual}，但当前正在执行的是 {expected}——\
                 已拒绝（迟到回执不得影响新任务）",
                slot + 1
            ),
        }
    }
}

/// 尝试启动下一个任务的结果。
///
/// ⚠️ **为什么不是 `Option<QueuedTask>`**：`Busy` 与 `Idle` 是**两种不同的状态**，
/// 用一个 `None` 表达会让调用方只能猜。调试面板要区分显示、编排器要区分处理
/// （Busy 时什么都不做，Idle 时该让灯回到"空闲"）。状态不明说，调用方就只能猜。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartOutcome {
    /// 已开始执行这条任务。
    Started(Box<QueuedTask>),
    /// 该槽已有任务在跑——新任务**留在队列里**（这是串行语义的正常情形，不是错误）。
    Busy { running_id: TaskId },
    /// 该槽队列为空，没有可启动的任务。
    Idle,
}

/// 单槽的队列视图（进快照）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SlotQueueView {
    pub index: usize,
    pub running: Option<RunningTask>,
    pub pending: Vec<QueuedTask>,
    pub depth: usize,
}

/// 队列快照。`PartialEq` 是给 `state.rs` 做"真变化才发事件"的指纹用的。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct QueueSnapshot {
    /// **正在执行的槽数**（不是任务条目数）。这是第 5 颗灯的输入。
    pub running_count: usize,
    /// 所有槽的待执行总数。
    pub total_pending: usize,
    /// 正在执行的任务各自绑的执行器（去重、已排序，便于直接比较与展示）。
    pub active_drivers: Vec<String>,
    /// 累计完成 / 失败条数（观测用）。
    pub completed: u64,
    pub failed: u64,
    pub slots: Vec<SlotQueueView>,
}

/// 单槽内部状态。
#[derive(Default)]
struct SlotState {
    pending: VecDeque<QueuedTask>,
    running: Option<RunningTask>,
}

/// 每槽一条 FIFO 的任务队列。
pub struct TaskQueue {
    slots: Vec<SlotState>,
    /// 下一个任务号（从 1 开始，0 保留给"无任务"）。
    next_id: TaskId,
    completed: u64,
    failed: u64,
}

impl TaskQueue {
    /// 建一个 `slot_count` 槽的队列。
    pub fn new(slot_count: usize) -> Self {
        let mut slots = Vec::with_capacity(slot_count);
        for _ in 0..slot_count {
            slots.push(SlotState::default());
        }
        Self {
            slots,
            next_id: 1,
            completed: 0,
            failed: 0,
        }
    }

    // -----------------------------------------------------------------------
    // 入队
    // -----------------------------------------------------------------------

    /// 把一条任务放进某槽的队尾。
    ///
    /// # 去重规则（这条容易理解错，写清楚）
    ///
    /// 去重**只看待执行区（pending），不看正在执行的那条（running）**：
    ///
    /// - pending 里已有同文本 → **拒绝**。这几乎必然是**同一次按键的重复触发**
    ///   （长按抖动、报文重发），重复入队会让用户按一次键却排到两条。
    /// - running 里是同文本 → **允许**。那条已经在跑了，用户又按一次说同一句，
    ///   语义是"再跑一轮"（比如"再生成一版"）。这不违反串行——它会等第一轮结束。
    ///
    /// 无论哪种，**都不增加并发**：本槽永远只有一条在跑。
    pub fn enqueue(
        &mut self,
        slot: usize,
        prompt: impl Into<String>,
        driver_id: impl Into<String>,
        now_ms: u64,
    ) -> Result<QueuedTask, QueueError> {
        self.check_slot(slot)?;

        let prompt = prompt.into();
        // 空文本不是一个"无效但可忽略"的输入——它是"识别没出字"或"误触发"，
        // 必须让调用方知道（否则用户按了键、灯没反应，无从判断哪里断了）。
        if prompt.trim().is_empty() {
            return Err(QueueError::EmptyPrompt { slot });
        }
        let driver_id = driver_id.into();

        let slot_state = &mut self.slots[slot];
        if let Some(existing) = slot_state
            .pending
            .iter()
            .find(|t| t.prompt == prompt && t.driver_id == driver_id)
        {
            return Err(QueueError::DuplicatePending {
                slot,
                existing_id: existing.id,
            });
        }
        if slot_state.pending.len() >= MAX_DEPTH_PER_SLOT {
            return Err(QueueError::QueueFull {
                slot,
                max: MAX_DEPTH_PER_SLOT,
            });
        }

        let task = QueuedTask {
            id: self.next_id,
            slot,
            prompt,
            driver_id,
            enqueued_at_ms: now_ms,
        };
        self.next_id += 1;
        slot_state.pending.push_back(task.clone());
        Ok(task)
    }

    // -----------------------------------------------------------------------
    // 出队 / 收尾
    // -----------------------------------------------------------------------

    /// 若该槽空闲且有待执行任务，取一条出来开始执行。
    ///
    /// **这是"同槽串行"的守门人**：它在 `running.is_some()` 时一律返回 `Busy`，
    /// 绝不出队。调用方不需要自己判断——不然每个调用点都要重复一遍这个前提。
    pub fn start_next(&mut self, slot: usize, now_ms: u64) -> Result<StartOutcome, QueueError> {
        self.check_slot(slot)?;
        let slot_state = &mut self.slots[slot];

        if let Some(running) = slot_state.running.as_ref() {
            return Ok(StartOutcome::Busy {
                running_id: running.task.id,
            });
        }
        let Some(task) = slot_state.pending.pop_front() else {
            return Ok(StartOutcome::Idle);
        };
        slot_state.running = Some(RunningTask {
            task: task.clone(),
            started_at_ms: now_ms,
        });
        Ok(StartOutcome::Started(Box::new(task)))
    }

    /// 当前占用该槽运行位的任务（**只读**，不改变任何状态）。
    ///
    /// # 为什么需要它
    ///
    /// [`super::state::dispatch_into`] 是"入队 + 启动"的**复合操作** —— 它在派发那一刻就把
    /// 任务放进了运行位。于是执行泵的第一步**不能**只看 `start_next` 的结果：
    /// 那时它会返回 [`StartOutcome::Busy`]（运行位已被占），而 `Busy` 的真实含义是
    /// "**当前该执行的是这一条**"，不是"没事可做"。认领走这里。
    pub fn running_task(&self, slot: usize) -> Result<Option<QueuedTask>, QueueError> {
        self.check_slot(slot)?;
        Ok(self.slots[slot].running.as_ref().map(|r| r.task.clone()))
    }

    /// 标记某任务**成功**结束，清出运行位。
    ///
    /// `task_id` 必须与当前正在执行的任务一致，否则拒绝——这是防**迟到回执**：
    /// 一个被取消/超时的旧任务，其回执可能在下一个任务已经开始之后才到；
    /// 若不校验，它会把新任务的运行位清掉，表现为"任务还在跑，但系统认为它完了"。
    pub fn finish(&mut self, slot: usize, task_id: TaskId) -> Result<QueuedTask, QueueError> {
        let task = self.take_running(slot, task_id)?;
        self.completed += 1;
        Ok(task)
    }

    /// 标记某任务**失败**结束，清出运行位。与 [`Self::finish`] 对称，仅计数分开。
    pub fn fail(&mut self, slot: usize, task_id: TaskId) -> Result<QueuedTask, QueueError> {
        let task = self.take_running(slot, task_id)?;
        self.failed += 1;
        Ok(task)
    }

    /// 从待执行区撤掉一条任务（**不影响正在执行的那条**）。
    ///
    /// 用户发现排错了，想撤掉——但已经跑起来的那条不能"撤销"，它只能被取消
    /// （由编排器调 cancel token），语义不同，所以是两个方法。
    pub fn cancel_pending(&mut self, slot: usize, task_id: TaskId) -> Result<QueuedTask, QueueError> {
        self.check_slot(slot)?;
        let slot_state = &mut self.slots[slot];
        let pos = slot_state
            .pending
            .iter()
            .position(|t| t.id == task_id)
            .ok_or(QueueError::NotRunning { slot })?;
        // 上面已确认存在，pop 必定成功。
        Ok(slot_state.pending.remove(pos).expect("just found"))
    }

    /// 清空全部槽位的待执行任务（不动正在执行的那条）。
    ///
    /// 返回被清掉的条数。给"一键清空积压"用。
    pub fn clear_pending(&mut self) -> usize {
        let mut n = 0;
        for s in &mut self.slots {
            n += s.pending.len();
            s.pending.clear();
        }
        n
    }

    /// 公共的收尾逻辑：校验 task_id 后取出运行位。
    fn take_running(&mut self, slot: usize, task_id: TaskId) -> Result<QueuedTask, QueueError> {
        self.check_slot(slot)?;
        let slot_state = &mut self.slots[slot];
        let Some(running) = slot_state.running.as_ref() else {
            return Err(QueueError::NotRunning { slot });
        };
        if running.task.id != task_id {
            return Err(QueueError::TaskIdMismatch {
                slot,
                expected: running.task.id,
                actual: task_id,
            });
        }
        // 上面已确认 Some 且 id 匹配。
        Ok(slot_state.running.take().expect("checked above").task)
    }

    // -----------------------------------------------------------------------
    // 观测
    // -----------------------------------------------------------------------

    /// 某槽待执行条数。
    pub fn depth(&self, slot: usize) -> usize {
        self.slots.get(slot).map(|s| s.pending.len()).unwrap_or(0)
    }

    /// 全部槽的待执行总数。
    pub fn total_pending(&self) -> usize {
        self.slots.iter().map(|s| s.pending.len()).sum()
    }

    /// **正在执行的槽数**（= 第 5 颗灯的输入）。
    ///
    /// 与 `total_pending()` 的区别是语义上的：待执行是"还欠着的"，正在执行是"此刻在烧资源"。
    /// 灯要表达的是后者——用户看灯是想知道"机器现在忙不忙"。
    pub fn running_count(&self) -> usize {
        self.slots.iter().filter(|s| s.running.is_some()).count()
    }

    /// 是否有任何任务在执行或待执行。
    pub fn is_idle(&self) -> bool {
        self.slots
            .iter()
            .all(|s| s.running.is_none() && s.pending.is_empty())
    }

    /// 正在执行的执行器 id（去重后排序）。
    ///
    /// 排序是刻意的：让快照**可比较**（`state.rs` 靠它判断"状态真变了没"），
    /// 否则同样的集合因为 HashMap 迭代顺序不同会抖出假变化。
    pub fn active_drivers(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .slots
            .iter()
            .filter_map(|s| s.running.as_ref().map(|r| r.task.driver_id.clone()))
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// 生成快照。
    pub fn snapshot(&self) -> QueueSnapshot {
        let slots = self
            .slots
            .iter()
            .enumerate()
            .map(|(i, s)| SlotQueueView {
                index: i,
                running: s.running.clone(),
                pending: s.pending.iter().cloned().collect(),
                depth: s.pending.len(),
            })
            .collect();

        QueueSnapshot {
            running_count: self.running_count(),
            total_pending: self.total_pending(),
            active_drivers: self.active_drivers(),
            completed: self.completed,
            failed: self.failed,
            slots,
        }
    }

    fn check_slot(&self, slot: usize) -> Result<(), QueueError> {
        if slot >= self.slots.len() {
            return Err(QueueError::SlotOutOfRange {
                slot,
                slots: self.slots.len(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q() -> TaskQueue {
        TaskQueue::new(4)
    }

    // -----------------------------------------------------------------------
    // 2.2 验收：同槽 3 条按序出
    // -----------------------------------------------------------------------

    #[test]
    fn same_slot_runs_strictly_in_order() {
        let mut q = q();
        let a = q.enqueue(0, "第一件", "d1", 1_000).unwrap();
        let b = q.enqueue(0, "第二件", "d1", 1_001).unwrap();
        let c = q.enqueue(0, "第三件", "d1", 1_002).unwrap();

        // 第一条：立刻可取。
        match q.start_next(0, 1_100).unwrap() {
            StartOutcome::Started(t) => assert_eq!(t.id, a.id),
            other => panic!("应启动第一条，实际 {other:?}"),
        }
        // 第二条、第三条：被 Busy 挡住，且仍然留在队列里（不是被丢弃）。
        for _ in 0..3 {
            match q.start_next(0, 1_200).unwrap() {
                StartOutcome::Busy { running_id } => assert_eq!(running_id, a.id),
                other => panic!("串行未生效，实际 {other:?}"),
            }
        }
        assert_eq!(q.depth(0), 2, "被挡住的任务必须留在队列里");

        q.finish(0, a.id).unwrap();
        match q.start_next(0, 1_300).unwrap() {
            StartOutcome::Started(t) => assert_eq!(t.id, b.id),
            other => panic!("第一条结束后应启动第二条，实际 {other:?}"),
        }
        q.finish(0, b.id).unwrap();
        match q.start_next(0, 1_400).unwrap() {
            StartOutcome::Started(t) => assert_eq!(t.id, c.id),
            other => panic!("应启动第三条，实际 {other:?}"),
        }
        q.finish(0, c.id).unwrap();
        assert_eq!(q.start_next(0, 1_500).unwrap(), StartOutcome::Idle);
        assert!(q.is_idle());
    }

    // -----------------------------------------------------------------------
    // 2.2 验收：4 槽并行不阻塞
    // -----------------------------------------------------------------------

    #[test]
    fn four_slots_run_in_parallel() {
        let mut q = q();
        for slot in 0..4 {
            q.enqueue(slot, format!("任务{slot}"), "d1", 1_000).unwrap();
        }
        // 四个槽各自动起来，互不阻塞。
        for slot in 0..4 {
            assert!(
                matches!(q.start_next(slot, 1_100).unwrap(), StartOutcome::Started(_)),
                "槽 {slot} 应能并行启动"
            );
        }
        assert_eq!(q.running_count(), 4);
    }

    #[test]
    fn one_busy_slot_does_not_block_others() {
        let mut q = q();
        q.enqueue(0, "慢任务", "d1", 1_000).unwrap();
        q.enqueue(1, "快任务", "d1", 1_000).unwrap();
        q.start_next(0, 1_100).unwrap();
        q.start_next(1, 1_100).unwrap();
        // 槽 0 还在跑，槽 1 跑完——槽 1 不应该被槽 0 拖住。
        let id1 = q.snapshot().slots[1]
            .running
            .as_ref()
            .expect("槽 1 应该在跑")
            .task
            .id;
        q.finish(1, id1).unwrap();
        assert_eq!(q.running_count(), 1);
        assert!(q.snapshot().slots[0].running.is_some(), "槽 0 仍在跑");
    }

    // -----------------------------------------------------------------------
    // 2.2 验收：重复绑同一任务不增加并发
    // -----------------------------------------------------------------------

    #[test]
    fn duplicate_pending_is_rejected_not_silently_dropped() {
        let mut q = q();
        let first = q.enqueue(0, "同一句话", "d1", 1_000).unwrap();
        let err = q.enqueue(0, "同一句话", "d1", 1_010).unwrap_err();
        match err {
            QueueError::DuplicatePending { slot, existing_id } => {
                assert_eq!(slot, 0);
                assert_eq!(existing_id, first.id);
            }
            other => panic!("应报重复，实际 {other:?}"),
        }
        assert_eq!(q.depth(0), 1, "重复的不该进队列");
        assert_eq!(q.total_pending(), 1);
    }

    #[test]
    fn same_prompt_to_different_slots_is_allowed() {
        let mut q = q();
        q.enqueue(0, "同一句话", "d1", 1_000).unwrap();
        // 不同槽是不同任务（各有各的灯、各有各的未读），必须允许。
        q.enqueue(1, "同一句话", "d1", 1_000).unwrap();
        assert_eq!(q.total_pending(), 2);
    }

    #[test]
    fn same_prompt_after_running_is_allowed_as_rerun() {
        let mut q = q();
        let a = q.enqueue(0, "再跑一轮", "d1", 1_000).unwrap();
        q.start_next(0, 1_100).unwrap();
        // running 里的那条已经在跑；此时再入同文本 = 用户想重跑。允许，但它会排队等。
        let b = q.enqueue(0, "再跑一轮", "d1", 1_200).unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(q.depth(0), 1);
        assert_eq!(q.running_count(), 1, "重跑也不增加并发");
    }

    // -----------------------------------------------------------------------
    // 2.2 验收：迟到回执不得影响新任务
    // -----------------------------------------------------------------------

    #[test]
    fn stale_finish_is_rejected_and_does_not_clear_new_task() {
        let mut q = q();
        let old = q.enqueue(0, "旧任务", "d1", 1_000).unwrap();
        q.start_next(0, 1_100).unwrap();

        // 模拟：旧任务被取消，运行位已清，新任务启动。
        // （这里用 finish 清掉旧的，再启动新的，模拟"交换"后的局面。）
        q.finish(0, old.id).unwrap();
        let new = q.enqueue(0, "新任务", "d1", 1_200).unwrap();
        q.start_next(0, 1_300).unwrap();

        // 旧任务的回执姗姗来迟。
        let err = q.finish(0, old.id).unwrap_err();
        match err {
            QueueError::TaskIdMismatch {
                slot,
                expected,
                actual,
            } => {
                assert_eq!(slot, 0);
                assert_eq!(expected, new.id);
                assert_eq!(actual, old.id);
            }
            other => panic!("迟到回执必须被拒，实际 {other:?}"),
        }
        // 关键：新任务的运行位**没有**被清掉。
        assert_eq!(
            q.snapshot().slots[0].running.as_ref().map(|r| r.task.id),
            Some(new.id)
        );
        assert_eq!(q.running_count(), 1);
    }

    #[test]
    fn finish_without_running_is_explicit_error() {
        let mut q = q();
        let t = q.enqueue(0, "还没跑", "d1", 1_000).unwrap();
        let err = q.finish(0, t.id).unwrap_err();
        assert_eq!(err, QueueError::NotRunning { slot: 0 });
    }

    // -----------------------------------------------------------------------
    // 边界与防御
    // -----------------------------------------------------------------------

    #[test]
    fn queue_full_is_explicit_and_nothing_is_dropped() {
        let mut q = q();
        for i in 0..MAX_DEPTH_PER_SLOT {
            q.enqueue(0, format!("任务{i}"), "d1", 1_000).unwrap();
        }
        let err = q.enqueue(0, "第 9 条", "d1", 1_000).unwrap_err();
        assert_eq!(
            err,
            QueueError::QueueFull {
                slot: 0,
                max: MAX_DEPTH_PER_SLOT
            }
        );
        assert_eq!(q.depth(0), MAX_DEPTH_PER_SLOT, "满队列不得丢已有任务");
    }

    #[test]
    fn empty_prompt_is_rejected() {
        let mut q = q();
        assert_eq!(
            q.enqueue(0, "   ", "d1", 1_000).unwrap_err(),
            QueueError::EmptyPrompt { slot: 0 }
        );
        assert_eq!(q.total_pending(), 0);
    }

    #[test]
    fn out_of_range_slot_is_rejected_on_every_entry_point() {
        let mut q = q();
        assert_eq!(
            q.enqueue(9, "x", "d1", 1_000).unwrap_err(),
            QueueError::SlotOutOfRange { slot: 9, slots: 4 }
        );
        assert_eq!(
            q.start_next(9, 1_000).unwrap_err(),
            QueueError::SlotOutOfRange { slot: 9, slots: 4 }
        );
        assert_eq!(
            q.finish(9, 1).unwrap_err(),
            QueueError::SlotOutOfRange { slot: 9, slots: 4 }
        );
        // 观测接口不 panic（它是给 UI 轮询的，不能因为一个坏下标把页面搞崩）。
        assert_eq!(q.depth(9), 0);
        assert_eq!(q.snapshot().slots.len(), 4, "越界不改变槽位总数");
    }

    #[test]
    fn running_count_counts_slots_not_queue_entries() {
        let mut q = q();
        // 单槽堆 3 条，只有 1 条在跑 → 运行数必须是 1，不是 3。
        for i in 0..3 {
            q.enqueue(0, format!("任务{i}"), "d1", 1_000).unwrap();
        }
        q.start_next(0, 1_100).unwrap();
        assert_eq!(q.running_count(), 1);
        assert_eq!(q.total_pending(), 2);
    }

    #[test]
    fn active_drivers_is_deduped_and_sorted() {
        let mut q = q();
        q.enqueue(0, "a", "z-driver", 1_000).unwrap();
        q.enqueue(1, "b", "a-driver", 1_000).unwrap();
        q.enqueue(2, "c", "a-driver", 1_000).unwrap();
        for s in 0..3 {
            q.start_next(s, 1_100).unwrap();
        }
        // 去重 + 排序 → 快照可比较，不会因为迭代顺序抖出假变化。
        assert_eq!(q.active_drivers(), vec!["a-driver", "z-driver"]);
    }

    #[test]
    fn cancel_pending_does_not_touch_running() {
        let mut q = q();
        let running = q.enqueue(0, "在跑", "d1", 1_000).unwrap();
        let waiting = q.enqueue(0, "在等", "d1", 1_001).unwrap();
        q.start_next(0, 1_100).unwrap();

        let removed = q.cancel_pending(0, waiting.id).unwrap();
        assert_eq!(removed.id, waiting.id);
        assert_eq!(q.depth(0), 0);
        // 正在执行的那条不受影响。
        assert_eq!(
            q.snapshot().slots[0].running.as_ref().map(|r| r.task.id),
            Some(running.id)
        );
    }

    #[test]
    fn clear_pending_keeps_running() {
        let mut q = q();
        q.enqueue(0, "在跑", "d1", 1_000).unwrap();
        q.enqueue(0, "在等1", "d1", 1_001).unwrap();
        q.enqueue(0, "在等2", "d1", 1_002).unwrap();
        q.start_next(0, 1_100).unwrap();

        assert_eq!(q.clear_pending(), 2);
        assert_eq!(q.total_pending(), 0);
        assert_eq!(q.running_count(), 1, "清积压不得动正在执行的任务");
    }

    #[test]
    fn completed_and_failed_are_counted_separately() {
        let mut q = q();
        let a = q.enqueue(0, "成功", "d1", 1_000).unwrap();
        let b = q.enqueue(1, "失败", "d1", 1_000).unwrap();
        q.start_next(0, 1_100).unwrap();
        q.start_next(1, 1_100).unwrap();
        q.finish(0, a.id).unwrap();
        q.fail(1, b.id).unwrap();

        let s = q.snapshot();
        assert_eq!(s.completed, 1);
        assert_eq!(s.failed, 1);
        assert_eq!(s.running_count, 0);
        assert!(s.active_drivers.is_empty());
    }

    #[test]
    fn snapshot_is_stable_when_nothing_changes() {
        // state.rs 靠这个性质判断"状态真变了没"。若快照不稳定，事件会刷屏。
        let mut q = q();
        q.enqueue(0, "任务", "d1", 1_000).unwrap();
        q.start_next(0, 1_100).unwrap();
        assert_eq!(q.snapshot(), q.snapshot());
    }

    #[test]
    fn ids_are_globally_unique_across_slots() {
        let mut q = q();
        let a = q.enqueue(0, "x", "d1", 1_000).unwrap();
        let b = q.enqueue(1, "x", "d1", 1_000).unwrap();
        let c = q.enqueue(2, "x", "d1", 1_000).unwrap();
        assert!(a.id != b.id && b.id != c.id && a.id != c.id);
        assert_eq!(a.id, 1, "任务号从 1 开始（0 保留给'无任务'）");
    }
}
