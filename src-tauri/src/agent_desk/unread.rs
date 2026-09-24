//! 未读账本 —— 播放租约与未读代际。
//!
//! 这是任务台**可信度的根**：灯亮 = "有一个结果，你还没听过"。
//! 这句话必须永远为真，否则整条链路的信息价值归零——用户会开始无视灯。
//!
//! # 第一性原理：未读的本质与它的三条硬属性
//!
//! 未读不是"一个数字"，它是一个**承诺**："有东西等你听"。
//! 要让这个承诺可信，它必须同时满足三条：
//!
//! 1. **跨进程重启存活。** 关掉 easyop 再开，灯还得亮着。做不到的话，
//!    重启就等于"通知被吃掉了"——用户永远不会知道那个结果存在。
//! 2. **只能被"完整听完"消费。** 播到一半拔电 = 没听完 = 未读保留。
//!    如果按"开始播"就算已读，用户拔电后就永远听不到那条内容了。
//! 3. **一次播放只消费一条。** 播放期间又来了新结果，用户听的是**旧那条**；
//!    新那条必须仍然未读。否则"听完"变成了"清空"。
//!
//! # 为什么必须有"租约"，而不是一个朴素计数器
//!
//! 播放是一个**有状态的物理过程**，最长可达 150 秒（TTS 上限）。这段时间里可能发生：
//!
//! - 用户又按了一次（抢占）
//! - 同槽又完成了一个任务（新结果进来）
//! - 拔电 / 进程被杀（播放中断）
//!
//! 没有租约时，回执（"播完了"）到达时我们**无法知道它对应哪一次播放**。
//! 一个迟到的回执会把**新产生的未读**消费掉——用户看到灯灭了，但那条内容
//! 他根本没听过。这就是方案里那条验收标准的由来：**迟到回执不得消费新租约**。
//!
//! ⇒ 租约 = `(slot, token)`，回执必须**精确匹配当前活跃租约**才算数。
//!
//! # 喇叭是全局单通道 —— 这是物理约束，不是设计选择
//!
//! 灯有 5 颗，可以同时表达 5 个槽的状态 ⇒ **灯语可并行**。
//! 喇叭只有 1 个 ⇒ **播放必须全局串行**。
//!
//! 所以这里的租约是**全局唯一**的：同一时刻只有一个槽能持有它。
//! 别的槽想播，必须先 `release` 掉当前的（抢占）——而 `release` **不消费未读**，
//! 被抢占的那个槽的灯会继续亮着，等用户回来听。
//!
//! # Notice 不在这里（语义边界，别搞错）
//!
//! 状态播报（"任务已开始 / 正在推进"）走**独立通道**，**不产生未读、不进这个账本**。
//! 它可被抢占、可静默丢弃。把 Notice 混进来，用户会看到未读灯亮、按下去只听到
//! 一句"正在推进"——灯立刻失去可信度。这条边界在 Phase 3 接 Notice 时同样适用。

use serde::{Deserialize, Serialize};

/// 每槽未读数上限（= 灯亮度档位上限）。
///
/// **到上限不再增**，而不是"内部继续累加、只是显示封顶"。理由是一致性优先于完整性：
/// 灯是"还剩几次要听"的承诺。若内部累加到 15 而灯显示 9，用户听到第 10 条时会发现
/// 灯早就满了却没归零——承诺就失效了。而"同一槽积压 10 条以上未读"在真实使用中
/// 不会发生（用户听到第 2 条就会去按了），所以这个边界几乎不会被触碰。
pub const MAX_UNREAD_PER_SLOT: u32 = 9;

/// 播放租约的有效期（毫秒）。
///
/// TTS 上限 150 秒，加上合成、下发、真实播放的余量，取 300 秒。
/// **必须有超时**：如果播放流程卡住但进程没死（比如下位机不回 ACK），
/// 没有超时的话这颗槽位会被一个僵尸租约永久锁住，表现为"按了键没反应"。
pub const LEASE_TTL_MS: u64 = 300_000;

