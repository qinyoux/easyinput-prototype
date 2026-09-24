//! 任务台运行时状态 —— 把队列与未读账本合成一个可整体替换的状态。
//!
//! # 为什么这两样必须放在一起
//!
//! 「队列」和「未读」不是两个独立子系统，它们是**同一个任务生命周期的两半**：
//!
//! ```text
//!   按键 → 入队 → 执行中 ─┬─ 成功 ─→ 有结果可听 ─→ 未读 +1
//!                        └─ 失败 ─→ 有话要说 ──→ 未读 +1
//!                        取消 ───→ 用户自己按的 ─→ 不产生未读
//! ```
//!
//! 拆成两个文件、两把锁，就会出现"任务已经跑完了但未读还没加上"的中间态被读到——
//! 前端恰好在那 10 毫秒轮询，就会看到一个自相矛盾的快照。**合成一个状态、一把锁，
//! 这类中间态从机制上不可能被观测到。**
//!
//! # 未读在「任务到达终态」时产生
//!
//! 未读的定义是「**有一条结果，你还没听过**」。关键是"结果"这个词的粒度：
//!
//! - 它**不是**"音频已经落进键盘 Flash"——那是**交付通道**的状态。交付失败时，
//!   用户更该知道"有结果但拿不到"，而不是灯不亮、什么都不知道。
//! - 它是"**这个任务已经结束了，而且它的结局值得你知道**"。
//!
//! ⇒ 未读在终态产生，由 [`complete_task`] 统一处理。四种终态的规则**刻意不同**：
//!
//! | 终态 | 产生未读 | 为什么 |
//! |---|---|---|
//! | 成功 | ✅ | 有结果要听 |
//! | 失败 | ✅ | **失败也必须被听到**——否则用户以为它还在跑，一直等 |
//! | 超时 | ✅ | 同上；而且"超时"本身就是一件需要用户处理的事 |
//! | 取消 | ❌ | 用户自己按的取消，他知道 |
//!
//! 把这条规则分散到各个调用点，几乎一定会漏掉"失败"那一格——那恰恰是最需要
//! 通知用户的格子。所以它必须是一个有单测守着的集中判断，而不是每处的 if。

use serde::Serialize;

use super::queue::{QueueError, QueueSnapshot, QueuedTask, StartOutcome, TaskId, TaskQueue};
use super::unread::{Lease, LeaseError, UnreadLedger, UnreadPersist, UnreadSnapshot};

/// 未读账本的持久化文件名（与 `agent_desk.json` 同目录）。
///
/// **单独一个文件**而不是并进 `agent_desk.json`：两者的写入频率差了几个数量级
/// （配置改一次 / 未读每次播放都变），并在一起会让配置文件被高频重写，
/// 也给多 agent 协作增加无谓的冲突面。
pub const UNREAD_FILE: &str = "agent_desk_unread.json";

/// 统一快照。
///
/// `PartialEq` 是**核心机制**：只有它逐字段可比较，事件层才能判断"状态真的变了吗"，
/// 从而做到"只在真变化时发事件"。没有它，就只能定时广播——前端每 2 秒被叫醒一次，
/// 却 99% 的时候什么都没变。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RuntimeSnapshot {
    pub queue: QueueSnapshot,
    pub unread: UnreadSnapshot,
    /// 是否有任务正在执行（第 5 颗灯的另一个视角）。
    pub busy: bool,
    /// 是否完全空闲（既没在跑，也没有排队）。
    ///
    /// 与 `!busy` **不等价**：有积压但还没开始跑时 `busy == false` 而 `idle == false`。
    /// UI 要区分这两种情形——前者是"待命"，后者是"正在消化积压"。
    pub idle: bool,
    /// 是否有任何未读（灯语的总开关）。
    pub any_unread: bool,
}

/// 任务台运行时状态。
pub struct DeskRuntime {
    slot_count: usize,
    queue: TaskQueue,
    unread: UnreadLedger,
    /// 上一次发出去的快照（事件层用来判"真变化"）。
    last_emitted: Option<RuntimeSnapshot>,
}

