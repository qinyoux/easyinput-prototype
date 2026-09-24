//! 火山引擎「豆包流式语音识别」WebSocket 客户端。
//!
//! 端点：wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async
//! 鉴权：新版控制台，请求头 `X-Api-Key` + `X-Api-Resource-Id` 等。
//!
//! 二进制协议（每一帧）：
//!   [header 4B][sequence 4B 可选][payload_size 4B 大端][payload]
//! header：
//!   byte0 = 0x11（protocol_version=1, header_size=1）
//!   byte1 = (message_type << 4) | flags
//!   byte2 = (serialization << 4) | compression
//!   byte3 = 0x00
//! flags：bit0 = 带 sequence；bit1 = 最后一包（负包）
//!
//! 参考：https://www.volcengine.com/docs/6561/1354869

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use http::HeaderValue;
use serde_json::Value;
use tauri::Emitter;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

// 消息类型
const MT_FULL_CLIENT_REQUEST: u8 = 0b0001;
const MT_AUDIO_ONLY_REQUEST: u8 = 0b0010;
const MT_FULL_SERVER_RESPONSE: u8 = 0b1001;
const MT_ERROR: u8 = 0b1111;

// flags
const FLAG_HAS_SEQUENCE: u8 = 0b0001;
const FLAG_LAST_PACKAGE: u8 = 0b0010;

// 序列化方式
const SER_RAW: u8 = 0b0000;
const SER_JSON: u8 = 0b0001;

/// 构造一帧二进制数据（不压缩）。
fn build_frame(
    message_type: u8,
    flags: u8,
    serialization: u8,
    sequence: Option<i32>,
    payload: &[u8],
) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + payload.len());
    v.push(0x11); // version=1, header_size=1
    v.push((message_type << 4) | flags);
    v.push(serialization << 4); // compression = NONE
    v.push(0x00);
    if let Some(seq) = sequence {
        v.extend_from_slice(&seq.to_be_bytes());
    }
    v.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    v.extend_from_slice(payload);
    v
}

/// 构造「音频结束」负包（最后一包）。
///
/// # 为什么单独抽成一个函数
///
/// 负包的 `sequence` 写法是**整条链路里最隐蔽的一处协议细节**：写错了不会当场失败 ——
/// 音频照样被逐字识别、文本照样一帧帧回来，只在**收尾那一刻**服务端回一个错误帧；
/// 而收尾错误会让整段识别结果作废。用户看到的现象是「说得清清楚楚，一个字都没上屏」。
///
/// 本机实测（2026-09-21，火山 `sauc/bigmodel_async`）四种写法的服务端反应：
///
/// | 负包写法 | 服务端 |
/// |---|---|
/// | `sequence = -1`（**旧写法，就是这次故障的原因**） | ❌ `autoAssignedSequence (-N) mismatch sequence in request (-1)` |
/// | `sequence = -(下一序号)` | ✅ 通过 |
/// | **不带 sequence，仅 LAST 标志** | ✅ 通过（本次采用） |
/// | `sequence = 下一序号`（正数） | ❌ 同样 mismatch |
///
/// 取"不带 sequence"这一种：它不依赖"我数得准这是第几帧"。`-(下一序号)` 也对，
/// 但一旦某帧发送失败、或将来有人在循环里加一次发送，序号就会错位 ——
/// 而这个错位只在收尾时炸，排查成本极高。**能不依赖计数就不依赖。**
///
/// 抽成函数 + 单测，是为了下次有人"顺手把 sequence 补回去"时会被绊住。
fn final_frame() -> Vec<u8> {
    build_frame(
        MT_AUDIO_ONLY_REQUEST,
        FLAG_LAST_PACKAGE, // 只有 LAST，**不带** HAS_SEQUENCE
        SER_RAW,
        None,
        &[],
    )
}

/// 首包（full client request）的 JSON 参数。
fn full_client_request_json() -> String {
    // pcm_s16le / 16k / mono；enable_ddc 顺滑去掉语气词，更接近 easyop 的「收拾」效果。
    let mut req = serde_json::json!({
        "audio": {
            "format": "pcm",
            "codec": "raw",
            "rate": 16000,
            "bits": 16,
            "channel": 1
        },
        "request": {
            "model_name": "bigmodel",
            "enable_itn": true,
            "enable_punc": true,
            "enable_ddc": true,
            "result_type": "full",
            "language": "zh-CN"
        }
    });
    inject_hotwords(&mut req);
    req.to_string()
}

/// 把 ASR 词库热词注入请求 JSON 的 `request.hotwords` 字段。
/// 火山引擎 bigmodel ASR 接受 inline 热词列表，格式：
/// `[{"text":"热词","weight":10}, ...]`，权重越高越优先匹配。
fn inject_hotwords(req: &mut Value) {
    let words = crate::asr_vocab::words();
    if words.is_empty() {
        return;
    }
    let hotwords: Vec<Value> = words
        .iter()
        .map(|w| serde_json::json!({ "text": w.text, "weight": w.weight }))
        .collect();
    if let Some(request) = req.get_mut("request") {
        request.as_object_mut().unwrap().insert("hotwords".into(), Value::Array(hotwords));
    }
}

/// 从一帧服务端二进制数据里解析出（累积文本, 是否最后一包）。
fn parse_server_frame(data: &[u8]) -> Option<(String, bool)> {
    if data.len() < 8 {
        return None;
    }
    let header_type = data[1] >> 4;
    let flags = data[1] & 0x0F;
    let compression = data[2] & 0x0F;

    let mut offset = 4usize;
    let mut sequence: i32 = 0;
    if flags & FLAG_HAS_SEQUENCE != 0 {
        if data.len() < offset + 4 {
            return None;
        }
        sequence = i32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
    }
    if data.len() < offset + 4 {
        return None;
    }
    let payload_size = u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]) as usize;
    offset += 4;
    if data.len() < offset + payload_size {
        return None;
    }
    let payload = &data[offset..offset + payload_size];

    let is_last = (flags & FLAG_LAST_PACKAGE) != 0 || sequence < 0;

    match header_type {
        MT_FULL_SERVER_RESPONSE => {
            // 客户端未开启压缩，服务端回包亦不压缩；这里仅防御性处理 gzip。
            let text = if compression == 0x01 {
                gzip_decompress(payload).and_then(|bytes| extract_text(&bytes))
            } else {
                extract_text(payload)
            };
            text.map(|t| (t, is_last))
        }
        MT_ERROR => {
            eprintln!("ASR 服务返回错误: {}", String::from_utf8_lossy(payload));
            None
        }
        _ => None,
    }
}

/// 简单 gzip 解压（防御性，正常不触发）。
fn gzip_decompress(data: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut decoder = flate2::read::GzDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).ok()?;
    Some(out)
}

