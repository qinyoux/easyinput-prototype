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
//! # 现状：v2 语义已实机验证通过（2026-10-14，v0.43.2）
//!
//! 四态（执行蓝 / 播放青 / 待播绿 / 空闲全灭）+ 三档亮度 + S5 按键闪
//! 均已在真板上确认为预期表现。期间踩过的坑见本文末尾「HID 描述符缓存」一节。
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
//!
//! # 一个必须知道的外部约束：Windows 的 HID 描述符缓存
//!
//! 本模块调用 [`transport::send_feature_report`]（底层 `HidD_SetFeature`）下发 `0x16`。
//! 若失败，**先怀疑设备没重新枚举，而不是先怀疑本文件的逻辑**。
//!
//! 2026-10-14 实测：全矩阵探测发现只有 `0x10` 能下发、`0x12`–`0x16` 全部返回
//! `ERROR_INVALID_PARAMETER (0x57)`。而固件源码里`0x16` 描述符一直是齐的
//! —— 根因是 **Windows 缓存了旧版 HID 报告描述符**，固件更新了描述符但设备没重新枚举，
//! 主机侧仍按旧描述符校验，于是"协议对不上"表现为"参数无效"而不是"找不到设备"。
//!
//! **排查口诀**：灯完全不响应 → 先让设备重新枚举（关机再开机 / 拔插 USB /
//! 设备管理器里禁用再启用），再回头看代码。反过来做会浪费很久。
//!
//! 顺带一个反直觉事实：`ERROR_INVALID_FUNCTION (0x01)` 与`0x57` 语义不同——
//! 前者是"这个 Report ID 设备根本不支持"，后者是"支持但本次参数/长度不对"。
//! 全矩阵探测要按错误码分类读，别一律当成"失败"。

use std::sync::atomic::{AtomicU32, Ordering};

use serde::Serialize;

use crate::agent_desk::state::RuntimeSnapshot;
// `LightState` 只被下方 `#[cfg(test)]` 的构造器用到，主代码不碰。
// 不加 cfg 限定的话，release 构建会报 unused import 警告。
#[cfg(test)]
use crate::agent_desk::state::LightState;
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

/// 单槽未读数的最高档位。**刻意不是 9 档线性亮度**，理由见 [`brightness_tier`]。

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

/// **槽位待播底色**（有未读等你听）。与固件 `kCompletedUnread` 同色。
const PENDING_COLOR: Rgb = Rgb::new(0, 24, 5);

/// 槽位**执行**色：任务已下发、正在跑。
///
/// 选**蓝**而不是复用黄色/橙色：黄色与橙色在 WS2812 上相邻，色觉障碍者几乎分不开，
/// 而"正在跑"是最高频、最需要一眼认出的状态，必须占一个色相上离其他三色都远的位置。
const RUNNING_COLOR: Rgb = Rgb::new(0, 12, 30);

/// 槽位**播放**色：喇叭正在念这个槽的结果。
///
/// **刻意不用白色**（v1 曾用白 `{18,22,22}`）：白在三色通道都拉满，亮度高、耗电大，
/// 且与"待播绿"在低亮度下容易糊在一起。改用**青**，色相独立于绿/蓝，且亮度
/// 明显低于蓝（14 vs 30），远处扫一眼也能分开"在念"和"在跑"。
const PLAYING_COLOR: Rgb = Rgb::new(0, 20, 14);

/// 槽位**空闲**色：既没在跑、也没在播、也没有未读。
///
/// **全灭**（`Rgb::OFF`）。这与 v2 初稿相反——初稿用极暗蓝 `(0,1,2)`，
/// 理由是"全灭要留给设备没连上"。真机证明那个理由**站不住**：
/// ① WS2812 在 0–30 量级下 `(0,1,2)` 实际可辨，用户会读成"这槽有微弱状态"，
///    与"空闲 = 没事"直接矛盾；② 真正的掉线本来就另有判据（`send_feature_report`
///    报错 + `led_ok` 日志翻转），不需要靠"全灭"这个视觉信号来表达。
/// 现在空闲就是灭，语义最直接：**亮 = 有状态，灭 = 没事**。
const IDLE_COLOR: Rgb = Rgb::OFF;