impl DeskRuntime {
    /// 建一个 `slot_count` 槽的运行时。
    pub fn new(slot_count: usize) -> Self {
        Self {
            slot_count,
            queue: TaskQueue::new(slot_count),
            unread: UnreadLedger::new(slot_count),
            last_emitted: None,
        }
    }

    // -----------------------------------------------------------------------
    // 只读访问
    // -----------------------------------------------------------------------

    pub fn queue(&self) -> &TaskQueue {
        &self.queue
    }

    pub fn unread(&self) -> &UnreadLedger {
        &self.unread
    }

    // -----------------------------------------------------------------------
    // 写访问
    //
    // 直接用 `&mut` 暴露子模块，而不是把每个方法都转发一遍：
    // 转发会让"队列有 15 个方法、运行时就有 15 个转发"成为必然，
    // 且每加一个队列方法都要动两个文件。
    // -----------------------------------------------------------------------

    pub fn queue_mut(&mut self) -> &mut TaskQueue {
        &mut self.queue
    }

    pub fn unread_mut(&mut self) -> &mut UnreadLedger {
        &mut self.unread
    }

    // -----------------------------------------------------------------------
    // 快照与变更检测
    // -----------------------------------------------------------------------

    /// 生成当前快照。
    pub fn snapshot(&self) -> RuntimeSnapshot {
        let queue = self.queue.snapshot();
        let unread = self.unread.snapshot();
        RuntimeSnapshot {
            busy: queue.running_count > 0,
            idle: self.queue.is_idle(),
            any_unread: unread.total_pending > 0,
            queue,
            unread,
        }
    }

    /// 若与上次发出的快照不同，返回新快照并记住它；否则返回 `None`。
    ///
    /// 用**结构比较**而不是时间戳/哈希：`RuntimeSnapshot` 全部字段都是确定性的
    /// （时间戳在任务入队时就固定，不会因为"现在几点"而抖），所以逐字段比较
    /// 既准确又便宜，还不会因为哈希碰撞漏掉变化。
    pub fn snapshot_if_changed(&mut self) -> Option<RuntimeSnapshot> {
        let cur = self.snapshot();
        if self.last_emitted.as_ref() == Some(&cur) {
            return None;
        }
        self.last_emitted = Some(cur.clone());
        Some(cur)
    }

    /// 丢弃变更检测的记忆，让下一次 [`Self::snapshot_if_changed`] 必定返回。
    ///
    /// 用在"前端刚重连/刚打开页面"时——它需要一份完整快照，
    /// 哪怕与上次发出的完全一样。
    pub fn forget_emitted(&mut self) {
        self.last_emitted = None;
    }

    // -----------------------------------------------------------------------
    // 持久化（只含未读）
    // -----------------------------------------------------------------------

    /// 导出未读状态。
    ///
    /// **队列刻意不持久化**：一个"正在执行"的任务在重启后并不存在——
    /// 它对应的子进程/HTTP 请求已经随进程一起消失了。把队列存下来再恢复，
    /// 只会得到一堆永远不会完成的幽灵任务。未读则必须存活（见 `unread.rs`）。
    pub fn to_persist(&self) -> UnreadPersist {
        self.unread.to_persist()
    }

    /// 从持久化恢复未读（队列一律清空）。
    pub fn apply_persist(&mut self, p: &UnreadPersist) {
        self.unread = UnreadLedger::from_persist(p, self.slot_count);
        // 恢复了状态 → 必须让下一次快照照常发出去（否则前端看到的还是旧值）。
        self.forget_emitted();
    }
}

/// 一次「入队 + 尝试启动」的结果。
///
/// 三态分开，是为了让 UI 能分别给话——**排队中**和**已经开始跑**对用户是
/// 完全不同的事（一个可以后悔，一个已经在烧 token 了）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DispatchOutcome {
    /// 已入队，且立刻开始执行。
    Started { task_id: TaskId, depth: usize },
    /// 已入队，但该槽正在忙，排在后面（`ahead` = 它前面还有几条）。
    Queued { task_id: TaskId, ahead: usize },
}

