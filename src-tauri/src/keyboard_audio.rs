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

    /// 停止监听线程（APP 退出时调用）。
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
}

impl Drop for KeyboardAudioManager {
    fn drop(&mut self) {
        self.shutdown();
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