// ── 待播亮度三档 ────────────────────────────────────────────────────────
//
// 三档而非 9 档线性：人眼对亮度是**对数**感知，而 `MAX_UNREAD_PER_SLOT = 9`
// 若线性分 1/9，最暗那档是 `{0, 24, 5} × 1/9 ≈ {0, 2, 0}` —— 实际接近看不见，
// 等于白占一个档位。三档每档都清晰可辨，且"到顶不再增"的意图不变。
//
// 分母固定 3：档位 t ∈ {1,2,3} 直接就是亮度分数，语义与"第几档"完全对齐。

/// 未读数分档的**分母**（= 最高档位）。亮度分数 = 档位 / 本常数。
const BRIGHTNESS_DENOM: u32 = 3;

/// 最高档位。到顶不再增——未读再多也是第 3 档。
const BRIGHTNESS_MAX_TIER: u32 = BRIGHTNESS_DENOM;

/// 第 5 颗灯：按键反馈专用。

/// 按键按下时第 5 颗灯闪一次的**全亮**色。
///
/// 用中性白：这一下表达的是"设备收到了你这一按"，是**输入确认**而不是任务状态，
/// 必须与任何槽位语义色都不同，也不该被误读成某个槽在跑。
const KEYPRESS_FLASH_COLOR: Rgb = Rgb::new(30, 30, 30);

/// 按键闪烁的**持续时间**（毫秒）。
///
/// 取 120ms：短到不干扰（不会让人以为"这一槽在跑"），长到余光能捕捉到。
/// 再短会错过（人眼对极短闪光只能感知到"暗了一下"），再长就会和状态色混淆。
pub const KEYPRESS_FLASH_MS: u64 = 120;

/// 第 5 颗灯按键闪烁后的**保持态**：全灭。
///
/// 与 [`IDLE_COLOR`] 一致——空闲即灭。S5 只在按键后那 120ms 里亮，
/// 之后必须回到全灭，否则"第五颗常亮一点"会被当成某种常驻状态。
const KEYPRESS_IDLE_COLOR: Rgb = Rgb::OFF;


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

/// 槽位灯色。**四选一**，互斥，优先级从上到下。
///
/// 排优先级不是随手定的，取决于"哪个更紧急"：
/// **播放 > 执行 > 待播 > 空闲**。理由是前两者都是**当下正在发生**的事，
/// 此刻就在发生的事必须压过"攒着等你处理"的事，否则灯会在你干活时一直提示未读。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotLight {
    /// 有未读结果等你听（亮度按未读数分三档）。
    Pending,
    /// 任务已下发、正在执行。
    Running,
    /// 喇叭正在念这个槽的结果。
    Playing,
    /// 没事。
    Idle,
}

/// 由快照算出这个槽该显示什么**状态**（不含亮度）。
///
/// 纯函数：不碰设备、不碰全局状态。播放优先于执行——两个能同时成立
/// （任务还没跑完就先播了上一条的结果），此时"正在念"是更贴近用户耳朵的事实。
pub fn slot_light_for(
    running: bool,
    playing: bool,
    pending: u32,
) -> SlotLight {
    if playing {
        SlotLight::Playing
    } else if running {
        SlotLight::Running
    } else if pending > 0 {
        SlotLight::Pending
    } else {
        SlotLight::Idle
    }
}

/// 把一个状态映射成颜色。`pending` 只在 [`SlotLight::Pending`] 下被用到。
pub fn color_for(state: SlotLight, pending: u32) -> Rgb {
    match state {
        SlotLight::Running => RUNNING_COLOR,
        SlotLight::Playing => PLAYING_COLOR,
        SlotLight::Idle => IDLE_COLOR,
        // 只有待播需要按未读数调亮度。
        SlotLight::Pending => match brightness_tier(pending) {
            Some((numerator, denominator)) => PENDING_COLOR.scaled(numerator, denominator),
            None => IDLE_COLOR,
        },
    }
}