/// 运行时的复合操作：入队后立即尝试启动。
///
/// 放在这里而不是拆给调用方，是因为这两步之间的**锁必须连续**：
/// 如果先"入队解锁"再"启动上锁"，中间窗口里另一个调用可能抢先启动，
/// 让本次调用误判成 `Queued`——用户看到"排队中"，实际上它已经在跑了。
pub fn dispatch_into(
    rt: &mut DeskRuntime,
    slot: usize,
    prompt: impl Into<String>,
    driver_id: impl Into<String>,
    now_ms: u64,
) -> Result<DispatchOutcome, QueueError> {
    let task = rt.queue_mut().enqueue(slot, prompt, driver_id, now_ms)?;
    let ahead = rt.queue().depth(slot);

    match rt.queue_mut().start_next(slot, now_ms)? {
        StartOutcome::Started(_) => Ok(DispatchOutcome::Started {
            task_id: task.id,
            depth: rt.queue().depth(slot),
        }),
        StartOutcome::Busy { .. } => Ok(DispatchOutcome::Queued {
            task_id: task.id,
            ahead,
        }),
        StartOutcome::Idle => {
            // 刚入队就 Idle，逻辑上不可能（除非槽位号错）。不静默返回一个假状态。
            Err(QueueError::NotRunning { slot })
        }
    }
}

/// 执行侧的**认领**：取该槽当前应当被执行的那一条任务。
///
/// # 为什么必须有它（而不是让执行泵直接调 `start_next`）
///
/// [`dispatch_into`] 是"入队 + 启动"的**复合操作** —— 它在派发那一刻就把任务放进了运行位。
/// 所以执行泵的第一步只有一个正确语义：**认领这条已被标记的任务并执行它**。
/// 此时 `start_next` 会返回 [`StartOutcome::Busy`]，而 `Busy` 的含义是
/// "**该执行的是这一条**"，**不是**"队列空了、可以收工"。
///
/// 把这两件事混为一谈的后果极其严重且**完全静默**：任务会永久停在"正在执行"——
/// 没有子进程、没有终态、没有未读、没有播报，**连超时都不会触发**
/// （超时在 `runner` 内部，而 `runner` 从未被调用）。
///
/// 反过来，同槽连续跑下一条时运行位是空的，`start_next` 会真正启动队首。
/// 两条路合并在这一个函数里，且**它是纯数据层操作** —— 因此可以被单测钉死，
/// 不必依赖 `tauri::AppHandle`。
pub fn claim_current(
    rt: &mut DeskRuntime,
    slot: usize,
    now_ms: u64,
) -> Result<Option<QueuedTask>, QueueError> {
    // 先取结果再 match：`match rt.queue_mut().start_next(..)` 会让 `&mut` 借用活到整个
    // match 结束，从而无法在分支里再借 `rt.queue()`。
    let outcome = rt.queue_mut().start_next(slot, now_ms)?;
    match outcome {
        StartOutcome::Started(t) => Ok(Some(*t)),
        // 已有运行位 ⇒ 认领它（`dispatch_into` 留下的，或本槽当前那一条）。
        StartOutcome::Busy { .. } => rt.queue().running_task(slot),
        StartOutcome::Idle => Ok(None),
    }
}

/// 任务终态（运行时视角）。
///
/// 与 `driver::RunStatus` 同构但**刻意分开**：那个枚举描述的是"执行器怎么结束的"，
/// 这个是"任务在这个槽位上怎么结束的"。两者的映射由编排器做（`TimedOut` 在
/// 用户视角就是"没做成，而且要知道"），保持运行时状态机不依赖执行器层——
/// 这样 `state.rs` 的测试完全不需要网络或子进程。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskTerminal {
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
}

impl TaskTerminal {
    /// 这个终态是否应该产生未读。
    ///
    /// 见模块文档的表格。**取消是唯一的例外**——因为那是用户自己按的。
    pub fn produces_unread(&self) -> bool {
        !matches!(self, TaskTerminal::Cancelled)
    }

    pub fn label(&self) -> &'static str {
        match self {
            TaskTerminal::Succeeded => "成功",
            TaskTerminal::Failed => "失败",
            TaskTerminal::TimedOut => "超时",
            TaskTerminal::Cancelled => "取消",
        }
    }
}

/// 终态处理的结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TerminalEffect {
    /// 被收尾的任务。
    pub task: QueuedTask,
    pub terminal: String,
    /// 本次是否产生了未读。
    pub produced_unread: bool,
    /// 处理完后该槽的未读数。
    pub unread: u32,
}