/// 借出的一张播放租约。
///
/// `slot` + `token` 是回执的校验依据。**`token` 全局单调递增、永不复用**——
/// 这是"迟到回执被识破"的唯一机制。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    /// 持有者槽位（0 基）。
    pub slot: usize,
    /// 全局唯一令牌。
    pub token: u64,
    /// 授予时刻（unix 毫秒）。
    pub granted_at_ms: u64,
    /// 授予时该槽的未读代际。**仅用于诊断与展示**（"你听的是第 3 代，现在是第 5 代"），
    /// 不参与放行判定——用户听的是一条真实内容，不该因为它"旧了"就不算数。
    pub generation: u64,
    /// 授予时该槽剩余的未读数。用于回执时核对"消费后剩几条"。
    pub granted_pending: u32,
}

/// 租约操作失败的原因。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaseError {
    SlotOutOfRange { slot: usize, slots: usize },
    /// 该槽没有未读，无物可播。**这是一个正常状态，但要显式说出来**——
    /// 否则"按了键没声音"会被误诊成音频链路坏了。
    NothingToPlay { slot: usize },
    /// 播放通道已被别的槽占用（喇叭只有一个）。调用方可选择抢占：
    /// 先 `release` 掉当前的再来 `grant`。
    ChannelBusy { holder_slot: usize, holder_token: u64 },
    /// 回执携带的租约不是当前活跃租约（迟到 / 已被抢占 / 已超时）。
    StaleLease {
        slot: usize,
        given_token: u64,
        current_token: Option<u64>,
    },
}

impl LeaseError {
    /// 面向用户的中文说明。
    pub fn message(&self) -> String {
        match self {
            LeaseError::SlotOutOfRange { slot, slots } => {
                format!("槽位号 {slot} 越界（有效范围 0..{slots}）")
            }
            LeaseError::NothingToPlay { slot } => {
                format!("槽位 S{} 没有未读内容可播", slot + 1)
            }
            LeaseError::ChannelBusy {
                holder_slot,
                holder_token,
            } => format!(
                "播放通道正被槽位 S{} 占用（租约 {holder_token}）——喇叭只有一个，需要先抢占",
                holder_slot + 1
            ),
            LeaseError::StaleLease {
                slot,
                given_token,
                current_token,
            } => match current_token {
                Some(t) => format!(
                    "槽位 S{} 的播放回执携带租约 {given_token}，但当前活跃租约是 {t}——\
                     已拒绝（迟到回执不得消费新租约）",
                    slot + 1
                ),
                None => format!(
                    "槽位 S{} 的播放回执携带租约 {given_token}，但当前没有任何活跃租约——\
                     已拒绝（该租约已被抢占或超时释放）",
                    slot + 1
                ),
            },
        }
    }
}

/// 一次成功回执的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ConsumeOutcome {
    pub slot: usize,
    /// 消费后剩余的未读数。
    pub remaining: u32,
}

/// 单槽未读视图（进快照）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SlotUnreadView {
    pub index: usize,
    pub pending: u32,
    pub generation: u64,
    /// 该槽是否正持有播放租约。
    pub leased: bool,
}

/// 未读账本快照。`PartialEq` 供 `state.rs` 做"真变化才发事件"的指纹。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UnreadSnapshot {
    pub total_pending: u32,
    /// 当前持有播放租约的槽位（`None` = 播放通道空闲）。
    pub playing_slot: Option<usize>,
    pub slots: Vec<SlotUnreadView>,
}

/// 持久化形态。**刻意不含租约**——见 [`UnreadLedger::to_persist`]。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnreadPersist {
    pub schema: u32,
    /// `(pending, generation)`，按槽位顺序。
    pub slots: Vec<(u32, u64)>,
}

/// 未读持久化的格式版本。
pub const UNREAD_SCHEMA: u32 = 1;

/// 单槽内部状态。
#[derive(Default, Clone)]
struct SlotUnread {
    pending: u32,
    generation: u64,
}

