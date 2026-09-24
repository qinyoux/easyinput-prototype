//! 任务台的设备灯语：把运行时快照映射成 5 颗 WS2812 的颜色，并通过 `0x16`
//! 逐像素帧下发给开发板。
//!
//! # 为什么需要一条独立通道，而不是复用既有的 `0x12` Agent Status
//!
//! 固件**已经有**一条 `0x12` Agent Status 通道（`agent_status.h`），它的状态枚举
//! （`kIdle` / `kRunning` / `kWaitingUser` / `kCompletedUnread` / `kFailed`）与任务台
//! 语义其实相当吻合。但它的渲染是**整条灯带一个颜色**：`render_agent_status()` 用
//! `state` 查表拿到**一个** `Rgb`，再按"到中心的距离"做衰减铺满 5 颗。
//!
//! 任务台要表达的却是**五路彼此独立**的信息：4 个槽各自有几条未读、喇叭正在播哪个槽、
//! 全局有几个槽在跑。一个 `state` 装不下这种结构，而"扩 `0x12` 的 payload"等于**修改**
//! 一条已在用的协议（固件 `AGENTS.md` 明确禁止擅自修改通信协议）。
//!
//! 所以走**纯增量**：新增 `0x16`，`0x12` 一个字节都不动。两条通道在固件侧的优先级是
//! `0x16`（任务台帧）> `0x12`（Agent 状态）> idle —— 谁在生效由各自的 ttl 决定。
//!
//! # 帧结构与 `0x12` 刻意同构
//!
//! 头部的 `version / flags / sequence / ttl_ms` 放在**与 `0x12` 完全相同的偏移**上
//! （`0x12`：`[0]=version [1]=state [2]=flags [4..8]=sequence [8..12]=ttl`）。这样固件侧
//! 可以直接复用两份现成的纯逻辑：`read_agent_status_u32_le()` 读多字节整数、
//! `agent_status_sequence_is_newer()` 判新旧 —— 少写一份、也少一处会漂移的分叉。
//!
//! ```text
//! offset  长度  含义
//! 0       1     version = 1
//! 1       1     flags   bit0 = active（0 = 清除本帧、立刻回落既有状态机）
//! 2       2     保留（必须写 0）
//! 4       4     sequence（u32 LE，单调递增，永不复用；旧帧被丢弃）
//! 8       4     ttl_ms（u32 LE；active=0 时强制写 0，与 0x12 的 Idle 规则同构）
//! 12      4     保留（必须写 0）
//! 16      15    5 颗灯的 RGB：pixel0.r/g/b … pixel4.r/g/b
//! 31      32    保留（必须写 0）
//! ```
//!
//! Feature Report 总长 = 1（Report ID = `0x16`）+ 63 = 64 字节，与既有 `0x10`/`0x14`
//! 的 64 字节形态一致（HID 描述符里 Report Count 一律 63）。
//!
//! # 下发节奏：心跳 + ttl
//!
//! 灯语**不是**"发一次就永远亮着"。每一帧自带 `ttl_ms`，到期后固件自动回落到既有
//! 状态机。所以上位机要**周期性重发**（心跳）。这样带来的两个性质都是我们想要的：
//!
//! - **上位机崩溃/退出 ⇒ 灯自动回落到 idle**，不会留下一个永远亮着的"幽灵未读灯"；
//! - 拔掉 USB 同理（固件侧 ttl 与连接无关，只看本地时钟）。
//!
//! 心跳周期必须显著小于 ttl（这里 5s 心跳 / 15s ttl，容忍两次连续丢帧）。

use std::sync::atomic::{AtomicU32, Ordering};

use serde::Serialize;

use crate::agent_desk::state::RuntimeSnapshot;
use crate::constants::REPORT_ID_DESK_LED;
use crate::transport;

/// 灯珠数量。真源是固件 `board_pins.h::kWs2812Count`，两侧必须一致。
pub const PIXEL_COUNT: usize = 5;

/// 协议版本。固件 `decode_desk_led_frame` 只认这一个值。
pub const DESK_LED_PROTOCOL_VERSION: u8 = 1;

/// payload 长度（不含 Report ID 首字节）。
pub const DESK_LED_PAYLOAD_LEN: usize = 63;

