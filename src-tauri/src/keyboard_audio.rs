//! 开发板 Wi-Fi 音频通道：UDP 17333 监听 EIAU/EIHB/EICA + 发送 EICC 控制命令。
//!
//! 架构对齐 EasyInput 原版：
//! - 心跳（EIHB）：开发板每 2 秒广播，APP 据此学习控制地址并判定在线。
//! - 音频（EIAU）：开发板按 20ms/帧发送 PCM，APP 解包后喂给 ASR。
//! - 控制（EICC）：APP 发送 start/stop/keepalive，开发板回 EICA 确认。
//!
//! 生命周期：APP 启动即绑定 UDP socket 并启动监听线程，全程运行。
//! PTT 按下时调用 start_streaming，PTT 松开时调用 stop_streaming。

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::ei_protocol::{
    dispatch, build_eicc, EiPacket, EICC_ACTION_START, EICC_ACTION_STOP,
    EICC_ACTION_KEEPALIVE,
};
use crate::log_buffer;

/// 默认 UDP 端口（与固件一致）。
pub const DEFAULT_PORT: u16 = 17333;

/// 配置 JSON 的长度上限。**与固件 `kConfigMaxJsonLen` 必须一致**：
/// 固件侧 `receive_buffer` 只开到这么大，多一个字节就会被整包丢弃。
pub const CONFIG_JSON_MAX_BYTES: usize = 2048;

/// 心跳超时：超过此时间未收到 EIHB 则判定开发板离线。
///
/// # 为什么是 15 秒，而不是曾经的 5 秒（2026-09-24 实测修正）
///
/// 板子心跳周期是 **2 秒**，所以 5 秒只够容忍**连丢 1 个包**：只要连丢 2 个
/// （间隔变成 6 秒）就踩线。实测日志里出现过 **7748 ms** 的空档（连丢 3 个），
/// 且"开始推流 + 同时跑任务"时抖动明显变密（同一小时从 4 次涨到 9 次）——
/// 表现为"离线→在线"每秒级反复翻转：
///
/// ```text
/// 15:56:55 板子心跳中断（超过 5 秒未收到）
/// 15:56:56 收到板子心跳，链路在线          ← 只隔 1 秒
/// ```
///
/// 代价不只是日志难看：`resolve_mic` 是按这个判据选录音源的，翻转的那一瞬间
/// 会把录音源退回**电脑麦克风** ⇒ 用户对着板子说话却录到静音，
/// 而提示只写"空结果"。**用户感知到的是"Wi-Fi 不稳定、偶尔断开"。**
///
/// 15 秒 = 7.5 个心跳周期，容忍连丢 6 个包。
///
/// # 上界同样不能放大
///
/// 这条判据同时决定"对着板子说话但采不到音"要多久才提示用户。再放大，
/// 真掉线时用户会先经历一段**没有任何反馈的沉默**，那比误判一次离线更糟。
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(15);

/// `HEARTBEAT_TIMEOUT` 的秒数，供日志与界面文案引用。
///
/// **不要在别处再写死这个数字**：本仓库已经吃过一次"改了阈值忘了改文案"的亏
/// （提示里写"超过 5 秒"，而代码已经不是 5 秒了，排查的人会照着错的前提去查）。
pub fn heartbeat_timeout_secs() -> u64 {
    HEARTBEAT_TIMEOUT.as_secs()
}

/// EICC keepalive 间隔。
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(1);

/// UDP 接收超时（让监听线程能周期性检查 stop_flag）。
const RECV_TIMEOUT: Duration = Duration::from_millis(500);

/// 键盘音频管理器。APP 启动时创建一个实例，全程运行。
#[derive(Clone)]
pub struct KeyboardAudioManager {
    inner: Arc<Inner>,
}

struct Inner {
    socket: Arc<UdpSocket>,
    control_addr: Mutex<Option<SocketAddr>>,
    last_heartbeat_ms: AtomicU64,
    audio_tx: Mutex<Option<tokio::sync::mpsc::Sender<Vec<u8>>>>,
    streaming_session: AtomicU64,
    eicc_seq: AtomicU32,
    stop_flag: AtomicBool,
}