/// 未读账本：每槽未读数 + 全局唯一的播放租约。
pub struct UnreadLedger {
    slots: Vec<SlotUnread>,
    /// 全局唯一活跃租约（喇叭单通道）。
    active: Option<Lease>,
    next_token: u64,
}

impl UnreadLedger {
    /// 建一个 `slot_count` 槽的空账本。
    pub fn new(slot_count: usize) -> Self {
        Self {
            slots: (0..slot_count).map(|_| SlotUnread::default()).collect(),
            active: None,
            next_token: 1,
        }
    }

    // -----------------------------------------------------------------------
    // 产生未读
    // -----------------------------------------------------------------------

    /// 某槽产生一条新未读，返回累计值。
    ///
    /// **不索取租约、不影响正在进行的播放**：用户正在听旧内容，新内容老老实实排队。
    /// 这正是"一次播放只消费一条"的实现方式——租约没有被作废，它消费的仍是它那一代。
    pub fn push_result(&mut self, slot: usize) -> Result<u32, LeaseError> {
        self.check_slot(slot)?;
        let s = &mut self.slots[slot];
        s.generation += 1;
        s.pending = (s.pending + 1).min(MAX_UNREAD_PER_SLOT);
        Ok(s.pending)
    }

    // -----------------------------------------------------------------------
    // 播放租约
    // -----------------------------------------------------------------------

    /// 索取播放租约。
    ///
    /// 成功的前提：① 该槽有未读；② 喇叭空闲（没有被别的槽占用）。
    /// 被占用时返回 [`LeaseError::ChannelBusy`]，**不自动抢占**——
    /// 抢占是一个会影响别处的决定，必须由调用方显式做（先 `release`），
    /// 不能在借东西的时候顺手把别人的东西抢了。
    pub fn grant(&mut self, slot: usize, now_ms: u64) -> Result<Lease, LeaseError> {
        self.check_slot(slot)?;

        if let Some(holder) = self.active {
            return Err(LeaseError::ChannelBusy {
                holder_slot: holder.slot,
                holder_token: holder.token,
            });
        }
        let s = &self.slots[slot];
        if s.pending == 0 {
            return Err(LeaseError::NothingToPlay { slot });
        }

        let lease = Lease {
            slot,
            token: self.next_token,
            granted_at_ms: now_ms,
            generation: s.generation,
            granted_pending: s.pending,
        };
        self.next_token += 1;
        self.active = Some(lease);
        Ok(lease)
    }

    /// 归还租约，**不消费未读**。
    ///
    /// 用于两种情形：① 用户抢占（播到一半按了别的槽）② 播放流程放弃。
    /// 无论哪种，该槽的灯都继续亮着——内容确实还没听完。
    pub fn release(&mut self, lease: Lease) -> Result<(), LeaseError> {
        self.check_slot(lease.slot)?;
        match self.active {
            Some(cur) if cur.token == lease.token => {
                self.active = None;
                Ok(())
            }
            other => Err(LeaseError::StaleLease {
                slot: lease.slot,
                given_token: lease.token,
                current_token: other.map(|c| c.token),
            }),
        }
    }

    /// 回执：这条内容**完整播完了**，消费掉一条未读。
    ///
    /// **只有精确匹配当前活跃租约的回执才有效。** 迟到回执（token 不匹配）一律拒绝，
    /// 且**不消费任何未读**——这是本模块存在的主要理由，见文件头说明。
    pub fn consume(&mut self, lease: Lease, _now_ms: u64) -> Result<ConsumeOutcome, LeaseError> {
        self.check_slot(lease.slot)?;
        match self.active {
            Some(cur) if cur.token == lease.token => {}
            other => {
                return Err(LeaseError::StaleLease {
                    slot: lease.slot,
                    given_token: lease.token,
                    current_token: other.map(|c| c.token),
                })
            }
        }
        // 校验通过：清租约 + 扣一条。
        self.active = None;
        let s = &mut self.slots[lease.slot];
        s.pending = s.pending.saturating_sub(1);
        Ok(ConsumeOutcome {
            slot: lease.slot,
            remaining: s.pending,
        })
    }