/// 从服务端帧提取错误文本（仅 MT_ERROR 类型返回 Some）。
/// 用于把语音服务的错误（鉴权失败、配额超限等）上报给前端，避免被静默吞掉。
fn extract_asr_error(data: &[u8]) -> Option<String> {
    if data.len() < 8 {
        return None;
    }
    let header_type = data[1] >> 4;
    if header_type != MT_ERROR {
        return None;
    }
    let flags = data[1] & 0x0F;
    let mut offset = 4usize;
    if flags & FLAG_HAS_SEQUENCE != 0 {
        if data.len() < offset + 4 {
            return None;
        }
        offset += 4;
    }
    if data.len() < offset + 4 {
        return None;
    }
    let payload_size = u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]) as usize;
    offset += 4;
    if data.len() < offset + payload_size {
        return None;
    }
    let payload = &data[offset..offset + payload_size];
    Some(format!(
        "语音服务返回错误: {}",
        String::from_utf8_lossy(payload)
    ))
}

/// 从返回 JSON 里提取累积文本，兼容 `result.text` 与 `payload_msg.result.text`。
fn extract_text(json: &[u8]) -> Option<String> {
    let v: Value = serde_json::from_slice(json).ok()?;
    if let Some(t) = v.pointer("/payload_msg/result/text").and_then(|x| x.as_str()) {
        return Some(t.to_string());
    }
    if let Some(t) = v.pointer("/result/text").and_then(|x| x.as_str()) {
        return Some(t.to_string());
    }
    None
}

// ---------------------------------------------------------------------------
// 日志辅助：日志要"能看懂"，前提是每条都短
// ---------------------------------------------------------------------------

/// 文本预览：截断 + 去换行。
///
/// 服务端回的是完整句子、还可能含换行；原样写进日志会把一条记录撑成十几行，
/// 真正要看的"第几包 / last / 有没有内容"反而被淹掉。
fn preview(s: &str) -> String {
    const MAX: usize = 120;
    let flat = s.replace(['\r', '\n'], " ");
    if flat.chars().count() <= MAX {
        return format!("{flat:?}");
    }
    let head: String = flat.chars().take(MAX).collect();
    format!("{head:?}…（共 {} 字）", flat.chars().count())
}

/// 十六进制头（前 16 字节）。收到不认识的回包时，光看帧类型就能判断是
/// 协议不匹配还是对端发了别的东西 —— 没有它只能猜。
fn hex_head(data: &[u8]) -> String {
    data.iter()
        .take(16)
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 回包 payload 的文本预览（跳过帧头、截断、拍平换行）。
///
/// # 为什么要单独有它
///
/// 收到"认不出来"的回包时，光有 16 字节 HEX 只能看出**帧类型**，
/// 却看不出**服务端到底说了什么** —— 而后者才是排查时要回答的问题。
/// 把 payload 打出来，一眼就能分开这两种情况：
/// - `{"result":{"text":"",...}}` ⇒ 对端听清了，只是这轮没内容（音频问题）；
/// - 完全另一套结构 ⇒ 对端不是我以为的那个服务，协议层对不上。
fn payload_preview(data: &[u8]) -> String {
    if data.len() < 4 {
        return "-".into();
    }
    let flags = data[1] & 0x0F;
    let mut offset = 4usize;
    if flags & FLAG_HAS_SEQUENCE != 0 {
        offset += 4;
    }
    if data.len() < offset + 4 {
        return "-".into();
    }
    let size = u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]) as usize;
    offset += 4;
    if data.len() < offset {
        return "-".into();
    }
    let end = (offset + size).min(data.len());
    let body = String::from_utf8_lossy(&data[offset..end]).replace(['\r', '\n'], " ");
    let head: String = body.chars().take(200).collect();
    format!("{head:?}")
}

// ---------------------------------------------------------------------------
// 防砖超时：这一节的存在理由全部相同 —— **识别任务必须能返回**
// ---------------------------------------------------------------------------
//
// ⛔ 为什么这不是"性能优化"而是"防砖"：
//
// 宿主侧（`session.rs`）把"当前有没有录音"当成一个**唯一的会话槽**，
// 而它的清空点在 `run_recognition` / `run_call` **返回之后**（`session.finish(gen)`）。
// 所以只要这两个函数里有任何一处 `await` 永远不完成，就会发生：
//
//   1. 识别任务永不返回 ⇒ 会话槽永远是"有人"；
//   2. 同一个入口再按 ⇒ 只会去"结束"那段永远不会结束的会话 ⇒ **按了没反应**；
//   3. 别的入口再按 ⇒ 被判成"归属冲突"而拒绝 ⇒ **所有语音入口一起失效**；
//   4. 界面上一个字都不报 —— 这个模块里没有任何一处会说自己卡住了。
//
// 三个 await 点都是网络 IO，**对端完全可以在不断开连接的情况下一直不响应**：
// 建连（TCP/TLS）、发包、以及收尾等"服务端的最后一包"。
// 每一处都必须自带截止时间。

/// 建连超时。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// 单包发送超时。
///
/// ⛔ 尤其重要：发包是在主循环的分支体里 `await` 的，一旦挂住，连
/// `stop_flag` 的 100ms 轮询都没机会跑 —— 用户按"停止"也停不下来。
const SEND_TIMEOUT: Duration = Duration::from_secs(5);

/// 收尾等待超时：发出负包后，最多再等服务端这么久。
///
/// 宁可少等几秒、拿已经收到的半截文本，也好过把整个语音功能锁死。
const FINALIZE_TIMEOUT: Duration = Duration::from_secs(8);

/// 一次识别跑完之后的**实况** —— 用来在"没识别到文字"时说清是哪一环断的。
///
/// # 为什么必须有它
///
/// 过去这里只返回一个字符串，于是三种完全不同的故障 ——
/// 「麦克风一个字节都没采到」「音频上行去了但服务端一声不吭」「服务端回了但内容为空」——
/// 在界面上都显示同一句"未识别到内容"。**失败不静默**在这里的具体含义是：
/// 说得出病在链路的哪一段，而不是把判断成本全推给用户。
// ⚠️ 刻意**没有** `Eq`：电平是 f32，不满足 `Eq`。别为了"整齐"把它加回去。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AsrOutcome {
    /// 识别出的文本（可能为空）。
    pub text: String,
    /// 实际上行给服务端的音频帧数与字节数。
    pub audio_frames: usize,
    pub audio_bytes: usize,
    /// 服务端回包数（含中间结果）。
    pub server_messages: usize,
    /// 服务端回过错误帧时把它带出来（**但不作废已识别到的文本**）。
    ///
    /// 为什么要带着它而不是直接 `Err`：错误帧最常出现在**收尾**那一刻，
    /// 而那时文本通常已经完整识别出来了。若因为它返回 `Err`，调用方会把
    /// `text` 整段丢掉 —— 用户"说了一句话、一个字都没上屏"，只看到"识别失败"。
    /// **已经听到的话，不能因为收尾出岔子而作废。** 有文本就照常交付，
    /// 错误另走提示，两者都不瞒。
    pub server_error: Option<String>,
    /// 上行音频的**峰值电平**（线性 0.0~1.0）。
    ///
    /// # 为什么必须带上它
    ///
    /// "服务端回了包但里面没有文字"有两种**完全相反**的原因：
    /// ① 用户根本没说话 / 离麦太远 ⇒ 音频本身就是静音；
    /// ② 音频有声，只是对端没识别出来 ⇒ 该查编码、格式、额度。
    ///
    /// 没有电平，这两种在日志里长得一模一样，只能靠用户回忆"我当时到底说了没"；
    /// 有了它，一行日志就能定案（正常说话约 -30 ~ -10 dBFS，安静房间底噪在 -60 以下）。
    pub peak: f32,
    /// 上行音频的**均方根电平**（线性 0.0~1.0）。比峰值更能代表"整体有没有声音"。
    pub rms: f32,
}