/// 终态处理失败的原因（队列与未读两层的错误各自保留身份，不压成一句）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalError {
    Queue(QueueError),
    Unread(LeaseError),
}

impl TerminalError {
    pub fn message(&self) -> String {
        match self {
            TerminalError::Queue(e) => e.message(),
            TerminalError::Unread(e) => e.message(),
        }
    }
}

/// 任务到达终态：清出运行位 + 按规则产生未读。
///
/// **队列与未读在同一次调用里一起改**，这是刻意的：拆成两次调用，两次之间就会出现
/// "任务已经不在跑了、但结果还没登记"的窗口。前端恰好在那时轮询，看到的快照是自相矛盾的。
pub fn complete_task(
    rt: &mut DeskRuntime,
    slot: usize,
    task_id: TaskId,
    terminal: TaskTerminal,
) -> Result<TerminalEffect, TerminalError> {
    // 成功走 finish（计入 completed），其余走 fail（计入 failed）。
    // 取消与超时也归入 failed 计数：从"这个槽位有没有产出可用结果"的角度看，
    // 它们确实没有。分开统计会让"成功率"这个数字失去意义。
    let task = match terminal {
        TaskTerminal::Succeeded => rt.queue_mut().finish(slot, task_id),
        _ => rt.queue_mut().fail(slot, task_id),
    }
    .map_err(TerminalError::Queue)?;

    let produced_unread = terminal.produces_unread();
    let unread = if produced_unread {
        rt.unread_mut()
            .push_result(slot)
            .map_err(TerminalError::Unread)?
    } else {
        // 不产生未读时，返回当前值（可能不为 0——之前的结果还在等用户听）。
        rt.unread()
            .pending(slot)
            .map_err(TerminalError::Unread)?
    };

    Ok(TerminalEffect {
        task,
        terminal: terminal.label().to_string(),
        produced_unread,
        unread,
    })
}

/// 把 `LeaseError` 与 `QueueError` 统一成用户可读的中文说明。
///
/// 这个函数存在的意义：**错误在跨层传递时最容易退化成 "操作失败" 四个字**。
/// 每个错误都带着自己的病因，这里只是把它们汇总到一个出口，不做任何压缩。
pub fn describe_queue_error(e: &QueueError) -> String {
    e.message()
}

pub fn describe_lease_error(e: &LeaseError) -> String {
    e.message()
}