/// 完整 Feature Report 长度（含 Report ID 首字节）。
pub const DESK_LED_REPORT_LEN: usize = 1 + DESK_LED_PAYLOAD_LEN;

/// 心跳周期（毫秒）。上位机按这个节奏重发同一帧。
pub const HEARTBEAT_MS: u64 = 5_000;

/// 默认 ttl（毫秒）。**必须显著大于 [`HEARTBEAT_MS`]**，否则网络/调度抖一下灯就闪回。
pub const DEFAULT_TTL_MS: u32 = 15_000;

/// 单槽未读数的亮度分档上限（第 3 档）。到顶不再增——见 [`unread_tier`]。
const UNREAD_TIER_MAX: u32 = 3;

/// 一个颜色。值域与固件既有色板一致（0–30 量级，不是 0–255）。
///
/// ⚠️ 不是笔误：固件 `led_strip_status.cpp` 里的色板用的就是 0–30 这个量级
/// （如 `kCompletedUnread` = `{0, 24, 5}`）。要跟既有灯效视觉一致，就必须用同一个量级。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const OFF: Rgb = Rgb { r: 0, g: 0, b: 0 };

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// 按 `numerator / denominator` 缩放（整数运算，结果截断）。
    ///
    /// 用整数而不是浮点：这些值最终要进 HID 报文的字节流，整数能让单测断言
    /// 写成精确相等，不必引入浮点容差这种"看起来在测其实没测死"的判据。
    fn scaled(self, numerator: u32, denominator: u32) -> Rgb {
        if denominator == 0 {
            return Rgb::OFF;
        }
        let scale = |v: u8| ((v as u32 * numerator) / denominator).min(255) as u8;
        Rgb::new(scale(self.r), scale(self.g), scale(self.b))
    }
}

/// 某槽有未读时用的底色（与固件 `kCompletedUnread` 同色）。
const UNREAD_COLOR: Rgb = Rgb::new(0, 24, 5);

/// 喇叭正在播这个槽时的颜色。
///
/// **刻意不用绿色**：绿色已经表示"有未读等你听"，而"正在播"是另一件事
/// （此刻正在消费未读）。用白色让这两种状态一眼可分，而不是靠亮度去猜。
const PLAYING_COLOR: Rgb = Rgb::new(18, 22, 22);

/// 第 5 颗灯：运行数 0–4 → 绿 / 黄 / 橙 / 紫 / 红。
///
/// 语义是**负载指示**：越忙越"烫"。两端的取舍：
/// - 0 运行时亮**绿**（待命），不是灭 —— 灭留给"设备没连上/未初始化"，两者必须可区分；
/// - 4 运行时红，与固件 `kFailed` 的红同色 —— 满负荷是需要被注意的状态。
const RUN_COLORS: [Rgb; 5] = [
    Rgb::new(0, 24, 5),  // 0 · 绿（待命）
    Rgb::new(24, 20, 0), // 1 · 黄
    Rgb::new(30, 12, 0), // 2 · 橙（与固件 kWaitingUser 同色）
    Rgb::new(16, 0, 28), // 3 · 紫
    Rgb::new(30, 0, 8),  // 4 · 红（与固件 kFailed 同色）
];

/// 一帧灯语。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct DeskLedFrame {
    /// 单调递增，永不复用。固件用它丢弃迟到的旧帧。
    pub sequence: u32,
    /// 存活时间（毫秒）。0 = 立刻失效。
    pub ttl_ms: u32,
    /// 是否生效。`false` 表示"清除任务台帧、立刻回落既有状态机"。
    pub active: bool,
    pub pixels: [Rgb; PIXEL_COUNT],
}

impl DeskLedFrame {
    /// 构造一帧"清除"帧：让设备回落到既有状态机（0x12 / idle）。
    pub fn clear(sequence: u32) -> Self {
        Self {
            sequence,
            ttl_ms: 0,
            active: false,
            pixels: [Rgb::OFF; PIXEL_COUNT],
        }
    }