    /// 释放所有超时租约，返回被释放的槽位列表。
    ///
    /// 由编排器的定时器周期性调用（或每次状态变更时顺手调）。
    /// 返回列表而不是数量，是为了让调用方能把"哪个槽的播放被判超时"记进日志——
    /// 一个静默失效的租约是最难查的故障。
    pub fn expire(&mut self, now_ms: u64) -> Vec<usize> {
        let Some(cur) = self.active else {
            return Vec::new();
        };
        if now_ms.saturating_sub(cur.granted_at_ms) >= LEASE_TTL_MS {
            self.active = None;
            return vec![cur.slot];
        }
        Vec::new()
    }

    // -----------------------------------------------------------------------
    // 观测
    // -----------------------------------------------------------------------

    /// 某槽未读数。
    pub fn pending(&self, slot: usize) -> Result<u32, LeaseError> {
        Ok(self
            .slots
            .get(slot)
            .ok_or(LeaseError::SlotOutOfRange {
                slot,
                slots: self.slots.len(),
            })?
            .pending)
    }

    /// 全部槽未读总数。
    pub fn total_pending(&self) -> u32 {
        self.slots.iter().map(|s| s.pending).sum()
    }

    /// 当前租约（若有）。
    pub fn active_lease(&self) -> Option<Lease> {
        self.active
    }

    /// 某槽是否正持有租约。
    pub fn is_playing(&self, slot: usize) -> bool {
        self.active.map(|l| l.slot == slot).unwrap_or(false)
    }

    /// 生成快照。
    pub fn snapshot(&self) -> UnreadSnapshot {
        let slots = self
            .slots
            .iter()
            .enumerate()
            .map(|(i, s)| SlotUnreadView {
                index: i,
                pending: s.pending,
                generation: s.generation,
                leased: self.is_playing(i),
            })
            .collect();
        UnreadSnapshot {
            total_pending: self.total_pending(),
            playing_slot: self.active.map(|l| l.slot),
            slots,
        }
    }

    // -----------------------------------------------------------------------
    // 持久化
    // -----------------------------------------------------------------------

    /// 导出持久化形态。
    ///
    /// **刻意不包含租约**——这是一个关键设计，不是遗漏：
    ///
    /// - 租约描述的是"某个进程正在播某段音频"，它是**进程内的物理状态**，
    ///   跨进程毫无意义（新进程并没有在播任何东西）。
    /// - 丢掉租约 ⇒ 重启后 `active` 为空 ⇒ 该槽的未读**原封不动**。
    ///   于是"播到一半拔电 → 未读保留"这条验收标准**自动成立**，
    ///   不需要任何额外的"恢复"逻辑。
    ///
    /// 换句话说：把不该持久化的东西持久化，才会制造"重启后未读凭空消失"这类 bug。
    pub fn to_persist(&self) -> UnreadPersist {
        UnreadPersist {
            schema: UNREAD_SCHEMA,
            slots: self.slots.iter().map(|s| (s.pending, s.generation)).collect(),
        }
    }

    /// 从持久化形态恢复。
    ///
    /// 槽位数以 `slot_count` 为准（配置是槽位数的真源）：
    /// 持久化里多出的槽被忽略，缺少的槽按 0 补齐。这样"改了槽位数"不会让恢复失败，
    /// 只会丢超出部分的历史——比整份丢弃要好。
    pub fn from_persist(p: &UnreadPersist, slot_count: usize) -> Self {
        let mut led = Self::new(slot_count);
        for (i, (pending, generation)) in p.slots.iter().enumerate() {
            if i >= slot_count {
                break;
            }
            led.slots[i].pending = (*pending).min(MAX_UNREAD_PER_SLOT);
            led.slots[i].generation = *generation;
        }
        led
    }