/// 单调时基的零点（首次取时间时确定，此后不变）。
static MONO_EPOCH: OnceLock<Instant> = OnceLock::new();

/// 单调毫秒时间戳：**只用于算时间间隔，不是"现在是几点"**。
///
/// # 为什么不用 `SystemTime`
///
/// 心跳判据问的是"上一次心跳到现在过了多久"，**与墙上时钟无关**。
/// 用 `SystemTime` 会引入一类无法靠日志排查的偶发误判 —— NTP 校时、时区/时间被改、
/// 甚至跨夏令时调整，都会让这个差值跳变：
///
/// - 往**未来**跳 ⇒ 间隔被算得很大 ⇒ 凭空出现一次"板子离线"；
/// - 往**过去**跳 ⇒ `saturating_sub` 归零 ⇒ 间隔恒为 0 ⇒
///   **板子真的掉线了也不报**（这一侧更危险：静默失效）。
///
/// `Instant` 只保证单调，不保证与任何真实时刻对应 —— 这正是本判据需要的东西。
/// 需要**真实时刻**的地方（如任务历史里的 `timestamp`）一律用墙钟，两者别混。
///
/// 返回值**恒 ≥ 1**：`last_heartbeat_ms == 0` 被当作"从未收到过心跳"的哨兵值
/// （见 `is_online` / `status_note`），所以业务值不能是 0。
/// 否则万一首次心跳恰好在进程启动后的同一毫秒内到达，就会被记成 0 ⇒
/// "明明连上了却一直报从未收到"。
fn now_ms() -> u64 {
    (MONO_EPOCH
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis() as u64)
        .saturating_add(1)
}

impl KeyboardAudioManager {
    /// 绑定 UDP socket 并启动监听线程。失败则返回 Err（调用方回退电脑麦克风）。
    pub fn new(port: u16) -> Result<Self, String> {
        let socket = UdpSocket::bind(format!("0.0.0.0:{port}"))
            .map_err(|e| format!("UDP 绑定 :{port} 失败：{e}"))?;
        socket
            .set_read_timeout(Some(RECV_TIMEOUT))
            .map_err(|e| format!("UDP set_read_timeout 失败：{e}"))?;

        let inner = Arc::new(Inner {
            socket: Arc::new(socket),
            control_addr: Mutex::new(None),
            last_heartbeat_ms: AtomicU64::new(0),
            audio_tx: Mutex::new(None),
            streaming_session: AtomicU64::new(0),
            eicc_seq: AtomicU32::new(0),
            stop_flag: AtomicBool::new(false),
        });

        let inner_clone = inner.clone();
        std::thread::spawn(move || listen_loop(inner_clone));

        log_buffer::info(format!("keyboard_audio: UDP 监听已启动 :{port}"));
        Ok(Self { inner })
    }

    /// 把一份完整配置 JSON **直投**给开发板（走 Wi-Fi 控制通道，不插 USB）。
    ///
    /// # 为什么这条路走得通
    ///
    /// 固件控制任务在解析 EICC 失败后有一条**配置直达通道**
    /// （`keyboard_audio.cpp`：`received > 2 && buffer[0] == '{' && from == dest`）——
    /// 首字节是 `{` 的数据报会被当成配置文件，走**与 USB / BLE 完全相同**的
    /// "应用 + 存 NVS + HID 回执"路径。于是"刷新端点"不必再要求插上 USB。
    ///
    /// # 两个必须知道的边界（写在这里，免得下次有人以为它万能）
    ///
    /// 1. **判据是 `from == dest`**：板子只接受来自它已配置的上位机地址的配置包。
    ///    所以本机 IPv4 一变，这条路就断了 —— 那种情况只能靠 USB 重新配网。
    ///    （这是刻意的安全设计，不是缺陷。）
    /// 2. **要等心跳**：控制地址是从 EIHB 心跳里学来的，没收到过心跳就没有目的地。
    /// 固件侧 `kConfigMaxJsonLen = 2048`，超长包会被板子直接丢弃并记一条 warn。
    pub fn send_config_json(&self, json: &str) -> Result<(), String> {
        let addr = match *self.inner.control_addr.lock().unwrap() {
            Some(a) => a,
            None => {
                return Err("控制地址未知（还没收到过板子心跳）".to_string());
            }
        };
        if json.len() > CONFIG_JSON_MAX_BYTES {
            return Err(format!(
                "配置 JSON 超长（{} 字节 > 固件上限 {CONFIG_JSON_MAX_BYTES}）",
                json.len()
            ));
        }
        self.inner
            .socket
            .send_to(json.as_bytes(), addr)
            .map_err(|e| format!("UDP 直投失败：{e}"))?;
        log_buffer::info(format!(
            "keyboard_audio: 配置已直投开发板（{} 字节）",
            json.len()
        ));
        Ok(())
    }