/// 判定"这一轮几乎没采到声音"的阈值（dBFS）。
///
/// 取 -55：正常说话哪怕离麦较远，峰值也在 -40 以上；而真实静音的房间底噪
/// 通常在 -70 以下。-55 落在中间，不会把"小声说话"误判成静音。
const SILENT_DBFS: f32 = -55.0;

impl AsrOutcome {
    /// 峰值电平换算成 dBFS（满刻度 = 0 dBFS）。完全无信号时给 -120 而不是 -inf。
    pub fn peak_dbfs(&self) -> f32 {
        if self.peak <= 0.0 {
            return -120.0;
        }
        20.0 * self.peak.log10()
    }

    /// RMS 电平（dBFS）。
    pub fn rms_dbfs(&self) -> f32 {
        if self.rms <= 0.0 {
            return -120.0;
        }
        20.0 * self.rms.log10()
    }

    /// 这一轮是不是"几乎没采到声音"。
    ///
    /// `audio_frames > 0` 是前提：一帧都没有属于另一个故障（见 `explain_empty`），
    /// 不能笼统归到"静音"里 —— 那会把"设备没打开"说成"你没说话"。
    pub fn is_silent(&self) -> bool {
        self.audio_frames > 0 && self.peak_dbfs() < SILENT_DBFS
    }
}

/// 把"没识别到文字"翻译成一句能指向病灶的话。
///
/// 分档依据是两个**可观测事实**：有没有音频上去、服务端有没有回过包。
/// 纯函数（不碰 AppHandle / 网络）⇒ 可被单测钉死；不抽出来的话，
/// 这段判断只能靠"看代码好像是对的"。
pub fn explain_empty(outcome: &AsrOutcome, mic_label: &str) -> String {
    if outcome.audio_frames == 0 {
        format!(
            "没收到任何音频（本轮录音源：{mic_label}）—— 麦克风没采到数据。\
             用开发板音频时查板子是否在推流；用电脑麦克风时查系统麦克风权限、\
             以及设备是否被别的程序占用。"
        )
    } else if outcome.is_silent() {
        // ⛔ 这一档最容易被误判：服务端行为（回了包、没文字）与"说了话但识别不出"
        // 完全一样，区别**只在音频电平**。不把它单独拎出来，用户就会去查识别额度、
        // 语言设置 —— 全部查错方向。真正的原因通常是"声音根本没进这个设备"。
        format!(
            "收到了 {} 帧音频，但它们几乎是静音（峰值 {:.0} dBFS，正常说话约 -30 ~ -10）。\
             本轮录音源是「{mic_label}」—— 如果你是对着开发板说的，而这里写的是电脑麦克风，\
             说明板子的 Wi-Fi 音频通道没上线，你的声音根本没进来。",
            outcome.audio_frames,
            outcome.peak_dbfs()
        )
    } else if outcome.server_messages == 0 {
        format!(
            "已上行 {} 帧音频（{:.1} KB），但语音服务一个包都没回 —— \
             像是连上了却没在处理，稍后重试或检查服务额度。",
            outcome.audio_frames,
            outcome.audio_bytes as f64 / 1024.0
        )
    } else {
        format!(
            "收到 {} 帧音频（{:.1} KB）、服务端回了 {} 个包，但里面没有文字 —— \
             多半是真没说话 / 声音太小，或识别语言和你说的话对不上。",
            outcome.audio_frames,
            outcome.audio_bytes as f64 / 1024.0,
            outcome.server_messages
        )
    }
}

/// 给音频接收端套一层计数，**不改变数据流**。
///
/// 单独抽出来是为了让"有没有音频上来"这件事可被测到：直接读 mpsc 的计数器
/// 只会把测试写成一堆线程，而这段逻辑本身是纯的（收一个、加一个）。
struct CountingAudio {
    rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    frames: usize,
    bytes: usize,
    /// 峰值电平（线性 0.0~1.0）。
    peak: f32,
    /// 平方和，用于算 RMS。
    sumsq: f64,
    /// 参与统计的样本数。
    samples: usize,
}

impl CountingAudio {
    fn new(rx: tokio::sync::mpsc::Receiver<Vec<u8>>) -> Self {
        Self {
            rx,
            frames: 0,
            bytes: 0,
            peak: 0.0,
            sumsq: 0.0,
            samples: 0,
        }
    }

    async fn recv(&mut self) -> Option<Vec<u8>> {
        let pcm = self.rx.recv().await?;
        self.frames += 1;
        self.bytes += pcm.len();
        self.absorb(&pcm);
        Some(pcm)
    }

    /// 统计这一帧的电平。**只看，绝不改数据** —— 上行的永远是原样的字节。
    ///
    /// 这是"用户说了话却没识别出来"与"声音压根没进来"之间唯一的客观分界线
    /// （见 `AsrOutcome::peak`）。少了它，这两种故障在日志里完全同形。
    fn absorb(&mut self, pcm: &[u8]) {
        for pair in pcm.chunks_exact(2) {
            let s = i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32768.0;
            let a = s.abs();
            if a > self.peak {
                self.peak = a;
            }
            let d = s as f64;
            self.sumsq += d * d;
            self.samples += 1;
        }
    }

    /// 平均电平（线性）。
    fn rms(&self) -> f32 {
        if self.samples == 0 {
            return 0.0;
        }
        (self.sumsq / self.samples as f64).sqrt() as f32
    }
}

/// 发一次「部分识别结果」（`asr-partial`）。
///
/// # ⛔ 空文本不发
///
/// 本机 ASR 走的是 `.../plan/sauc/bigmodel_**nostream**` 端点：识别过程中服务端
/// 回的每一帧文本**都是空的**（实测 `#1`~`#22` 全部 `文本=""`），真正的文本只在
/// 收尾那一下给出来。
///
/// 把空帧照单转发，等于让每一个下游**用空格把已有内容一遍遍擦掉**：字幕浮层、
/// 语音面板、编辑浮层会同时"看起来什么都没显示"，而后端日志里
/// 「语音：识别成功 N 字」一切正常 —— **成功却静默**，比报错更难查。
///
/// 判断收在这一处、而不是散在 8 个发射点上：漏掉任何一个，就是又开了一条
/// 静默的小路（`run_call` 当初自己加了判断，`run_recognition` 漏了 —— 这就是
/// 它们表现不一致的来源）。
fn emit_partial(app: &tauri::AppHandle, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    let _ = app.emit("asr-partial", text);
}