/// 未读数 → 亮度档位 `(分子, 分母)`。返回 `None` 表示"无未读"。
///
/// 档位边界：1–3 → 第 1 档；4–6 → 第 2 档；≥7 → 第 3 档（到顶饱和）。
/// 分子就是档位本身（1/3、2/3、3/3），分母固定 [`BRIGHTNESS_DENOM`]。
pub fn brightness_tier(pending: u32) -> Option<(u32, u32)> {
    if pending == 0 {
        return None;
    }
    let tier = ((pending + 2) / 3).min(BRIGHTNESS_MAX_TIER);
    Some((tier, BRIGHTNESS_DENOM))
}

/// 由快照算出 5 颗灯的颜色。**纯函数**：不碰设备、不碰全局状态。
///
/// 布局：
/// - 第 1–4 颗（index 0–3）= 槽 1–4 的状态色，四选一（执行/播放/待播/空闲）；
///   待播时亮度按未读数分三档。
/// - 第 5 颗（index 4）= **按键反馈**，不表达任何槽位状态。
///
/// `flash` **刻意不进 `RuntimeSnapshot`**：那份结构是 `Serialize + Eq` 的对外契约，
/// 而闪烁依据是 `Instant`（单调时钟，不可序列化、跨进程比较无意义）。
/// 把它塞进去会为了一个灯效去改序列化合同 —— 那是把局部需求泄漏到公共结构。
pub fn language_for(snapshot: &RuntimeSnapshot, flash: Option<&KeypressFlash>) -> [Rgb; PIXEL_COUNT] {
    let mut pixels = [IDLE_COLOR; PIXEL_COUNT];

    for slot in 0..(PIXEL_COUNT - 1) {
        // 队列视图给"是否在执行"，未读视图给"是否在播"和"未读数"——三者在不同结构里。
        // 缺任一视图时按"没有该信息"处理（false / 0），绝不让整个映射 panic。
        let running = snapshot
            .queue
            .slots
            .get(slot)
            .and_then(|s| s.running.as_ref())
            .is_some();
        let view = snapshot.unread.slots.get(slot);
        let playing = view.map(|v| v.leased).unwrap_or(false);
        let pending = view.map(|v| v.pending).unwrap_or(0);

        pixels[slot] = color_for(slot_light_for(running, playing, pending), pending);
    }

    pixels[PIXEL_COUNT - 1] = keypress_color(flash);
    pixels
}

/// 第 5 颗灯的颜色：`flash` 尚在有效期内 ⇒ 全亮，否则保持极暗。
///
/// **只判"距上次按下是否已超过 [`KEYPRESS_FLASH_MS`]"，不比较时间戳大小**：
/// `Instant::elapsed()` 在时钟异常时会返回一个**饱和的巨大值**（而不是 panic 或负数），
/// 所以这里"超过阈值 ⇒ 熄灭"的单向判断天然安全，不会出现常亮。
/// 墙钟（`SystemTime`）会因时区同步/NTP 校正跳变，用它做闪烁必然出鬼，
/// 所以 [`KeypressFlash`] 存的是 `Instant` 而不是时间戳。
fn keypress_color(flash: Option<&KeypressFlash>) -> Rgb {
    match flash {
        Some(f) => {
            if f.since.elapsed().as_millis() as u64 <= KEYPRESS_FLASH_MS {
                KEYPRESS_FLASH_COLOR
            } else {
                KEYPRESS_IDLE_COLOR
            }
        }
        None => KEYPRESS_IDLE_COLOR,
    }
}

/// 全局单调序号。`fetch_add` 保证多线程下也**不会复用**同一个值。
static SEQUENCE: AtomicU32 = AtomicU32::new(0);

/// 一次按键闪烁的记录：按下时刻。
///
/// **用单调时钟（[`Instant`]）而不是墙钟**：灯语只关心"距刚才多久"，
/// 而用户改系统时间会让墙钟跳变——那会让一次 120ms 的闪烁变成常亮或永不亮。
/// `Instant` 不受系统时间调整影响。
#[derive(Clone, Copy, Debug)]
pub struct KeypressFlash {
    pub since: std::time::Instant,
}

