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
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::ei_protocol::{
    dispatch, build_eicc, EiPacket, EICC_ACTION_START, EICC_ACTION_STOP,
    EICC_ACTION_KEEPALIVE,
};
use crate::log_buffer;

/// 默认 UDP 端口（与固件一致）。
pub const DEFAULT_PORT: u16 = 17333;

/// 心跳超时：超过此时间未收到 EIHB 则判定开发板离线。
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(5);

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

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
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

    /// 开发板是否在线（最近 5 秒内收到过 EIHB 心跳）。
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
        format!("最近一次心跳在 {ago} ms 前（超过 5 秒即判离线）")
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
}