/// 一次「按住说话 → 松手上屏」的完整识别会话。
///
/// - `audio_rx`：录音线程传来的 PCM 数据（16k/16bit/mono，小端）。
/// - `stop_flag`：松手信号，置位后发负包并收尾。
/// 返回最终文本 + 本次实况（见 `AsrOutcome`），文本已由服务端按
/// `result_type=full` 全量返回。
pub async fn run_recognition(
    app: tauri::AppHandle,
    api_key: String,
    asr_url: String,
    resource_id: String,
    audio_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    stop_flag: Arc<AtomicBool>,
) -> Result<AsrOutcome, String> {
    // 1. 建立带鉴权头的 WebSocket 连接
    let request_id = uuid::Uuid::new_v4().to_string();
    let mut request = asr_url
        .as_str()
        .into_client_request()
        .map_err(|e| format!("构造请求失败: {e}"))?;
    {
        let headers = request.headers_mut();
        headers.insert(
            "X-Api-Key",
            HeaderValue::from_str(&api_key).map_err(|e| format!("API Key 非法: {e}"))?,
        );
        headers.insert(
            "X-Api-Resource-Id",
            HeaderValue::from_str(&resource_id).map_err(|e| format!("Resource ID 非法: {e}"))?,
        );
        headers.insert(
            "X-Api-Request-Id",
            HeaderValue::from_str(&request_id).map_err(|e| e.to_string())?,
        );
        headers.insert("X-Api-Sequence", HeaderValue::from_static("-1"));
    }

    let connect_started = Instant::now();
    let (ws, resp) = tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(request))
        .await
        .map_err(|_| {
            crate::log_buffer::err(format!(
                "asr: 建连超时（{} 秒）url={asr_url}",
                CONNECT_TIMEOUT.as_secs()
            ));
            format!(
                "连接语音服务超时（{} 秒无响应）—— 网络或服务端异常",
                CONNECT_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| {
            crate::log_buffer::err(format!("asr: 建连失败 url={asr_url} err={e}"));
            format!("连接语音服务失败: {e}")
        })?;
    // 握手响应头里的 `X-Tt-Logid` 是**向服务端追查的唯一凭据**（提工单要它）。
    // 出错时手上没有它，就只能对着"服务端没反应"干瞪眼。
    let logid = resp
        .headers()
        .get("x-tt-logid")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_string();
    crate::log_buffer::info(format!(
        "asr: 建连成功 url={asr_url} resource_id={resource_id} request_id={request_id} \
         耗时={}ms logid={logid}",
        connect_started.elapsed().as_millis()
    ));
    let (mut sink, mut stream) = ws.split();

    // 2. 发送首包（full client request，sequence=1）
    let first_json = full_client_request_json();
    crate::log_buffer::info(format!("asr: → 首包 seq=1 json={first_json}"));
    let first = build_frame(
        MT_FULL_CLIENT_REQUEST,
        FLAG_HAS_SEQUENCE,
        SER_JSON,
        Some(1),
        first_json.as_bytes(),
    );
    tokio::time::timeout(SEND_TIMEOUT, sink.send(Message::Binary(first)))
        .await
        .map_err(|_| format!("发送首包超时（{} 秒）", SEND_TIMEOUT.as_secs()))?
        .map_err(|e| format!("发送首包失败: {e}"))?;

    // 3. 主循环：录音数据→发包；松手→退出；回包→实时推文本
    let mut seq: i32 = 2;
    let mut final_text = String::new();
    // 服务端回包数（区分"服务端一声不吭"和"回了但没文字"，见 `AsrOutcome`）。
    let mut server_messages: usize = 0;
    // 服务端错误帧（记录但**不作废文本**，见 `AsrOutcome::server_error`）。
    let mut server_error: Option<String> = None;

    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // 套一层计数：结束时才知道"到底有没有音频上来"（见 `AsrOutcome`）。
    let mut audio = CountingAudio::new(audio_rx);

    loop {
        tokio::select! {
            maybe_audio = audio.recv() => {
                match maybe_audio {
                    Some(pcm) => {
                        let frame = build_frame(
                            MT_AUDIO_ONLY_REQUEST,
                            FLAG_HAS_SEQUENCE,
                            SER_RAW,
                            Some(seq),
                            &pcm,
                        );
                        seq += 1;
                        // 每 25 帧（约 2.5 秒音频）记一次上行实况：日志要能回答
                        // "音频到底有没有上去"，又不能被逐帧刷屏。
                        if seq % 25 == 0 {
                            crate::log_buffer::info(format!(
                                "asr: → 上行音频 seq={seq} 累计 {} 帧 / {} KB",
                                audio.frames,
                                audio.bytes / 1024
                            ));
                        }
                        tokio::time::timeout(SEND_TIMEOUT, sink.send(Message::Binary(frame)))
                            .await
                            .map_err(|_| {
                                crate::log_buffer::err(format!(
                                    "asr: 发送音频超时（{} 秒）seq={seq}",
                                    SEND_TIMEOUT.as_secs()
                                ));
                                format!(
                                    "发送音频超时（{} 秒）—— 上行链路卡住，本次录音已放弃",
                                    SEND_TIMEOUT.as_secs()
                                )
                            })?
                            .map_err(|e| {
                                crate::log_buffer::err(format!(
                                    "asr: 发送音频失败 seq={seq} err={e}"
                                ));
                                format!("发送音频失败: {e}")
                            })?;
                    }
                    None => break,
                }
            }
            _ = tick.tick() => {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }
            }
            maybe_msg = stream.next() => {
                match maybe_msg {
                    Some(Ok(Message::Binary(data))) => {
                        server_messages += 1;
                        if let Some((text, is_last)) = parse_server_frame(&data) {
                            final_text = text.clone();
                            crate::log_buffer::info(format!(
                                "asr: ← 回包 #{server_messages} last={is_last} 文本={}",
                                preview(&text)
                            ));
                            emit_partial(&app, &text);
                        } else if let Some(err) = extract_asr_error(&data) {
                            // ⛔ 记录，但**不 return**：错误帧常出现在收尾那一刻，
                            // 而那时文本往往已经完整。返回 Err 会让调用方把
                            // `final_text` 整段丢掉 —— 用户"说了半天一个字都没上屏"。
                            crate::log_buffer::err(format!(
                                "asr: ← 错误帧 #{server_messages} {err}"
                            ));
                            let _ = app.emit("asr-state", &err);
                            server_error = Some(err);
                            break;
                        } else {
                            // ⛔ 过去这里只有 16 字节 HEX：能看出"帧类型认不出"，
                            // 却看不出"服务端到底说了什么"。而后者才是排查要回答的问题。
                            crate::log_buffer::info(format!(
                                "asr: ← 回包 #{server_messages} 非文本 {} 字节 head={} payload={}",
                                data.len(),
                                hex_head(&data),
                                payload_preview(&data)
                            ));
                        }
                    }
                    Some(Ok(Message::Text(data))) => {
                        if let Some(t) = extract_text(data.as_bytes()) {
                            final_text = t.clone();
                            crate::log_buffer::info(format!(
                                "asr: ← 文本帧 #{server_messages} 文本={}",
                                preview(&t)
                            ));
                            emit_partial(&app, &t);
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(format!("接收失败: {e}")),
                    None => break,
                }
            }
        }
    }

    // 4. 发送最后一包（负包），通知服务端音频结束。
    //    ⚠️ 写法见 `final_frame()` —— 这里曾经写死 `sequence = -1`，
    //    正是"语音识别不了"那个故障的根因。
    let _ = sink.send(Message::Binary(final_frame())).await;
    crate::log_buffer::info(format!(
        "asr: → 负包（仅 LAST 标志）｜本次上行 {} 帧 / {} KB，已收 {} 个回包",
        audio.frames,
        audio.bytes / 1024,
        server_messages
    ));

    // 5. 收尾：读取剩余回包，直到最后一包，取最终文本。
    //
    // ⛔ 这一段**必须**有截止时间：它等的是"服务端的最后一包"或"对端断开连接"，
    // 而服务端完全可能既不回最后一包、也不断开 —— 那么 `stream.next()` 会挂到进程结束。
    // 后果见本文件顶部「防砖超时」一节：会话槽永远不清空 ⇒ 所有语音入口一起失效，
    // 且一个字都不报。宁可少等几秒、拿已经收到的半截文本。
    let deadline = tokio::time::sleep(FINALIZE_TIMEOUT);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => {
                crate::log_buffer::warn(format!(
                    "asr: 收尾等待超过 {} 秒仍未收到最后一包，先用已有文本交付（这次录音没被卡死）",
                    FINALIZE_TIMEOUT.as_secs()
                ));
                break;
            }
            msg = stream.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        server_messages += 1;
                        if let Some((text, is_last)) = parse_server_frame(&data) {
                            final_text = text.clone();
                            crate::log_buffer::info(format!(
                                "asr: ← 收尾回包 #{server_messages} last={is_last} 文本={}",
                                preview(&text)
                            ));
                            emit_partial(&app, &text);
                            if is_last {
                                break;
                            }
                        } else if let Some(err) = extract_asr_error(&data) {
                            crate::log_buffer::err(format!(
                                "asr: ← 收尾错误帧 #{server_messages} {err}"
                            ));
                            let _ = app.emit("asr-state", &err);
                            server_error = Some(err);
                            break;
                        } else {
                            // ⛔ 这个分支**过去根本不存在** —— 既非文本帧、也非错误帧的回包
                            // 被直接丢掉，日志里连一行都不留。而收尾恰恰是最容易出岔子的一步，
                            // "到底有没有收到最后一包、收到的是什么"于是完全不可查。
                            crate::log_buffer::info(format!(
                                "asr: ← 收尾回包 #{server_messages} 非文本非错误 {} 字节 head={} payload={}",
                                data.len(),
                                hex_head(&data),
                                payload_preview(&data)
                            ));
                        }
                    }
                    Some(Ok(Message::Text(data))) => {
                        server_messages += 1;
                        if let Some(t) = extract_text(data.as_bytes()) {
                            final_text = t.clone();
                            emit_partial(&app, &t);
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => break,
                }
            }
        }
    }

    let _ = sink.close().await;

    let outcome = AsrOutcome {
        text: final_text,
        audio_frames: audio.frames,
        audio_bytes: audio.bytes,
        server_messages,
        server_error,
        peak: audio.peak,
        rms: audio.rms(),
    };
    crate::log_buffer::info(format!(
        "asr: 识别结束 文本 {} 字｜上行 {} 帧 / {} KB｜回包 {} 个｜错误 {}｜\
         音频电平 峰值={:.0}dBFS RMS={:.0}dBFS",
        outcome.text.chars().count(),
        outcome.audio_frames,
        outcome.audio_bytes / 1024,
        outcome.server_messages,
        outcome.server_error.as_deref().unwrap_or("无"),
        outcome.peak_dbfs(),
        outcome.rms_dbfs()
    ));

    // 只有"一个字都没有**且**服务端报了错"才判失败。有文本就照常交付 ——
    // 收尾阶段的错误不该把用户已经说出去的话作废（见 `AsrOutcome::server_error`）。
    if outcome.text.trim().is_empty() {
        if let Some(e) = outcome.server_error.clone() {
            return Err(e);
        }
    }
    Ok(outcome)
}

