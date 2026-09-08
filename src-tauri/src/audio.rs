//! 麦克风录音：cpal 采集，重采样到 16k / 16bit / 单声道 PCM。
//!
//! 输出为小端 i16 字节流，直接喂给火山引擎流式 ASR。

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// 录音会话。持有底层流，`drop` 时自动停止并释放麦克风。
pub struct AudioCapture {
    _stream: cpal::Stream,
}

/// 跨回调共享状态：累积 mono 样本、输入参数、输出通道。
struct AudioState {
    pending: Vec<f32>,
    in_rate: u32,
    in_channels: u16,
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
}

impl AudioState {
    /// 追加 mono 样本，达到约 200ms 输入时重采样并送出一块 PCM。
    fn push_mono(&mut self, mono: &[f32]) {
        self.pending.extend_from_slice(mono);
        let chunk = (self.in_rate / 5) as usize; // 200ms 的样本数
        while self.pending.len() >= chunk {
            let take: Vec<f32> = self.pending.drain(..chunk).collect();
            let pcm = resample_to_16k(&take, self.in_rate);
            let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
            if self.tx.blocking_send(bytes).is_err() {
                self.pending.clear();
                break;
            }
        }
    }
}

impl AudioCapture {
    /// 开始录音，把 16k/16bit/mono 的 PCM 分块通过 `tx` 发出。
    pub fn start(tx: tokio::sync::mpsc::Sender<Vec<u8>>) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("未检测到麦克风输入设备")?;
        let supported = device.default_input_config().map_err(|e| e.to_string())?;

        let sample_format = supported.sample_format();
        let in_rate = supported.sample_rate().0;
        let in_channels = supported.channels();
        let stream_config: cpal::StreamConfig = supported.into();

        let state = Arc::new(Mutex::new(AudioState {
            pending: Vec::new(),
            in_rate,
            in_channels,
            tx,
        }));

        let stream = match sample_format {
            cpal::SampleFormat::F32 => build_f32(&device, &stream_config, state)?,
            cpal::SampleFormat::I16 => build_i16(&device, &stream_config, state)?,
            cpal::SampleFormat::U16 => build_u16(&device, &stream_config, state)?,
            other => return Err(format!("不支持的麦克风采样格式: {other:?}")),
        };

        stream.play().map_err(|e| e.to_string())?;
        Ok(Self { _stream: stream })
    }
}

fn build_f32(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    state: Arc<Mutex<AudioState>>,
) -> Result<cpal::Stream, String> {
    device
        .build_input_stream(
            config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mut st = state.lock().unwrap();
                let mut mono = Vec::with_capacity(data.len() / st.in_channels as usize);
                if st.in_channels == 1 {
                    mono.extend_from_slice(data);
                } else {
                    for ch in data.chunks(st.in_channels as usize) {
                        let sum: f32 = ch.iter().sum();
                        mono.push(sum / st.in_channels as f32);
                    }
                }
                st.push_mono(&mono);
            },
            |e| eprintln!("录音错误: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

fn build_i16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    state: Arc<Mutex<AudioState>>,
) -> Result<cpal::Stream, String> {
    device
        .build_input_stream(
            config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                let mut st = state.lock().unwrap();
                let mut mono = Vec::with_capacity(data.len() / st.in_channels as usize);
                if st.in_channels == 1 {
                    mono.extend(data.iter().map(|&s| s as f32 / 32768.0));
                } else {
                    for ch in data.chunks(st.in_channels as usize) {
                        let sum: f32 = ch.iter().map(|&s| s as f32 / 32768.0).sum();
                        mono.push(sum / st.in_channels as f32);
                    }
                }
                st.push_mono(&mono);
            },
            |e| eprintln!("录音错误: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

fn build_u16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    state: Arc<Mutex<AudioState>>,
) -> Result<cpal::Stream, String> {
    device
        .build_input_stream(
            config,
            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                let mut st = state.lock().unwrap();
                let mut mono = Vec::with_capacity(data.len() / st.in_channels as usize);
                let conv = |&s: &u16| (s as f32 - 32768.0) / 32768.0;
                if st.in_channels == 1 {
                    mono.extend(data.iter().map(conv));
                } else {
                    for ch in data.chunks(st.in_channels as usize) {
                        let sum: f32 = ch.iter().map(conv).sum();
                        mono.push(sum / st.in_channels as f32);
                    }
                }
                st.push_mono(&mono);
            },
            |e| eprintln!("录音错误: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

/// 线性插值重采样到 16k，并转 i16。
fn resample_to_16k(input: &[f32], in_rate: u32) -> Vec<i16> {
    const OUT_RATE: u32 = 16000;
    if in_rate == OUT_RATE {
        return input.iter().map(|&s| f32_to_i16(s)).collect();
    }
    let ratio = in_rate as f64 / OUT_RATE as f64;
    let out_len = (input.len() as f64 / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let idx = pos as usize;
        let frac = (pos - idx as f64) as f32;
        let s0 = input[idx];
        let s1 = if idx + 1 < input.len() {
            input[idx + 1]
        } else {
            s0
        };
        let s = s0 * (1.0 - frac) + s1 * frac;
        out.push(f32_to_i16(s));
    }
    out
}

#[inline]
fn f32_to_i16(s: f32) -> i16 {
    let s = s.clamp(-1.0, 1.0);
    (s * 32767.0) as i16
}