/// 当前租约的展示文本（调试面板用）。
pub fn lease_label(l: Option<Lease>) -> String {
    match l {
        Some(l) => format!("S{} 持有（租约 {}）", l.slot + 1, l.token),
        None => "空闲".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_desk::unread::UNREAD_SCHEMA;

    fn rt() -> DeskRuntime {
        DeskRuntime::new(4)
    }

    // -----------------------------------------------------------------------
    // 合成状态 + 变更检测
    // -----------------------------------------------------------------------

    #[test]
    fn snapshot_if_changed_only_fires_on_real_change() {
        let mut r = rt();
        // 第一次：什么都没发生，但仍应发一次（前端要拿到初始状态）。
        assert!(r.snapshot_if_changed().is_some());

        // 无变化 → 不发。这是"不刷屏"的实现。
        assert!(r.snapshot_if_changed().is_none());
        assert!(r.snapshot_if_changed().is_none());

        // 真变化 → 发。
        r.queue_mut().enqueue(0, "任务", "d1", 1_000).unwrap();
        assert!(r.snapshot_if_changed().is_some());
        assert!(r.snapshot_if_changed().is_none(), "发了之后立刻回到静默");

        // 启动也是真变化。
        r.queue_mut().start_next(0, 1_100).unwrap();
        let s = r.snapshot_if_changed().unwrap();
        assert_eq!(s.queue.running_count, 1);
        assert!(s.busy);
    }

    #[test]
    fn snapshot_aggregates_queue_and_unread() {
        let mut r = rt();
        r.queue_mut().enqueue(0, "任务", "d1", 1_000).unwrap();
        r.queue_mut().start_next(0, 1_100).unwrap();
        r.unread_mut().push_result(2).unwrap();
        r.unread_mut().push_result(2).unwrap();
        r.unread_mut().push_result(1).unwrap();

        let s = r.snapshot();
        assert_eq!(s.queue.running_count, 1);
        assert_eq!(s.unread.total_pending, 3);
        assert!(s.busy);
        assert!(!s.idle, "有任务在跑就不算空闲");
        assert!(s.any_unread);
    }

    #[test]
    fn idle_is_not_the_same_as_not_busy() {
        // 有积压但还没开始执行：busy=false 而 idle=false。
        // UI 要靠这个区分「待命」与「正在消化积压」——两者都不忙，但含义完全不同。
        let mut r = rt();
        r.queue_mut().enqueue(0, "排队中", "d1", 1_000).unwrap();

        let s = r.snapshot();
        assert_eq!(s.queue.total_pending, 1);
        assert_eq!(s.queue.running_count, 0);
        assert!(!s.busy, "没有任务在跑");
        assert!(!s.idle, "但队列里有东西，不算空闲");
    }

    #[test]
    fn truly_empty_runtime_is_idle() {
        let r = rt();
        let s = r.snapshot();
        assert!(s.idle, "什么都没排、什么都没跑 = 空闲");
        assert!(!s.busy);
    }

    #[test]
    fn forget_emitted_forces_a_fresh_snapshot() {
        // 前端刚打开页面时，即使状态没变也需要一份完整快照。
        let mut r = rt();
        r.snapshot_if_changed().unwrap();
        assert!(r.snapshot_if_changed().is_none());
        r.forget_emitted();
        assert!(r.snapshot_if_changed().is_some());
    }

    #[test]
    fn unread_change_alone_is_detected() {
        // 未读是独立于队列的状态源，它的变化必须能单独触发事件。
        let mut r = rt();
        r.snapshot_if_changed().unwrap();
        r.unread_mut().push_result(0).unwrap();
        let s = r.snapshot_if_changed().unwrap();
        assert_eq!(s.unread.total_pending, 1);
    }

    #[test]
    fn playback_lease_change_is_detected() {
        let mut r = rt();
        r.unread_mut().push_result(3).unwrap();
        r.snapshot_if_changed().unwrap();

        let lease = r.unread_mut().grant(3, 1_000).unwrap();
        let s = r.snapshot_if_changed().unwrap();
        assert_eq!(s.unread.playing_slot, Some(3));

        // 归还（抢占）也是一次真变化。
        r.unread_mut().release(lease).unwrap();
        let s = r.snapshot_if_changed().unwrap();
        assert_eq!(s.unread.playing_slot, None);
    }

    // -----------------------------------------------------------------------
    // 复合操作：入队 + 启动
    // -----------------------------------------------------------------------

    #[test]
    fn dispatch_on_idle_slot_starts_immediately() {
        let mut r = rt();
        let out = dispatch_into(&mut r, 0, "任务", "d1", 1_000).unwrap();
        match out {
            DispatchOutcome::Started { task_id, depth } => {
                assert_eq!(task_id, 1);
                assert_eq!(depth, 0);
            }
            other => panic!("空闲槽应立刻启动，实际 {other:?}"),
        }
        assert_eq!(r.queue().running_count(), 1);
    }

    #[test]
    fn dispatch_on_busy_slot_queues_with_ahead_count() {
        let mut r = rt();
        dispatch_into(&mut r, 0, "第一个", "d1", 1_000).unwrap();
        // 槽 0 正在忙：第二条必须排队，且明确告知"前面还有几条"。
        let out = dispatch_into(&mut r, 0, "第二个", "d1", 1_001).unwrap();
        match out {
            DispatchOutcome::Queued { ahead, .. } => assert_eq!(ahead, 1),
            other => panic!("忙槽应排队，实际 {other:?}"),
        }
        let out = dispatch_into(&mut r, 0, "第三个", "d1", 1_002).unwrap();
        match out {
            DispatchOutcome::Queued { ahead, .. } => assert_eq!(ahead, 2),
            other => panic!("忙槽应排队，实际 {other:?}"),
        }
        assert_eq!(r.queue().running_count(), 1, "排队不增加并发");
    }

    #[test]
    fn dispatch_propagates_queue_errors_instead_of_swallowing_them() {
        let mut r = rt();
        assert_eq!(
            dispatch_into(&mut r, 9, "任务", "d1", 1_000).unwrap_err(),
            QueueError::SlotOutOfRange { slot: 9, slots: 4 }
        );
        // 空文本是"识别没出字"或"误触发"，必须报错——不能静默当成一条任务。
        assert_eq!(
            dispatch_into(&mut r, 0, "   ", "d1", 1_000).unwrap_err(),
            QueueError::EmptyPrompt { slot: 0 }
        );
    }

    #[test]
    fn dispatch_while_busy_queues_as_a_rerun_not_a_duplicate() {
        // 第一条已经在跑；此时再来一条**同文本**，语义是"再跑一轮"（不是重复触发），
        // 所以它该排队而不是被拒。去重只针对"还没开始跑的重复触发"。
        let mut r = rt();
        dispatch_into(&mut r, 0, "再跑一轮", "d1", 1_000).unwrap();
        match dispatch_into(&mut r, 0, "再跑一轮", "d1", 1_001).unwrap() {
            DispatchOutcome::Queued { ahead, .. } => assert_eq!(ahead, 1),
            other => panic!("忙槽的重跑应排队，实际 {other:?}"),
        }
        assert_eq!(r.queue().running_count(), 1, "重跑不增加并发");
    }

    // -----------------------------------------------------------------------
    // 执行侧认领：泵的第一步
    //
    // ⚠️ 这一段曾经是**测试盲区**，代价是一条完全静默的缺陷：
    // `pump_slot` 把 `StartOutcome::Busy` 当成"队列空了"直接退出，
    // 而 `dispatch_into` 早在派发那一刻就把任务放进了运行位——
    // 于是任务永久停在"正在执行"：没有子进程、没有终态、没有未读、没有播报，
    // 连超时都不会触发（超时在 runner 内部，而 runner 从未被调用）。
    // 175 个测试全绿也照样漏掉它，因为它们只断言"派发返回了什么"，
    // 从不断言"泵随后能不能真的取到任务"。
    // -----------------------------------------------------------------------

    #[test]
    fn claim_after_dispatch_takes_the_task_dispatch_already_marked_running() {
        let mut r = rt();
        let out = dispatch_into(&mut r, 0, "任务", "d1", 1_000).unwrap();
        let id = match out {
            DispatchOutcome::Started { task_id, .. } => task_id,
            other => panic!("空闲槽应立刻启动，实际 {other:?}"),
        };

        // 泵看到的第一手信息是 Busy（**不是** Idle）—— 所以不能据此收工。
        assert!(matches!(
            r.queue_mut().start_next(0, 1_001).unwrap(),
            StartOutcome::Busy { .. }
        ));

        // 认领必须拿到**同一条**任务，否则它会被永久搁置。
        let claimed = claim_current(&mut r, 0, 1_002).unwrap();
        assert_eq!(claimed.map(|t| t.id), Some(id));
    }

    #[test]
    fn claim_on_idle_slot_starts_the_next_pending_one() {
        // 同槽连续执行：上一条收终态后运行位是空的，认领应当启动队首。
        let mut r = rt();
        dispatch_into(&mut r, 0, "第一条", "d1", 1_000).unwrap();
        dispatch_into(&mut r, 0, "第二条", "d1", 1_001).unwrap();

        // 第一条收终态（清出运行位），就像泵跑完它之后那样。
        complete_task(&mut r, 0, 1, TaskTerminal::Succeeded).unwrap();

        let claimed = claim_current(&mut r, 0, 2_000).unwrap();
        assert_eq!(claimed.map(|t| t.id), Some(2), "应认领到队首的第二条");
    }

    #[test]
    fn claim_on_empty_slot_reports_nothing_to_do() {
        let mut r = rt();
        assert!(claim_current(&mut r, 0, 1_000).unwrap().is_none());
    }

    #[test]
    fn claim_never_reports_busy_as_nothing_to_do() {
        // 直接钉住这条语义边界：只要运行位有人占着，认领就**必须**给出那条任务。
        // 一旦有人把 Busy 简化成返回 None，这个测试立刻红。
        let mut r = rt();
        dispatch_into(&mut r, 0, "在跑", "d1", 1_000).unwrap();
        for i in 0..3 {
            let claimed = claim_current(&mut r, 0, 1_000 + i).unwrap();
            assert!(
                claimed.is_some(),
                "运行位被占时必须认领到任务（第 {i} 次），不能报成没有事做"
            );
        }
    }

    #[test]
    fn claim_propagates_slot_range_errors() {
        // 越界槽位不能静默返回 None —— 那会让"配错槽位号"表现成"任务没人执行"。
        let mut r = rt();
        assert_eq!(
            claim_current(&mut r, 9, 1_000).unwrap_err(),
            QueueError::SlotOutOfRange { slot: 9, slots: 4 }
        );
    }

    // -----------------------------------------------------------------------
    // 持久化边界：未读存活、队列不存活
    // -----------------------------------------------------------------------

    #[test]
    fn only_unread_is_persisted_queue_is_not() {
        let mut r = rt();
        r.queue_mut().enqueue(0, "跑着呢", "d1", 1_000).unwrap();
        r.queue_mut().start_next(0, 1_100).unwrap();
        r.unread_mut().push_result(1).unwrap();

        let p = r.to_persist();
        assert_eq!(p.schema, UNREAD_SCHEMA);

        let mut restored = rt();
        restored.apply_persist(&p);

        // 未读活下来了。
        assert_eq!(restored.unread().total_pending(), 1);
        // 队列没有——重启后那个"正在执行"的任务不复存在，恢复它只会得到幽灵任务。
        assert_eq!(restored.queue().running_count(), 0);
        assert_eq!(restored.queue().total_pending(), 0);
        assert!(restored.queue().is_idle());
    }

    #[test]
    fn apply_persist_forces_next_snapshot_to_fire() {
        // 恢复状态后必须让事件照常发出——否则前端看到的还是恢复前的旧值。
        let mut r = rt();
        r.snapshot_if_changed().unwrap();
        assert!(r.snapshot_if_changed().is_none());

        r.apply_persist(&r.to_persist());
        assert!(
            r.snapshot_if_changed().is_some(),
            "恢复后首次快照必须发出"
        );
    }

    #[test]
    fn apply_persist_respects_current_slot_count() {
        let mut src = DeskRuntime::new(4);
        for s in 0..4 {
            src.unread_mut().push_result(s).unwrap();
        }
        let p = src.to_persist();

        // 运行时只有 2 个槽：多出的历史被忽略，不 panic。
        let mut dst = DeskRuntime::new(2);
        dst.apply_persist(&p);
        assert_eq!(dst.unread().total_pending(), 2);
        assert_eq!(dst.snapshot().queue.slots.len(), 2);
    }

    // -----------------------------------------------------------------------
    // 终态 → 未读（四种终态规则不同，这里逐条钉死）
    // -----------------------------------------------------------------------

    #[test]
    fn succeeded_produces_unread_and_clears_running_slot() {
        let mut r = rt();
        let t = r.queue_mut().enqueue(0, "任务", "d1", 1_000).unwrap();
        r.queue_mut().start_next(0, 1_100).unwrap();

        let eff = complete_task(&mut r, 0, t.id, TaskTerminal::Succeeded).unwrap();
        assert!(eff.produced_unread);
        assert_eq!(eff.unread, 1);
        assert_eq!(eff.terminal, "成功");
        assert_eq!(r.queue().running_count(), 0, "必须清出运行位");
        assert_eq!(r.queue().snapshot().completed, 1);
    }

    #[test]
    fn failure_also_produces_unread() {
        // ★ 这条最容易漏：失败必须被听到，否则用户以为它还在跑、一直等。
        let mut r = rt();
        let t = r.queue_mut().enqueue(0, "任务", "d1", 1_000).unwrap();
        r.queue_mut().start_next(0, 1_100).unwrap();

        let eff = complete_task(&mut r, 0, t.id, TaskTerminal::Failed).unwrap();
        assert!(eff.produced_unread, "失败不产生未读 = 用户永远不知道它挂了");
        assert_eq!(eff.unread, 1);
        assert_eq!(r.queue().snapshot().failed, 1);
    }

    #[test]
    fn timeout_produces_unread() {
        let mut r = rt();
        let t = r.queue_mut().enqueue(0, "任务", "d1", 1_000).unwrap();
        r.queue_mut().start_next(0, 1_100).unwrap();
        let eff = complete_task(&mut r, 0, t.id, TaskTerminal::TimedOut).unwrap();
        assert!(eff.produced_unread, "超时是要用户处理的事，必须通知");
    }

    #[test]
    fn cancellation_does_not_produce_unread() {
        // 用户自己按的取消 —— 他知道，不该再给他一个未读。
        let mut r = rt();
        let t = r.queue_mut().enqueue(0, "任务", "d1", 1_000).unwrap();
        r.queue_mut().start_next(0, 1_100).unwrap();

        let eff = complete_task(&mut r, 0, t.id, TaskTerminal::Cancelled).unwrap();
        assert!(!eff.produced_unread);
        assert_eq!(eff.unread, 0);
        assert_eq!(r.queue().running_count(), 0, "取消也要清出运行位");
    }

    #[test]
    fn cancel_keeps_previously_accumulated_unread() {
        let mut r = rt();
        // 先有一条历史未读。
        r.unread_mut().push_result(0).unwrap();
        let t = r.queue_mut().enqueue(0, "任务", "d1", 1_000).unwrap();
        r.queue_mut().start_next(0, 1_100).unwrap();

        let eff = complete_task(&mut r, 0, t.id, TaskTerminal::Cancelled).unwrap();
        assert!(!eff.produced_unread);
        assert_eq!(eff.unread, 1, "取消不该把已有未读抹掉");
    }

    #[test]
    fn stale_terminal_is_rejected_and_does_not_touch_unread() {
        // 迟到回执跨两层都要被挡住：既不能清错运行位，也不能凭空加一条未读。
        let mut r = rt();
        let old = r.queue_mut().enqueue(0, "旧", "d1", 1_000).unwrap();
        r.queue_mut().start_next(0, 1_100).unwrap();
        complete_task(&mut r, 0, old.id, TaskTerminal::Succeeded).unwrap();

        let new = r.queue_mut().enqueue(0, "新", "d1", 1_200).unwrap();
        r.queue_mut().start_next(0, 1_300).unwrap();

        let err = complete_task(&mut r, 0, old.id, TaskTerminal::Succeeded).unwrap_err();
        assert!(matches!(err, TerminalError::Queue(QueueError::TaskIdMismatch { .. })));
        assert_eq!(r.unread().total_pending(), 1, "旧回执不得再加一条未读");
        assert_eq!(
            r.queue().snapshot().slots[0]
                .running
                .as_ref()
                .map(|t| t.task.id),
            Some(new.id),
            "新任务仍在跑"
        );
    }

    #[test]
    fn terminal_error_keeps_its_cause_readable() {
        let e = TerminalError::Queue(QueueError::NotRunning { slot: 1 });
        assert!(e.message().contains("S2"));
    }

    // -----------------------------------------------------------------------
    // 错误描述不退化
    // -----------------------------------------------------------------------

    #[test]
    fn error_descriptions_keep_their_cause() {
        // 每条错误都必须说得出"是谁、为什么"，不能退化成"操作失败"。
        let e = QueueError::DuplicatePending {
            slot: 0,
            existing_id: 7,
        };
        let msg = describe_queue_error(&e);
        assert!(msg.contains("S1"), "要说清哪个槽：{msg}");
        assert!(msg.contains('7'), "要说清是哪条任务：{msg}");

        let e = LeaseError::ChannelBusy {
            holder_slot: 1,
            holder_token: 3,
        };
        let msg = describe_lease_error(&e);
        assert!(msg.contains("S2"));
        assert!(msg.contains('3'));
    }

    #[test]
    fn lease_label_is_human_readable() {
        assert_eq!(lease_label(None), "空闲");
        let l = Lease {
            slot: 2,
            token: 9,
            granted_at_ms: 0,
            generation: 1,
            granted_pending: 1,
        };
        let s = lease_label(Some(l));
        assert!(s.contains("S3"), "对外显示用 1 基槽位号：{s}");
        assert!(s.contains('9'));
    }
}