// ===== 电话式连续断句（Call 模式）=====

/// 通话模式首包参数。
///
/// `enable_nonstream: true` 开启二遍识别，会**默认启用 VAD 分句**：服务端在
/// 静音判停时用非流式模型重识别该分句，返回带 `definite: true` 的定稿结果。
/// `end_window_size` 控制判停静音阈值（默认 800ms，最小 200ms），这里取 500ms
/// 兼顾「说完自然停顿」与「别太早切句」。
/// `result_type: "single"` 表示每包只含当前句，避免重复注入历史句子。
fn call_request_json() -> String {
    let mut req = serde_json::json!({
        "audio": {
            "format": "pcm",
            "codec": "raw",
            "rate": 16000,
            "bits": 16,
            "channel": 1
        },
        "request": {
            "model_name": "bigmodel",
            "enable_itn": true,
            "enable_punc": true,
            "enable_ddc": true,
            "enable_nonstream": true,
            "end_window_size": 500,
            "result_type": "single",
            "language": "zh-CN"
        }
    });
    inject_hotwords(&mut req);
    req.to_string()
}

/// 从返回 JSON 提取 (文本, 是否定稿 definite)。
/// Call 模式用 `result_type=single` + `enable_nonstream`，服务端在 VAD 判停后返回带
/// `definite: true` 的定稿句；中间结果 `definite` 缺失或为 false。
/// 缺失时默认 false（中间结果），由 run_call 的客户端停顿检测决定注入时机。
fn extract_result(json: &[u8]) -> Option<(String, bool)> {
    let v: Value = serde_json::from_slice(json).ok()?;
    let result = v
        .pointer("/payload_msg/result")
        .or_else(|| v.pointer("/result"))?;
    let text = result.pointer("/text")?.as_str()?.to_string();
    let definite = result
        .pointer("/definite")
        .and_then(|d| d.as_bool())
        .unwrap_or(false);
    Some((text, definite))
}