impl KeypressFlash {
    pub fn now() -> Self {
        Self { since: std::time::Instant::now() }
    }
}


/// 取下一个序号。从 1 开始（0 保留给"尚未发过任何帧"）。
pub fn next_sequence() -> u32 {
    SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1
}

/// 把快照算成灯语并下发给设备。
///
/// `flash` 是最近一次按键时刻；心跳重发时传 `None` 即可（第 5 颗会保持暗）——
/// 闪烁只有"按下那一瞬间"才有意义，心跳续期不需要重复表达它。
///
/// 失败**必须**把原因带回去（"失败不静默"）：调用方要么显示它、要么记日志，
/// 但不能假装灯已经变了。
pub fn push_snapshot(
    snapshot: &RuntimeSnapshot,
    flash: Option<&KeypressFlash>,
) -> Result<DeskLedFrame, String> {
    let frame = DeskLedFrame {
        sequence: next_sequence(),
        ttl_ms: DEFAULT_TTL_MS,
        active: true,
        pixels: language_for(snapshot, flash),
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
    use crate::agent_desk::queue::{
        QueueSnapshot, QueuedTask, RunningTask, SlotQueueView, TaskId,
    };
    use crate::agent_desk::unread::{SlotUnreadView, UnreadSnapshot};

    /// 构造一个可控的快照：给定各槽未读数、各槽是否在跑、播放中的槽。
    fn snapshot(
        unread: &[u32],
        running: &[bool],
        playing: Option<usize>,
    ) -> RuntimeSnapshot {
        let slot_count = 4;
        let queue_slots = (0..slot_count)
            .map(|index| SlotQueueView {
                index,
                running: running.get(index).copied().unwrap_or(false).then(|| RunningTask {
                    task: QueuedTask {
                        id: index as TaskId + 1,
                        slot: index,
                        prompt: "p".into(),
                        driver_id: "d".into(),
                        enqueued_at_ms: 0,
                    },
                    started_at_ms: 0,
                }),
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
        let running_count = running.iter().filter(|r| **r).count();
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
            // 派生值一律走同一个纯函数，**不在测试里手写档位** ——
            // 否则灯语一改，测试会拿旧档位把新实现"验证"过。
            light: LightState::from_queue(running_count, 0),
            busy: running_count > 0,
            idle: running_count == 0,
            any_unread: unread.iter().any(|n| *n > 0),
        }
    }

    /// 全空闲基准：四槽都没跑、没播、没未读。
    fn idle_snapshot() -> RuntimeSnapshot {
        snapshot(&[0; 4], &[false; 4], None)
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

    // ── 四种状态：互斥且可区分 ────────────────────────────────────────────

    #[test]
    fn four_states_map_to_four_distinct_colors() {
        let colors = [
            RUNNING_COLOR,
            PLAYING_COLOR,
            PENDING_COLOR,
            IDLE_COLOR,
        ];
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j], "状态色 {i} 与 {j} 必须不同");
            }
        }
    }

    #[test]
    fn slot_state_is_exactly_one_of_four() {
        // 穷举 2^3 组合（running / playing / pending），断言互斥优先级：
        // playing > running > pending > idle。
        assert_eq!(slot_light_for(true, true, 0), SlotLight::Playing);
        assert_eq!(slot_light_for(false, true, 0), SlotLight::Playing);
        assert_eq!(slot_light_for(true, false, 0), SlotLight::Running);
        assert_eq!(slot_light_for(true, false, 5), SlotLight::Running, "执行优先于待播");
        assert_eq!(slot_light_for(false, false, 5), SlotLight::Pending);
        assert_eq!(slot_light_for(false, false, 0), SlotLight::Idle);
        assert_eq!(slot_light_for(false, true, 5), SlotLight::Playing, "播放优先级最高");
    }

    #[test]
    fn running_slot_is_blue_and_is_not_confused_with_pending_green() {
        // 这条正对着实机踩到的坑：以前"亮着"会被读成"在执行"，
        // 现在执行与待播必须是色相上离得开的两种颜色。
        let pixels = language_for(&snapshot(&[0; 4], &[true, false, false, false], None), None);
        assert_eq!(pixels[0], RUNNING_COLOR);
        assert_ne!(RUNNING_COLOR, PENDING_COLOR);
        assert!(
            RUNNING_COLOR.b > RUNNING_COLOR.g && RUNNING_COLOR.b > RUNNING_COLOR.r,
            "执行必须是蓝主导，实际 {RUNNING_COLOR:?}"
        );
        assert!(
            PENDING_COLOR.g > PENDING_COLOR.b && PENDING_COLOR.g > PENDING_COLOR.r,
            "待播必须是绿主导，实际 {PENDING_COLOR:?}"
        );
    }

    #[test]
    fn playing_slot_is_cyan_and_distinct_from_running_blue() {
        let pixels = language_for(&snapshot(&[0; 4], &[true, false, false, false], Some(0)), None);
        assert_eq!(pixels[0], PLAYING_COLOR, "播放优先于执行");
        assert_ne!(PLAYING_COLOR, RUNNING_COLOR, "播放与执行不能同色");
        assert!(PLAYING_COLOR.g > 0 && PLAYING_COLOR.b > 0 && PLAYING_COLOR.r == 0);
    }

    #[test]
    fn idle_slot_is_fully_off() {
        let pixels = language_for(&idle_snapshot(), None);
        for (i, pixel) in pixels.iter().enumerate().take(PIXEL_COUNT - 1) {
            assert_eq!(*pixel, Rgb::OFF, "槽 {i} 空闲必须全灭");
        }
        // 语义收敛成一句话：**亮 = 有状态，灭 = 没事**。
        // 掉线另有判据（send_feature_report 报错 + led_ok 日志翻转），
        // 不再靠"全灭"这个视觉信号去表达。
        assert_eq!(IDLE_COLOR, Rgb::OFF, "空闲就是全灭，不做极暗蓝");
    }

    // ── 待播三档亮度 ──────────────────────────────────────────────────────

    #[test]
    fn brightness_tier_boundaries_are_one_to_three_four_to_six_seven_plus() {
        assert_eq!(brightness_tier(0), None);
        assert_eq!(brightness_tier(1), Some((1, 3)));
        assert_eq!(brightness_tier(3), Some((1, 3)), "3 仍是第 1 档");
        assert_eq!(brightness_tier(4), Some((2, 3)), "4 进第 2 档");
        assert_eq!(brightness_tier(6), Some((2, 3)), "6 仍是第 2 档");
        assert_eq!(brightness_tier(7), Some((3, 3)), "7 进第 3 档");
        assert_eq!(brightness_tier(9), Some((3, 3)), "到上限不再增");
        assert_eq!(brightness_tier(200), Some((3, 3)), "必须饱和不能越界");
    }

    #[test]
    fn pending_brightness_increases_monotonically_across_three_tiers() {
        let one = language_for(&snapshot(&[1, 0, 0, 0], &[false; 4], None), None)[0];
        let four = language_for(&snapshot(&[4, 0, 0, 0], &[false; 4], None), None)[0];
        let seven = language_for(&snapshot(&[7, 0, 0, 0], &[false; 4], None), None)[0];

        assert_ne!(one, four, "第 1、2 档必须不同");
        assert_ne!(four, seven, "第 2、3 档必须不同");
        assert!(one.g < four.g && four.g < seven.g, "亮度必须随档位递增");
        // 三档都必须是绿主导（缩放不改变色相）
        for (name, c) in [("第1档", one), ("第2档", four), ("第3档", seven)] {
            assert!(c.g > c.r && c.g > c.b, "{name} 必须绿主导，实际 {c:?}");
        }
    }

    #[test]
    fn pending_brightness_does_not_leak_into_other_states() {
        // 未读再多也不该改变"执行"或"播放"的颜色——亮度分档只属于待播。
        let one = language_for(&snapshot(&[1, 0, 0, 0], &[true, false, false, false], None), None)[0];
        let many = language_for(&snapshot(&[9, 0, 0, 0], &[true, false, false, false], None), None)[0];
        assert_eq!(one, many, "执行态不得受未读数影响");
        assert_eq!(one, RUNNING_COLOR);
    }

    // ── S5 按键闪烁 ───────────────────────────────────────────────────────

    #[test]
    fn fifth_pixel_is_keypress_feedback_not_a_slot_state() {
        let snap = snapshot(&[3, 3, 3, 3], &[false; 4], None);
        let no_flash = language_for(&snap, None);
        let with_flash = language_for(&snap, Some(&KeypressFlash::now()));

        // 闪烁只影响第 5 颗，前 4 颗（槽位状态）必须一模一样。
        assert_eq!(no_flash[..PIXEL_COUNT - 1], with_flash[..PIXEL_COUNT - 1]);

        let fifth = with_flash[PIXEL_COUNT - 1];
        assert_eq!(fifth, KEYPRESS_FLASH_COLOR, "按下 120ms 内必须是全亮");
        assert_ne!(fifth, no_flash[PIXEL_COUNT - 1], "闪烁与保持态必须不同");
        // S5 不得复用任何槽位语义色，否则会被误读成某个槽的状态。
        for state_color in [RUNNING_COLOR, PLAYING_COLOR, PENDING_COLOR, IDLE_COLOR] {
            assert_ne!(fifth, state_color, "S5 不能复用槽位状态色");
        }
    }

    #[test]
    fn fifth_pixel_returns_to_dim_after_flash_expires() {
        // 直接用过期时刻构造：闪了 1 秒，早过了 120ms。
        let expired = KeypressFlash { since: std::time::Instant::now() - std::time::Duration::from_secs(1) };
        let pixels = language_for(&idle_snapshot(), Some(&expired));
        assert_eq!(pixels[PIXEL_COUNT - 1], KEYPRESS_IDLE_COLOR, "超时后必须灭回去");
        assert_ne!(KEYPRESS_IDLE_COLOR, KEYPRESS_FLASH_COLOR);
    }

    #[test]
    fn fifth_pixel_without_any_keypress_is_off() {
        let pixels = language_for(&idle_snapshot(), None);
        assert_eq!(pixels[PIXEL_COUNT - 1], Rgb::OFF, "S5 静止时必须全灭");
        // 心跳重发时不传闪烁 ⇒ 第 5 颗保持灭，不会每 5 秒假闪一次。
        assert_eq!(KEYPRESS_IDLE_COLOR, Rgb::OFF);
    }

    // ── 退化输入 ───────────────────────────────────────────────────────────

    #[test]
    fn fewer_slots_than_pixels_does_not_panic_and_leaves_them_idle() {
        // 快照只有 1 个槽（协议允许的退化输入），不能 panic、也不许借用别的槽的数据。
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
                slots: vec![SlotUnreadView { index: 0, pending: 5, generation: 0, leased: false }],
            },
            light: LightState::from_queue(0, 0),
            busy: false,
            idle: true,
            any_unread: true,
        };
        let pixels = language_for(&snap, None);
        assert_ne!(pixels[0], IDLE_COLOR, "存在的槽必须按未读点亮");
        assert_eq!(pixels[0], PENDING_COLOR.scaled(2, 3), "5 条未读 = 第 2 档");
        assert_eq!(pixels[1], IDLE_COLOR, "不存在的槽必须是空闲色");
        assert_eq!(pixels[3], IDLE_COLOR);
    }

    #[test]
    fn slot_index_selects_its_own_pixel_only() {
        // 钦友实机报的第二个现象：按 S2 灯还是 S1 亮。根因是映射没读"哪个槽在跑"。
        // 这条测试把"槽位隔离"钉死：只有自己那一颗会变。
        let pixels = language_for(&snapshot(&[0; 4], &[false, true, false, false], None), None);
        assert_eq!(pixels[0], IDLE_COLOR, "槽 1 没跑必须是空闲");
        assert_eq!(pixels[1], RUNNING_COLOR, "槽 2 在跑必须是执行色");
        assert_eq!(pixels[2], IDLE_COLOR);
        assert_eq!(pixels[3], IDLE_COLOR);
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