    /// 编码成一条完整的 64 字节 Feature Report（首字节 = Report ID）。
    pub fn encode(&self) -> [u8; DESK_LED_REPORT_LEN] {
        let mut report = [0u8; DESK_LED_REPORT_LEN];
        report[0] = REPORT_ID_DESK_LED;
        let p = &mut report[1..];

        p[0] = DESK_LED_PROTOCOL_VERSION;
        p[1] = if self.active { 0x01 } else { 0x00 };
        // p[2..4] 保留（已初始化为 0）
        p[4..8].copy_from_slice(&self.sequence.to_le_bytes());
        // active=false 时 ttl 强制为 0：与固件 0x12 的"Idle 强制 ttl=0"同构，
        // 让"清除"这件事不依赖调用方记得传 0。
        let ttl = if self.active { self.ttl_ms } else { 0 };
        p[8..12].copy_from_slice(&ttl.to_le_bytes());
        // p[12..16] 保留
        for (index, pixel) in self.pixels.iter().enumerate() {
            let base = 16 + index * 3;
            p[base] = pixel.r;
            p[base + 1] = pixel.g;
            p[base + 2] = pixel.b;
        }
        // p[31..63] 保留
        report
    }
}

/// 由快照算出 5 颗灯的颜色。**纯函数**：不碰设备、不碰全局状态。
///
/// 布局：
/// - 第 1–4 颗（index 0–3）= 槽 1–4 的未读（绿，亮度分 3 档）；
/// - 第 5 颗（index 4）= 正在执行的槽数（0–4 → 绿/黄/橙/紫/红）。
pub fn language_for(snapshot: &RuntimeSnapshot) -> [Rgb; PIXEL_COUNT] {
    let mut pixels = [Rgb::OFF; PIXEL_COUNT];

    for slot in 0..(PIXEL_COUNT - 1) {
        let Some(view) = snapshot.unread.slots.get(slot) else {
            // 快照槽数比灯珠少：留灭。不 panic、也不"借用"别的槽的数据。
            continue;
        };
        pixels[slot] = if view.leased {
            PLAYING_COLOR
        } else {
            match unread_tier(view.pending) {
                Some((numerator, denominator)) => UNREAD_COLOR.scaled(numerator, denominator),
                None => Rgb::OFF,
            }
        };
    }

    let running = snapshot.queue.running_count.min(RUN_COLORS.len() - 1);
    pixels[PIXEL_COUNT - 1] = RUN_COLORS[running];
    pixels
}

/// 未读数 → 亮度档位 `(分子, 分母)`。返回 `None` 表示"无未读、灭灯"。
///
/// **刻意不是 9 档线性亮度**：`MAX_UNREAD_PER_SLOT` 是 9，若按 1/9…9/9 线性，
/// 最暗那档是 `{0, 24, 5} × 1/9 = {0, 2, 0}` —— 在 WS2812 上基本等于看不见，
/// 而人眼对亮度是**对数**感知的，线性分档在低端几乎不可分辨。
/// 改 3 档（1–3 / 4–6 / ≥7）后每一档都清晰可辨，且"到顶不再增"的意图不变。
fn unread_tier(pending: u32) -> Option<(u32, u32)> {
    if pending == 0 {
        return None;
    }
    let tier = ((pending + 2) / 3).min(UNREAD_TIER_MAX);
    Some((tier, UNREAD_TIER_MAX))
}

/// 全局单调序号。`fetch_add` 保证多线程下也**不会复用**同一个值。
static SEQUENCE: AtomicU32 = AtomicU32::new(0);

/// 取下一个序号。从 1 开始（0 保留给"尚未发过任何帧"）。
pub fn next_sequence() -> u32 {
    SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1
}

/// 把快照算成灯语并下发给设备。
///
/// 失败**必须**把原因带回去（"失败不静默"）：调用方要么显示它、要么记日志，
/// 但不能假装灯已经变了。
pub fn push_snapshot(snapshot: &RuntimeSnapshot) -> Result<DeskLedFrame, String> {
    let frame = DeskLedFrame {
        sequence: next_sequence(),
        ttl_ms: DEFAULT_TTL_MS,
        active: true,
        pixels: language_for(snapshot),
    };
    transport::send_feature_report(&frame.encode())?;
    Ok(frame)
}