/// 解析服务端帧，返回 (文本, definite, 是否最后一包)。
fn parse_call_frame(data: &[u8]) -> Option<(String, bool, bool)> {
    if data.len() < 8 {
        return None;
    }
    let header_type = data[1] >> 4;
    let flags = data[1] & 0x0F;
    let compression = data[2] & 0x0F;

    let mut offset = 4usize;
    let mut sequence: i32 = 0;
    if flags & FLAG_HAS_SEQUENCE != 0 {
        if data.len() < offset + 4 {
            return None;
        }
        sequence = i32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
    }
    if data.len() < offset + 4 {
        return None;
    }
    let payload_size = u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]) as usize;
    offset += 4;
    if data.len() < offset + payload_size {
        return None;
    }
    let payload = &data[offset..offset + payload_size];

    let is_last = (flags & FLAG_LAST_PACKAGE) != 0 || sequence < 0;

    match header_type {
        MT_FULL_SERVER_RESPONSE => {
            let text = if compression == 0x01 {
                gzip_decompress(payload).and_then(|bytes| extract_result(&bytes))
            } else {
                extract_result(payload)
            };
            text.map(|(t, d)| (t, d, is_last))
        }
        MT_ERROR => {
            eprintln!("ASR 服务返回错误: {}", String::from_utf8_lossy(payload));
            None
        }
        _ => None,
    }
}

/// 一次「打电话」式连续通话：持续录音、持续发包，服务端 VAD 自动断句。
///
/// 注入策略（避免过快/重复粘贴）：
/// - 服务端定稿（definite=true）：直接注入（去重）。
/// - 中间结果（definite 缺失/false）：累积到 current_text 实时上屏，**停顿超过阈值**
///   才注入（客户端 VAD 兜底，覆盖服务端不返回 definite 的场景）。
/// - 去重：与上次注入文本相同则跳过。
/// - 挂断收尾：注入剩余未发送的 current_text。
pub async fn run_call(
    app: tauri::AppHandle,
    api_key: String,
    asr_url: String,
    resource_id: String,
    mut audio_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    stop_flag: Arc<AtomicBool>,
    inj_tx: tokio::sync::mpsc::Sender<String>,
) -> Result<(), String> {
    // 1. 建连（与 run_recognition 相同）
    let request_id = uuid::Uuid::new_v4().to_string();
    let mut request = asr_url
        .as_str()
        .into_client_request()
        .map_err(|e| format!("构造请求失败: {e}"))?;
    {
        let headers = request.headers_mut();
        headers.insert(
            "X-Api-Key",
            HeaderValue::from_str(&api_key).map_err(|e| format!("API Key 非法: {e}"))?,
        );
        headers.insert(
            "X-Api-Resource-Id",
            HeaderValue::from_str(&resource_id).map_err(|e| format!("Resource ID 非法: {e}"))?,
        );
        headers.insert(
            "X-Api-Request-Id",
            HeaderValue::from_str(&request_id).map_err(|e| e.to_string())?,
        );
        headers.insert("X-Api-Sequence", HeaderValue::from_static("-1"));
    }

    let connect_started = Instant::now();
    let (ws, resp) = tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(request))
        .await
        .map_err(|_| {
            crate::log_buffer::err(format!(
                "asr(call): 建连超时（{} 秒）url={asr_url}",
                CONNECT_TIMEOUT.as_secs()
            ));
            format!(
                "连接语音服务超时（{} 秒无响应）—— 网络或服务端异常",
                CONNECT_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| {
            crate::log_buffer::err(format!("asr(call): 建连失败 url={asr_url} err={e}"));
            format!("连接语音服务失败: {e}")
        })?;
    let logid = resp
        .headers()
        .get("x-tt-logid")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_string();
    crate::log_buffer::info(format!(
        "asr(call): 建连成功 url={asr_url} resource_id={resource_id} request_id={request_id} \
         耗时={}ms logid={logid}",
        connect_started.elapsed().as_millis()
    ));
    let (mut sink, mut stream) = ws.split();

    // 2. 首包（通话参数）
    let first_json = call_request_json();
    crate::log_buffer::info(format!("asr(call): → 首包 seq=1 json={first_json}"));
    let first = build_frame(
        MT_FULL_CLIENT_REQUEST,
        FLAG_HAS_SEQUENCE,
        SER_JSON,
        Some(1),
        first_json.as_bytes(),
    );
    tokio::time::timeout(SEND_TIMEOUT, sink.send(Message::Binary(first)))
        .await
        .map_err(|_| format!("发送首包超时（{} 秒）", SEND_TIMEOUT.as_secs()))?
        .map_err(|e| format!("发送首包失败: {e}"))?;

    // 3. 主循环：持续发包；定稿句或停顿触发注入；stop 后退出。
    let mut seq: i32 = 2;
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut idle_check = tokio::time::interval(Duration::from_millis(200));
    idle_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // 客户端停顿检测状态：中间结果累积到 current_text，停顿超过阈值则注入。
    let mut current_text = String::new();
    let mut last_update = Instant::now();
    let mut last_injected = String::new();
    let mut last_injected_time = Instant::now();
    const IDLE_THRESHOLD: Duration = Duration::from_millis(1000);
    /// 去重时间窗口：仅在此窗口内与上次注入内容相同时才跳过，
    /// 超过窗口后允许重复注入（用户有意重复说相同的话）。
    const DEDUP_WINDOW: Duration = Duration::from_secs(2);
    // 服务端错误帧：记录但不当场失败（通话是**边识别边投递**的，
    // 已经投出去的句子不该因为收尾出错而作废）。见 `AsrOutcome::server_error`。
    let mut server_error: Option<String> = None;

    loop {
        tokio::select! {
            maybe_audio = audio_rx.recv() => {
                match maybe_audio {
                    Some(pcm) => {
                        let frame = build_frame(
                            MT_AUDIO_ONLY_REQUEST,
                            FLAG_HAS_SEQUENCE,
                            SER_RAW,
                            Some(seq),
                            &pcm,
                        );
                        seq += 1;
                        tokio::time::timeout(SEND_TIMEOUT, sink.send(Message::Binary(frame)))
                            .await
                            .map_err(|_| {
                                format!(
                                    "发送音频超时（{} 秒）—— 上行链路卡住，本次录音已放弃",
                                    SEND_TIMEOUT.as_secs()
                                )
                            })?
                            .map_err(|e| format!("发送音频失败: {e}"))?;
                    }
                    None => break,
                }
            }
            _ = tick.tick() => {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }
            }
            _ = idle_check.tick() => {
                // 停顿检测：当前句非空且距最后更新超过阈值，注入并清空。
                if !current_text.is_empty() && last_update.elapsed() >= IDLE_THRESHOLD {
                    let text = current_text.trim().to_string();
                    current_text.clear();
                    if !text.is_empty() && (text != last_injected || last_injected_time.elapsed() >= DEDUP_WINDOW) {
                        let _ = inj_tx.send(text.clone()).await;
                        last_injected = text;
                        last_injected_time = Instant::now();
                    }
                }
            }
            maybe_msg = stream.next() => {
                match maybe_msg {
                    Some(Ok(Message::Binary(data))) => {
                        if let Some((text, definite, _)) = parse_call_frame(&data) {
                            let text = text.trim().to_string();
                            if !text.is_empty() {
                                emit_partial(&app, &text);
                                if definite {
                                    if text != last_injected || last_injected_time.elapsed() >= DEDUP_WINDOW {
                                        let _ = inj_tx.send(text.clone()).await;
                                        last_injected = text;
                                        last_injected_time = Instant::now();
                                    }
                                    current_text.clear();
                                } else {
                                    current_text = text;
                                    last_update = Instant::now();
                                }
                            }
                        } else if let Some(err) = extract_asr_error(&data) {
                            crate::log_buffer::err(format!("asr(call): ← 错误帧 {err}"));
                            let _ = app.emit("asr-state", &err);
                            server_error = Some(err);
                            break;
                        }
                    }
                    Some(Ok(Message::Text(data))) => {
                        if let Some((text, definite)) = extract_result(data.as_bytes()) {
                            let text = text.trim().to_string();
                            if !text.is_empty() {
                                emit_partial(&app, &text);
                                if definite {
                                    if text != last_injected || last_injected_time.elapsed() >= DEDUP_WINDOW {
                                        let _ = inj_tx.send(text.clone()).await;
                                        last_injected = text;
                                        last_injected_time = Instant::now();
                                    }
                                    current_text.clear();
                                } else {
                                    current_text = text;
                                    last_update = Instant::now();
                                }
                            }
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(format!("接收失败: {e}")),
                    None => break,
                }
            }
        }
    }

    // 4. 注入剩余未发送的当前句（挂断时用户可能没停顿）。
    if !current_text.is_empty() {
        let text = current_text.trim().to_string();
        if !text.is_empty() && (text != last_injected || last_injected_time.elapsed() >= DEDUP_WINDOW) {
            let _ = inj_tx.send(text).await;
        }
    }

    // 5. 负包收尾，捕获最后一句话。
    //    ⚠️ 写法见 `final_frame()`（旧代码这里写死 `Some(-1)`，是语音失效的根因）。
    let _ = sink.send(Message::Binary(final_frame())).await;
    crate::log_buffer::info("asr(call): → 负包（仅 LAST 标志）");

    // ⛔ 与 `run_recognition` 的收尾同理：必须有截止时间。
    // 通话模式下挂在这里更明显 —— 用户按了"挂断"却再也按不出下一段通话（整个语音入口一起死），
    // 因为会话槽一直被认为"有人"。挂了通话也**必须**能返回。
    let deadline = tokio::time::sleep(FINALIZE_TIMEOUT);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => {
                crate::log_buffer::warn(format!(
                    "asr: 通话收尾超过 {} 秒未收到最后一包，直接收尾（这次通话没被卡死）",
                    FINALIZE_TIMEOUT.as_secs()
                ));
                break;
            }
            msg = stream.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        if let Some((text, definite, is_last)) = parse_call_frame(&data) {
                            let text = text.trim().to_string();
                            if !text.is_empty() {
                                emit_partial(&app, &text);
                                if definite && (text != last_injected || last_injected_time.elapsed() >= DEDUP_WINDOW) {
                                    let _ = inj_tx.send(text.clone()).await;
                                    last_injected = text;
                                    last_injected_time = Instant::now();
                                }
                            }
                            if is_last {
                                break;
                            }
                        } else if let Some(err) = extract_asr_error(&data) {
                            crate::log_buffer::err(format!("asr(call): ← 收尾错误帧 {err}"));
                            let _ = app.emit("asr-state", &err);
                            server_error = Some(err);
                            break;
                        }
                    }
                    Some(Ok(Message::Text(data))) => {
                        if let Some((text, definite)) = extract_result(data.as_bytes()) {
                            let text = text.trim().to_string();
                            if !text.is_empty() && definite && (text != last_injected || last_injected_time.elapsed() >= DEDUP_WINDOW) {
                                emit_partial(&app, &text);
                                let _ = inj_tx.send(text.clone()).await;
                                last_injected = text;
                                last_injected_time = Instant::now();
                            }
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => break,
                }
            }
        }
    }

    let _ = sink.close().await;

    // 通话是**边识别边投递**的：已经投出去的句子不该因为收尾出错而作废。
    // 所以只有"一句都没投出去 + 服务端报了错"才判通话失败。
    if let Some(e) = server_error {
        crate::log_buffer::err(format!("asr(call): 通话结束（服务端报错）{e}"));
        if last_injected.is_empty() {
            return Err(e);
        }
    }
    crate::log_buffer::info("asr(call): 通话结束");
    Ok(())
}

