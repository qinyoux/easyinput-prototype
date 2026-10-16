//! PC 侧音频输出 —— 把 PCM 播到系统默认输出设备。
//!
//! # 为什么播放在**后端**做，而不是丢给前端
//!
//! 前端当然能播（`<audio>` / Web Audio 都行），但这个选择是错的。原因不是技术偏好，
//! 而是**承诺的可靠性**：
//!
//! 「听完才消费未读」是一条**不可丢的承诺**（见 `unread.rs` 的模块文档）。
//! "播完了"就是这条承诺的判定点。把它放在 webview 里，意味着以下任意一件事
//! 都会让承诺失效：
//!
//! - 用户刷了页面 / 前端热重载；
//! - webview 渲染进程崩溃（这不是罕见事件）；
//! - 前端一个未捕获的异常让回调没跑到。
//!
//! 而在进程内播放，"播完了"是一个**本地事实**，不依赖任何外部组件的可信度。
//! 这也让板子路（固件喇叭）将来可以平替进来：上层只依赖"播完了"这个信号，
//! 不关心声音是从哪个喇叭出来的。
//!
//! # 依赖
//!
//! `cpal` 本来就在依赖树里（`audio.rs` 用它做**录音**）。这里只补上**输出**方向，
//! 因此不新增任何依赖、不新增任何下载。
//!
//! # 可中断
//!
//! 播放句柄带 [`PlaybackControl::stop`]。「播到一半被中断 ⇒ 未读保留」这条语义
//! （Phase 3.5 的核心）就靠它验证：中断后调用方**不得**消费未读。
//! 因此 [`PlaybackControl::completed_fully`] 刻意把"被中断"排除在"播完"之外。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// 建流就绪的超时。声卡被独占时 `recv` 会一直等，必须有上限。
const STREAM_READY_TIMEOUT: Duration = Duration::from_secs(5);
/// 播放线程的轮询粒度。音频本身有几十毫秒缓冲，10ms 足够且不烧 CPU。
const POLL_INTERVAL: Duration = Duration::from_millis(10);
/// 播放硬上限的安全余量（在音频时长之上）。
///
/// 设备被拔掉时声卡回调可能**再也不被调用**，靠 `finished` 等就会永远挂着。
/// 有了它，最坏情况下这个线程也一定会退出。
const PLAYBACK_HARD_LIMIT_MARGIN: Duration = Duration::from_secs(10);

/// 播放句柄：既能等它结束，也能中断它。
#[derive(Clone)]
pub struct PlaybackControl {
    stop: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    /// 已交给声卡的**样本**数（已按目标声道数展开，因此与 `total_samples` 同口径）。
    played_samples: Arc<AtomicUsize>,
    /// 总样本数（同上口径）。
    total_samples: usize,
    /// **设备侧**精确播放时长（毫秒，已含重采样与声道展开）。
    ///
    /// 调用方等"播完"时必须用它，不许自己拿 `total_samples` 除采样率估 ——
    /// 那次估算把毫秒当样本数，酿成过 v0.41.15 的"长播报被掐断"（见
    /// [`keepalive_deadline`] 的回归测试）。
    duration_ms: u64,
}

impl PlaybackControl {
    /// 请求中断。
    ///
    /// ⚠️ **中断不等于听完** —— 调用方在此之后**不得**消费未读。
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// 是否已被请求中断。
    pub fn stop_requested(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// 是否已经结束了（播完或被中断）——即"声卡不会再出声了"。
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }

    /// 是否**完整**播完。
    ///
    /// ⭐ 这是消费未读的**唯一凭据**。被中断的播放永远不算 ——
    /// 否则"播到一半拔电，未读却没了"这个 bug 就会从这里溜进去。
    pub fn completed_fully(&self) -> bool {
        self.is_finished() && !self.stop_requested()
    }

    /// 已播出的样本数。
    pub fn played_samples(&self) -> usize {
        self.played_samples.load(Ordering::Relaxed)
    }

    /// 总样本数（与 [`Self::played_samples`] 同口径）。
    pub fn total_samples(&self) -> usize {
        self.total_samples
    }

    /// **设备侧**精确播放时长（毫秒）。
    pub fn duration_ms(&self) -> u64 {
        self.duration_ms
    }

