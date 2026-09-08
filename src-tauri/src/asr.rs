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

/// 首包（full client request）的 JSON 参数。
fn full_client_request_json() -> String {
    // pcm_s16le / 16k / mono；enable_ddc 顺滑去掉语气词，更接近 easyop 的「收拾」效果。
    serde_json::json!({
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
    })
    .to_string()
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

/// 一次「按住说话 → 松手上屏」的完整识别会话。
///
/// - `audio_rx`：录音线程传来的 PCM 数据（16k/16bit/mono，小端）。
/// - `stop_flag`：松手信号，置位后发负包并收尾。
/// 返回最终的累积文本（已由服务端按 result_type=full 全量返回）。
pub async fn run_recognition(
    app: tauri::AppHandle,
    api_key: String,
    asr_url: String,
    resource_id: String,
    mut audio_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    stop_flag: Arc<AtomicBool>,
) -> Result<String, String> {
    // 1. 建立带鉴权头的 WebSocket 连接
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
            HeaderValue::from_str(&uuid::Uuid::new_v4().to_string())
                .map_err(|e| e.to_string())?,
        );
        headers.insert("X-Api-Sequence", HeaderValue::from_static("-1"));
    }

    let (ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("连接语音服务失败: {e}"))?;
    let (mut sink, mut stream) = ws.split();

    // 2. 发送首包（full client request，sequence=1）
    let first = build_frame(
        MT_FULL_CLIENT_REQUEST,
        FLAG_HAS_SEQUENCE,
        SER_JSON,
        Some(1),
        full_client_request_json().as_bytes(),
    );
    sink.send(Message::Binary(first))
        .await
        .map_err(|e| format!("发送首包失败: {e}"))?;

    // 3. 主循环：录音数据→发包；松手→退出；回包→实时推文本
    let mut seq: i32 = 2;
    let mut final_text = String::new();

    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

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
                        sink.send(Message::Binary(frame))
                            .await
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
            maybe_msg = stream.next() => {
                match maybe_msg {
                    Some(Ok(Message::Binary(data))) => {
                        if let Some((text, _is_last)) = parse_server_frame(&data) {
                            final_text = text.clone();
                            let _ = app.emit("asr-partial", &text);
                        } else if let Some(err) = extract_asr_error(&data) {
                            eprintln!("{err}");
                            let _ = app.emit("asr-state", &err);
                            return Err(err);
                        }
                    }
                    Some(Ok(Message::Text(data))) => {
                        if let Some(t) = extract_text(data.as_bytes()) {
                            final_text = t.clone();
                            let _ = app.emit("asr-partial", &t);
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(format!("接收失败: {e}")),
                    None => break,
                }
            }
        }
    }

    // 4. 发送最后一包（负包），通知服务端音频结束
    let last = build_frame(
        MT_AUDIO_ONLY_REQUEST,
        FLAG_HAS_SEQUENCE | FLAG_LAST_PACKAGE,
        SER_RAW,
        Some(-1),
        &[],
    );
    let _ = sink.send(Message::Binary(last)).await;

    // 5. 收尾：读取剩余回包，直到最后一包，取最终文本
    while let Some(msg) = stream.next().await {
        match msg {
            Ok(Message::Binary(data)) => {
                if let Some((text, is_last)) = parse_server_frame(&data) {
                    final_text = text.clone();
                    let _ = app.emit("asr-partial", &text);
                    if is_last {
                        break;
                    }
                } else if let Some(err) = extract_asr_error(&data) {
                    eprintln!("{err}");
                    let _ = app.emit("asr-state", &err);
                    return Err(err);
                }
            }
            Ok(Message::Text(data)) => {
                if let Some(t) = extract_text(data.as_bytes()) {
                    final_text = t.clone();
                    let _ = app.emit("asr-partial", &t);
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }

    let _ = sink.close().await;

    Ok(final_text)
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
    serde_json::json!({
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
    })
    .to_string()
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
            HeaderValue::from_str(&uuid::Uuid::new_v4().to_string())
                .map_err(|e| e.to_string())?,
        );
        headers.insert("X-Api-Sequence", HeaderValue::from_static("-1"));
    }

    let (ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("连接语音服务失败: {e}"))?;
    let (mut sink, mut stream) = ws.split();

    // 2. 首包（通话参数）
    let first = build_frame(
        MT_FULL_CLIENT_REQUEST,
        FLAG_HAS_SEQUENCE,
        SER_JSON,
        Some(1),
        call_request_json().as_bytes(),
    );
    sink.send(Message::Binary(first))
        .await
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
                        sink.send(Message::Binary(frame))
                            .await
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
                                let _ = app.emit("asr-partial", &text);
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
                            eprintln!("{err}");
                            let _ = app.emit("asr-state", &err);
                            return Err(err);
                        }
                    }
                    Some(Ok(Message::Text(data))) => {
                        if let Some((text, definite)) = extract_result(data.as_bytes()) {
                            let text = text.trim().to_string();
                            if !text.is_empty() {
                                let _ = app.emit("asr-partial", &text);
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
    let last = build_frame(
        MT_AUDIO_ONLY_REQUEST,
        FLAG_HAS_SEQUENCE | FLAG_LAST_PACKAGE,
        SER_RAW,
        Some(-1),
        &[],
    );
    let _ = sink.send(Message::Binary(last)).await;

    while let Some(msg) = stream.next().await {
        match msg {
            Ok(Message::Binary(data)) => {
                if let Some((text, definite, is_last)) = parse_call_frame(&data) {
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        let _ = app.emit("asr-partial", &text);
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
                    eprintln!("{err}");
                    let _ = app.emit("asr-state", &err);
                    return Err(err);
                }
            }
            Ok(Message::Text(data)) => {
                if let Some((text, definite)) = extract_result(data.as_bytes()) {
                    let text = text.trim().to_string();
                    if !text.is_empty() && definite && (text != last_injected || last_injected_time.elapsed() >= DEDUP_WINDOW) {
                        let _ = app.emit("asr-partial", &text);
                        let _ = inj_tx.send(text.clone()).await;
                        last_injected = text;
                        last_injected_time = Instant::now();
                    }
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }

    let _ = sink.close().await;
    Ok(())
}