#[cfg(test)]
mod tests {
    //! 「没识别到文字」必须**说得出病在哪一段链路**。
    //!
    //! 这段判据的价值全在"事后能定位"：它写错一个字，用户就只能看到
    //! "未识别到内容"这种什么都不指的信息，排查成本原封不动转回给用户。
    //! 所以三种断点各自钉一条。
    use super::*;

    #[test]
    fn no_audio_points_at_the_microphone() {
        let out = AsrOutcome::default();
        let msg = explain_empty(&out, "开发板麦克风");
        assert!(msg.contains("没收到任何音频"), "{msg}");
        assert!(msg.contains("开发板麦克风"), "必须说清用的是哪个录音源: {msg}");
    }

    /// 音频上去了、服务端一声不吭 —— 与"没收到音频"是**完全不同**的病，
    /// 指向的排查方向也不同（前者查服务，后者查麦克风）。
    #[test]
    fn audio_sent_but_server_mute_points_at_the_service() {
        let out = AsrOutcome {
            audio_frames: 42,
            audio_bytes: 40960,
            server_messages: 0,
            // 有声（约 -10 dBFS）—— 不然会先命中"静音"那一档，测不到本档。
            peak: 0.3,
            ..Default::default()
        };
        let msg = explain_empty(&out, "电脑麦克风");
        assert!(msg.contains("42 帧"), "{msg}");
        assert!(msg.contains("40.0 KB"), "字节数要换算成 KB: {msg}");
        assert!(msg.contains("一个包都没回"), "{msg}");
        assert!(!msg.contains("麦克风没采到数据"), "不该甩锅给麦克风: {msg}");
    }

    /// ⛔ 静音必须**单独成档**：它与"说了话但没识别出"在服务端行为上一模一样
    /// （都回包、都没文字），区别只在电平。混在一起会让用户去查识别额度、语言设置，
    /// 而真正的原因往往是"声音根本没进这个设备"。
    #[test]
    fn silent_audio_is_reported_as_silence_not_as_unrecognized_speech() {
        let out = AsrOutcome {
            audio_frames: 86,
            audio_bytes: 537 * 1024,
            server_messages: 2,
            peak: 0.0016, // ≈ -56 dBFS，低于 -55 阈值
            rms: 0.0004,
            ..Default::default()
        };
        assert!(out.is_silent(), "峰值 -56 dBFS 必须判为静音");
        let msg = explain_empty(&out, "电脑麦克风");
        assert!(msg.contains("几乎是静音"), "{msg}");
        assert!(msg.contains("-56"), "必须给出实测电平，那是定案的依据: {msg}");
        assert!(msg.contains("电脑麦克风"), "必须说清声音是从哪个设备进来的: {msg}");
        assert!(
            !msg.contains("没有文字"),
            "静音档不该退回含糊的\"回了包但没文字\": {msg}"
        );
    }