/// 下发一帧"清除"，让设备回落到既有状态机。
pub fn push_clear() -> Result<DeskLedFrame, String> {
    let frame = DeskLedFrame::clear(next_sequence());
    transport::send_feature_report(&frame.encode())?;
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_desk::queue::{QueueSnapshot, SlotQueueView};
    use crate::agent_desk::unread::{SlotUnreadView, UnreadSnapshot};

    /// 构造一个可控的快照：给定各槽未读数、运行数、播放中的槽。
    fn snapshot(unread: &[u32], running_count: usize, playing: Option<usize>) -> RuntimeSnapshot {
        let slot_count = 4;
        let queue_slots = (0..slot_count)
            .map(|index| SlotQueueView {
                index,
                running: None,
                pending: Vec::new(),
                depth: 0,
            })
            .collect();
        let unread_slots = (0..slot_count)
            .map(|index| SlotUnreadView {
                index,
                pending: unread.get(index).copied().unwrap_or(0),
                generation: 0,
                leased: playing == Some(index),
            })
            .collect();
        RuntimeSnapshot {
            queue: QueueSnapshot {
                running_count,
                total_pending: 0,
                active_drivers: Vec::new(),
                completed: 0,
                failed: 0,
                slots: queue_slots,
            },
            unread: UnreadSnapshot {
                total_pending: unread.iter().sum(),
                playing_slot: playing,
                slots: unread_slots,
            },
            busy: running_count > 0,
            idle: running_count == 0,
            any_unread: unread.iter().any(|n| *n > 0),
        }
    }

    #[test]
    fn encode_lays_out_header_and_pixels_at_fixed_offsets() {
        let frame = DeskLedFrame {
            sequence: 0x1122_3344,
            ttl_ms: 15_000,
            active: true,
            pixels: [
                Rgb::new(1, 2, 3),
                Rgb::new(4, 5, 6),
                Rgb::new(7, 8, 9),
                Rgb::new(10, 11, 12),
                Rgb::new(13, 14, 15),
            ],
        };
        let report = frame.encode();

        assert_eq!(report.len(), 64);
        assert_eq!(report[0], REPORT_ID_DESK_LED);
        assert_eq!(report[1], DESK_LED_PROTOCOL_VERSION);
        assert_eq!(report[2], 0x01, "active 必须落到 flags bit0");
        assert_eq!(&report[3..5], &[0, 0], "保留位必须为 0");
        assert_eq!(
            &report[5..9],
            &0x1122_3344u32.to_le_bytes(),
            "sequence 必须在偏移 4（payload 内）"
        );
        assert_eq!(&report[9..13], &15_000u32.to_le_bytes(), "ttl 必须在 payload 偏移 8");
        // 保留区（payload 12..16）必须为 0
        assert_eq!(&report[13..17], &[0, 0, 0, 0]);
        // 第一颗灯在 payload 偏移 16（报文偏移 17）
        assert_eq!(&report[17..20], &[1, 2, 3]);
        assert_eq!(&report[20..23], &[4, 5, 6]);
        assert_eq!(&report[29..32], &[13, 14, 15], "第 5 颗灯必须落在 payload 偏移 28");
        // 尾部保留区
        assert!(report[32..].iter().all(|b| *b == 0), "尾部保留区必须全 0");
    }

    #[test]
    fn clear_frame_zeroes_ttl_and_all_pixels() {
        let frame = DeskLedFrame::clear(7);
        assert!(!frame.active);
        assert_eq!(frame.ttl_ms, 0);
        let report = frame.encode();
        assert_eq!(report[2], 0x00, "非 active 时 flags bit0 必须为 0");
        assert_eq!(&report[9..13], &0u32.to_le_bytes());
        // 全灭
        assert!(report[17..32].iter().all(|b| *b == 0));
    }

    #[test]
    fn inactive_frame_forces_zero_ttl_even_if_caller_sets_one() {
        // "清除"这件事不能依赖调用方记得传 ttl=0 —— 与固件 0x12 的 Idle 规则同构。
        let frame = DeskLedFrame {
            sequence: 1,
            ttl_ms: 99_999,
            active: false,
            pixels: [Rgb::OFF; PIXEL_COUNT],
        };
        let report = frame.encode();
        assert_eq!(&report[9..13], &0u32.to_le_bytes());
    }

    #[test]
    fn no_unread_leaves_first_four_pixels_dark() {
        let pixels = language_for(&snapshot(&[0, 0, 0, 0], 0, None));
        for (i, pixel) in pixels.iter().enumerate().take(PIXEL_COUNT - 1) {
            assert_eq!(*pixel, Rgb::OFF, "槽 {i} 无未读必须是灭的");
        }
    }

    #[test]
    fn unread_brightness_has_three_tiers_and_saturates() {
        let one = language_for(&snapshot(&[1, 0, 0, 0], 0, None))[0];
        let four = language_for(&snapshot(&[4, 0, 0, 0], 0, None))[0];
        let seven = language_for(&snapshot(&[7, 0, 0, 0], 0, None))[0];
        let nine = language_for(&snapshot(&[9, 0, 0, 0], 0, None))[0];
        let huge = language_for(&snapshot(&[200, 0, 0, 0], 0, None))[0];

        // 三档彼此不同
        assert_ne!(one, four);
        assert_ne!(four, seven);
        // 亮度单调不减
        assert!(one.g < four.g && four.g < seven.g, "亮度必须随未读数递增");
        // 到上限不再增（9 与 200 同一档）
        assert_eq!(nine, seven);
        assert_eq!(huge, seven, "超过上限必须饱和，不能继续变亮");
        // 色相必须仍是"未读绿"的缩放：绿色通道主导。
        // 不断言 r/b 恰好为 0 —— 底色里 b=5，缩到 1/3 档是 1（整数截断），
        // 写死成 0 会让这条测试变成"看运气"的东西。判"主导"才是有意义的判据。
        assert!(
            one.g > one.r && one.g > one.b,
            "未读灯必须以绿为主导，实际 {one:?}"
        );
    }

    #[test]
    fn playing_slot_is_white_not_green_so_it_is_never_confused_with_unread() {
        let pixels = language_for(&snapshot(&[3, 0, 0, 0], 0, Some(0)));
        assert_eq!(pixels[0], PLAYING_COLOR);
        assert_ne!(pixels[0], UNREAD_COLOR.scaled(1, 3));
        // 白色三通道都非零 —— 这是它与绿色最直观的区别
        assert!(pixels[0].r > 0 && pixels[0].g > 0 && pixels[0].b > 0);
    }

    #[test]
    fn running_count_maps_to_five_distinct_colors() {
        let colors: Vec<Rgb> = (0..=4)
            .map(|n| language_for(&snapshot(&[0; 4], n, None))[PIXEL_COUNT - 1])
            .collect();
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j], "运行数 {i} 与 {j} 必须不同色");
            }
        }
        assert_eq!(colors[0], RUN_COLORS[0]);
        assert_eq!(colors[4], RUN_COLORS[4]);
    }

    #[test]
    fn running_count_beyond_four_saturates_at_red() {
        let red = language_for(&snapshot(&[0; 4], 4, None))[PIXEL_COUNT - 1];
        let more = language_for(&snapshot(&[0; 4], 99, None))[PIXEL_COUNT - 1];
        assert_eq!(more, red, "运行数超过灯能表达的档位必须饱和，不能越界");
    }

    #[test]
    fn fewer_slots_than_pixels_does_not_panic_and_leaves_them_dark() {
        // 快照只有 2 个槽（协议允许的退化输入），不能 panic、也不许借用别的槽的数据。
        let snap = RuntimeSnapshot {
            queue: QueueSnapshot {
                running_count: 0,
                total_pending: 0,
                active_drivers: Vec::new(),
                completed: 0,
                failed: 0,
                slots: Vec::new(),
            },
            unread: UnreadSnapshot {
                total_pending: 0,
                playing_slot: None,
                slots: vec![
                    SlotUnreadView { index: 0, pending: 5, generation: 0, leased: false },
                ],
            },
            busy: false,
            idle: true,
            any_unread: true,
        };
        let pixels = language_for(&snap);
        assert_ne!(pixels[0], Rgb::OFF, "存在的槽必须正常点亮");
        assert_eq!(pixels[1], Rgb::OFF);
        assert_eq!(pixels[3], Rgb::OFF);
    }

    #[test]
    fn sequence_never_repeats() {
        let a = next_sequence();
        let b = next_sequence();
        let c = next_sequence();
        assert!(a < b && b < c, "序号必须严格递增");
        assert_ne!(a, 0, "0 保留给『尚未发过帧』");
    }
}