    fn check_slot(&self, slot: usize) -> Result<(), LeaseError> {
        if slot >= self.slots.len() {
            return Err(LeaseError::SlotOutOfRange {
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

    fn led() -> UnreadLedger {
        UnreadLedger::new(4)
    }

    // -----------------------------------------------------------------------
    // 2.5 验收：迟到回执不消费新租约（本模块存在的理由）
    // -----------------------------------------------------------------------

    #[test]
    fn late_ack_never_consumes_new_lease() {
        let mut l = led();
        l.push_result(0).unwrap();

        // 第一次播放：拿到租约 token=1。
        let old = l.grant(0, 1_000).unwrap();
        assert_eq!(old.token, 1);

        // 播放中……用户抢占（或流程放弃），归还租约。
        l.release(old).unwrap();
        // 又来了新结果，用户再次索取租约 → token=2。
        l.push_result(0).unwrap();
        let new = l.grant(0, 2_000).unwrap();
        assert_eq!(new.token, 2);

        // 旧的回执姗姗来迟（"第一段播完了"）。
        let err = l.consume(old, 2_500).unwrap_err();
        match err {
            LeaseError::StaleLease {
                slot,
                given_token,
                current_token,
            } => {
                assert_eq!(slot, 0);
                assert_eq!(given_token, 1);
                assert_eq!(current_token, Some(2));
            }
            other => panic!("迟到回执必须被拒，实际 {other:?}"),
        }
        // 关键断言：未读**一条都没被消费**，且新租约仍然活跃。
        assert_eq!(l.pending(0).unwrap(), 2);
        assert_eq!(l.active_lease().map(|x| x.token), Some(2));
    }

    #[test]
    fn ack_after_release_without_new_lease_is_also_rejected() {
        let mut l = led();
        l.push_result(0).unwrap();
        let lease = l.grant(0, 1_000).unwrap();
        l.release(lease).unwrap();

        // 没有新租约，旧回执仍然无效（租约已归还 = 这次播放已被判定作废）。
        let err = l.consume(lease, 1_500).unwrap_err();
        assert_eq!(
            err,
            LeaseError::StaleLease {
                slot: 0,
                given_token: lease.token,
                current_token: None
            }
        );
        assert_eq!(l.pending(0).unwrap(), 1);
    }

    // -----------------------------------------------------------------------
    // 2.5 验收：断电重启未读仍在
    // -----------------------------------------------------------------------

    #[test]
    fn unread_survives_restart_without_any_recovery_logic() {
        let mut l = led();
        l.push_result(0).unwrap();
        l.push_result(0).unwrap();
        l.push_result(2).unwrap();

        // 播放槽 0 的第一条，播到一半 —— 拔电。
        let lease = l.grant(0, 1_000).unwrap();
        assert_eq!(lease.slot, 0);

        // 落盘 → 新进程恢复（这正是"拔电重启"的完整模拟）。
        let persisted = l.to_persist();
        let restored = UnreadLedger::from_persist(&persisted, 4);

        // 未读原封不动；租约消失（新进程并没有在播任何东西）。
        assert_eq!(restored.pending(0).unwrap(), 2);
        assert_eq!(restored.pending(2).unwrap(), 1);
        assert_eq!(restored.total_pending(), 3);
        assert_eq!(
            restored.active_lease(),
            None,
            "租约必须随进程消亡——它描述的是进程内的物理播放状态"
        );
        // 且恢复后能立刻重新播放（不会因为"旧租约还在"被锁住）。
        assert!(restored.clone_for_test().grant(0, 9_999).is_ok());
    }

    #[test]
    fn consume_outcome_is_persisted_too() {
        let mut l = led();
        l.push_result(0).unwrap();
        l.push_result(0).unwrap();
        let lease = l.grant(0, 1_000).unwrap();
        l.consume(lease, 1_100).unwrap();

        let restored = UnreadLedger::from_persist(&l.to_persist(), 4);
        assert_eq!(restored.pending(0).unwrap(), 1, "已消费的要落盘");
    }

    #[test]
    fn persist_tolerates_slot_count_change() {
        let mut l = led();
        for s in 0..4 {
            l.push_result(s).unwrap();
        }
        let p = l.to_persist();

        // 槽位变少：多出的历史被丢弃，不报错。
        let fewer = UnreadLedger::from_persist(&p, 2);
        assert_eq!(fewer.snapshot().slots.len(), 2);
        assert_eq!(fewer.total_pending(), 2);

        // 槽位变多：缺的按 0 补齐。
        let more = UnreadLedger::from_persist(&p, 8);
        assert_eq!(more.snapshot().slots.len(), 8);
        assert_eq!(more.total_pending(), 4);
        assert_eq!(more.pending(7).unwrap(), 0);
    }

    // -----------------------------------------------------------------------
    // 2.5 验收：抢占不消费未读
    // -----------------------------------------------------------------------

    #[test]
    fn preemption_keeps_unread() {
        let mut l = led();
        l.push_result(0).unwrap();
        l.push_result(1).unwrap();

        let a = l.grant(0, 1_000).unwrap();
        // 用户按了槽 1 —— 必须先抢占（release 槽 0），而 release 不消费未读。
        l.release(a).unwrap();
        let b = l.grant(1, 1_500).unwrap();
        assert_eq!(b.slot, 1);

        // 槽 0 的未读还在（灯继续亮），槽 1 正在播。
        assert_eq!(l.pending(0).unwrap(), 1);
        assert!(l.is_playing(1));
        assert!(!l.is_playing(0));
    }

    #[test]
    fn channel_busy_is_explicit_and_does_not_auto_preempt() {
        let mut l = led();
        l.push_result(0).unwrap();
        l.push_result(1).unwrap();
        let a = l.grant(0, 1_000).unwrap();

        // 喇叭被槽 0 占用 → 槽 1 索取必须明确报错，而不是偷偷把槽 0 抢掉。
        let err = l.grant(1, 1_100).unwrap_err();
        assert_eq!(
            err,
            LeaseError::ChannelBusy {
                holder_slot: 0,
                holder_token: a.token
            }
        );
        // 槽 0 的租约没有被影响。
        assert_eq!(l.active_lease().map(|x| x.slot), Some(0));
    }

    // -----------------------------------------------------------------------
    // 一次播放只消费一条
    // -----------------------------------------------------------------------

    #[test]
    fn new_result_during_playback_is_not_consumed() {
        let mut l = led();
        l.push_result(0).unwrap();
        let lease = l.grant(0, 1_000).unwrap();

        // 播放中，同槽又完成一个任务 → 新未读进来。
        l.push_result(0).unwrap();
        assert_eq!(l.pending(0).unwrap(), 2);

        // 用户听完的是**旧那条**，所以只减一条。
        let out = l.consume(lease, 5_000).unwrap();
        assert_eq!(out.remaining, 1, "新来的那条必须仍然未读");
        assert_eq!(l.pending(0).unwrap(), 1);
    }

    #[test]
    fn three_unread_need_three_playbacks() {
        let mut l = led();
        for _ in 0..3 {
            l.push_result(0).unwrap();
        }
        for expect_remaining in [2, 1, 0] {
            let lease = l.grant(0, 1_000).unwrap();
            let out = l.consume(lease, 1_100).unwrap();
            assert_eq!(out.remaining, expect_remaining);
        }
        // 三条都听完 → 没得播了，且这个状态被明确说出来。
        assert_eq!(
            l.grant(0, 2_000).unwrap_err(),
            LeaseError::NothingToPlay { slot: 0 }
        );
    }

    // -----------------------------------------------------------------------
    // 上限与超时
    // -----------------------------------------------------------------------

    #[test]
    fn pending_is_capped_at_display_max() {
        let mut l = led();
        for _ in 0..(MAX_UNREAD_PER_SLOT + 5) {
            l.push_result(0).unwrap();
        }
        assert_eq!(l.pending(0).unwrap(), MAX_UNREAD_PER_SLOT);
        // 代际仍在累加（它是历史计数，不是显示值）。
        assert_eq!(l.snapshot().slots[0].generation, (MAX_UNREAD_PER_SLOT + 5) as u64);
    }

    #[test]
    fn generation_increments_on_every_push() {
        let mut l = led();
        assert_eq!(l.snapshot().slots[0].generation, 0);
        l.push_result(0).unwrap();
        l.push_result(0).unwrap();
        assert_eq!(l.snapshot().slots[0].generation, 2);
    }

    #[test]
    fn stale_lease_expires_and_frees_the_channel() {
        let mut l = led();
        l.push_result(0).unwrap();
        l.push_result(1).unwrap();
        let a = l.grant(0, 1_000).unwrap();

        // 还没到点：不动。
        assert!(l.expire(1_000 + LEASE_TTL_MS - 1).is_empty());
        assert!(l.is_playing(0));

        // 到点：释放，且**明确报告是哪个槽**（静默失效的租约最难查）。
        let expired = l.expire(1_000 + LEASE_TTL_MS);
        assert_eq!(expired, vec![0]);
        assert_eq!(l.active_lease(), None);
        // 未读不受影响；通道腾出来了，槽 1 现在能播。
        assert_eq!(l.pending(0).unwrap(), 1);
        assert!(l.grant(1, 1_000 + LEASE_TTL_MS).is_ok());

        let _ = a;
    }

    #[test]
    fn expired_lease_ack_is_rejected() {
        let mut l = led();
        l.push_result(0).unwrap();
        let lease = l.grant(0, 1_000).unwrap();
        l.expire(1_000 + LEASE_TTL_MS);

        // 超时释放之后，那个播放流程若还活着并把回执送回来 —— 必须被拒。
        let err = l.consume(lease, 1_000 + LEASE_TTL_MS + 1).unwrap_err();
        assert!(matches!(err, LeaseError::StaleLease { .. }));
        assert_eq!(l.pending(0).unwrap(), 1, "超时不是'听完'，未读必须保留");
    }

    // -----------------------------------------------------------------------
    // 边界与快照
    // -----------------------------------------------------------------------

    #[test]
    fn out_of_range_slot_is_rejected_everywhere() {
        let mut l = led();
        assert_eq!(
            l.push_result(9).unwrap_err(),
            LeaseError::SlotOutOfRange { slot: 9, slots: 4 }
        );
        assert_eq!(
            l.grant(9, 1_000).unwrap_err(),
            LeaseError::SlotOutOfRange { slot: 9, slots: 4 }
        );
        assert!(matches!(
            l.pending(9),
            Err(LeaseError::SlotOutOfRange { .. })
        ));
    }

    #[test]
    fn snapshot_marks_playing_slot_and_is_stable() {
        let mut l = led();
        l.push_result(0).unwrap();
        l.push_result(3).unwrap();
        let _lease = l.grant(3, 1_000).unwrap();

        let s = l.snapshot();
        assert_eq!(s.playing_slot, Some(3));
        assert_eq!(s.total_pending, 2);
        assert!(s.slots[3].leased);
        assert!(!s.slots[0].leased);
        // 状态未变 → 快照必须逐字节相同（否则 state.rs 会刷屏发事件）。
        assert_eq!(s, l.snapshot());
    }

    #[test]
    fn empty_ledger_reports_totals_as_zero() {
        let l = led();
        let s = l.snapshot();
        assert_eq!(s.total_pending, 0);
        assert_eq!(s.playing_slot, None);
        assert!(s.slots.iter().all(|v| v.pending == 0 && !v.leased));
    }

    // 测试辅助：`grant` 需要 `&mut self`，而上面有几处断言要复用不可变引用。
    impl UnreadLedger {
        fn clone_for_test(&self) -> Self {
            Self {
                slots: self.slots.clone(),
                active: self.active,
                next_token: self.next_token,
            }
        }
    }
}