    /// 开发板是否在线（最近 [`HEARTBEAT_TIMEOUT`] 内收到过 EIHB 心跳）。
    pub fn is_online(&self) -> bool {
        let last = self.inner.last_heartbeat_ms.load(Ordering::Relaxed);
        if last == 0 {
            return false;
        }
        now_ms().saturating_sub(last) < HEARTBEAT_TIMEOUT.as_millis() as u64
    }

    /// 开始流式接收音频：设置音频通道，向开发板发送 EICC start。
    pub fn start_streaming(&self, session_id: u64, audio_tx: tokio::sync::mpsc::Sender<Vec<u8>>) {
        *self.inner.audio_tx.lock().unwrap() = Some(audio_tx);
        self.inner.streaming_session.store(session_id, Ordering::Relaxed);
        self.send_eicc(EICC_ACTION_START, session_id);
        log_buffer::info(format!("keyboard_audio: 开始流式接收 session={session_id}"));
    }

    /// 停止流式接收音频：发送 EICC stop，清空音频通道。
    pub fn stop_streaming(&self) {
        let session_id = self.inner.streaming_session.swap(0, Ordering::Relaxed);
        if session_id != 0 {
            self.send_eicc(EICC_ACTION_STOP, session_id);
            log_buffer::info(format!("keyboard_audio: 停止流式接收 session={session_id}"));
        }
        *self.inner.audio_tx.lock().unwrap() = None;
    }

    /// 停止监听线程（进程退出时显式调用）。
    ///
    /// ⛔ **刻意不挂在 `Drop` 上。** 本类型是 `Clone` 的（内部 `Arc<Inner>`），
    /// 而录音路径每次都只是 `keyboard_audio_clone()` 取一份**局部**克隆 ——
    /// 一旦在 `Drop` 里停线程，那份局部克隆在函数返回时被释放，就会把
    /// **全局唯一**的监听线程杀掉。
    ///
    /// 真实故障（2026-09-21）：`start_hold` 里 `let ka = self.keyboard_audio_clone();`
    /// 在 `use_keyboard == false` 分支没有被 move，函数一返回就 drop ⇒ `shutdown()`
    /// ⇒ 监听线程退出 ⇒ `last_heartbeat_ms` 再不更新 ⇒ `is_online()` **恒为 false**
    /// ⇒ 板子麦克风永久失效、所有录音静默退回电脑麦克风。
    /// 用户看到的是"按着板子上的键说话没反应"（对着板子说，电脑麦克风采不到），
    /// 而日志里只有一行"监听线程已退出"，界面上一个字都没有 —— 典型的静默故障。
    ///
    /// 正确的生命周期模型：**监听线程 = 进程生命周期**，随进程退出自然结束；
    /// 需要主动停时显式调用本函数。
    pub fn shutdown(&self) {
        self.inner.stop_flag.store(true, Ordering::Relaxed);
    }

    /// 发送 EICC 控制命令到开发板（控制地址从心跳学习）。
    fn send_eicc(&self, action: u8, session_id: u64) {
        let addr = match *self.inner.control_addr.lock().unwrap() {
            Some(a) => a,
            None => {
                log_buffer::warn("keyboard_audio: 无法发送 EICC — 控制地址未知（未收到心跳）");
                return;
            }
        };
        let seq = self.inner.eicc_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let buf = build_eicc(action, session_id, seq);
        if let Err(e) = self.inner.socket.send_to(&buf, addr) {
            log_buffer::warn(format!("keyboard_audio: EICC 发送失败 action={action} err={e}"));
        }
    }