    /// 轮询等待播放结束，最多等 `timeout`。返回是否已结束。
    ///
    /// 刻意做成**轮询**而不是 join 一个线程：调用方在等待期间还要做别的判断
    /// （租约是否超时、用户是否又按了键）。
    pub fn wait_finished(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.is_finished() {
                return true;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        self.is_finished()
    }
}

/// 把源 PCM 整形成"设备能直接吃的"样本序列：重采样到设备采样率 + 复制到目标声道数。
///
/// 抽成纯函数是为了可单测 —— 真正的 `cpal` 播放需要一块声卡（测试机不一定有），
/// 而"采样率/声道数对不上"恰恰是这类代码最常见的错，必须能被自动测住。
pub fn prepare_samples(pcm: &[i16], src_rate: u32, dst_rate: u32, channels: usize) -> Vec<i16> {
    let channels = channels.max(1);
    let resampled: Vec<i16> = if src_rate == dst_rate || src_rate == 0 || dst_rate == 0 {
        pcm.to_vec()
    } else {
        crate::tts::resample_linear(pcm, src_rate, dst_rate)
    };

    if channels == 1 {
        return resampled;
    }
    // 单声道 → 多声道：每个样本复制到所有声道。
    // ⚠️ 不在这里做"立体声 → 单声道"的混音：本项目的音频源统一是单声道，
    // 支持反方向只会引入一条永远走不到的代码路径。
    let mut out = Vec::with_capacity(resampled.len() * channels);
    for sample in resampled {
        for _ in 0..channels {
            out.push(sample);
        }
    }
    out
}

/// 把 PCM 播到默认输出设备，**立即返回**一个控制句柄。
///
/// 播放本身跑在一个专用线程里：`cpal::Stream` 必须在它的生命周期内被保活
/// （一旦 drop 就停播），所以不能建完就丢。
pub fn play_pcm(pcm: Vec<i16>, sample_rate: u32) -> Result<PlaybackControl, String> {
    if pcm.is_empty() {
        return Err("没有可播放的音频样本".to_string());
    }
    if sample_rate == 0 {
        return Err("采样率为 0，无法播放".to_string());
    }

    let stop = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let played_samples = Arc::new(AtomicUsize::new(0));

    // 建流是同步的、且可能失败（没声卡 / 被独占）。用 channel 把结果——包括
    // **设备侧的真实总样本数与时长**——带回调用方。带上这两个数是为了让
    // `played_samples / total_samples` 口径一致、让调用方的等待超时有精确值，
    // 否则进度读数会撒谎、等待时长会拍脑袋。
    //
    // 「失败不静默」在这条链路上尤其重要：静默失败会让"未读怎么没被消费"
    // 变成一个查不出原因的谜。
    let (ready_tx, ready_rx) = mpsc::channel::<Result<PlaybackReady, String>>();

    {
        let stop = stop.clone();
        let finished = finished.clone();
        let played_samples = played_samples.clone();
        std::thread::Builder::new()
            .name("easyop-desk-audio".into())
            .spawn(move || {
                let (stream, ready) = match build_stream(
                    &pcm,
                    sample_rate,
                    stop.clone(),
                    finished.clone(),
                    played_samples.clone(),
                ) {
                    Ok(pair) => {
                        if let Err(e) = pair.0.play() {
                            let _ = ready_tx.send(Err(format!("启动播放失败：{e}")));
                            return;
                        }
                        // 只要开始播了，就必须让调用方知道设备侧的真实读数。
                        if ready_tx.send(Ok(pair.1)).is_err() {
                            // 调用方已经走了（超时返回），没必要继续保活。
                            return;
                        }
                        pair
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };

                // ---- 保活直到播完 / 被中断 / 触到硬上限 ----
                //
                // ⚠️ 时长必须来自**设备侧**的 `ready.duration_ms`（已含重采样与
                // 声道展开），不许拿源 PCM 现算 —— 那次现算在 v0.41.15 把毫秒
                // 当样本数，48kHz 下音频时长被缩成 1/48，超过 10 秒的播报全部
                // 在 ~10.2 秒处被这个截止时间 drop 流掐断（回归测试见
                // `keepalive_deadline_covers_the_whole_audio`）。
                let deadline = keepalive_deadline(ready.duration_ms);
                while !finished.load(Ordering::Relaxed) && Instant::now() < deadline {
                    std::thread::sleep(POLL_INTERVAL);
                }
                // 无论如何都要置 finished：否则 wait_finished 的调用方会一直等到超时。
                finished.store(true, Ordering::Relaxed);
                drop(stream);
            })
            .map_err(|e| format!("创建播放线程失败：{e}"))?;
    }

    match ready_rx.recv_timeout(STREAM_READY_TIMEOUT) {
        Ok(Ok(ready)) => Ok(PlaybackControl {
            stop,
            finished,
            played_samples,
            total_samples: ready.total_samples,
            duration_ms: ready.duration_ms,
        }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("音频设备在 5 秒内没有就绪（可能被其它程序独占）".to_string()),
    }
}

/// 建流成功时回传给调用方的**设备侧**读数。
#[derive(Clone, Copy, Debug)]
struct PlaybackReady {
    total_samples: usize,
    duration_ms: u64,
}

/// 保活截止时刻 = 音频时长 + 硬上限余量。
///
/// ⚠️ **单位必须在这里被钉死**：进来的是毫秒，出去的也必须是毫秒。
/// v0.41.15 曾把 [`total_from_pcm`] 返回的毫秒当样本数，套
/// `micros = ms * 1_000_000 / rate` ⇒ 48kHz 下时长缩到 1/48，
/// 35 秒的音频只保活 10.7 秒，播到 10.96 秒就被 drop 流掐断。
fn keepalive_deadline(duration_ms: u64) -> Instant {
    Instant::now() + Duration::from_millis(duration_ms) + PLAYBACK_HARD_LIMIT_MARGIN
}

/// 原始单声道 PCM 的时长（毫秒）。
fn total_from_pcm(pcm: &[i16], sample_rate: u32) -> u64 {
    if sample_rate == 0 {
        return 0;
    }
    (pcm.len() as u64) * 1000 / sample_rate as u64
}

/// 在当前线程建好输出流（尚未 `play()`），返回流与**设备侧读数**。
fn build_stream(
    pcm: &[i16],
    sample_rate: u32,
    stop: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    played_samples: Arc<AtomicUsize>,
) -> Result<(cpal::Stream, PlaybackReady), String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "系统没有可用的音频输出设备".to_string())?;
    let supported = device
        .default_output_config()
        .map_err(|e| format!("读取输出设备默认配置失败：{e}"))?;

    let device_rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;

    // ⚠️ **每次播放都记下设备名**，这不是啰嗦。
    //
    // "设置了却听不到声音"是这个模块最难查的故障：喇叭没有反馈通道，
    // 用户只能说"没响"。而最大的一类成因是**声音去了另一台设备**
    // （系统默认输出是耳机 / 虚拟声卡 / 会议软件抢走的虚拟设备），
    // 那种情况下代码每一步都成功，唯独物理声音不在这里。
    // 有了这行日志，"看耳机 / 看详情，一眼就分得清是软件没播还是播错了地方。
    // 另外把两侧采样率都打出来：源与设备不一致时会走重采样，
    // 速率差得多表现为"声音变调"或"只响半句"，不对比根本看不出来。
    let device_name = device
        .name()
        .unwrap_or_else(|_| "（读不到设备名）".to_string());
    crate::log_buffer::info(format!(
        "任务台音频输出：设备「{device_name}」· 设备 {device_rate}Hz · {channels} 声道；\
         源 {sample_rate}Hz · {} 样本（≈{}ms）",
        pcm.len(),
        total_from_pcm(pcm, sample_rate)
    ));
    let config: cpal::StreamConfig = supported.config();
    let format = supported.sample_format();

    let samples = Arc::new(prepare_samples(pcm, sample_rate, device_rate, channels));
    let total = samples.len();
    // 设备侧时长：样本数已按目标声道展开，先还原成"帧"再除采样率。
    // 用设备侧值而不是源 PCM 估算：重采样比例不是整数时两者有毫秒级差异，
    // 而等待超时要的是**精确值**（超一分钟的长播报，误差按秒算就不能接受）。
    let frames = total / channels.max(1);
    let duration_ms = (frames as u64) * 1000 / device_rate.max(1) as u64;
    let err_fn = |e: cpal::StreamError| {
        crate::log_buffer::warn(format!("音频输出流出错：{e}"));
    };

    // 每种采样格式需要一个类型化的闭包（cpal 的设计），所以这里是一段重复的 match。
    // 重复的只是"把 i16 归一化到目标格式"这一行，游标推进逻辑三者完全一致 ——
    // 这样"播到哪了"在任何格式下语义都相同，不会出现"i16 机器正常、f32 机器停不下来"
    // 这种只在特定声卡上复现的怪问题。
    let stream = match format {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config,
            {
                let samples = samples.clone();
                let played = played_samples.clone();
                let stop = stop.clone();
                let finished = finished.clone();
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    fill_channels(data, &samples, &played, &stop, &finished, |s| {
                        s as f32 / 32768.0
                    }, |slot| *slot = 0.0);
                }
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &config,
            {
                let samples = samples.clone();
                let played = played_samples.clone();
                let stop = stop.clone();
                let finished = finished.clone();
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    fill_channels(data, &samples, &played, &stop, &finished, |s| s, |slot| {
                        *slot = 0
                    });
                }
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_output_stream(
            &config,
            {
                let samples = samples.clone();
                let played = played_samples.clone();
                let stop = stop.clone();
                let finished = finished.clone();
                move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    fill_channels(
                        data,
                        &samples,
                        &played,
                        &stop,
                        &finished,
                        |s| (s as i32 + 32768) as u16,
                        |slot| *slot = 32768,
                    );
                }
            },
            err_fn,
            None,
        ),
        other => return Err(format!("不支持的输出采样格式：{other:?}")),
    }
    .map_err(|e| format!("创建输出流失败：{e}"))?;