    /// 反过来：有声就不许被误判成静音（否则会把用户的正常录音说成"你没说话"）。
    #[test]
    fn normal_level_audio_is_not_flagged_as_silent() {
        let out = AsrOutcome {
            audio_frames: 30,
            peak: 0.2, // ≈ -14 dBFS
            ..Default::default()
        };
        assert!(!out.is_silent(), "-14 dBFS 是正常说话电平，不能判静音");
    }

    /// `is_silent` 的前提是"确实收到过音频"。一帧都没有属于另一个故障
    /// （设备没打开 / 没权限），说成"静音"就等于把"没插话筒"讲成"你没说话"。
    #[test]
    fn no_audio_is_not_classified_as_silence() {
        let out = AsrOutcome::default(); // audio_frames = 0, peak = 0
        assert!(
            !out.is_silent(),
            "帧数为 0 时必须走\"没收到音频\"那一档，不能归到静音"
        );
    }

    /// 电平统计：**只看不改**。它算错，`explain_empty` 就会把两种相反的故障说反。
    #[tokio::test]
    async fn counting_audio_measures_level_without_altering_samples() {
        let (tx, rx) = tokio::sync::mpsc::channel::<Vec<u8>>(4);
        let mut audio = CountingAudio::new(rx);

        // 半幅方波：峰值 0.5，RMS 也是 0.5（|±0.5| 恒定）。
        let half = (i16::MAX as f32 * 0.5) as i16;
        let samples: Vec<i16> = vec![half, -half, half, -half];
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        tx.try_send(bytes.clone()).unwrap();
        drop(tx);

        let got = audio.recv().await.expect("一帧");
        assert_eq!(got, bytes, "⛔ 统计层绝不能改动上行数据");

        assert!(
            (audio.peak - 0.5).abs() < 0.01,
            "峰值应约 0.5，实际 {}",
            audio.peak
        );
        assert!(
            (audio.rms() - 0.5).abs() < 0.01,
            "半幅方波的 RMS 就是 0.5，实际 {}",
            audio.rms()
        );
        assert_eq!(audio.samples, 4);
    }

    /// 回包容器的 payload 预览：要能看见"服务端说了什么"，而不是只有帧类型。
    #[test]
    fn payload_preview_skips_header_and_shows_body() {
        // 头 4 字节 + 长度 4 字节 + body（不带 sequence）
        let body = br#"{"result":{"text":""}}"#;
        let mut frame = vec![0x11, 0x90, 0x10, 0x00];
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frame.extend_from_slice(body);
        let p = payload_preview(&frame);
        assert!(p.contains("result"), "必须看到 payload 正文: {p}");
        assert!(p.contains("text"), "必须看到关键字段，否则等于没记: {p}");
    }

    #[test]
    fn server_replied_without_text_points_at_the_speech_itself() {
        let out = AsrOutcome {
            audio_frames: 8,
            audio_bytes: 2048,
            server_messages: 3,
            // 有声：否则会先命中"静音"档，测不到本档的措辞。
            peak: 0.3,
            ..Default::default()
        };
        let msg = explain_empty(&out, "电脑麦克风");
        assert!(msg.contains("回了 3 个包"), "{msg}");
        assert!(msg.contains("没有文字"), "{msg}");
    }

    #[tokio::test]
    async fn counting_audio_counts_frames_and_bytes_without_changing_the_stream() {
        let (tx, rx) = tokio::sync::mpsc::channel::<Vec<u8>>(8);
        let mut audio = CountingAudio::new(rx);
        tx.send(vec![0u8; 100]).await.unwrap();
        tx.send(vec![0u8; 50]).await.unwrap();
        drop(tx);

        let a = audio.recv().await.expect("第一帧");
        let b = audio.recv().await.expect("第二帧");
        assert_eq!(a.len(), 100);
        assert_eq!(b.len(), 50, "数据必须原样透传");
        assert!(audio.recv().await.is_none(), "发送端关闭后要收尾");

        assert_eq!(audio.frames, 2);
        assert_eq!(audio.bytes, 150);
    }

    /// ⛔ 回归钉子：负包**必须不带 sequence**。
    ///
    /// 这条测试存在的唯一理由是一次真实故障：负包曾写死 `sequence = -1`，
    /// 服务端回 `autoAssignedSequence (-N) mismatch sequence in request (-1)`，
    /// 于是**每一次录音都在收尾那一刻被判定失败**，用户说出去的话一个字都不上屏。
    /// 而音频识别本身完全正常 —— 排查时极易误判成"麦克风坏了"。
    ///
    /// 谁要是将来"顺手把 sequence 补回去"（看起来更完整、更对称），这条会拦住他。
    #[test]
    fn final_frame_carries_no_sequence_only_the_last_flag() {
        let f = final_frame();
        assert_eq!(f.len(), 8, "头 4 字节 + 长度 4 字节，不该有 sequence 字段：{f:02X?}");
        assert_eq!(f[0], 0x11, "protocol_version=1 / header_size=1");
        assert_eq!(f[1] >> 4, MT_AUDIO_ONLY_REQUEST, "消息类型必须是音频包");
        assert_eq!(
            f[1] & FLAG_HAS_SEQUENCE,
            0,
            "⛔ 不能带 sequence —— 带了就会在收尾时被服务端拒绝"
        );
        assert_ne!(
            f[1] & FLAG_LAST_PACKAGE,
            0,
            "必须带 LAST 标志，否则服务端不知道音频结束"
        );
        assert_eq!(f[2], SER_RAW << 4, "音频包不压缩、不 JSON");
        assert_eq!(&f[4..8], &[0, 0, 0, 0], "空 payload");
        // 对照：旧写法（带 sequence=-1）**长度不同**，这就是当年那个 bug 的形状。
        let old = build_frame(
            MT_AUDIO_ONLY_REQUEST,
            FLAG_HAS_SEQUENCE | FLAG_LAST_PACKAGE,
            SER_RAW,
            Some(-1),
            &[],
        );
        assert_eq!(old.len(), 12, "旧写法多 4 字节 sequence");
        assert_ne!(old, f, "新旧写法必须能区分，否则这条测试形同虚设");
    }

    /// 有文本就**不许**因为服务端错误帧作废 —— 这是用户"说出去的话"。
    #[test]
    fn server_error_does_not_wipe_already_recognized_text() {
        let out = AsrOutcome {
            text: "打开计算器".to_string(),
            audio_frames: 30,
            audio_bytes: 96000,
            server_messages: 4,
            server_error: Some("deck decode failed".to_string()),
            // 有电平（否则会被静音档抢先，掩盖"文本必须保住"这条断言的重点）。
            peak: 0.4,
            rms: 0.1,
        };
        // 失败判定只看"有没有文本"，不看有没有错误 —— 两者都要报，但文本优先保住。
        assert!(!out.text.trim().is_empty(), "有文本就不是失败");
        assert!(out.server_error.is_some(), "错误也要带出去，不瞒着");
    }
}