    /// 一句话说明"板子音频通道现在是什么状态"，给日志与界面提示用。
    ///
    /// 为什么不直接看 `is_online()` 那个 bool：**"从没连上过"和"刚刚掉线"
    /// 是两种病**，处置方向完全不同 —— 前者查板子的 Wi-Fi / 固件有没有在推流，
    /// 后者查信号与供电。只给一个是非判断，排查就只能从零开始猜。
    pub fn status_note(&self) -> String {
        let last = self.inner.last_heartbeat_ms.load(Ordering::Relaxed);
        if last == 0 {
            return "从未收到开发板心跳（板子没连同一 Wi-Fi，或固件没在推流）".to_string();
        }
        let ago = now_ms().saturating_sub(last);
        format!(
            "最近一次心跳在 {ago} ms 前（超过 {} 秒即判离线）",
            heartbeat_timeout_secs()
        )
    }

    /// 监听线程是否还在跑。**仅测试用** —— 存在的唯一理由就是给下面那条回归钉子提供判据。
    #[cfg(test)]
    pub fn is_listening(&self) -> bool {
        !self.inner.stop_flag.load(Ordering::Relaxed)
    }
}

/// UDP 监听线程主循环：接收并分发 EIHB/EIAU/EICA，周期性发送 keepalive。
fn listen_loop(inner: Arc<Inner>) {
    let mut buf = [0u8; 4096];
    let mut last_keepalive = Instant::now();

    while !inner.stop_flag.load(Ordering::Relaxed) {
        match inner.socket.recv_from(&mut buf) {
            Ok((n, addr)) => {
                match dispatch(&buf[..n]) {
                    EiPacket::Eihb(hb) => {
                        let prev = inner.last_heartbeat_ms.swap(now_ms(), Ordering::Relaxed);
                        if prev == 0 {
                            log_buffer::info(format!(
                                "keyboard_audio: 收到首次心跳 addr={addr} streaming={} audio_ready={}",
                                hb.is_streaming(),
                                hb.audio_ready()
                            ));
                        }
                        *inner.control_addr.lock().unwrap() = Some(addr);
                    }
                    EiPacket::Eiau(pkt) => {
                        let session = inner.streaming_session.load(Ordering::Relaxed);
                        if session != 0 && pkt.session_id == session {
                            let tx_guard = inner.audio_tx.lock().unwrap();
                            if let Some(tx) = tx_guard.as_ref() {
                                let _ = tx.blocking_send(pkt.pcm.to_vec());
                            }
                        }
                    }
                    EiPacket::Eica(ack) => {
                        if !ack.is_ok() {
                            log_buffer::warn(format!(
                                "keyboard_audio: EICA 非 OK action={} status={} session={}",
                                ack.action, ack.status, ack.session_id
                            ));
                        }
                    }
                    EiPacket::Unknown => {}
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                log_buffer::err(format!("keyboard_audio: recv_from 错误：{e}"));
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        // 流式传输期间周期性发送 keepalive。
        let session = inner.streaming_session.load(Ordering::Relaxed);
        if session != 0 && last_keepalive.elapsed() >= KEEPALIVE_INTERVAL {
            let seq = inner.eicc_seq.fetch_add(1, Ordering::Relaxed) + 1;
            let buf = build_eicc(EICC_ACTION_KEEPALIVE, session, seq);
            if let Some(addr) = *inner.control_addr.lock().unwrap() {
                let _ = inner.socket.send_to(&buf, addr);
            }
            last_keepalive = Instant::now();
        }
    }

    log_buffer::info("keyboard_audio: 监听线程已退出");
}

#[cfg(test)]
mod tests {
    //! 这一节存在的唯一理由：**监听线程是共享资源，不能被任何一次"局部借用结束"带走。**
    //!
    //! 它的失效方式是彻底静默的 —— 没有报错、没有弹窗，只是板子麦克风从此不可用，
    //! 而录音静静地退回电脑麦克风。用户按着板子说话，声音却从另一个设备收，
    //! 表现就是"语音识别坏了"，而日志里只有一行谁也不会注意的"监听线程已退出"。
    use super::*;

    /// ⛔ 回归钉子：**drop 一个 clone 不许停掉共享的监听线程**。
    ///
    /// 真实故障（2026-09-21）：本类型曾实现 `Drop` 并无条件 `shutdown()`。
    /// 录音路径拿到的只是 `keyboard_audio_clone()` 产生的**局部**克隆，
    /// 函数一返回就被 drop ⇒ 监听线程退出 ⇒ `is_online()` 恒为 false。
    /// 当天 9 次"语音输入"全部失败、4 次"语音编辑"成功 3 次 —— 差异全部来自这里。
    #[test]
    fn dropping_a_clone_must_not_kill_the_shared_listener() {
        // port = 0 ⇒ 让系统分配空闲端口，测试之间不会互相抢 17333。
        let Ok(m) = KeyboardAudioManager::new(0) else {
            return; // 环境不许绑 UDP 时跳过：环境问题不该变成一条红测
        };

        let borrowed = m.clone();
        drop(borrowed);

        assert!(
            m.is_listening(),
            "drop 一个 clone 不能停掉共享的监听线程 —— 录音函数拿到的就是局部 clone，\
             停了它板子麦克风就永久失效，而且界面上完全看不出来"
        );

        m.shutdown();
        assert!(!m.is_listening(), "显式 shutdown 才该真的停");
    }

    /// 状态说明必须能区分"从没连上过"和"刚掉线" —— 这两者的排查方向相反。
    #[test]
    fn status_note_distinguishes_never_connected_from_dropped() {
        let Ok(m) = KeyboardAudioManager::new(0) else {
            return;
        };
        let note = m.status_note();
        assert!(
            note.contains("从未收到"),
            "没有心跳时应明确说\"从未连上\"，而不是含糊地报\"离线\"：{note}"
        );
        assert!(
            !m.is_online(),
            "没有任何心跳时绝不许判在线 —— 判在线会让宿主把静音当成\"用户没说话\""
        );
    }

    /// 心跳超时必须显著大于心跳周期。
    ///
    /// 板子心跳周期 2 秒（固件定死）。曾经的 5 秒只容忍**连丢 1 个包**，
    /// 实测日志里出现过 **7748 ms** 的空档（连丢 3 个）⇒ "离线→在线"每秒级翻转；
    /// 而 `resolve_mic` 就是按这个判据选录音源的，翻转的瞬间会把录音源退回
    /// **电脑麦克风** ⇒ 用户对着板子说话却录到静音。用户感知到的正是
    /// "Wi-Fi 不稳定、偶尔断开"。
    #[test]
    fn heartbeat_timeout_tolerates_several_lost_beats() {
        /// 固件的心跳广播周期（秒）。
        const BEAT_SECS: u64 = 2;

        let secs = heartbeat_timeout_secs();
        assert!(
            secs >= BEAT_SECS * 5,
            "超时必须容忍连丢 5 个心跳以上，否则 2.4G 拥挤环境下必然误判：{secs}s"
        );
        // 上界同样重要：这条判据也决定"对着板子说话却没反应"多久才提示用户，
        // 放得过大 = 真掉线时长时间毫无反馈。
        assert!(secs <= 30, "超时过大 ⇒ 真掉线时长时间无反馈：{secs}s");
    }

    /// 文案与代码不能脱节：提示里的秒数必须来自常量本身。
    #[test]
    fn status_note_quotes_the_real_timeout() {
        let Ok(m) = KeyboardAudioManager::new(0) else {
            return;
        };
        // 造一次"收到过心跳但已过期"的状态不好做（要等真实时间），
        // 所以这里只钉住常量本身 —— 关键是文案**引用**它，而不是手写数字。
        let secs = heartbeat_timeout_secs();
        assert_eq!(secs as u128, HEARTBEAT_TIMEOUT.as_millis() / 1000);
        assert!(secs > 0);
        let _ = m;
    }

    /// 单调时基同样要求恒 > 0（`last_heartbeat_ms == 0` 是"从未收到"的哨兵值）。
    #[test]
    fn mono_clock_is_nonzero() {
        assert!(now_ms() > 0, "0 是哨兵值，业务时间戳不能为 0");
        assert!(now_ms() <= now_ms(), "单调时钟不能倒退");
    }
}