    Ok((
        stream,
        PlaybackReady {
            total_samples: total,
            duration_ms,
        },
    ))
}

/// 填充一个输出缓冲：推进游标、写样本、越过末尾填静音、播完置 finished、被中断则静音。
///
/// 用泛型把"格式转换"与"静音值"抽出去，让三种采样格式共用**同一份**游标逻辑 ——
/// 这是刻意的：三份复制的状态机是这类代码最容易出现分歧的地方。
fn fill_channels<T: Copy>(
    data: &mut [T],
    samples: &[i16],
    played: &AtomicUsize,
    stop: &AtomicBool,
    finished: &AtomicBool,
    convert: impl Fn(i16) -> T,
    silence: impl Fn(&mut T),
) {
    if stop.load(Ordering::Relaxed) {
        // 被中断：立刻静音并标记结束，且**不推进游标** ——
        // 推进游标会把"播了多少"变成假账。
        for slot in data.iter_mut() {
            silence(slot);
        }
        finished.store(true, Ordering::Relaxed);
        return;
    }

    let total = samples.len();
    let base = played.load(Ordering::Relaxed);
    let take = total.saturating_sub(base).min(data.len());

    for (index, slot) in data.iter_mut().enumerate() {
        if index < take {
            *slot = convert(samples[base + index]);
        } else {
            silence(slot);
        }
    }

    let next = base + take;
    played.store(next, Ordering::Relaxed);
    if next >= total {
        finished.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个和 `fill_channels` 同口径的测试夹具。
    fn fixture() -> (Vec<i16>, AtomicUsize, AtomicBool, AtomicBool) {
        (
            vec![1i16, 2, 3, 4],
            AtomicUsize::new(0),
            AtomicBool::new(false),
            AtomicBool::new(false),
        )
    }

    fn fill_i16_test(
        data: &mut [i16],
        samples: &[i16],
        played: &AtomicUsize,
        stop: &AtomicBool,
        finished: &AtomicBool,
    ) {
        fill_channels(data, samples, played, stop, finished, |s| s, |slot| *slot = 0);
    }

    // ---------- 纯逻辑：整形 ----------

    #[test]
    fn prepare_samples_resamples_when_rates_differ() {
        // 48 kHz 的 480 个样本 → 16 kHz 应当在 160 上下。
        let pcm = vec![1000i16; 480];
        let out = prepare_samples(&pcm, 48_000, 16_000, 1);
        assert!(
            out.len() >= 150 && out.len() <= 170,
            "重采样后长度应接近 1/3，实际 {}",
            out.len()
        );
    }

    #[test]
    fn prepare_samples_keeps_mono_untouched_at_same_rate() {
        let pcm = vec![7i16, -7, 3];
        assert_eq!(prepare_samples(&pcm, 48_000, 48_000, 1), pcm);
    }

    #[test]
    fn prepare_samples_duplicates_to_every_channel() {
        let pcm = vec![5i16, -5];
        assert_eq!(
            prepare_samples(&pcm, 48_000, 48_000, 2),
            vec![5, 5, -5, -5],
            "每个样本要复制到两个声道"
        );
    }

    #[test]
    fn prepare_samples_treats_zero_channels_as_mono() {
        // 声道数 0 是设备报告的异常值，必须当成单声道而不是除零。
        let pcm = vec![1i16, 2];
        assert_eq!(prepare_samples(&pcm, 48_000, 48_000, 0), pcm);
    }

    #[test]
    fn prepare_samples_tolerates_zero_rate_without_dividing_by_zero() {
        let pcm = vec![1i16, 2, 3];
        assert_eq!(prepare_samples(&pcm, 0, 48_000, 1), pcm);
        assert_eq!(prepare_samples(&pcm, 48_000, 0, 1), pcm);
    }

    // ---------- 纯逻辑：填充与游标 ----------

    #[test]
    fn fill_marks_finished_exactly_at_the_end() {
        let (samples, played, stop, finished) = fixture();
        let mut buf = [0i16; 2];
        fill_i16_test(&mut buf, &samples, &played, &stop, &finished);
        assert_eq!(buf, [1, 2]);
        assert!(!finished.load(Ordering::Relaxed), "没播完不应标记结束");

        let mut buf2 = [0i16; 2];
        fill_i16_test(&mut buf2, &samples, &played, &stop, &finished);
        assert_eq!(buf2, [3, 4]);
        assert!(finished.load(Ordering::Relaxed), "播完最后一个样本即结束");
        assert_eq!(played.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn fill_pads_with_silence_past_the_end() {
        let samples = vec![9i16];
        let played = AtomicUsize::new(0);
        let stop = AtomicBool::new(false);
        let finished = AtomicBool::new(false);

        let mut buf = [7i16; 4];
        fill_i16_test(&mut buf, &samples, &played, &stop, &finished);
        assert_eq!(buf, [9, 0, 0, 0], "超出音频长度的部分必须是静音");
        assert!(finished.load(Ordering::Relaxed));
    }

    #[test]
    fn fill_stop_silences_and_finishes_without_advancing() {
        let samples = vec![9i16; 100];
        let played = AtomicUsize::new(0);
        let stop = AtomicBool::new(true); // 已被请求中断
        let finished = AtomicBool::new(false);

        let mut buf = [7i16; 4];
        fill_i16_test(&mut buf, &samples, &played, &stop, &finished);
        assert_eq!(buf, [0, 0, 0, 0], "中断后必须立刻静音");
        assert!(finished.load(Ordering::Relaxed));
        assert_eq!(
            played.load(Ordering::Relaxed),
            0,
            "中断时不得推进游标 —— 否则'播了多少'变成假账"
        );
    }

    #[test]
    fn f32_conversion_maps_full_scale_correctly() {
        let samples = vec![0i16, 32767, -32768];
        let played = AtomicUsize::new(0);
        let stop = AtomicBool::new(false);
        let finished = AtomicBool::new(false);

        let mut buf = [0f32; 3];
        fill_channels(
            &mut buf,
            &samples,
            &played,
            &stop,
            &finished,
            |s| s as f32 / 32768.0,
            |slot| *slot = 0.0,
        );
        assert!((buf[0] - 0.0).abs() < 1e-9);
        assert!(buf[1] > 0.99 && buf[1] <= 1.0);
        assert!((buf[2] + 1.0).abs() < 1e-9, "i16 最小值应映射到 -1.0");
    }

    #[test]
    fn u16_conversion_centres_on_the_midpoint() {
        let samples = vec![0i16, -32768, 32767];
        let played = AtomicUsize::new(0);
        let stop = AtomicBool::new(false);
        let finished = AtomicBool::new(false);

        let mut buf = [0u16; 3];
        fill_channels(
            &mut buf,
            &samples,
            &played,
            &stop,
            &finished,
            |s| (s as i32 + 32768) as u16,
            |slot| *slot = 32768,
        );
        assert_eq!(buf[0], 32768, "零样本对应 u16 中点");
        assert_eq!(buf[1], 0, "i16 最小值对应 u16 最小");
        assert_eq!(buf[2], 65535, "i16 最大值对应 u16 最大");
    }

    // ---------- 句柄语义 ----------

    #[test]
    fn play_rejects_empty_or_invalid_input() {
        assert!(play_pcm(Vec::new(), 48_000).is_err());
        assert!(play_pcm(vec![0i16; 10], 0).is_err());
    }

    #[test]
    fn completion_requires_not_being_stopped() {
        let make = |stop: bool, finished: bool| PlaybackControl {
            stop: Arc::new(AtomicBool::new(stop)),
            finished: Arc::new(AtomicBool::new(finished)),
            played_samples: Arc::new(AtomicUsize::new(10)),
            total_samples: 100,
            duration_ms: 100,
        };

        assert!(make(false, true).completed_fully());
        assert!(!make(false, false).completed_fully(), "还在播不算播完");
        assert!(
            !make(true, true).completed_fully(),
            "被中断的播放绝不算完整播完 —— 消费未读只能凭完整播完"
        );
    }

    #[test]
    fn stop_is_idempotent_and_observable() {
        let make = || PlaybackControl {
            stop: Arc::new(AtomicBool::new(false)),
            finished: Arc::new(AtomicBool::new(false)),
            played_samples: Arc::new(AtomicUsize::new(0)),
            total_samples: 100,
            duration_ms: 100,
        };
        let control = make();
        assert!(!control.stop_requested());
        control.stop();
        control.stop();
        assert!(control.stop_requested());
    }

    #[test]
    fn wait_finished_returns_immediately_when_already_done() {
        let control = PlaybackControl {
            stop: Arc::new(AtomicBool::new(false)),
            finished: Arc::new(AtomicBool::new(true)),
            played_samples: Arc::new(AtomicUsize::new(0)),
            total_samples: 0,
            duration_ms: 0,
        };
        assert!(control.wait_finished(Duration::from_millis(50)));
    }

    #[test]
    fn wait_finished_times_out_when_still_playing() {
        let control = PlaybackControl {
            stop: Arc::new(AtomicBool::new(false)),
            finished: Arc::new(AtomicBool::new(false)),
            played_samples: Arc::new(AtomicUsize::new(0)),
            total_samples: 100,
            duration_ms: 100,
        };
        let started = Instant::now();
        assert!(!control.wait_finished(Duration::from_millis(30)));
        assert!(
            started.elapsed() >= Duration::from_millis(25),
            "应当真的等满超时，而不是立刻返回"
        );
    }

    #[test]
    fn keepalive_deadline_covers_the_whole_audio() {
        // 回归 v0.41.15：保活截止把毫秒当样本数，48kHz 下时长缩 1/48。
        // 实测伤害：13.2s 的播报播到 10.5s 断（78%），35s 的播报播到 10.96s 断（31%）。
        // 现在：35 秒音频的保活期必须 > 35 秒（外加 10 秒硬上限余量）。
        let deadline = keepalive_deadline(35_000);
        assert!(
            deadline > Instant::now() + Duration::from_millis(35_000),
            "保活期必须长于音频本身，否则长播报会被 drop 流掐断"
        );
        // 0 时长也不能触发"立即过期"那种反向错误（余量仍在）。
        assert!(keepalive_deadline(0) > Instant::now());
    }

    #[test]
    fn total_from_pcm_computes_duration() {
        // 48000 个样本 @48kHz = 1000 毫秒。
        assert_eq!(total_from_pcm(&vec![0i16; 48_000], 48_000), 1000);
        assert_eq!(total_from_pcm(&vec![0i16; 24_000], 48_000), 500);
        assert_eq!(total_from_pcm(&[], 48_000), 0);
        assert_eq!(total_from_pcm(&vec![0i16; 10], 0), 0, "采样率为 0 不得除零");
    }
}
