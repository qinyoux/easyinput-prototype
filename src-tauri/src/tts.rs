//! TTS 能力（Capability Core 的 `tts`）—— 文本 → 48 kHz 单声道 PCM。
//!
//! # 为什么是通用能力而不是 Agent 任务台的私有件
//!
//! "把一段文字念出来"不属于任何一个业务场景：内容台的配音、语音编辑的确认播报、
//! 任务台的完成总结都要用。按《easyop 扩展机制规范》§二，通用能力并入 Capability Core，
//! 不新建私有目录 —— 否则同一件事会有三份实现、三套错误码。
//!
//! # 两个 provider，两套完全不同的协议（都是实测，不是推断）
//!
//! ## 豆包语音合成大模型（`Volc`，默认）
//!
//! 豆包这条路上有**两条传输**，报文完全不是一个东西，却由**同一份配置**描述 ——
//! 走哪条由端点的 scheme 决定（见 [`TtsConfig::transport`]）：
//!
//! | 端点 | 传输 | 上行 | 下行 |
//! |---|---|---|---|
//! | `https://…/api/v3/tts/unidirectional` | HTTP | POST + JSON | chunked NDJSON，每行一个 JSON |
//! | `wss://…/api/v3/tts/unidirectional/stream` | WebSocket | 一帧二进制（载荷是那段 JSON） | 一帧一帧的二进制（载荷是裸 PCM） |
//! | `wss://…/api/v3/tts/bidirection` | WebSocket | **先握手、再逐帧喂文本**（帧型不同） | 同上 |
//!
//! 之所以做成「同一份配置、两种传输」而不是两个 provider：对用户而言
//! **服务商、账号、密钥、音色、采样率全都一样**，不同的只是"这句话怎么送出去"。
//! 多开一个 provider 会逼用户在两个下拉框之间同步同样的字段 —— 那种同步一定会漏。
//!
//! ### HTTP 路径
//!
//! ```text
//! POST https://openspeech.bytedance.com/api/v3/tts/unidirectional
//! X-Api-Key: <新版控制台的 API Key>
//! X-Api-Resource-Id: seed-tts-1.0
//! { "user": {"uid": "easyop"},
//!   "req_params": { "text": "...", "speaker": "zh_female_cancan_mars_bigtts",
//!                   "audio_params": {"format": "pcm", "sample_rate": 48000} } }
//! ```
//!
//! 响应是 **HTTP chunked 的 NDJSON 流**（`Content-Type: text/plain`），每行一个
//! JSON：`{"code":0,"message":"","data":"<base64 音频片段>"}`，最后一行
//! `{"code":20000000,"message":"OK","data":null}` 收尾。把各行的 `data` 按 base64
//! 解码后**顺序拼接**即得完整音频。
//!
//! 三个实测要点：
//!
//! 1. **直接要 48 kHz，不重采样。** `sample_rate` 支持 48000，而固件也就要 48 kHz，
//!    所以豆包这条路径**零重采样**——省一次滤波，也少一个"采样率不匹配"的故障面。
//! 2. **`format=pcm` 最干净。** 传 `wav` 时服务端会按流式语义**反复插入 WAV 头**
//!    （实测头部 `RIFF` 长度字段是 `0xFFFFFFFF`），拼起来是一份畸形 WAV；
//!    `pcm` 是裸 16-bit 小端单声道，拼完即可用。
//! 3. **错误码有两种形状，都要认。** 鉴权错是 HTTP 401 + `{"header":{"code":...}}`；
//!    而音色与 resource 不匹配是 **HTTP 200** + `{"code":55000000,...}`。
//!    **只看 HTTP 状态码会把后者当成成功**，然后拿到一段空音频。
//!
//! 文本长度实测：1500 字仍被接受（无 512 之类的硬限制），所以 `max_chars` 是
//! **客户端自保**——真正卡住时长的是固件"单条音频 ≤ 150 秒"的播放约束。
//!
//! ### WebSocket 路径（`wss://…/tts/unidirectional/stream`）
//!
//! 同一个服务、同一份 JSON，换成一帧二进制发出去，音频一帧一帧回来。
//! 帧格式与 ASR 同源（`asr.rs` 的头部注释），但**多两个字段**，见 `parse_server_frame`：
//!
//! ```text
//! 上行（客户端）：[0x11][mt|flags][ser|comp][0x00] [sequence 4B] [payload_size 4B] [payload]
//! 下行（服务端）：[0x11][mt|flags][ser|comp][0x00] [event 4B] [sid_len 4B][sid] [payload_size 4B] [payload]
//! ```
//!
//! 实测到的四个要点（都是踩出来的，不是查文档抄的）：
//!
//! 1. **`event` 字段长得像长度。** 350/351/352 这一组事件号恰好落在"长度"的量级上，
//!    若把它当 `payload_size` 读，后面的字节会**整体错位**，而且错位后仍能凑出一段
//!    "听起来像语音"的数据 —— 听不出错，只是音频比预期短一大截（实测 4.0 秒会变成 0.04 秒）。
//!    所以解析器按字段名逐个取，取不出来就报错，绝不猜。
//! 2. **`event 152` 才是最后一个包**，但它前面还有一个 `351`。两者都是"结束了"的意思，
//!    任一出现即可停止读取；只认其中一个会在另一个上多等一个超时周期。
//! 3. **服务端不回 close。** 音频发完后连接**一直开着**，不会自己断开 ——
//!    所以"读完了"只能靠事件号判定，不能等 stream 返回 `None`。
//! 4. **握手仍是同一个 `X-Api-Resource-Id` 闸门**：resource_id 不被账号授权时，
//!    **握手阶段就 403**（连不上，而不是连上后报业务错）。这一点与音色不匹配的
//!    `55000000`（HTTP 200 或已建连后被拒）是**两种不同的故障**，报错文案要分开。
//!
//! ### 双向流式路径（`wss://…/tts/bidirection`）
//!
//! **同一条 WS 上还有第二种帧型**，与上面的单向流式**只差第 5~8 字节的语义**，
//! 但两者不能互换 —— 发错的那一方**不报错，只是一帧都不回**：
//!
//! ```text
//! 单向流式上行：[0x11][mt|flags=0b0001][ser|comp][0x00] [sequence 4B] [payload_size 4B] [payload]
//! 双向流式上行：[0x11][mt|flags=0b0100][ser|comp][0x00] [event 4B] [(sid_len 4B + sid)?] [payload_size 4B] [payload]
//!                                                                 ↑ 位置相同、含义不同
//! ```
//!
//! 而且双向流式**必须先握手再喂文本**，不是"一帧把整段话发完"：
//!
//! ```text
//! StartConnection(1) → ConnectionStarted(50)
//! → StartSession(100) → SessionStarted(150)        ← 服务端参数在这一帧生效
//! → TaskRequest(200)                               ← 文本在这一帧生效
//! → 350(TTSSentenceStart) / 352(TTSResponse，音频) / 351(TTSSentenceEnd)
//! → FinishSession(102) → SessionFinished(152)
//! → FinishConnection(2) → ConnectionFinished(52)
//! ```
//!
//! **三个实测钉死的点**（都不是读文档能推出来的，见 `scripts/probe_tts_bidirection.py`）：
//!
//! 1. ⛔ **把单向帧发给双向端点 = 端点通、但一帧都不回。** 服务端握手照收（HTTP 200），
//!    读完帧发现没有合法 `event`，就**静静等**。调用方看到的是"建连成功"，
//!    然后是 10 秒超时、0 帧、0 采样 —— 2026-09-23 钦友日志里那条
//!    「等待音频超时（10 秒无新帧，已收 0 帧）」就是它。
//! 2. ⛔ **会话类上行帧必须带非空 `session_id`**，连接类帧则不能带。实测三档：
//!    不带 sid 段 → `parse payload size failed: body too short`（服务端把我们的
//!    `payload_size` 当成了 sid 长度）；带空 sid 段 → `session ID length is zero`；
//!    **带真 sid → `SessionStarted(150)`，随后音频正常回来**。
//!    所以这条规则用 `FrameScope` 类型钉死，而不是靠调用方记得传。
//! 3. ✅ **`351` 的载荷里带回服务端收到的全文**（`{"phonemes":[],"text":"…","words":[]}`），
//!    与单向流式一样可以直接拿来做"文本有没有被截断"的交叉验证，不必额外请求。
//!
//! ⚠️ **端点前缀别乱改**：本机账号的 key 只在 `/plan/` 网关下有效，
//! 直接换成文档里的 `wss://openspeech.bytedance.com/api/v3/tts/bidirection` 会 **401**。
//! 走哪条路**由端点路径的末段决定**（`is_bidirection()`），这条判断也是"端点的固有属性"，
//! 不另加下拉框 —— 理由与 `transport()` 相同。
//!
//! ## 阿里云百炼（`Bailian`，备选）
//!
//! 不走 OpenAI 兼容协议，是 DashScope 原生协议，返回一个音频 URL 需要再下载一次：
//!
//! ```text
//! POST https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation
//! Authorization: Bearer <key>
//! { "model": "qwen3-tts-flash",
//!   "input": { "text": "...", "voice": "Cherry", "language_type": "Chinese" } }
//! ```
//!
//! ⚠️ 百炼输出 **24 kHz**，而固件 `speaker_audio_contract.h` 的
//! `max98357a_supports_lrclk()` 只接受 8k/16k/32k/44.1k/48k/88.2k/96k —— **24 kHz 不在其中**。
//! 所以百炼路径必须重采样到 48 kHz。**豆包路径不需要**，这正是默认用豆包的一个附带好处。
//!
//! # 交付采样率统一为 48 kHz
//!
//! 无论哪个 provider，本模块对外只交付 48 kHz。下游（下发链路）永远拿到可以直接
//! 写进 I2S 的帧，**让"播放不出来"这个故障不可能由采样率不匹配引起**。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use http::HeaderValue;
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::capability::ErrorCode;
use crate::log_buffer;

/// 课程给的「单条音频上限」。**这是提示不是硬限制**：超出只记警告，
/// 真正拦它的是 Phase 3 的下发链路（固件播放队列就那么长）。
/// 放在这里是因为"总结太长"这件事在合成阶段就已经能看出来了，
/// 等到下发才报错等于把诊断信息推迟了一个环节。
pub const ADVISORY_MAX_PLAYBACK_MS: u64 = 150_000;

/// base64 解码（容忍缺省填充）。
///
/// 火山返回的片段在实测里都是标准 base64，但**缺 `=` 填充**在流式分片里很常见，
/// 而严格模式会因此整条失败。所以先按标准解，失败再按无填充解。
fn decode_b64_fragment(s: &str) -> Result<Vec<u8>, String> {
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let engine = base64::engine::general_purpose::STANDARD;
    match engine.decode(&clean) {
        Ok(v) => Ok(v),
        Err(_) => base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(clean.trim_end_matches('='))
            .map_err(|e| e.to_string()),
    }
}

/// 从上游错误响应里挖出人类可读的原因。
///
/// **两种形状都要认**（实测）：鉴权失败是 `{"header":{"code":...,"message":...}}`，
/// 业务失败是 `{"code":...,"message":...}`。只读顶层会漏掉前者，
/// 用户只会看到一句没用的 "HTTP 401"。
fn extract_provider_error(bytes: &[u8]) -> String {
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) {
        for holder in [v.get("header").unwrap_or(&v), &v] {
            if let Some(code) = holder.get("code").and_then(|c| c.as_i64()) {
                let msg = holder
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("(无 message)");
                return format!("code={code} message={msg}");
            }
        }
    }
    let raw = String::from_utf8_lossy(bytes);
    let raw = raw.trim();
    if raw.is_empty() {
        "(空响应体)".to_string()
    } else {
        format!("原始响应：{}", raw.chars().take(400).collect::<String>())
    }
}

/// 构造豆包 v3 单向流式的请求体。
///
/// **抽成纯函数是为了可离线测试**：这个 body 有两个"看着对、实际必错"的坑，
/// 而且两者都只能在真机上撞出来——抽出来就能用单测钉死，不必每次改都烧一次 API 额度。
///
/// 坑一：`additions` 必须是**字符串**（内容是 JSON 文本），传嵌套对象会被服务端
/// Go 结构体拒绝（`cannot unmarshal object ... of type string`），且以 HTTP 200 +
/// `code=55000000` 返回——只看状态码会当成成功。
/// 坑二：`format` 只能给 `pcm`。给 `wav` 时流式语义会让服务端反复插入 WAV 头，
/// 拼出来是畸形文件（实测头部长度字段是 `0xFFFFFFFF`）。
fn volc_request_body(cfg: &TtsConfig, text: &str) -> serde_json::Value {
    let mut req_params = serde_json::json!({
        "text": text,
        "speaker": cfg.voice.trim(),
        "audio_params": {
            "format": "pcm",
            "sample_rate": cfg.target_sample_rate,
        }
    });
    if !cfg.explicit_language.trim().is_empty() {
        let additions = serde_json::json!({ "explicit_language": cfg.explicit_language.trim() });
        req_params["additions"] = serde_json::Value::String(additions.to_string());
    }
    serde_json::json!({
        "user": { "uid": "easyop" },
        "req_params": req_params,
    })
}

/// 固件播放采样率——与 `speaker_audio_contract.h` 的
/// `kSpeakerPlaybackSampleRate` 必须一致。改这里就要改固件，反之亦然。
pub const FIRMWARE_SAMPLE_RATE: u32 = 48_000;

// ---- 豆包（火山引擎）语音合成大模型 ----

/// 豆包 TTS v3 单向流式端点（HTTP chunked + NDJSON）。
///
/// 双向流式的官方端点是 `wss://openspeech.bytedance.com/api/v3/tts/bidirection`。
/// 这里**不给它常量的原因**是：本机账号的 key 只在 `/plan/` 网关下有效，
/// 常量一旦存在，早晚会有人把它填进配置，然后拿到一个 **401 `Invalid X-Api-Key`** ——
/// 而"401"看起来像密钥错了，实际是路径少了 `/plan/`。**留一个会误导人的默认值，
/// 比没有默认值更糟。** 所以双向流式一律由用户填的端点决定（路径末段为 `bidirection`），
/// 见 `is_bidirection()`。
pub const VOLC_TTS_ENDPOINT: &str =
    "https://openspeech.bytedance.com/api/v3/tts/unidirectional";

/// 豆包 `X-Api-Resource-Id` 默认值。
///
/// - `seed-tts-1.0`：豆包语音合成模型 1.0 的音色
/// - `seed-tts-2.0`：模型 2.0 的音色（**音色 ID 不能跨版本混用**，混用会返回
///   `code=55000000 resource ID is mismatched with speaker related resource`）
pub const VOLC_TTS_RESOURCE_ID: &str = "seed-tts-1.0";

/// 豆包默认音色（1.0 音色列表内的中文女声）。
pub const VOLC_TTS_SPEAKER: &str = "zh_female_cancan_mars_bigtts";

/// 响应流结束哨兵码。收到它表示合成正常结束。
const VOLC_CODE_DONE: i64 = 20_000_000;

// ---------------------------------------------------------------------------
// 火山二进制帧编解码（WS 传输专用）
// ---------------------------------------------------------------------------
//
// 头部 4 字节的语义（与 asr.rs 同一套）：
//   byte0 = 0x11        protocol_version=1, header_size=1
//   byte1 = (mt<<4)|flags
//   byte2 = (ser<<4)|compression
//   byte3 = 0x00

/// 消息类型：客户端整包请求（载荷是那段 JSON 参数）。
const MT_FULL_CLIENT_REQUEST: u8 = 0b0001;
/// 消息类型：服务端整包应答（载荷是 JSON：会话信息 / 音素 / 收尾文本）。
const MT_FULL_SERVER_RESPONSE: u8 = 0b1001;
/// 消息类型：服务端音频包（载荷是裸 PCM）。
const MT_AUDIO_ONLY_SERVER_RESPONSE: u8 = 0b1011;
/// 消息类型：错误帧（多一个 4 字节错误码）。
const MT_ERROR: u8 = 0b1111;

/// flags：带 4 字节 sequence。
const FLAG_HAS_SEQUENCE: u8 = 0b0001;
/// flags：带 4 字节 event（**只有服务端回包会带**）。
const FLAG_HAS_EVENT: u8 = 0b0100;

/// 序列化方式。
const SER_RAW: u8 = 0b0000;
const SER_JSON: u8 = 0b0001;

// 服务端事件号。**数值是官方定的，两条 WS 路径共用同一套**（官方文档 §5）；
// 下面这些常量的名字是从**单向流式**的视角起的（那边一次发完，所以 350 看着像"会话开始"、
// 351 看着像"整条流结束"）。官方名对照：
//
//   350 = TTSSentenceStart（句开始，载荷是空音素 JSON）
//   351 = TTSSentenceEnd  （句结束，**载荷里带回服务端收到的全文**）
//   352 = TTSResponse     （音频都挂在这个事件上）
//   152 = SessionFinished （收尾记账包，载荷 `{}`）
//
// ⚠️ 别按名字去推另一条路径的行为：两边数值相同、角色相承，但**上行帧型不同**。

/// 服务端事件号 350：句开始（载荷是空音素的 JSON）。
const EV_SESSION_STARTED: u32 = 350;
/// 服务端事件号 351：句结束 —— **整条流到这里就完了**，且载荷含回读全文（实测）。
const EV_TTS_SESSION_FINISHED: u32 = 351;
/// 服务端事件号 352：音频包挂在这个事件上。
const EV_TTS_SENTENCE_START: u32 = 352;
/// 服务端事件号 152：收尾记账包（实测跟在 351 之后，载荷是 `{}`）。
///
/// 351 与 152 都表示"结束了"，任一出现即可停止读取 —— 只认其中一个，
/// 会在另一个上白等一个超时周期（实测 351 与 152 是**两帧**，不是一帧）。
const EV_SESSION_FINISHED: u32 = 152;

// ---- 双向流式（`/tts/bidirection`）的连接类 / 会话类事件 ----
//
// 这五个只有双向流式用得上：单向流式是一条 WS 上直接发一段 JSON，没有握手可言。
// **连接类与会话类的区别不只是名字** —— 会话类上行帧必须挂 session_id（见 `FrameScope`）。

/// 上行 1：声明建立连接（HTTP Upgrade 之后的第一帧，载荷 `{}`）。
const EV_START_CONNECTION: u32 = 1;
/// 上行 2：结束连接（收尾用，失败不影响音频）。
const EV_FINISH_CONNECTION: u32 = 2;
/// 下行 50：建连成功，载荷是 `{}`，**另带 connection_id**。
const EV_CONNECTION_STARTED: u32 = 50;
/// 下行 51：建连失败。
const EV_CONNECTION_FAILED: u32 = 51;
/// 下行 52：连接已结束。
const EV_CONNECTION_FINISHED: u32 = 52;
/// 上行 100：声明创建会话。**服务端参数（speaker / audio_params / additions）在这一帧生效。**
const EV_START_SESSION: u32 = 100;
/// 上行 102：文本给完了。
const EV_FINISH_SESSION: u32 = 102;
/// 下行 150：会话已开始。
const EV_SESSION_STARTED_BIDIR: u32 = 150;
/// 下行 153：会话失败（**失败原因在载荷里**，要读出来，不能只报"失败了"）。
const EV_SESSION_FAILED: u32 = 153;
/// 上行 200：喂文本。**`req_params.text` 在这一帧生效**（不是 StartSession）。
const EV_TASK_REQUEST: u32 = 200;

/// 错误码：鉴权失败（key 无效）。实测 HTTP 401 与错误帧都用它。
const VOLC_CODE_AUTH: u32 = 45_000_010;
/// 错误码：音色与 resource 不同版本。
const VOLC_CODE_RESOURCE_MISMATCH: u32 = 55_000_000;

/// 双向流式上行帧的作用域。
///
/// # 为什么用类型而不是注释
///
/// "连接类帧不带 session_id、会话类帧必须带非空 session_id"这条规则，
/// **三种试点都能发出一个"看起来正常"的帧**，只有服务端会拒：
///
/// | 发法 | 服务端回 |
/// |---|---|
/// | 会话类不带 sid 段 | `parse payload size failed: body too short` |
/// | 会话类带空 sid 段 | `session ID length is zero` |
/// | **会话类带真 sid** | ✅ `SessionStarted(150)` |
///
/// 而且它会**伪装成无关的错**：服务端把 `payload_size` 当成 sid 长度去读，报的是
/// "body too short"，跟"你没传 session_id"看不出关系。所以这里把规则写进类型，
/// 让"会话类帧忘了 sid"在编译期就没法表达，而不是靠调用方记得。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameScope<'a> {
    /// 连接类（`StartConnection` / `FinishConnection`）：**不带** session_id。
    Connection,
    /// 会话类（`StartSession` / `TaskRequest` / `FinishSession`）：**必须带**非空 session_id。
    Session(&'a str),
}

/// 构造双向流式的一帧上行。
///
/// 布局：`[头 4B][event 4B][(sid_len 4B + sid)?][payload_size 4B][payload]`
///
/// 空 sid 返回 `Err` 而不是静默发出去：一个必定被服务端拒、且报错文案指不到病灶的帧，
/// 没有任何理由让它离开这个函数。
fn build_bidirection_frame(
    event: u32,
    scope: FrameScope<'_>,
    json_payload: &[u8],
) -> Result<Vec<u8>, String> {
    let mut v = Vec::with_capacity(16 + json_payload.len());
    v.push(0x11);
    // flags = 0b0100：**带 event 号**（单向流式这里是 0b0001 带 sequence ——
    // 字节位置相同、含义不同，这就是"发错也不报错"的根源）。
    v.push((MT_FULL_CLIENT_REQUEST << 4) | FLAG_HAS_EVENT);
    v.push(SER_JSON << 4); // 压缩 = NONE
    v.push(0x00);
    v.extend_from_slice(&event.to_be_bytes());
    match scope {
        FrameScope::Connection => {}
        FrameScope::Session(sid) => {
            if sid.is_empty() {
                return Err(format!(
                    "双向流式的会话类帧（event {event}）必须带非空 session_id：\
                     空串会被服务端拒为 `session ID length is zero`，而少了整段则被读成\
                     payload 长度错位（`body too short`）。"
                ));
            }
            v.extend_from_slice(&(sid.len() as u32).to_be_bytes());
            v.extend_from_slice(sid.as_bytes());
        }
    }
    v.extend_from_slice(&(json_payload.len() as u32).to_be_bytes());
    v.extend_from_slice(json_payload);
    Ok(v)
}

/// 客户端 → 服务端的一帧。
///
/// 只有首包一种形态（单向流式：整段文本一次给完），所以 `sequence` 在这里
/// 定死为 1 —— 留个参数是为了让单测能钉住"它必须带 sequence"这件事。
fn build_client_frame(sequence: Option<i32>, json_payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(12 + json_payload.len());
    v.push(0x11);
    v.push((MT_FULL_CLIENT_REQUEST << 4) | FLAG_HAS_SEQUENCE);
    v.push(SER_JSON << 4); // 压缩 = NONE
    v.push(0x00);
    if let Some(seq) = sequence {
        v.extend_from_slice(&seq.to_be_bytes());
    }
    v.extend_from_slice(&(json_payload.len() as u32).to_be_bytes());
    v.extend_from_slice(json_payload);
    v
}

/// 服务端 → 客户端的一帧，解析结果。
#[derive(Debug, PartialEq, Eq)]
pub struct ServerFrame {
    pub message_type: u8,
    /// 事件号（仅当 flags 标了 HAS_EVENT）。无则为 `None`。
    pub event: Option<u32>,
    /// 错误码（仅错误帧）。
    pub code: Option<u32>,
    /// 回包里的 `session_id` / `connection_id` 段。
    ///
    /// 同一段位置、两种叫法：连接类事件放 `connection_id`，会话类事件放 `session_id`。
    /// 保留它的价值在**排障**：日志里有一条服务端认的 id，出问题时才有东西能对上，
    /// 否则两边都在说"我那条请求"，却没有任何共同的坐标。
    pub session_id: Option<String>,
    /// 载荷：音频帧是裸 PCM；JSON 帧是那段 JSON 文本。
    pub payload: Vec<u8>,
}

/// 解析服务端一帧。
///
/// # 为什么返回 `Result` 而不是 `Option`
///
/// 解析不出来有两种可能：① 对端发了别的东西（协议不匹配）；② 我们读错位了。
/// 后者**不报错的后果最严重** —— 错位的数据仍能被 `chunks_exact(2)` 切成 i16，
/// 交付出去就是一段"短了一百倍、但听不出解码失败"的音频。所以这里一律显式失败，
/// 并在错误里带上头部十六进制，让"到底收到什么"在一次日志里就能看清。
///
/// # 字段顺序（实测）
///
/// `[头 4B] [event 4B（有 HAS_EVENT 时）] [sid_len 4B][sid] [payload_size 4B] [payload]`
///
/// `session_id` 只在服务端回包出现（客户端上行不带）。这里靠"长度字段是否落得进
/// 剩余字节"来判定 sid 段是否存在：sid 长度必然是几十字节的小数，而 payload_size
/// 在音频帧上是几万 —— 两者量级差得远，用边界校验即可分开，不必两套解析器。
fn parse_server_frame(data: &[u8]) -> Result<ServerFrame, String> {
    if data.len() < 8 {
        return Err(format!(
            "帧短于 8 字节（{} 字节，head={}）",
            data.len(),
            hex_head(data)
        ));
    }
    let message_type = data[1] >> 4;
    let flags = data[1] & 0x0F;

    if message_type == MT_ERROR {
        // 错误帧：头 4B + 错误码 4B + payload_size 4B + JSON（**不带 event**）
        let (code, off) = read_u32(data, 4).ok_or_else(|| format!("错误帧过短（head={}）", hex_head(data)))?;
        return Ok(ServerFrame {
            message_type,
            event: None,
            code: Some(code),
            session_id: None,
            payload: read_payload(data, off).map_err(|e| format!("错误帧{e}"))?,
        });
    }

    let mut off = 4usize;
    let mut event = None;
    if flags & FLAG_HAS_EVENT != 0 {
        let (ev, next) = read_u32(data, off)
            .ok_or_else(|| format!("标了 event 但字节不够（head={}）", hex_head(data)))?;
        event = Some(ev);
        off = next;
    }
    if flags & FLAG_HAS_SEQUENCE != 0 {
        let (_, next) = read_u32(data, off)
            .ok_or_else(|| format!("标了 sequence 但字节不够（head={}）", hex_head(data)))?;
        // 服务端序号对本用途无意义，读出来只为跳过它。
        off = next;
    }
    // id 段：只在"有 event 的回包"里出现，长度必然是小数（实测 36，是 uuid 的长度）。
    // 连接类事件这里是 `connection_id`，会话类事件是 `session_id`，**布局相同**。
    let mut session_id = None;
    if event.is_some() {
        if let Some((sid_len, next)) = read_u32(data, off) {
            let sid_len = sid_len as usize;
            if (1..=128).contains(&sid_len) && next + sid_len + 4 <= data.len() {
                session_id = Some(String::from_utf8_lossy(&data[next..next + sid_len]).to_string());
                off = next + sid_len;
            }
        }
    }

    Ok(ServerFrame {
        message_type,
        event,
        code: None,
        session_id,
        payload: read_payload(data, off)?,
    })
}

/// 从 `off` 读一个大端 u32，返回 (值, 下一个位置)。字节不够返回 `None`。
fn read_u32(data: &[u8], off: usize) -> Option<(u32, usize)> {
    if data.len() < off + 4 {
        return None;
    }
    Some((
        u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]),
        off + 4,
    ))
}

/// 在 `off` 处读「长度 + 载荷」。
///
/// 长度越界一律报错而不是截断到剩余字节：截断会让"读错位"伪装成"读到了短音频"，
/// 而错位的载荷照样能被切成 i16 —— 那是一条听不出异常、却全程交付坏数据的路径。
fn read_payload(data: &[u8], off: usize) -> Result<Vec<u8>, String> {
    let (size, body) = read_u32(data, off)
        .ok_or_else(|| format!("缺少 payload 长度字段（head={}）", hex_head(data)))?;
    let size = size as usize;
    if size > data.len() - body {
        return Err(format!(
            " payload 长度 {} 超过剩余 {} 字节（head={}）",
            size,
            data.len() - body,
            hex_head(data)
        ));
    }
    Ok(data[body..body + size].to_vec())
}

/// 前 16 字节的十六进制 —— 收到不认识的帧时，光看它就能判断是协议不匹配还是读错位。
fn hex_head(data: &[u8]) -> String {
    data.iter()
        .take(16)
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 把错误帧的内容翻译成一句能指向病灶的话。
///
/// **两个错误码指向完全相反的修改方向**：一个是账号/密钥，一个是音色配置。
/// 混成一句"上游报错"就等于把排查成本原封不动转回给用户。
fn explain_volc_code(code: u32, raw: &str) -> String {
    let detail = extract_provider_error(raw.as_bytes());
    match code {
        VOLC_CODE_AUTH => format!(
            "豆包 TTS 鉴权失败（{code}）。这个 `X-Api-Key` 无效或不属于语音合成服务：\
             方舟（ark）的 key 与语音技术控制台的 key 不是同一把，确认后用语音控制台的那把。\
             原始响应：{detail}"
        ),
        VOLC_CODE_RESOURCE_MISMATCH => format!(
            "豆包 TTS 音色与资源 ID 版本不匹配（{code}）。\
             `seed-tts-1.0` 只认 1.0 音色（如 zh_female_cancan_mars_bigtts），\
             `seed-tts-2.0` 只认 2.0 音色（如 zh_female_vv_uranus_bigtts，实测可用）。\
             两个值必须同版本，混用必然失败。原始响应：{detail}"
        ),
        _ => format!("豆包 TTS 业务错误（{code}）：{detail}"),
    }
}

/// WS 传输的三个超时。与 `asr.rs` 的"防砖"理由完全相同：
/// 这些 `await` 点对端可以**一直不响应也不断开**，没有截止时间就会把调用方永久挂住。
const WS_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const WS_SEND_TIMEOUT: Duration = Duration::from_secs(10);
/// 单帧接收超时。取 10 秒：一段 150 秒上限的音频，服务端推流不会出现 10 秒静默；
/// 真出现了，也说明链路已经坏掉，早失败比挂住强。
const WS_RECV_IDLE_TIMEOUT: Duration = Duration::from_secs(10);

/// 已建好的火山 TTS WebSocket 连接。
type VolcWs = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// 与火山 TTS 建立带鉴权的 WebSocket 连接（**单向 / 双向两条路径共用**）。
///
/// # 为什么必须共用一份
///
/// 这段里有两处**只能有一份**的东西：
///
/// 1. **握手阶段的 401/403 与建连后的 `55000000` 是两个相反的方向。**
///    前者要改「账号 / 套餐 / 密钥」，后者要改「音色与 resource 的版本搭配」。
///    复制一份代码的下场是往后只改一处，另一处留着旧文案继续把排查带偏 ——
///    而这两类错误的症状（"没反应"）本来就长得一样，文案是唯一的区分手段。
/// 2. **`X-Tt-Logid` 是向服务端追查的唯一凭据**（提工单要它）。漏了它，
///    就只能对着"服务端没反应"干瞪眼。
///
/// `tag` 只决定日志前缀（`豆包 TTS(ws)` / `豆包 TTS(bidir)`）——两条路径的超时与帧型
/// 不同，混在一条日志流里会误判。`extra_headers` 供双向流式补 `X-Api-Connect-Id`。
///
/// 返回 `(连接, logid, request_id)`。
async fn volc_ws_connect(
    cfg: &TtsConfig,
    url: &str,
    tag: &str,
    extra_headers: &[(&str, String)],
    start: Instant,
) -> Result<(VolcWs, String, String), TtsError> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let mut request = url.into_client_request().map_err(|e| {
        TtsError::InvalidInput(format!("TTS 端点不是合法的 WebSocket URL：{e}（{url}）"))
    })?;
    {
        let headers = request.headers_mut();
        headers.insert(
            "X-Api-Key",
            HeaderValue::from_str(cfg.api_key.trim())
                .map_err(|e| TtsError::InvalidInput(format!("API Key 非法：{e}")))?,
        );
        headers.insert(
            "X-Api-Resource-Id",
            HeaderValue::from_str(cfg.resource_id.trim())
                .map_err(|e| TtsError::InvalidInput(format!("Resource ID 非法：{e}")))?,
        );
        headers.insert(
            "X-Api-Request-Id",
            HeaderValue::from_str(&request_id)
                .map_err(|e| TtsError::InvalidInput(format!("Request ID 非法：{e}")))?,
        );
        for (k, v) in extra_headers {
            headers.insert(
                http::header::HeaderName::from_bytes(k.as_bytes())
                    .map_err(|e| TtsError::InvalidInput(format!("附加请求头 {k} 非法：{e}")))?,
                HeaderValue::from_str(v)
                    .map_err(|e| TtsError::InvalidInput(format!("附加请求头 {k} 的值非法：{e}")))?,
            );
        }
    }

    let (ws, resp) =
        tokio::time::timeout(WS_CONNECT_TIMEOUT, tokio_tungstenite::connect_async(request))
            .await
            .map_err(|_| {
                TtsError::Network(format!(
                    "{tag} 建连超时（{} 秒无响应）：{url}",
                    WS_CONNECT_TIMEOUT.as_secs()
                ))
            })?
            .map_err(|e| {
                let msg = match &e {
                    tokio_tungstenite::tungstenite::Error::Http(r) => {
                        let code = r.status().as_u16();
                        let hint = if code == 401 || code == 403 {
                            " —— 该 X-Api-Resource-Id 未获账号授权，或 API Key 无效。\
                             握手阶段就被拒，说明要改的是「账号 / 套餐 / 密钥」，不是音色。\
                             若端点写的是不带 `/plan/` 的官方路径，注意本机账号的 key \
                             只在 `/plan/` 网关下有效。"
                        } else {
                            ""
                        };
                        format!("{tag} 握手被拒 HTTP {code}{hint}（{url}）")
                    }
                    other => format!("{tag} 建连失败：{other}（{url}）"),
                };
                log_buffer::err(msg.clone());
                TtsError::Network(msg)
            })?;

    let logid = resp
        .headers()
        .get("x-tt-logid")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_string();
    log_buffer::info(format!(
        "{tag} 建连成功：{} ms，request_id={request_id} logid={logid}",
        start.elapsed().as_millis()
    ));

    Ok((ws, logid, request_id))
}

/// 双向流式的发送半（`VolcWs::split()` 的产物）。
type BidirSink = futures_util::stream::SplitSink<VolcWs, Message>;
/// 双向流式的接收半。
type BidirStream = futures_util::stream::SplitStream<VolcWs>;

/// 双向流式：发一帧，带超时。
///
/// 每一步都单独超时，而不是靠一个总超时：**"哪一步卡住"本身就是最关键的诊断信息**。
/// 官方文档那条链路有五个往返，笼统地报"超时"等于把五选一丢给用户去猜。
async fn bidir_send(
    sink: &mut BidirSink,
    frame: Vec<u8>,
    logid: &str,
    what: &str,
) -> Result<(), TtsError> {
    tokio::time::timeout(WS_SEND_TIMEOUT, sink.send(Message::Binary(frame)))
        .await
        .map_err(|_| {
            TtsError::Network(format!(
                "豆包 TTS(bidir) 发送 {what} 超时（{} 秒，logid={logid}）",
                WS_SEND_TIMEOUT.as_secs()
            ))
        })?
        .map_err(|e| TtsError::Network(format!("豆包 TTS(bidir) 发送 {what} 失败：{e}（logid={logid}）")))
}

/// 双向流式：收一帧二进制并解析。
///
/// 文本帧**不丢弃**而是记下来：该端点主要在异常时才发文本帧，
/// 把"服务端用文本帧说了一句话"静默吞掉，等于自己删掉最难拿到的那条线索。
async fn bidir_recv(
    stream: &mut BidirStream,
    wait: Duration,
    logid: &str,
) -> Result<ServerFrame, TtsError> {
    loop {
        let msg = match tokio::time::timeout(wait, stream.next()).await {
            Err(_) => {
                return Err(TtsError::Network(format!(
                    "豆包 TTS(bidir) 等待回帧超时（{} 秒无新帧，logid={logid}）",
                    wait.as_secs()
                )))
            }
            Ok(None) => {
                return Err(TtsError::Provider(format!(
                    "豆包 TTS(bidir) 连接在对端给出结束事件前被关闭（logid={logid}）"
                )))
            }
            Ok(Some(Err(e))) => {
                return Err(TtsError::Network(format!(
                    "豆包 TTS(bidir) 接收失败：{e}（logid={logid}）"
                )))
            }
            Ok(Some(Ok(m))) => m,
        };
        match msg {
            Message::Binary(b) => {
                return parse_server_frame(&b).map_err(|e| {
                    TtsError::Decode(format!("豆包 TTS(bidir) 回帧解析失败：{e}（logid={logid}）"))
                })
            }
            Message::Text(t) => {
                log_buffer::warn(format!("豆包 TTS(bidir) 服务端文本帧：{t}（logid={logid}）"));
            }
            Message::Close(_) => {
                return Err(TtsError::Provider(format!(
                    "豆包 TTS(bidir) 对端主动关闭了连接（logid={logid}）"
                )))
            }
            _ => {}
        }
    }
}

/// 校验刚收到的那一帧是不是我们要的那一步。
///
/// 失败帧（`ConnectionFailed` / `SessionFailed`）**必须把载荷读出来**：
/// 服务端的排障信息就在里面，只报一句"会话失败"等于把免费拿到的原因扔掉。
fn bidir_expect(
    frame: &ServerFrame,
    want: u32,
    want_name: &str,
    logid: &str,
) -> Result<(), TtsError> {
    if frame.message_type == MT_ERROR {
        let code = frame.code.unwrap_or(0);
        let raw = String::from_utf8_lossy(&frame.payload).to_string();
        let msg = format!("{}（{want_name} 失败，logid={logid}）", explain_volc_code(code, &raw));
        log_buffer::err(msg.clone());
        return Err(TtsError::Provider(msg));
    }
    let detail = String::from_utf8_lossy(&frame.payload);
    match frame.event {
        Some(ev) if ev == want => Ok(()),
        Some(EV_CONNECTION_FAILED) => Err(TtsError::Provider(format!(
            "豆包 TTS(bidir) 建连被服务端拒绝（ConnectionFailed，logid={logid}）：{detail}"
        ))),
        Some(EV_SESSION_FAILED) => Err(TtsError::Provider(format!(
            "豆包 TTS(bidir) 会话失败（SessionFailed，logid={logid}）：{detail}"
        ))),
        other => Err(TtsError::Provider(format!(
            "豆包 TTS(bidir) 期望 {want_name}（event {want}），却收到 event {other:?}（logid={logid}）：\
             {detail}"
        ))),
    }
}

// ---- 阿里云百炼（备选）----

/// 百炼 DashScope 原生 TTS 端点（北京地域）。
pub const BAILIAN_TTS_ENDPOINT: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation";

/// 百炼 TTS 默认模型。
pub const BAILIAN_TTS_MODEL: &str = "qwen3-tts-flash";

/// qwen-tts 系的输入上限（**Token**，不是字符）。
pub const BAILIAN_TTS_MAX_CHARS: usize = 512;

// ---------------------------------------------------------------------------
// 配置
// ---------------------------------------------------------------------------

/// TTS 服务商。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TtsProvider {
    /// 豆包语音合成大模型（火山引擎，`seed-tts-1.0`）。默认。
    ///
    /// 选它当默认有三个实测理由：① 可以直接要 48 kHz，**零重采样**；
    /// ② 一次请求拿完整音频，没有"再下载一次 URL"的第二步；
    /// ③ 与现有 ASR 同在火山引擎账号体系下，少管一个账号。
    Volc,
    /// 阿里云百炼（DashScope 原生协议）。备选：输出 24 kHz，需要重采样。
    Bailian,
}

impl Default for TtsProvider {
    fn default() -> Self {
        TtsProvider::Volc
    }
}

impl TtsProvider {
    /// 中文标签（UI 用）。
    pub fn label(&self) -> &'static str {
        match self {
            TtsProvider::Volc => "豆包语音合成大模型（火山引擎）",
            TtsProvider::Bailian => "阿里云百炼",
        }
    }
}

/// TTS 配置。
///
/// `api_key` **默认空串**：密钥由用户在设置页填写，不写进仓库、不写进 prompt、不进截图。
///
/// # 字段按 provider 归属
///
/// 两个 provider 的协议不同，所以配置字段也分两套。**共用的只有 `api_key`、
/// `voice`（音色名）、`target_sample_rate`、`max_chars`、`timeout_secs`**：
///
/// | 字段 | 豆包 | 百炼 |
/// |---|---|---|
/// | `endpoint` | v3 unidirectional | multimodal-generation |
/// | `resource_id` | ✅ `seed-tts-1.0` | 忽略 |
/// | `model` | 忽略 | ✅ `qwen3-tts-flash` |
/// | `explicit_language` | ✅ `zh-cn` | 忽略 |
/// | `language_type` | 忽略 | ✅ `Chinese` |
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TtsConfig {
    pub provider: TtsProvider,
    pub endpoint: String,
    /// 豆包专用：`X-Api-Resource-Id`。
    #[serde(default)]
    pub resource_id: String,
    /// 百炼专用：模型名。
    pub model: String,
    #[serde(default)]
    pub api_key: String,
    /// 音色名。豆包 = `speaker` ID（如 `zh_female_cancan_mars_bigtts`）；
    /// 百炼 = `voice` 名（如 `Cherry`）。两者不通用，换 provider 要一起换。
    pub voice: String,
    /// 豆包专用：`req_params.additions.explicit_language`。空则不传（服务端按中英混处理）。
    #[serde(default)]
    pub explicit_language: String,
    /// 百炼专用：`input.language_type`。
    pub language_type: String,
    /// 交付采样率。默认即固件要求，**不要改**，除非同步改固件。
    pub target_sample_rate: u32,
    /// 单次合成文本上限（字符数，客户端自保）。
    ///
    /// 豆包服务端实测**接受 1500 字**，没有 512 这类硬限制；这个上限挡的是
    /// "误把一整篇文档丢进来"——真正卡时长的是固件"单条音频 ≤ 150 秒"的约束。
    pub max_chars: usize,
    /// 请求超时（秒）。
    pub timeout_secs: u32,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            provider: TtsProvider::Volc,
            endpoint: VOLC_TTS_ENDPOINT.to_string(),
            resource_id: VOLC_TTS_RESOURCE_ID.to_string(),
            model: BAILIAN_TTS_MODEL.to_string(),
            api_key: String::new(),
            voice: VOLC_TTS_SPEAKER.to_string(),
            explicit_language: "zh-cn".to_string(),
            language_type: "Chinese".to_string(),
            target_sample_rate: FIRMWARE_SAMPLE_RATE,
            max_chars: BAILIAN_TTS_MAX_CHARS,
            timeout_secs: 30,
        }
    }
}

impl TtsConfig {
    /// 是否已具备发起请求的最小条件。
    pub fn is_configured(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    /// 实际使用的端点：字段留空时回落到该 provider 的官方端点。
    ///
    /// **只对"空"兜底，不对"填错"兜底。** 填了内容的端点一律照用——用户可能把它
    /// 挂在自己的代理或自建网关上；只有留空才说明"我没想法，用默认的"。
    ///
    /// 注：`resource_id` 刻意**不**做这种兜底，留空就是错（见 `synthesize_volc`）。
    /// 因为 1.0 / 2.0 决定的不只是音色可用范围，还决定计费商品条目——
    /// 这种选择不该由程序替用户猜。
    pub fn effective_endpoint(&self) -> &str {
        let v = self.endpoint.trim();
        if v.is_empty() {
            match self.provider {
                TtsProvider::Volc => VOLC_TTS_ENDPOINT,
                TtsProvider::Bailian => BAILIAN_TTS_ENDPOINT,
            }
        } else {
            v
        }
    }

    /// 这条 WS 端点走的是不是**双向流式**协议。
    ///
    /// # 为什么又是"端点的固有属性"
    ///
    /// 同一条 WS 上，单向流式与双向流式的**帧型不同**（第 5~8 字节：`sequence` vs `event`），
    /// 且双向必须先握手。发错的后果不是报错，而是**一帧都不回**（服务端静静等）——
    /// 也就是说用户看到的是"建连成功 + 超时"，那句提示完全指不到真正的原因。
    ///
    /// 既然端点路径已经把答案写在里面了（`…/tts/unidirectional/stream` vs
    /// `…/tts/bidirection`），就**不要**再让用户选一次：多一个下拉框
    /// = 多一个"选了 A 却填了 B 的端点"的错配机会，而那种错配的症状恰好最难查。
    ///
    /// 判据取**路径末段严格等于 `bidirection`**：
    /// - 查 query/fragment 前先切掉，`…/bidirection?x=1` 也能认；
    /// - 末段比对而不是 `contains`，`…/my-bidirection-proxy` 这类自建网关
    ///   只要照着协议转发就会被认（末段正是 `bidirection`），而
    ///   `…/bidirection/stream2` 这种就**不**会被误认成双向。
    pub fn is_bidirection(&self) -> bool {
        let raw = self.effective_endpoint().trim().to_ascii_lowercase();
        let path = raw.split(['?', '#']).next().unwrap_or("");
        path.trim_end_matches('/').rsplit('/').next() == Some("bidirection")
    }

    /// 走哪条传输。
    ///
    /// # 为什么由端点 scheme 决定，而不是加一个下拉框
    ///
    /// 传输方式是**端点的固有属性**：一个 `wss://` 的 URL 用 HTTP POST 去请求，
    /// 结果必然是失败；反过来也一样。既然端点已经唯一确定了传输，
    /// 再让用户去选一次，就只是多了一个"选了 HTTP 却填了 wss 端点"的错配机会 ——
    /// 而这种错配报出来的错（连接被拒 / 400）完全指不到真正的原因。
    ///
    /// 判 `ws://` 也一并认：本地网关与内网代理常用非 TLS 的 ws 调试。
    pub fn transport(&self) -> Transport {
        let e = self.effective_endpoint().trim().to_ascii_lowercase();
        if e.starts_with("wss://") || e.starts_with("ws://") {
            Transport::WebSocket
        } else {
            Transport::Http
        }
    }
}

/// 传输方式（由端点 scheme 推出，见 [`TtsConfig::transport`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    /// HTTP POST + chunked NDJSON 响应。
    Http,
    /// WebSocket 二进制帧。
    WebSocket,
}

// ---------------------------------------------------------------------------
// 错误（带统一错误码）
// ---------------------------------------------------------------------------

/// TTS 错误——带 `ErrorCode` 以便三个适配面返回同一个信封。
#[derive(Clone, Debug)]
pub enum TtsError {
    /// API Key 未填。
    NotConfigured(String),
    /// 入参不合法（空文本、超长、采样率不支持）。
    InvalidInput(String),
    /// 网络层失败。
    Network(String),
    /// 上游返回错误或格式不符。
    Provider(String),
    /// 音频解码失败（WAV 结构异常）。
    Decode(String),
    /// easyop 内部错误。
    ///
    /// 当前没有构造点（所有失败路径都能归到上面几类）。**保留**是为了让错误分类完整——
    /// `code()` / `message()` 的 match 需要覆盖它，且下发给链路的落盘/编码失败会用上。
    #[allow(dead_code)]
    Internal(String),
}

impl TtsError {
    /// 映射到统一错误码表。
    pub fn code(&self) -> ErrorCode {
        match self {
            TtsError::NotConfigured(_) => ErrorCode::NotConfigured,
            TtsError::InvalidInput(_) => ErrorCode::InvalidInput,
            TtsError::Network(_) => ErrorCode::NetworkError,
            TtsError::Provider(_) => ErrorCode::ProviderError,
            TtsError::Decode(_) => ErrorCode::ProviderError,
            TtsError::Internal(_) => ErrorCode::Internal,
        }
    }

    /// 错误消息。
    pub fn message(&self) -> String {
        match self {
            TtsError::NotConfigured(m)
            | TtsError::InvalidInput(m)
            | TtsError::Network(m)
            | TtsError::Provider(m)
            | TtsError::Decode(m)
            | TtsError::Internal(m) => m.clone(),
        }
    }
}

impl std::fmt::Display for TtsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

// ---------------------------------------------------------------------------
// 合成结果
// ---------------------------------------------------------------------------

/// 一次合成产物。
#[derive(Clone, Debug, Serialize)]
pub struct TtsAudio {
    /// 48 kHz 单声道 PCM（i16）。**下游直接用它，不要再假设别的采样率。**
    pub pcm: Vec<i16>,
    /// 实际采样率（恒等于 `target_sample_rate`）。
    pub sample_rate: u32,
    /// 时长（毫秒）。
    pub duration_ms: u64,
    /// 服务端计费的字符数（回读校验用）。
    pub characters: Option<u64>,
}

impl TtsAudio {
    /// 转成 16-bit 小端单写道的裸 PCM 字节流。
    ///
    /// **Phase 3 的音频下发链路会用它**（Opus 编码器的输入需要裸 PCM）。
    /// 现在尚无调用点——合成结果目前只走 `to_wav_bytes()` 落盘试听。
    #[allow(dead_code)]
    pub fn to_pcm_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pcm.len() * 2);
        for s in &self.pcm {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }

    /// 包成标准 WAV（仅调试/落盘试听用；下发链路不要用它）。
    pub fn to_wav_bytes(&self) -> Vec<u8> {
        wav_from_pcm16_mono(&self.pcm, self.sample_rate)
    }
}

// ---------------------------------------------------------------------------
// 客户端
// ---------------------------------------------------------------------------

struct TtsInner {
    /// 用 `Mutex` 包住是为了 `set_config` 能换掉超时（与 `LlmClient` 同形）。
    /// 取用时 **clone 后立刻释放锁**，避免跨 `await` 持有非 Send 的 guard。
    http: Mutex<reqwest::Client>,
    config: Mutex<TtsConfig>,
}

/// TTS 客户端（线程安全，可克隆）。与 `LlmClient` 同样是 `Arc<Inner>` 形状。
#[derive(Clone)]
pub struct TtsClient {
    inner: Arc<TtsInner>,
}

fn build_http_client(timeout_secs: u32) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.max(5) as u64))
        .pool_idle_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(4)
        .build()
        .expect("tts reqwest client build")
}

impl TtsClient {
    pub fn new(config: TtsConfig) -> Self {
        let http = build_http_client(config.timeout_secs);
        Self {
            inner: Arc::new(TtsInner {
                http: Mutex::new(http),
                config: Mutex::new(config),
            }),
        }
    }

    /// 当前配置。
    pub fn config(&self) -> TtsConfig {
        self.inner.config.lock().unwrap().clone()
    }

    /// 更新配置（并重建 HTTP 客户端以应用新超时）。
    pub fn set_config(&self, config: TtsConfig) {
        log_buffer::info(format!(
            "TTS 配置更新: provider={} model={} voice={} rate={} key_len={}",
            config.provider.label(),
            config.model,
            config.voice,
            config.target_sample_rate,
            config.api_key.len()
        ));
        let http = build_http_client(config.timeout_secs);
        *self.inner.http.lock().unwrap() = http;
        *self.inner.config.lock().unwrap() = config;
    }

    /// 合成：文本 → 48 kHz 单声道 PCM。
    pub async fn synthesize(&self, text: &str) -> Result<TtsAudio, TtsError> {
        let cfg = self.config();
        let text = text.trim();
        if text.is_empty() {
            return Err(TtsError::InvalidInput("待合成文本为空".to_string()));
        }
        if text.chars().count() > cfg.max_chars {
            return Err(TtsError::InvalidInput(format!(
                "文本 {} 字，超过上限 {} 字。摘要侧需控长。",
                text.chars().count(),
                cfg.max_chars
            )));
        }
        if !cfg.is_configured() {
            return Err(TtsError::NotConfigured(
                "TTS API Key 未配置，请在「设置 → Agent 任务台」中填写".to_string(),
            ));
        }
        if cfg.target_sample_rate != FIRMWARE_SAMPLE_RATE {
            // 不是"拒绝"，而是明确提示：改了采样率固件就播不出来。
            return Err(TtsError::InvalidInput(format!(
                "交付采样率 {} Hz 与固件要求 {} Hz 不一致。改这里必须同步改固件 \
                 speaker_audio_contract.h 的 kSpeakerPlaybackSampleRate 并重新烧录。",
                cfg.target_sample_rate, FIRMWARE_SAMPLE_RATE
            )));
        }

        match (cfg.provider, cfg.transport()) {
            // 豆包的两条 WS 路径**帧型不同**，由端点路径分流（见 `is_bidirection`）。
            (TtsProvider::Volc, Transport::WebSocket) => {
                if cfg.is_bidirection() {
                    self.synthesize_volc_bidirection_ws(&cfg, text).await
                } else {
                    self.synthesize_volc_ws(&cfg, text).await
                }
            }
            // 双向流式**只有 WebSocket**：填了 https 之后还得握手，HTTP 表达不了。
            // 这种错配必须当场说清 —— 拿 HTTP POST 去撞，回来的是 404/400，
            // 与"端点填错了"看不出因果关系。
            (TtsProvider::Volc, Transport::Http) if cfg.is_bidirection() => {
                Err(TtsError::InvalidInput(
                    "双向流式（…/tts/bidirection）只提供 WebSocket 端点，但当前端点是 http(s)://。\
                     把端点改成 wss://…/tts/bidirection；若要走 HTTP，请改用 \
                     https://…/api/v3/tts/unidirectional（单向流式）。"
                        .to_string(),
                ))
            }
            (TtsProvider::Volc, Transport::Http) => self.synthesize_volc(&cfg, text).await,
            (TtsProvider::Bailian, Transport::Http) => self.synthesize_bailian(&cfg, text).await,
            // 百炼只有 HTTP：填了 wss 端点说明配置串了，明确说清而不是拿 HTTP 去撞 wss。
            (TtsProvider::Bailian, Transport::WebSocket) => Err(TtsError::InvalidInput(
                "百炼（DashScope）没有 WebSocket 端点，但当前端点填的是 ws:// / wss://。\
                 两端导流方式不通用：要么把端点改回 https://dashscope.aliyuncs.com/…，\
                 要么把服务商切回豆包。"
                    .to_string(),
            )),
        }
    }

    /// 豆包两条传输共用的前置校验。
    ///
    /// # 为什么必须先拦，而不是让服务端报
    ///
    /// 这两个字段错都不是"格式错"，而是**版本搭配错**：
    /// - `resource_id` 留空：服务端直接 403（握手就断），日志里只有一句"连接失败"；
    /// - 音色与 resource 不同版本：服务端**建连成功**，然后回 `55000000`。
    ///
    /// 后者尤其容易看走眼 —— 连接是通的、HTTP 是 200 的，只有一行业务错。
    /// 在发请求之前拦住，报错里直接给出"哪个字段该填什么"，比让用户去翻译上游错误码快得多。
    fn volc_preflight(cfg: &TtsConfig) -> Result<(), TtsError> {
        if cfg.resource_id.trim().is_empty() {
            return Err(TtsError::NotConfigured(
                "豆包 TTS 缺少 X-Api-Resource-Id（应填 seed-tts-1.0 或 seed-tts-2.0）。\
                 音色与 resource 必须同版本，混用会返回 55000000。"
                    .to_string(),
            ));
        }
        if cfg.voice.trim().is_empty() {
            return Err(TtsError::InvalidInput(
                "豆包 TTS 的音色（speaker）为空。".to_string(),
            ));
        }
        Ok(())
    }

    /// 豆包语音合成大模型（火山引擎 v3 单向流式，HTTP chunked + NDJSON）。
    ///
    /// 交付 **48 kHz**，**不重采样**——直接向服务端要 48 kHz（实测支持），
    /// 省一次滤波，也省掉"重采样写错了导致音调异常"这一类难查的故障。
    async fn synthesize_volc(&self, cfg: &TtsConfig, text: &str) -> Result<TtsAudio, TtsError> {
        let start = std::time::Instant::now();

        Self::volc_preflight(cfg)?;

        let body = volc_request_body(cfg, text);

        log_buffer::info(format!(
            "豆包 TTS 请求: resource={} speaker={} rate={} text_len={} key_len={}",
            cfg.resource_id.trim(),
            cfg.voice.trim(),
            cfg.target_sample_rate,
            text.chars().count(),
            cfg.api_key.len()
        ));

        // guard 必须先释放，不能跨 await（与 synthesize_bailian 同因）。
        let http = self.inner.http.lock().unwrap().clone();
        let resp = http
            .post(cfg.effective_endpoint())
            .header("X-Api-Key", cfg.api_key.trim())
            .header("X-Api-Resource-Id", cfg.resource_id.trim())
            // 每次请求带唯一 ID：上游排障时能按它对齐一次具体调用。
            .header("X-Api-Request-Id", uuid::Uuid::new_v4().to_string())
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| TtsError::Network(format!("豆包 TTS 请求失败: {e}")))?;

        let status = resp.status();
        // X-Tt-Logid 是火山侧的排障命脉：出问题时报这个号，技术支持能直接定位。
        let logid = resp
            .headers()
            .get("X-Tt-Logid")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-")
            .to_string();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| TtsError::Network(format!("豆包 TTS 响应读取失败: {e}")))?;

        if !status.is_success() {
            let detail = extract_provider_error(&bytes);
            let msg = format!("豆包 TTS 返回 {status}（logid={logid}）：{detail}");
            log_buffer::err(msg.clone());
            return Err(TtsError::Provider(msg));
        }

        // 逐行解析 NDJSON。**HTTP 200 不等于成功**：音色与 resource 不匹配时
        // 上游返回的是 200 + {"code":55000000}，只看状态码会拿到一段空音频。
        let text_body = String::from_utf8_lossy(&bytes);
        let mut pcm: Vec<i16> = Vec::new();
        let mut done = false;
        for line in text_body.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(line).map_err(|e| {
                TtsError::Provider(format!(
                    "豆包 TTS 响应不是合法 NDJSON：{e}（logid={logid}）"
                ))
            })?;

            // 缺 code 视为 0：实测正常分片都带 code，这里只是不让格式微调把功能打死。
            let code = v.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
            if code == VOLC_CODE_DONE {
                done = true;
                continue;
            }
            if code != 0 {
                let msg = v
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("(无 message)");
                let detail = format!("豆包 TTS 业务错误 code={code} message={msg}（logid={logid}）");
                log_buffer::err(detail.clone());
                return Err(TtsError::Provider(detail));
            }

            if let Some(d) = v.get("data").and_then(|d| d.as_str()) {
                if d.is_empty() {
                    continue;
                }
                let chunk = decode_b64_fragment(d).map_err(|e| {
                    TtsError::Decode(format!("豆包 TTS 音频片段 base64 解码失败：{e}"))
                })?;
                pcm.reserve(chunk.len() / 2);
                for c in chunk.chunks_exact(2) {
                    pcm.push(i16::from_le_bytes([c[0], c[1]]));
                }
            }
        }

        if !done {
            // 没收到结束哨兵 = 流被截断。**绝不能当成功**：宁可报错，
            // 也不要让用户听到半句话却以为"总结就这么多"。
            return Err(TtsError::Provider(format!(
                "豆包 TTS 响应流未正常结束（缺结束哨兵 code={VOLC_CODE_DONE}，logid={logid}）"
            )));
        }
        if pcm.is_empty() {
            return Err(TtsError::Provider(format!(
                "豆包 TTS 返回 0 个采样（logid={logid}）。常见原因：音色与 resource_id 版本不匹配。"
            )));
        }

        let duration_ms = (pcm.len() as u64 * 1000) / cfg.target_sample_rate as u64;
        if duration_ms > ADVISORY_MAX_PLAYBACK_MS {
            log_buffer::warn(format!(
                "豆包 TTS 合成 {duration_ms} ms，超过课程的 {} 秒播放上限（总结侧应控长）",
                ADVISORY_MAX_PLAYBACK_MS / 1000
            ));
        }

        log_buffer::info(format!(
            "豆包 TTS 完成: {} Hz {} 采样，时长 {} ms，耗时 {} ms，logid={logid}",
            cfg.target_sample_rate,
            pcm.len(),
            duration_ms,
            start.elapsed().as_millis()
        ));

        Ok(TtsAudio {
            pcm,
            sample_rate: cfg.target_sample_rate,
            duration_ms,
            // 豆包的流式响应不带计费字段（实测分片只有 code/message/data/sentence）。
            characters: None,
        })
    }

    /// 豆包语音合成大模型（火山引擎 v3 单向流式，**WebSocket 二进制帧**）。
    ///
    /// 与 [`Self::synthesize_volc`] 是**同一个服务、同一段请求 JSON**，区别只在
    /// "这段 JSON 怎么送出去、音频怎么收回来"：HTTP 那边是一次 POST 读一整个 chunked 流，
    /// 这边是发一帧二进制、然后一帧一帧收音频。协议细节见 `parse_server_frame`。
    ///
    /// 交付同样固定 48 kHz：采样率是在**请求体里**要的，与传输无关，
    /// 所以两条路都零重采样（这是"同配置双传输"能成立的关键，不是巧合）。
    async fn synthesize_volc_ws(&self, cfg: &TtsConfig, text: &str) -> Result<TtsAudio, TtsError> {
        let start = Instant::now();
        Self::volc_preflight(cfg)?;

        let url = cfg.effective_endpoint().trim().to_string();
        let body = volc_request_body(cfg, text);
        let json = serde_json::to_vec(&body)
            .map_err(|e| TtsError::Internal(format!("序列化 TTS 请求体失败：{e}")))?;

        log_buffer::info(format!(
            "豆包 TTS(ws) 请求: url={url} resource={} speaker={} rate={} text_len={} key_len={}",
            cfg.resource_id.trim(),
            cfg.voice.trim(),
            cfg.target_sample_rate,
            text.chars().count(),
            cfg.api_key.len()
        ));

        // 1. 建连（与双向流式路径共用同一个函数 —— 见 `volc_ws_connect`）。
        let (ws, logid, _request_id) =
            volc_ws_connect(cfg, &url, "豆包 TTS(ws)", &[], start).await?;

        let (mut sink, mut stream) = ws.split();

        // 2. 发首帧（整段文本一次给完 —— 这正是"单向流式"的含义，不需要再发结束包）。
        tokio::time::timeout(
            WS_SEND_TIMEOUT,
            sink.send(Message::Binary(build_client_frame(Some(1), &json))),
        )
        .await
        .map_err(|_| {
            TtsError::Network(format!(
                "豆包 TTS(ws) 发送请求超时（{} 秒）",
                WS_SEND_TIMEOUT.as_secs()
            ))
        })?
        .map_err(|e| TtsError::Network(format!("豆包 TTS(ws) 发送请求失败：{e}")))?;

        // 3. 收音频。**必须有总截止时间**，理由与 asr.rs 的「防砖超时」完全相同：
        //    对端可以在不断开连接的情况下一直不推帧，`stream.next()` 就会挂到进程结束，
        //    而调用方（任务台命令）会因此永远不返回。
        let deadline = Instant::now() + Duration::from_secs(cfg.timeout_secs.max(5) as u64);
        let mut pcm: Vec<i16> = Vec::new();
        let mut frames = 0usize;
        let mut sentences = 0usize;
        let mut finished = false;

        while !finished {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(TtsError::Network(format!(
                    "豆包 TTS(ws) 超过总超时 {} 秒仍未收完（已收 {frames} 帧 / {} 采样，logid={logid}）",
                    cfg.timeout_secs,
                    pcm.len()
                )));
            }
            let wait = remaining.min(WS_RECV_IDLE_TIMEOUT);

            let msg = match tokio::time::timeout(wait, stream.next()).await {
                Err(_) => {
                    return Err(TtsError::Network(format!(
                        "豆包 TTS(ws) 等待音频超时（{} 秒无新帧，已收 {frames} 帧 / {} 采样，logid={logid}）",
                        wait.as_secs(),
                        pcm.len()
                    )));
                }
                Ok(None) => {
                    // 没有结束事件就断开 = 流被截断。**绝不能当成功**：
                    // 用户会听到半句话却以为"总结就这么多"。
                    return Err(TtsError::Provider(format!(
                        "豆包 TTS(ws) 连接在收到结束事件前被对端关闭（已收 {frames} 帧 / {} 采样，logid={logid}）",
                        pcm.len()
                    )));
                }
                Ok(Some(Err(e))) => {
                    return Err(TtsError::Network(format!("豆包 TTS(ws) 接收失败：{e}")))
                }
                Ok(Some(Ok(m))) => m,
            };

            let data = match msg {
                Message::Binary(b) => b,
                // 该端点只回二进制帧；文本帧不该出现（实测无）。忽略而不是报错，
                // 因为它不影响音频，且报错会把"音频已经拿到"的局面搅掉。
                Message::Text(_) => continue,
                // 对端主动关：当作流已结束，由下面的 `finished` 判定把关（没收到事件就是失败）。
                Message::Close(_) => break,
                _ => continue,
            };
            frames += 1;

            let frame = parse_server_frame(&data).map_err(|e| {
                TtsError::Decode(format!("豆包 TTS(ws) 回帧解析失败：{e}（logid={logid}）"))
            })?;

            if frame.message_type == MT_ERROR {
                let code = frame.code.unwrap_or(0);
                let raw = String::from_utf8_lossy(&frame.payload).to_string();
                let msg = format!("{}（logid={logid}）", explain_volc_code(code, &raw));
                log_buffer::err(msg.clone());
                return Err(TtsError::Provider(msg));
            }

            if frame.message_type == MT_AUDIO_ONLY_SERVER_RESPONSE {
                pcm.reserve(frame.payload.len() / 2);
                for c in frame.payload.chunks_exact(2) {
                    pcm.push(i16::from_le_bytes([c[0], c[1]]));
                }
            }

            if frame.message_type == MT_FULL_SERVER_RESPONSE {
                // 纵深防御：错误也可能塞在"正常应答"的 JSON 里 —— HTTP 路径实测就有这种形状
                // （HTTP 200 + code=55000000）。这里同样不放过，免得被当成空音频交付。
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&frame.payload) {
                    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
                        let msg = format!("豆包 TTS(ws) 业务错误：{err}（logid={logid}）");
                        log_buffer::err(msg.clone());
                        return Err(TtsError::Provider(msg));
                    }
                }
            }

            match frame.event {
                Some(ev) if ev == EV_SESSION_STARTED => {
                    log_buffer::info("豆包 TTS(ws) 会话已建立（event 350），服务端开始推流");
                }
                Some(ev) if ev == EV_TTS_SENTENCE_START => sentences += 1,
                Some(ev) if ev == EV_TTS_SESSION_FINISHED => {
                    // 服务端在收尾帧里回带**它收到的全文**：拿它与请求文本对一下字数，
                    // 是"文本有没有被截断/编码错"最便宜的一次交叉验证（不需要额外请求）。
                    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&frame.payload) {
                        if let Some(t) = v.get("text").and_then(|x| x.as_str()) {
                            log_buffer::info(format!(
                                "豆包 TTS(ws) 服务端回读文本 {} 字（请求 {} 字）",
                                t.chars().count(),
                                text.chars().count()
                            ));
                        }
                    }
                    finished = true;
                }
                Some(ev) if ev == EV_SESSION_FINISHED => finished = true,
                _ => {}
            }
        }

        let _ = sink.close().await;

        if !finished {
            return Err(TtsError::Provider(format!(
                "豆包 TTS(ws) 响应流未正常结束（缺结束事件 {EV_TTS_SESSION_FINISHED}/{EV_SESSION_FINISHED}，\
                 已收 {frames} 帧 / {} 采样，logid={logid}）",
                pcm.len()
            )));
        }
        if pcm.is_empty() {
            return Err(TtsError::Provider(format!(
                "豆包 TTS(ws) 返回 0 个采样（logid={logid}）。常见原因：音色与 resource_id 版本不匹配。"
            )));
        }

        let duration_ms = (pcm.len() as u64 * 1000) / cfg.target_sample_rate as u64;
        if duration_ms > ADVISORY_MAX_PLAYBACK_MS {
            log_buffer::warn(format!(
                "豆包 TTS(ws) 合成 {duration_ms} ms，超过课程的 {} 秒播放上限（总结侧应控长）",
                ADVISORY_MAX_PLAYBACK_MS / 1000
            ));
        }
        log_buffer::info(format!(
            "豆包 TTS(ws) 完成: {} Hz {} 采样，时长 {} ms，{} 帧 / {} 句，耗时 {} ms，logid={logid}",
            cfg.target_sample_rate,
            pcm.len(),
            duration_ms,
            frames,
            sentences,
            start.elapsed().as_millis()
        ));

        Ok(TtsAudio {
            pcm,
            sample_rate: cfg.target_sample_rate,
            duration_ms,
            // 与 HTTP 路径同理：该端点不回带计费字段。
            characters: None,
        })
    }

    /// 豆包语音合成大模型（火山引擎 v3 **双向流式**，端点路径末段为 `bidirection`）。
    ///
    /// 与 [`Self::synthesize_volc_ws`] 是**同一个服务、同一份请求参数**，
    /// 差别全在"怎么把话送进去"：
    ///
    /// | | 单向流式 | 双向流式 |
    /// |---|---|---|
    /// | 上行帧第 5~8 字节 | `sequence` | **`event`** |
    /// | 交互 | 一帧把整段 JSON 发完 | 握手 → 建会话 → `TaskRequest` 喂文本 → 收口 |
    /// | 文本放哪 | 那唯一一帧 | **`TaskRequest`**（服务参数在 `StartSession`） |
    ///
    /// # 它对 easyop 的价值，以及现在还没拿到的部分
    ///
    /// 双向流式的卖点是**首包更早**：文本可以边产生边喂，服务端不必等全文到齐才开口。
    /// 本函数现在仍**一次把整段文本发出去**（任务台的播报文本本来就是一次成型的，
    /// 拆帧只会多一处"拆错句"的风险），所以现在拿到的收益是这条链路本身可用；
    /// 真正吃到首包优势要等"把 LLM 的流式输出直接接进来"——
    /// 那时只需把下面那一次 `TaskRequest` 换成一个循环，**帧格式与收尾逻辑都不用再动**。
    ///
    /// # 收尾判据特意与单向流式不同
    ///
    /// 单向那条路是"`351` 或 `152` 任一出现即停"。这条**只认 `152`（SessionFinished）**：
    /// `351` 是 `TTSSentenceEnd`，在 TTS1.0 音色下是**每句各来一次**的，
    /// 拿它收尾会把后面几句的音频整段丢掉 —— 而且丢得毫无提示（前几句正常、后几句没了）。
    /// 官方文档只在 `SessionFinished` 上写了"必须收到之后再开下一个 session"，
    /// 那是唯一无歧义的终点。`351` 在这条路上继续用来做"回读全文"的交叉验证。
    ///
    /// 交付物与错误语义与另一条路径完全一致：同样固定 48 kHz、同样"缺结束事件算失败"、
    /// 同样"0 采样算失败"。调用方（任务台播报）不需要知道自己走的是哪条路。
    async fn synthesize_volc_bidirection_ws(
        &self,
        cfg: &TtsConfig,
        text: &str,
    ) -> Result<TtsAudio, TtsError> {
        let start = Instant::now();
        Self::volc_preflight(cfg)?;

        let url = cfg.effective_endpoint().trim().to_string();
        log_buffer::info(format!(
            "豆包 TTS(bidir) 请求: url={url} resource={} speaker={} rate={} text_len={} key_len={}",
            cfg.resource_id.trim(),
            cfg.voice.trim(),
            cfg.target_sample_rate,
            text.chars().count(),
            cfg.api_key.len()
        ));

        // `X-Api-Connect-Id` 每连接唯一、**不可复用**（官方文档 §6）；
        // `session_id` 由客户端生成 —— 实测服务端不下发只回显，且**空串必被拒**。
        let connect_id = uuid::Uuid::new_v4().to_string();
        let session_id = uuid::Uuid::new_v4().to_string();

        let (ws, logid, _request_id) = volc_ws_connect(
            cfg,
            &url,
            "豆包 TTS(bidir)",
            &[("X-Api-Connect-Id", connect_id.clone())],
            start,
        )
        .await?;
        let (mut sink, mut stream) = ws.split();

        let to_t = |e: String| TtsError::Internal(format!("构造双向流式帧失败：{e}"));

        // ---- 1) StartConnection → ConnectionStarted(50) ----
        bidir_send(
            &mut sink,
            build_bidirection_frame(EV_START_CONNECTION, FrameScope::Connection, b"{}")
                .map_err(to_t)?,
            &logid,
            "StartConnection",
        )
        .await?;
        let f = bidir_recv(&mut stream, WS_RECV_IDLE_TIMEOUT, &logid).await?;
        bidir_expect(&f, EV_CONNECTION_STARTED, "ConnectionStarted", &logid)?;
        log_buffer::info(format!(
            "豆包 TTS(bidir) 连接已建立（event 50），connection_id={}",
            f.session_id.as_deref().unwrap_or("-")
        ));

        // ---- 2) StartSession → SessionStarted(150)。**服务端参数在这一帧生效。** ----
        let mut req_params = serde_json::json!({
            "speaker": cfg.voice.trim(),
            "audio_params": {
                "format": "pcm",
                "sample_rate": cfg.target_sample_rate,
            }
        });
        if !cfg.explicit_language.trim().is_empty() {
            // ⚠️ `additions` 必须是**字符串**（内容是 JSON 文本）—— 与单向流式同一个坑，
            //    传嵌套对象会被服务端 Go 结构体拒绝，且裹在 `200 + 55000000` 里。
            req_params["additions"] = serde_json::Value::String(
                serde_json::json!({ "explicit_language": cfg.explicit_language.trim() }).to_string(),
            );
        }
        // 文本**只放 TaskRequest**（官方文档：服务参数在 StartSession 生效、文本在
        // TaskRequest 生效）。实测两边都放也能跑，但只在一边放能少一个"会不会被合成两遍"的疑问。
        let session_body = serde_json::json!({
            "event": EV_START_SESSION,
            "namespace": "BidirectionalTTS",
            "req_params": req_params,
        });
        let raw = serde_json::to_vec(&session_body)
            .map_err(|e| TtsError::Internal(format!("序列化 TTS 会话参数失败：{e}")))?;
        bidir_send(
            &mut sink,
            build_bidirection_frame(EV_START_SESSION, FrameScope::Session(&session_id), &raw)
                .map_err(to_t)?,
            &logid,
            "StartSession",
        )
        .await?;
        let f = bidir_recv(&mut stream, WS_RECV_IDLE_TIMEOUT, &logid).await?;
        bidir_expect(&f, EV_SESSION_STARTED_BIDIR, "SessionStarted", &logid)?;
        log_buffer::info(format!(
            "豆包 TTS(bidir) 会话已开始（event 150），session_id={}",
            f.session_id.as_deref().unwrap_or("-")
        ));

        // ---- 3) TaskRequest：**文本在这一帧生效** ----
        let task_body = serde_json::json!({
            "event": EV_TASK_REQUEST,
            "req_params": { "text": text },
        });
        let raw = serde_json::to_vec(&task_body)
            .map_err(|e| TtsError::Internal(format!("序列化 TTS 请求体失败：{e}")))?;
        bidir_send(
            &mut sink,
            build_bidirection_frame(EV_TASK_REQUEST, FrameScope::Session(&session_id), &raw)
                .map_err(to_t)?,
            &logid,
            "TaskRequest",
        )
        .await?;

        // ---- 4) FinishSession：告诉服务端"文本给完了"，它才会收口回 152 ----
        bidir_send(
            &mut sink,
            build_bidirection_frame(EV_FINISH_SESSION, FrameScope::Session(&session_id), b"{}")
                .map_err(to_t)?,
            &logid,
            "FinishSession",
        )
        .await?;

        // ---- 5) 收音频直到 SessionFinished(152) ----
        // 总截止时间与"单帧空闲超时"两道都要有：对端可以不断开也不推帧，
        // 只靠其中一道，另一道场景下调用方会被永久挂住。
        let deadline = Instant::now() + Duration::from_secs(cfg.timeout_secs.max(5) as u64);
        let mut pcm: Vec<i16> = Vec::new();
        let mut frames = 0usize;
        let mut sentences = 0usize;
        let mut finished = false;

        while !finished {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(TtsError::Network(format!(
                    "豆包 TTS(bidir) 超过总超时 {} 秒仍未收到 SessionFinished（已收 {frames} 帧 / \
                     {} 采样，logid={logid}）",
                    cfg.timeout_secs,
                    pcm.len()
                )));
            }
            let f = bidir_recv(&mut stream, remaining.min(WS_RECV_IDLE_TIMEOUT), &logid).await?;
            frames += 1;

            if f.message_type == MT_ERROR {
                let code = f.code.unwrap_or(0);
                let raw = String::from_utf8_lossy(&f.payload).to_string();
                let msg = format!("{}（logid={logid}）", explain_volc_code(code, &raw));
                log_buffer::err(msg.clone());
                return Err(TtsError::Provider(msg));
            }

            if f.message_type == MT_AUDIO_ONLY_SERVER_RESPONSE {
                pcm.reserve(f.payload.len() / 2);
                for c in f.payload.chunks_exact(2) {
                    pcm.push(i16::from_le_bytes([c[0], c[1]]));
                }
            } else if f.message_type == MT_FULL_SERVER_RESPONSE {
                // 同一个"服务端整包应答"位置上，正常时是文本/音素 JSON、异常时可能塞错误。
                // 后者不放过，免得被当成"这一帧没音频"而静默略过。
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&f.payload) {
                    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
                        let msg = format!("豆包 TTS(bidir) 业务错误：{err}（logid={logid}）");
                        log_buffer::err(msg.clone());
                        return Err(TtsError::Provider(msg));
                    }
                }
                // ⚠️ 实测音频只走 `mt=0b1011`（裸 PCM，serialization=RAW）。
                // 若日后挂到整包应答上（载荷混合），**不能当成 JSON 硬解后再瞎猜** ——
                // 那属于"读错位仍能凑出可交付数据"的高危形状，所以这里明确留一条带十六进制的线。
                if f.event == Some(EV_TTS_SENTENCE_START) {
                    log_buffer::warn(format!(
                        "豆包 TTS(bidir) 收到整包应答形式的音频帧（event 352，mt=0b1001），\
                         当前不解析其载荷 —— 若音频缺失请带这条日志排查（logid={logid}，\
                         head={}）",
                        hex_head(&f.payload)
                    ));
                }
            }

            match f.event {
                Some(ev) if ev == EV_TTS_SENTENCE_START => sentences += 1,
                // 351 在这条路上**不是终点**（TTS1.0 每句都会来一次），只用来回读全文。
                Some(ev) if ev == EV_TTS_SESSION_FINISHED => {
                    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&f.payload) {
                        if let Some(t) = v.get("text").and_then(|x| x.as_str()) {
                            log_buffer::info(format!(
                                "豆包 TTS(bidir) 服务端回读文本 {} 字（请求 {} 字）",
                                t.chars().count(),
                                text.chars().count()
                            ));
                        }
                    }
                }
                Some(EV_SESSION_FAILED) => {
                    return Err(TtsError::Provider(format!(
                        "豆包 TTS(bidir) 会话失败（SessionFailed，logid={logid}）：{}",
                        String::from_utf8_lossy(&f.payload)
                    )))
                }
                Some(ev) if ev == EV_CONNECTION_FINISHED => {
                    log_buffer::info(format!("豆包 TTS(bidir) 连接已结束（ConnectionFinished，event 52）"));
                }
                Some(ev) if ev == EV_SESSION_FINISHED => finished = true,
                _ => {}
            }
        }

        // ---- 6) FinishConnection：收尾。失败**不影响音频**，所以只记不报。 ----
        match build_bidirection_frame(EV_FINISH_CONNECTION, FrameScope::Connection, b"{}") {
            Ok(frame) => {
                if let Err(e) = bidir_send(&mut sink, frame, &logid, "FinishConnection").await {
                    log_buffer::warn(format!("豆包 TTS(bidir) 发送 FinishConnection 失败（忽略）：{e}"));
                } else {
                    match bidir_recv(&mut stream, WS_RECV_IDLE_TIMEOUT, &logid).await {
                        Ok(f) => log_buffer::info(format!(
                            "豆包 TTS(bidir) 连接已结束（event {:?}）",
                            f.event
                        )),
                        Err(e) => log_buffer::warn(format!(
                            "豆包 TTS(bidir) 未收到 ConnectionFinished（忽略）：{e}"
                        )),
                    }
                }
            }
            Err(e) => log_buffer::warn(format!("豆包 TTS(bidir) 构造 FinishConnection 失败：{e}")),
        }
        let _ = sink.close().await;

        if !finished {
            return Err(TtsError::Provider(format!(
                "豆包 TTS(bidir) 响应流未正常结束（缺结束事件 {EV_SESSION_FINISHED}/SessionFinished，\
                 已收 {frames} 帧 / {} 采样，logid={logid}）",
                pcm.len()
            )));
        }
        if pcm.is_empty() {
            return Err(TtsError::Provider(format!(
                "豆包 TTS(bidir) 返回 0 个采样（logid={logid}）。常见原因：音色与 resource_id 版本不匹配。"
            )));
        }

        let duration_ms = (pcm.len() as u64 * 1000) / cfg.target_sample_rate as u64;
        if duration_ms > ADVISORY_MAX_PLAYBACK_MS {
            log_buffer::warn(format!(
                "豆包 TTS(bidir) 合成 {duration_ms} ms，超过课程的 {} 秒播放上限（总结侧应控长）",
                ADVISORY_MAX_PLAYBACK_MS / 1000
            ));
        }
        log_buffer::info(format!(
            "豆包 TTS(bidir) 完成: {} Hz {} 采样，时长 {} ms，{} 帧 / {} 句，耗时 {} ms，logid={logid}",
            cfg.target_sample_rate,
            pcm.len(),
            duration_ms,
            frames,
            sentences,
            start.elapsed().as_millis()
        ));

        Ok(TtsAudio {
            pcm,
            sample_rate: cfg.target_sample_rate,
            duration_ms,
            // 该端点不回带计费字段（实测分片只有 event/句子/音频）。
            characters: None,
        })
    }

    /// 百炼 DashScope 原生 TTS。
    async fn synthesize_bailian(&self, cfg: &TtsConfig, text: &str) -> Result<TtsAudio, TtsError> {
        let start = std::time::Instant::now();
        let body = serde_json::json!({
            "model": cfg.model,
            "input": {
                "text": text,
                "voice": cfg.voice,
                "language_type": cfg.language_type,
            }
        });

        log_buffer::info(format!(
            "TTS 请求: model={} voice={} text_len={} key_len={}",
            cfg.model,
            cfg.voice,
            text.chars().count(),
            cfg.api_key.len()
        ));

        // clone Client（内部 Arc，开销极低）后**立即释放 MutexGuard**。
        // 写成链式 `self.inner.http.lock().unwrap().post(..).send().await` 会让
        // guard 的临时值活到整条语句结束 —— 即跨越 await，future 立刻不再是 Send，
        // 编译期报 "future cannot be sent between threads safely"。
        let http = self.inner.http.lock().unwrap().clone();
        let resp = http
            .post(cfg.effective_endpoint())
            .header("Authorization", format!("Bearer {}", cfg.api_key.trim()))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| TtsError::Network(format!("TTS 请求失败: {e}")))?;

        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| TtsError::Network(format!("TTS 响应读取失败: {e}")))?;

        if !status.is_success() {
            let text = String::from_utf8_lossy(&bytes);
            let msg = format!("TTS 返回错误 {status}: {text}");
            log_buffer::err(msg.clone());
            return Err(TtsError::Provider(msg));
        }

        let v: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| TtsError::Provider(format!("TTS 响应不是合法 JSON: {e}")))?;

        // 上游若返回 code/message，先透出来——比"字段缺失"有用得多。
        if let Some(code) = v.get("code").and_then(|c| c.as_str()) {
            let msg = v
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("(无 message)");
            let detail = format!("TTS 业务错误 code={code} message={msg}");
            log_buffer::err(detail.clone());
            return Err(TtsError::Provider(detail));
        }

        let audio_url = v
            .get("output")
            .and_then(|o| o.get("audio"))
            .and_then(|a| a.get("url"))
            .and_then(|u| u.as_str())
            .ok_or_else(|| {
                TtsError::Provider(format!(
                    "TTS 响应缺少 output.audio.url，原始响应前 400 字：{}",
                    truncate(&String::from_utf8_lossy(&bytes), 400)
                ))
            })?;

        let characters = v
            .get("usage")
            .and_then(|u| u.get("characters"))
            .and_then(|c| c.as_u64());

        // 音频 URL 24 小时有效，需二次下载。
        let audio_bytes = self.download(audio_url).await?;
        let (pcm_src, src_rate) = parse_wav_pcm16_mono(&audio_bytes)?;
        let pcm = resample_linear(&pcm_src, src_rate, cfg.target_sample_rate);
        let duration_ms = (pcm.len() as u64 * 1000) / cfg.target_sample_rate as u64;

        log_buffer::info(format!(
            "TTS 完成: 源 {} Hz {} 采样 → 交付 {} Hz {} 采样，时长 {} ms，耗时 {} ms，计费字符 {}",
            src_rate,
            pcm_src.len(),
            cfg.target_sample_rate,
            pcm.len(),
            duration_ms,
            start.elapsed().as_millis(),
            characters.map(|c| c.to_string()).unwrap_or_else(|| "?".into())
        ));

        Ok(TtsAudio {
            pcm,
            sample_rate: cfg.target_sample_rate,
            duration_ms,
            characters,
        })
    }

    /// 下载音频 URL（24h 有效）。
    async fn download(&self, url: &str) -> Result<Vec<u8>, TtsError> {
        // 同上：guard 必须先释放，不能跨 await。
        let http = self.inner.http.lock().unwrap().clone();
        let resp = http
            .get(url)
            .send()
            .await
            .map_err(|e| TtsError::Network(format!("音频下载失败: {e}")))?;
        if !resp.status().is_success() {
            return Err(TtsError::Network(format!(
                "音频下载失败 status={}",
                resp.status()
            )));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| TtsError::Network(format!("音频读取失败: {e}")))?;
        Ok(bytes.to_vec())
    }
}

/// 截断字符串用于日志/错误消息（按字符，不切断 UTF-8）。
fn truncate(s: &str, max_chars: usize) -> String {
    let mut out: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        out.push('…');
    }
    out
}

// ---------------------------------------------------------------------------
// WAV 解析 / 生成（零依赖手写，避开新依赖）
// ---------------------------------------------------------------------------

/// 解析 WAV，取出 16-bit 单声道 PCM 与采样率。
///
/// 按 chunk **遍历**而不是硬编码 44 字节头：百炼返回的 wav 可能带 `LIST`/`fact`
/// 等附加块，写死偏移就会把元数据当音频。
pub fn parse_wav_pcm16_mono(wav: &[u8]) -> Result<(Vec<i16>, u32), TtsError> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err(TtsError::Decode(format!(
            "不是合法 WAV（前 12 字节：{:02X?}）",
            &wav[..wav.len().min(12)]
        )));
    }

    let mut pos = 12usize;
    let mut sample_rate = 0u32;
    let mut channels = 0u16;
    let mut bits = 0u16;
    let mut data: Option<&[u8]> = None;

    while pos + 8 <= wav.len() {
        let id = &wav[pos..pos + 4];
        let size = u32::from_le_bytes([wav[pos + 4], wav[pos + 5], wav[pos + 6], wav[pos + 7]])
            as usize;
        let body_start = pos + 8;
        let body_end = body_start.saturating_add(size).min(wav.len());

        if id == b"fmt " && size >= 16 {
            let b = &wav[body_start..body_end];
            channels = u16::from_le_bytes([b[2], b[3]]);
            sample_rate = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
            bits = u16::from_le_bytes([b[14], b[15]]);
        } else if id == b"data" {
            data = Some(&wav[body_start..body_end]);
        }

        // RIFF 块按偶数字节对齐。
        pos = body_start + size + (size & 1);
    }

    let data = data.ok_or_else(|| TtsError::Decode("WAV 缺少 data 块".to_string()))?;
    if channels != 1 {
        return Err(TtsError::Decode(format!(
            "WAV 为 {channels} 声道，本模块只接受单声道"
        )));
    }
    if bits != 16 {
        return Err(TtsError::Decode(format!(
            "WAV 位深 {bits}，本模块只接受 16-bit"
        )));
    }
    if sample_rate == 0 {
        return Err(TtsError::Decode("WAV 采样率为 0".to_string()));
    }

    let pcm: Vec<i16> = data
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    Ok((pcm, sample_rate))
}

/// 线性插值重采样。
///
/// 本用例是 24k → 48k 的整数倍上采样，线性插值足够（语音带宽远低于 12 kHz 的
/// 奈奎斯特上限，且下游是 8 Ω 小喇叭，不是听感苛刻的场景）。**不做 sinc 插值**：
/// 收益在喇叭上听不出来，代价是每帧一次长卷积。
pub fn resample_linear(input: &[i16], from: u32, to: u32) -> Vec<i16> {
    if from == to || input.is_empty() || from == 0 || to == 0 {
        return input.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    let last = *input.last().unwrap_or(&0);
    for i in 0..out_len {
        let src = i as f64 / ratio;
        let i0 = src.floor() as usize;
        let frac = src - i0 as f64;
        let a = *input.get(i0).unwrap_or(&0) as f64;
        let b = *input.get(i0 + 1).unwrap_or(&last) as f64;
        let v = a + (b - a) * frac;
        out.push(v.round().clamp(-32768.0, 32767.0) as i16);
    }
    out
}

/// 把 16-bit 单声道 PCM 包成 WAV。
pub fn wav_from_pcm16_mono(pcm: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = (pcm.len() * 2) as u32;
    let byte_rate = sample_rate * 2; // 单声道 × 16 bit
    let mut out = Vec::with_capacity(44 + data_len as usize);

    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_firmware_rate() {
        let cfg = TtsConfig::default();
        assert_eq!(cfg.target_sample_rate, FIRMWARE_SAMPLE_RATE);
        assert!(!cfg.is_configured(), "默认 key 必须为空——由用户填");
    }

    #[test]
    fn resample_doubles_length() {
        let input: Vec<i16> = (0..100).map(|i| (i * 100) as i16).collect();
        let out = resample_linear(&input, 24_000, 48_000);
        assert_eq!(out.len(), 200);
        assert_eq!(out[0], 0);
        // 线性插值：第 2n 个样本等于原第 n 个。
        for n in 0..100 {
            assert!((out[2 * n] as i32 - input[n] as i32).abs() <= 1);
        }
    }

    #[test]
    fn resample_identity_when_rates_equal() {
        let input: Vec<i16> = vec![1, 2, 3];
        assert_eq!(resample_linear(&input, 48_000, 48_000), input);
    }

    #[test]
    fn resample_handles_empty() {
        assert!(resample_linear(&[], 24_000, 48_000).is_empty());
    }

    #[test]
    fn wav_roundtrip() {
        let pcm: Vec<i16> = vec![0, 100, -100, 32767, -32768];
        let wav = wav_from_pcm16_mono(&pcm, 48_000);
        let (back, rate) = parse_wav_pcm16_mono(&wav).unwrap();
        assert_eq!(rate, 48_000);
        assert_eq!(back, pcm);
    }

    #[test]
    fn wav_parser_skips_extra_chunks() {
        // 造一个带 LIST 附加块的 wav：硬编码 44 字节偏移的实现会在这里失败。
        let pcm: Vec<i16> = vec![1, 2, 3, 4];
        let mut wav = wav_from_pcm16_mono(&pcm, 24_000);
        let list = b"LIST\x04\x00\x00\x00INFO";
        // 插到 data 块之前
        let data_pos = wav.windows(4).position(|w| w == b"data").unwrap();
        for (i, b) in list.iter().enumerate() {
            wav.insert(data_pos + i, *b);
        }
        let riff_size = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&riff_size.to_le_bytes());

        let (back, rate) = parse_wav_pcm16_mono(&wav).unwrap();
        assert_eq!(rate, 24_000);
        assert_eq!(back, pcm);
    }

    #[test]
    fn reject_non_wav() {
        assert!(parse_wav_pcm16_mono(b"not a wav at all").is_err());
    }

    #[test]
    fn reject_multichannel() {
        let mut wav = wav_from_pcm16_mono(&[1, 2], 48_000);
        wav[22] = 2; // channels = 2
        let err = parse_wav_pcm16_mono(&wav).unwrap_err();
        assert!(matches!(err, TtsError::Decode(_)));
    }

    #[test]
    fn error_codes_map_to_capability_table() {
        assert_eq!(
            TtsError::NotConfigured("x".into()).code(),
            ErrorCode::NotConfigured
        );
        assert_eq!(
            TtsError::Network("x".into()).code(),
            ErrorCode::NetworkError
        );
        assert_eq!(
            TtsError::Provider("x".into()).code(),
            ErrorCode::ProviderError
        );
    }

    #[tokio::test]
    async fn unconfigured_key_fails_loudly() {
        let client = TtsClient::new(TtsConfig::default());
        let err = client.synthesize("测试").await.unwrap_err();
        // 失败不静默：没填 key 必须报 NotConfigured，不许返回空音频。
        assert_eq!(err.code(), ErrorCode::NotConfigured);
    }

    #[tokio::test]
    async fn empty_text_rejected() {
        let client = TtsClient::new(TtsConfig::default());
        let err = client.synthesize("   ").await.unwrap_err();
        assert_eq!(err.code(), ErrorCode::InvalidInput);
    }

    #[tokio::test]
    async fn oversized_text_rejected() {
        let client = TtsClient::new(TtsConfig::default());
        let long = "字".repeat(BAILIAN_TTS_MAX_CHARS + 1);
        let err = client.synthesize(&long).await.unwrap_err();
        assert_eq!(err.code(), ErrorCode::InvalidInput);
        assert!(err.message().contains("超过上限"));
    }

    #[tokio::test]
    async fn mismatched_sample_rate_rejected_with_actionable_message() {
        let mut cfg = TtsConfig::default();
        cfg.api_key = "sk-test".to_string();
        cfg.target_sample_rate = 16_000;
        let client = TtsClient::new(cfg);
        let err = client.synthesize("测试").await.unwrap_err();
        assert_eq!(err.code(), ErrorCode::InvalidInput);
        assert!(err.message().contains("kSpeakerPlaybackSampleRate"));
    }

    #[tokio::test]
    async fn volc_missing_resource_id_fails_loudly() {
        let mut cfg = TtsConfig::default();
        cfg.api_key = "test-key".to_string();
        cfg.resource_id = String::new();
        let client = TtsClient::new(cfg);
        let err = client.synthesize("测试").await.unwrap_err();
        // 缺 resource_id 不能退化成"用默认值试试"——音色与 resource 版本绑死，
        // 猜错会拿到 55000000。让它在发起网络请求之前就明确失败。
        assert_eq!(err.code(), ErrorCode::NotConfigured);
        assert!(err.message().contains("X-Api-Resource-Id"));
    }

    #[test]
    fn decode_b64_tolerates_missing_padding() {
        // 标准填充
        assert_eq!(decode_b64_fragment("QUJD").unwrap(), b"ABC");
        // 缺填充（流式分片里常见）
        assert_eq!(decode_b64_fragment("QQ").unwrap(), b"A");
        assert_eq!(decode_b64_fragment("QUI").unwrap(), b"AB");
        // 带换行/空格
        assert_eq!(decode_b64_fragment("QU\nJD ").unwrap(), b"ABC");
        assert!(decode_b64_fragment("!!!!").is_err());
    }

    #[test]
    fn provider_error_reads_both_shapes() {
        // 形状 1：业务错误，code 在顶层（音色与 resource 不匹配，实测 HTTP 200）
        let a = extract_provider_error(
            br#"{"reqid":"","code":55000000,"message":"resource ID is mismatched"}"#,
        );
        assert!(a.contains("55000000"), "{a}");
        assert!(a.contains("mismatched"), "{a}");

        // 形状 2：鉴权错误，code 嵌在 header 下（实测 HTTP 401）
        let b = extract_provider_error(
            br#"{"header":{"code":45000010,"message":"Invalid X-Api-Key"}}"#,
        );
        assert!(b.contains("45000010"), "{b}");
        assert!(b.contains("Invalid X-Api-Key"), "{b}");

        // 非 JSON：退化成原文截断，而不是空字符串
        let c = extract_provider_error(b"gateway timeout");
        assert!(c.contains("gateway timeout"), "{c}");

        assert!(extract_provider_error(b"").contains("空响应体"));
    }

    #[test]
    fn volc_defaults_are_self_consistent() {
        let cfg = TtsConfig::default();
        assert_eq!(cfg.provider, TtsProvider::Volc);
        assert_eq!(cfg.endpoint, VOLC_TTS_ENDPOINT);
        assert_eq!(cfg.resource_id, VOLC_TTS_RESOURCE_ID);
        assert_eq!(cfg.voice, VOLC_TTS_SPEAKER);
        // 豆包要 48 kHz 是实测支持的，所以默认值直接落在固件采样率上——
        // 这条断言守着"豆包路径零重采样"这个前提。
        assert_eq!(cfg.target_sample_rate, FIRMWARE_SAMPLE_RATE);
    }

    #[test]
    fn empty_endpoint_falls_back_per_provider() {
        let mut cfg = TtsConfig::default();
        cfg.endpoint = String::new();
        assert_eq!(cfg.effective_endpoint(), VOLC_TTS_ENDPOINT);

        cfg.provider = TtsProvider::Bailian;
        assert_eq!(cfg.effective_endpoint(), BAILIAN_TTS_ENDPOINT);

        // 填了内容就照用——不能因为"看起来不像官方域名"就替用户改掉。
        cfg.endpoint = "https://my-proxy.local/tts".into();
        assert_eq!(cfg.effective_endpoint(), "https://my-proxy.local/tts");
    }

    /// 豆包 `additions` 必须是**字符串**，不是嵌套对象。
    ///
    /// 这是真机踩出来的：传对象时服务端返回
    /// `json: cannot unmarshal object into Go struct field TTSReqParams.req_params.additions of type string`，
    /// 而且裹在 **HTTP 200 + code=55000000** 里——只看状态码会当成成功、拿到 0 采样。
    #[test]
    fn volc_additions_is_encoded_as_json_string() {
        let mut cfg = TtsConfig::default();
        cfg.explicit_language = "zh-cn".into();
        let body = volc_request_body(&cfg, "你好");
        let additions = &body["req_params"]["additions"];
        assert!(
            additions.is_string(),
            "additions 必须是字符串，实际是 {additions}"
        );
        // 字符串解开后要能还原出原来那个对象——否则只是"形状对了但内容是坏的"。
        let inner: serde_json::Value =
            serde_json::from_str(additions.as_str().unwrap()).expect("additions 里应是合法 JSON");
        assert_eq!(inner["explicit_language"], "zh-cn");

        // 留空则不传该字段（而不是传个空对象/空串）。
        cfg.explicit_language = String::new();
        let body = volc_request_body(&cfg, "你好");
        assert!(body["req_params"].get("additions").is_none());
    }

    /// 豆包只能要 `pcm`：给 `wav` 时服务端按流式语义反复插 WAV 头，拼出来是畸形文件。
    #[test]
    fn volc_audio_params_pin_pcm_and_target_rate() {
        let cfg = TtsConfig::default();
        let body = volc_request_body(&cfg, "你好");
        let audio = &body["req_params"]["audio_params"];
        assert_eq!(audio["format"], "pcm");
        // 采样率直接取目标值（= 固件 48 kHz），这条守着"豆包路径零重采样"。
        assert_eq!(audio["sample_rate"], FIRMWARE_SAMPLE_RATE);
        assert_eq!(body["req_params"]["text"], "你好");
    }

    /// 真机联调：需要提供 API Key，因此默认 `#[ignore]`。
    ///
    /// ```bash
    /// EASYOP_TTS_KEY=<豆包 API Key> cargo test --lib tts::live -- --ignored --nocapture
    /// ```
    ///
    /// 它会真发一次请求、把音频写到临时目录，并断言"采样率 = 48 kHz 且时长 > 0"。
    /// **这是唯一能证明协议实现正确的测试**——单测只能证明我们自己写的解析逻辑自洽。
    #[tokio::test]
    #[ignore = "需要真实 API Key（EASYOP_TTS_KEY）与网络"]
    async fn live_volc_synthesis_produces_playable_48k_pcm() {
        let key = std::env::var("EASYOP_TTS_KEY").expect("需要环境变量 EASYOP_TTS_KEY");
        let mut cfg = TtsConfig::default();
        cfg.api_key = key;
        let client = TtsClient::new(cfg);
        let audio = client
            .synthesize("任务台已经接通，这是一段语音合成测试。")
            .await
            .expect("真实合成不应失败");

        assert_eq!(audio.sample_rate, FIRMWARE_SAMPLE_RATE);
        assert!(audio.duration_ms > 500, "时长 {} ms 太短", audio.duration_ms);
        assert_eq!(audio.pcm.len() as u64 * 1000 / 48_000, audio.duration_ms);

        let out = std::env::temp_dir().join("easyop_tts_live.wav");
        std::fs::write(&out, audio.to_wav_bytes()).unwrap();
        println!("已写出 {}（{} ms）", out.display(), audio.duration_ms);
    }

    #[test]
    fn audio_to_pcm_bytes_is_le() {
        let a = TtsAudio {
            pcm: vec![1, -1],
            sample_rate: 48_000,
            duration_ms: 0,
            characters: None,
        };
        assert_eq!(a.to_pcm_bytes(), vec![1, 0, 0xFF, 0xFF]);
    }

    // ---- WS 传输 ----

    /// 造一帧服务端回包（与实测结构一致：头 4 + event 4 + sid_len 4 + sid + size 4 + payload）。
    fn server_frame(event: u32, sid: &str, payload: &[u8], mt: u8, ser: u8) -> Vec<u8> {
        let mut v = vec![0x11, (mt << 4) | FLAG_HAS_EVENT, ser << 4, 0x00];
        v.extend_from_slice(&event.to_be_bytes());
        v.extend_from_slice(&(sid.len() as u32).to_be_bytes());
        v.extend_from_slice(sid.as_bytes());
        v.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        v.extend_from_slice(payload);
        v
    }

    /// 传输由端点 scheme 决定，不由 provider 决定。
    ///
    /// 这条守着"同一份配置两种传输"的设计：一旦有人把 transport 改成配置字段，
    /// 就会出现"选了 HTTP 却填 wss 端点"的错配，报出来的错还指不到原因。
    #[test]
    fn transport_follows_endpoint_scheme() {
        let mut cfg = TtsConfig::default();
        assert_eq!(cfg.transport(), Transport::Http, "默认端点是 https");

        cfg.endpoint = "wss://openspeech.bytedance.com/api/v3/tts/unidirectional/stream".into();
        assert_eq!(cfg.transport(), Transport::WebSocket);

        cfg.endpoint = "ws://127.0.0.1:8080/tts".into();
        assert_eq!(cfg.transport(), Transport::WebSocket, "内网调试常用非 TLS 的 ws");

        cfg.endpoint = "  WSS://UP.example.com/tts  ".into();
        assert_eq!(
            cfg.transport(),
            Transport::WebSocket,
            "前后空格与大小写都不能影响判定"
        );

        // 留空 = 回落官方端点（HTTP）。
        cfg.endpoint = String::new();
        assert_eq!(cfg.transport(), Transport::Http);
    }

    /// 把 `"11 94 10 00"` 这样的十六进制串转成字节（下面几个真帧夹具要用）。
    fn unhex(s: &str) -> Vec<u8> {
        let digits: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(digits.len() % 2, 0, "十六进制串长度必须是偶数：{s}");
        (0..digits.len() / 2)
            .map(|i| u8::from_str_radix(&digits[i * 2..i * 2 + 2], 16).expect("十六进制"))
            .collect()
    }

    /// 双向流式的判定：只认**路径末段**是 `bidirection`。
    ///
    /// 判错的代价不是报错，而是**一帧都不回**（把单向帧发给双向端点，服务端静静等），
    /// 所以"什么样的 URL 算双向"必须钉死 —— 包括大小写、前后空格、query、末尾斜杠，
    /// 以及 `contains` 会误命中而末段比对不会的那两个反例。
    #[test]
    fn bidirection_follows_endpoint_path() {
        let mut cfg = TtsConfig::default();

        cfg.endpoint = "wss://openspeech.bytedance.com/api/v3/plan/tts/bidirection".into();
        assert!(cfg.is_bidirection(), "本机在用的端点必须判为双向");

        cfg.endpoint = "  WSS://GW.example.com/tts/Bidirection/?a=1#b  ".into();
        assert!(
            cfg.is_bidirection(),
            "大小写 / 前后空格 / query / fragment / 末尾斜杠都不该影响判定"
        );

        cfg.endpoint = "wss://openspeech.bytedance.com/api/v3/tts/unidirectional/stream".into();
        assert!(
            !cfg.is_bidirection(),
            "单向流式不能判成双向（帧型不同，判错就一帧不回）"
        );

        // ⛔ 这两条是 `contains("bidirection")` 会误判、而末段比对不会的
        cfg.endpoint = "wss://gw.example.com/bidirection-proxy".into();
        assert!(!cfg.is_bidirection(), "末段不是 bidirection，不该误判");
        cfg.endpoint = "wss://gw.example.com/bidirection/stream2".into();
        assert!(!cfg.is_bidirection(), "末段不是 bidirection，不该误判");

        cfg.endpoint = String::new();
        assert!(!cfg.is_bidirection(), "留空回落的是 https 单向端点");
    }

    /// 上行帧的字节布局必须与**官方文档的字节表**逐字节一致。
    ///
    /// 官方文档 §12.3 把 `StartConnection` 的完整布局写出来了：
    /// `11 14 10 00 | 00 00 00 01 | 00 00 00 02 | 7b 7d`。
    /// 拿它当断言，等于把"我们的构造函数"和"官方文档"直接对上 ——
    /// 比"我读了一遍觉得对"强得多。
    #[test]
    fn bidirection_connection_frame_matches_official_byte_table() {
        let start =
            build_bidirection_frame(EV_START_CONNECTION, FrameScope::Connection, b"{}").unwrap();
        assert_eq!(
            start,
            unhex("11 14 10 00 00 00 00 01 00 00 00 02 7b 7d"),
            "StartConnection 必须与官方字节表一致"
        );

        let fin =
            build_bidirection_frame(EV_FINISH_CONNECTION, FrameScope::Connection, b"{}").unwrap();
        assert_eq!(
            fin,
            unhex("11 14 10 00 00 00 00 02 00 00 00 02 7b 7d"),
            "FinishConnection 只有 event 号不同"
        );

        // ⛔ flags 必须是 0b0100（带 event），**不是**单向流式的 0b0001（带 sequence）。
        //    这一位写错，服务端不报错、只是一帧都不回 —— 所以钉在这里。
        assert_eq!(start[1] & 0x0F, FLAG_HAS_EVENT, "双向流式带的是 event 号");
        assert_eq!(
            start[1] & 0x0F & FLAG_HAS_SEQUENCE,
            0,
            "双向流式**不能**带 sequence（那个位置要放 event）"
        );
        assert_eq!(start[2] >> 4, SER_JSON, "载荷是 JSON");
    }

    /// 会话类帧必须带**非空** session_id；连接类帧必须**不带**。
    ///
    /// 三种试点里只有一种对，另两种都被服务端拒，而且报错文案指不到病灶
    /// （服务端把 `payload_size` 当 sid 长度读 → `body too short`）。
    /// 所以这条规则写进类型、并把"空 sid"钉成 `Err`，让它出不去这个函数。
    #[test]
    fn bidirection_session_frame_carries_session_id() {
        // 实测服务端回显的 sid 形状
        let sid = "f26a7525-238f-4850-83c6-6f5d2d69dd34";

        let f = build_bidirection_frame(EV_START_SESSION, FrameScope::Session(sid), b"{}").unwrap();
        assert_eq!(f[0], 0x11);
        assert_eq!(f[1] >> 4, MT_FULL_CLIENT_REQUEST);
        assert_eq!(f[1] & 0x0F, FLAG_HAS_EVENT);
        assert_eq!(&f[4..8], &EV_START_SESSION.to_be_bytes(), "event 段");
        assert_eq!(&f[8..12], &(sid.len() as u32).to_be_bytes(), "sid 长度段");
        assert_eq!(&f[12..12 + sid.len()], sid.as_bytes(), "sid 本体");
        assert_eq!(
            &f[12 + sid.len()..16 + sid.len()],
            &2u32.to_be_bytes(),
            "sid 之后才是 payload 长度"
        );
        assert_eq!(&f[16 + sid.len()..], b"{}");

        // 空 sid：宁可在这里失败，也不要发出一个必定被拒、错误又指不到病灶的帧
        let err =
            build_bidirection_frame(EV_START_SESSION, FrameScope::Session(""), b"{}").unwrap_err();
        assert!(err.contains("session_id"), "错误要指名病灶：{err}");

        // 连接类**不能**带 sid 段：多出 40 字节，服务端会把 sid 长度当 payload_size 读
        let conn =
            build_bidirection_frame(EV_START_CONNECTION, FrameScope::Connection, b"{}").unwrap();
        assert_eq!(conn.len(), 14, "连接类帧只有 头4 + event4 + size4 + `{{}}`2");
    }

    /// 用**实测抓下来的真帧**钉住解析器的偏移量。
    ///
    /// 为什么不用手搓的帧：手搓帧会和解析器犯同一个错，一起错就一起过。
    /// 下面这几串都是从 `…/plan/tts/bidirection` 实收回来的
    /// （`scripts/_capture_bidir_frames.py` 抓的）。
    ///
    /// 解析器读错位的后果是**静默**的：错位的载荷照样能被 `chunks_exact(2)` 切成 i16，
    /// 交付出去是一段听不出解码失败、却短了一百倍的音频。
    #[test]
    fn parse_real_bidirection_frames() {
        // ── ConnectionStarted(50)：带 connection_id，载荷 `{}` ──
        let conn = unhex(
            "11 94 10 00 00 00 00 32 00 00 00 24 31 32 38 34 33 37 32 34 2d 39 64 37 65 2d 34 34 33 36 \
             2d 39 32 33 63 2d 36 33 36 34 62 38 30 30 63 34 65 36 00 00 00 02 7b 7d",
        );
        let f = parse_server_frame(&conn).unwrap();
        assert_eq!(f.message_type, MT_FULL_SERVER_RESPONSE);
        assert_eq!(f.event, Some(50));
        assert_eq!(
            f.session_id.as_deref(),
            Some("12843724-9d7e-4436-923c-6364b800c4e6"),
            "连接类事件这一段放的是 connection_id，**布局与会话类完全相同**"
        );
        assert_eq!(f.payload, b"{}");

        // ── SessionStarted(150) ──
        let sess = unhex(
            "11 94 10 00 00 00 00 96 00 00 00 24 66 32 36 61 37 35 32 35 2d 32 33 38 66 2d 34 38 35 30 \
             2d 38 33 63 36 2d 36 66 35 64 32 64 36 39 64 64 33 34 00 00 00 02 7b 7d",
        );
        let f = parse_server_frame(&sess).unwrap();
        assert_eq!(f.event, Some(150));
        assert_eq!(f.session_id.as_deref(), Some("f26a7525-238f-4850-83c6-6f5d2d69dd34"));
        assert_eq!(f.payload, b"{}");

        // ── TTSSentenceStart(350)：句开始，文本为空 ──
        let sent_start = unhex(
            "11 94 10 00 00 00 01 5e 00 00 00 24 66 32 36 61 37 35 32 35 2d 32 33 38 66 2d 34 38 35 30 \
             2d 38 33 63 36 2d 36 66 35 64 32 64 36 39 64 64 33 34 00 00 00 24 7b 22 70 68 6f 6e 65 6d \
             65 73 22 3a 5b 5d 2c 22 74 65 78 74 22 3a 22 22 2c 22 77 6f 72 64 73 22 3a 5b 5d 7d",
        );
        let f = parse_server_frame(&sent_start).unwrap();
        assert_eq!(f.event, Some(350));
        assert_eq!(f.payload, br#"{"phonemes":[],"text":"","words":[]}"#);

        // ── TTSSentenceEnd(351)：**载荷里带回服务端收到的全文** ──
        let sent_end = unhex(
            "11 94 10 00 00 00 01 5f 00 00 00 24 66 32 36 61 37 35 32 35 2d 32 33 38 66 2d 34 38 35 30 \
             2d 38 33 63 36 2d 36 66 35 64 32 64 36 39 64 64 33 34 00 00 00 2d 7b 22 70 68 6f 6e 65 6d \
             65 73 22 3a 5b 5d 2c 22 74 65 78 74 22 3a 22 e5 a5 bd e7 9a 84 e3 80 82 22 2c 22 77 6f 72 \
             64 73 22 3a 5b 5d 7d",
        );
        let f = parse_server_frame(&sent_end).unwrap();
        assert_eq!(f.event, Some(351));
        let text = serde_json::from_slice::<serde_json::Value>(&f.payload).unwrap()["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(text, "好的。", "351 的回读文本要能取出来（用来交叉验证没被截断）");

        // ── TTSResponse(352)：**音频帧**。`mt=0b1011`、serialization=RAW ──
        // 真帧的载荷是 34816 字节，这里保留**真实的帧头与 sid**，只把载荷换成 8 字节：
        // 要钉的是偏移量（涨到 34816 也不会让偏移更对），不是载荷大小。
        let mut audio = unhex(
            "11 b4 00 00 00 00 01 60 00 00 00 24 66 32 36 61 37 35 32 35 2d 32 33 38 66 2d 34 38 35 30 \
             2d 38 33 63 36 2d 36 66 35 64 32 64 36 39 64 64 33 34",
        );
        audio.extend_from_slice(&8u32.to_be_bytes());
        audio.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
        let f = parse_server_frame(&audio).unwrap();
        assert_eq!(f.message_type, MT_AUDIO_ONLY_SERVER_RESPONSE);
        assert_eq!(f.event, Some(352), "352 是 event，**不是** payload 长度");
        assert_eq!(f.session_id.as_deref(), Some("f26a7525-238f-4850-83c6-6f5d2d69dd34"));
        assert_eq!(f.payload.len(), 8, "载荷必须恰好是那 8 字节，不能被 event 号顶掉");
        assert_eq!(f.payload, vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);

        // ── ConnectionFinished(52) ──
        let fin = unhex(
            "11 94 10 00 00 00 00 34 00 00 00 24 31 32 38 34 33 37 32 34 2d 39 64 37 65 2d 34 34 33 36 \
             2d 39 32 33 63 2d 36 33 36 34 62 38 30 30 63 34 65 36 00 00 00 02 7b 7d",
        );
        let f = parse_server_frame(&fin).unwrap();
        assert_eq!(f.event, Some(52));
        assert_eq!(f.payload, b"{}");

        // ── SessionFinished(152)：双向流式的**唯一**收尾判据 ──
        let done = server_frame(152, "f26a7525-238f-4850-83c6-6f5d2d69dd34", b"{}",
                                MT_FULL_SERVER_RESPONSE, SER_JSON);
        let f = parse_server_frame(&done).unwrap();
        assert_eq!(f.event, Some(152));
        assert_eq!(f.payload, b"{}");
    }

    /// 把单向帧发给双向端点时，服务端回的那句话必须**原样留给用户**。
    ///
    /// 实测原文（2026-09-23）：
    /// `decode ws request failed: unable to decode V1 protocol message: parse payload size failed: body too short`
    ///
    /// 这句里没有一个字提到"你没传 session_id"，所以它是**唯一**能指出病灶的线索；
    /// 若被 `explain_volc_code` 归到"鉴权失败"去，用户会去查密钥，方向彻底跑偏。
    #[test]
    fn bidirection_protocol_error_is_not_mistaken_for_auth() {
        let body = br#"{"error":"decode ws request failed: unable to decode V1 protocol message: parse payload size failed: body too short"}"#;
        let mut raw = vec![0x11, MT_ERROR << 4, SER_JSON << 4, 0x00];
        // 按实测错误帧形状构造：头 4 + 错误码 4 + 长度 4 + JSON（**不带 event**）
        raw.extend_from_slice(&45_000_000u32.to_be_bytes());
        raw.extend_from_slice(&(body.len() as u32).to_be_bytes());
        raw.extend_from_slice(body);

        let f = parse_server_frame(&raw).unwrap();
        assert_eq!(f.code, Some(45_000_000));
        assert_eq!(f.event, None, "错误帧没有 event 字段");

        let msg = explain_volc_code(45_000_000, &String::from_utf8_lossy(&f.payload));
        assert!(msg.contains("body too short"), "实测原文必须原样保留：{msg}");
        assert!(
            !msg.contains("鉴权失败"),
            "45_000_123 那一档是鉴权，45_000_000 不是；混为一谈会把排查带去查密钥：{msg}"
        );
    }

    /// 双向流式**只有 WebSocket**：端点写成 https 必须当场说清，而不是拿 HTTP 去撞。
    #[tokio::test]
    async fn bidirection_with_https_endpoint_is_rejected_with_direction() {
        let mut cfg = TtsConfig::default();
        cfg.endpoint = "https://openspeech.bytedance.com/api/v3/plan/tts/bidirection".into();
        cfg.api_key = "k".into();
        let client = TtsClient::new(cfg);
        let err = client.synthesize("测试").await.unwrap_err();
        assert_eq!(err.code(), ErrorCode::InvalidInput);
        assert!(err.message().contains("双向流式"), "{}", err.message());
        assert!(
            err.message().contains("wss://"),
            "要给出该改成什么，而不是只说「错了」：{}",
            err.message()
        );
    }

    /// 端点末段是 `bidirection` 时，分流必须真的走到双向那条路。
    ///
    /// 用一个不可能连通的地址来区分：两条路径的报错前缀不同
    /// （`豆包 TTS(bidir)` vs `豆包 TTS(ws)`），据此在**离线**环境里就能确认分流生效 ——
    /// 否则"加了双向路径但根本没被调用"这种错，要等真机上出现"建连成功却一帧不回"才发现。
    #[tokio::test]
    async fn bidirection_endpoint_dispatches_to_bidirection_path() {
        let mut cfg = TtsConfig::default();
        cfg.endpoint = "wss://127.0.0.1:1/plan/tts/bidirection".into();
        cfg.api_key = "k".into();
        let client = TtsClient::new(cfg);
        let err = client.synthesize("测试").await.unwrap_err();
        assert!(
            err.message().contains("bidir"),
            "必须走到双向路径（报错前缀应为 豆包 TTS(bidir)）：{}",
            err.message()
        );

        // 反向：单向端点不能跑到双向那条路上去
        let mut cfg = TtsConfig::default();
        cfg.endpoint = "wss://127.0.0.1:1/plan/tts/unidirectional/stream".into();
        cfg.api_key = "k".into();
        let client = TtsClient::new(cfg);
        let err = client.synthesize("测试").await.unwrap_err();
        assert!(
            !err.message().contains("bidir"),
            "单向端点不该走双向路径：{}",
            err.message()
        );
    }

    #[test]
    fn client_frame_is_head_seq_size_payload() {
        let f = build_client_frame(Some(1), br#"{"a":1}"#);
        assert_eq!(f[0], 0x11, "protocol_version=1 / header_size=1");
        assert_eq!(f[1] >> 4, MT_FULL_CLIENT_REQUEST, "必须是整包请求");
        assert_eq!(f[1] & FLAG_HAS_SEQUENCE, FLAG_HAS_SEQUENCE, "首包带 sequence");
        assert_eq!(f[2] >> 4, SER_JSON, "载荷是 JSON");
        assert_eq!(f[3], 0x00);
        assert_eq!(&f[4..8], &1i32.to_be_bytes(), "sequence=1");
        assert_eq!(&f[8..12], &7u32.to_be_bytes(), "payload_size");
        assert_eq!(&f[12..], br#"{"a":1}"#);
    }

    /// 音频帧必须**完整**取回载荷 —— 尤其不能被 event 号顶掉。
    ///
    /// ⛔ 这是本模块最贵的一次教训：`352`（事件号）与"载荷长度"量级相近，
    /// 一旦把它当长度读，整帧错位，得到的音频会**短一百倍**却仍能解码成 i16 ——
    /// 听起来只是"怎么这么快念完了"，完全不像故障。所以把真实量级的帧钉在这里。
    #[test]
    fn audio_frame_payload_is_not_confused_with_event_number() {
        let audio = vec![0xABu8; 34_816]; // 实测音频帧的载荷量级
        let raw = server_frame(
            EV_TTS_SENTENCE_START,
            "6badd408-352a-4927-9475-b898f9a3944b",
            &audio,
            MT_AUDIO_ONLY_SERVER_RESPONSE,
            SER_RAW,
        );
        let f = parse_server_frame(&raw).expect("真实结构的帧必须解析成功");
        assert_eq!(f.message_type, MT_AUDIO_ONLY_SERVER_RESPONSE);
        assert_eq!(f.event, Some(352));
        assert_eq!(
            f.payload.len(),
            34_816,
            "载荷必须是 34816 字节，不是 352 —— 后者说明把 event 当成了长度"
        );
    }

    #[test]
    fn parse_server_frame_reads_session_starter_json() {
        let body = br#"{"phonemes":[],"text":"","words":[]}"#;
        let raw = server_frame(EV_SESSION_STARTED, "sid-0001", body, MT_FULL_SERVER_RESPONSE, SER_JSON);
        let f = parse_server_frame(&raw).unwrap();
        assert_eq!(f.event, Some(350));
        assert_eq!(f.payload, body);
    }

    #[test]
    fn parse_error_frame_carries_code_and_json() {
        // 实测错误帧：头 4 + 错误码 4 + 长度 4 + JSON（**不带 event**）
        let body = br#"{"error":"resource ID is mismatched with speaker related resource"}"#;
        let mut raw = vec![0x11, MT_ERROR << 4, SER_JSON << 4, 0x00];
        raw.extend_from_slice(&VOLC_CODE_RESOURCE_MISMATCH.to_be_bytes());
        raw.extend_from_slice(&(body.len() as u32).to_be_bytes());
        raw.extend_from_slice(body);

        let f = parse_server_frame(&raw).unwrap();
        assert_eq!(f.message_type, MT_ERROR);
        assert_eq!(f.code, Some(55_000_000));
        assert_eq!(f.event, None, "错误帧没有 event 字段");
        assert_eq!(f.payload, body);
    }

    /// 结构对不上时**必须报错**，不许猜。
    ///
    /// 猜错的代价不是"报错难看"，而是把错位数据当音频交付 —— 那是一条无声的坏数据路径。
    #[test]
    fn parse_server_frame_fails_loudly_on_broken_shape() {
        // 太短
        assert!(parse_server_frame(&[0x11, 0x94, 0x10, 0x00]).is_err());
        // 标了长度但字节不够
        let mut raw = vec![0x11, MT_AUDIO_ONLY_SERVER_RESPONSE << 4, SER_RAW << 4, 0x00];
        raw.extend_from_slice(&999u32.to_be_bytes());
        raw.extend_from_slice(&10u32.to_be_bytes());
        raw.extend_from_slice(b"short");
        let err = parse_server_frame(&raw).unwrap_err();
        assert!(err.contains("超过剩余"), "错误要指出长度越界，实际：{err}");
        assert!(err.contains("0x") || err.contains("11 "), "要带上头部十六进制：{err}");
    }

    /// 两个错误码指向**相反**的修改方向，文案不许糊成一句。
    #[test]
    fn volc_error_hints_point_at_the_right_field() {
        let auth = explain_volc_code(VOLC_CODE_AUTH, r#"{"error":"Invalid X-Api-Key"}"#);
        assert!(auth.contains("鉴权"), "{auth}");
        assert!(auth.contains("X-Api-Key"), "{auth}");
        assert!(!auth.contains("音色与资源"), "鉴权错不该提音色：{auth}");

        let mismatch = explain_volc_code(
            VOLC_CODE_RESOURCE_MISMATCH,
            r#"{"error":"resource ID is mismatched with speaker related resource"}"#,
        );
        assert!(mismatch.contains("音色与资源 ID"), "{mismatch}");
        assert!(
            mismatch.contains("zh_female_vv_uranus_bigtts"),
            "必须给出实测可用的 2.0 音色示例，否则用户仍要自己猜：{mismatch}"
        );
        assert!(!mismatch.contains("Invalid X-Api-Key"), "{mismatch}");
    }

    /// 百炼没有 WS 端点：填了 wss 必须明确说清，而不是拿 HTTP 去撞 wss。
    #[tokio::test]
    async fn bailian_with_ws_endpoint_is_rejected_with_direction() {
        let mut cfg = TtsConfig::default();
        cfg.provider = TtsProvider::Bailian;
        cfg.endpoint = "wss://dashscope.aliyuncs.com/whatever".into();
        cfg.api_key = "sk-test".into();
        let client = TtsClient::new(cfg);
        let err = client.synthesize("测试").await.unwrap_err();
        assert_eq!(err.code(), ErrorCode::InvalidInput);
        assert!(err.message().contains("百炼"), "{}", err.message());
        assert!(err.message().contains("https://"), "要给出改回哪里：{}", err.message());
    }

    /// WS 路径的空文本/超长/未配置三条前置校验必须与 HTTP 路径**完全一致**。
    ///
    /// 分开实现最容易出现的就是"HTTP 拦了、WS 漏了"——那种漏在真机上表现为
    /// 一次必然失败的网络往返，日志里看不出是本地就没拦住。
    #[tokio::test]
    async fn ws_path_shares_the_same_preflight_checks() {
        let ws = "wss://example.invalid/api/v3/tts/unidirectional/stream";

        let mut cfg = TtsConfig::default();
        cfg.endpoint = ws.into();
        let client = TtsClient::new(cfg);
        assert_eq!(
            client.synthesize("测试").await.unwrap_err().code(),
            ErrorCode::NotConfigured,
            "没填 key 必须照样拦"
        );

        let mut cfg = TtsConfig::default();
        cfg.endpoint = ws.into();
        cfg.api_key = "k".into();
        let client = TtsClient::new(cfg);
        assert_eq!(
            client.synthesize("   ").await.unwrap_err().code(),
            ErrorCode::InvalidInput
        );
        let long = "字".repeat(BAILIAN_TTS_MAX_CHARS + 1);
        assert_eq!(
            client.synthesize(&long).await.unwrap_err().code(),
            ErrorCode::InvalidInput
        );

        // resource_id 留空：WS 与 HTTP 一样在发请求前就失败（否则握手必 403）。
        let mut cfg = TtsConfig::default();
        cfg.endpoint = ws.into();
        cfg.api_key = "k".into();
        cfg.resource_id = String::new();
        let err = TtsClient::new(cfg).synthesize("测试").await.unwrap_err();
        assert_eq!(err.code(), ErrorCode::NotConfigured);
        assert!(err.message().contains("X-Api-Resource-Id"), "{}", err.message());
    }

    /// 真机联调（WS 传输）：需要真实 API Key，因此默认 `#[ignore]`。
    ///
    /// ```bash
    /// EASYOP_TTS_KEY=<火山语音 API Key> \
    /// EASYOP_TTS_WS_ENDPOINT=wss://openspeech.bytedance.com/api/v3/tts/unidirectional/stream \
    /// cargo test --lib tts::tests::live_volc_ws -- --ignored --nocapture
    /// ```
    ///
    /// 可选环境变量：`EASYOP_TTS_SPEAKER`（默认 `zh_female_vv_uranus_bigtts`）、
    /// `EASYOP_TTS_RESOURCE`（默认 `seed-tts-2.0`）。
    /// 断言"采样率 = 48 kHz 且时长 > 500 ms"——**这是唯一能证明 WS 协议实现正确的测试**，
    /// 单测只能证明我们自己的编解码自洽。
    #[tokio::test]
    #[ignore = "需要真实 API Key（EASYOP_TTS_KEY）与网络"]
    async fn live_volc_ws_synthesis_produces_playable_48k_pcm() {
        let key = std::env::var("EASYOP_TTS_KEY").expect("需要环境变量 EASYOP_TTS_KEY");
        let mut cfg = TtsConfig::default();
        cfg.api_key = key;
        cfg.endpoint = std::env::var("EASYOP_TTS_WS_ENDPOINT").unwrap_or_else(|_| {
            "wss://openspeech.bytedance.com/api/v3/tts/unidirectional/stream".to_string()
        });
        cfg.resource_id =
            std::env::var("EASYOP_TTS_RESOURCE").unwrap_or_else(|_| "seed-tts-2.0".to_string());
        cfg.voice = std::env::var("EASYOP_TTS_SPEAKER")
            .unwrap_or_else(|_| "zh_female_vv_uranus_bigtts".to_string());

        let client = TtsClient::new(cfg);
        let audio = client
            .synthesize("任务台已经接通，这是一段语音合成测试。")
            .await
            .expect("真实 WS 合成不应失败");

        assert_eq!(audio.sample_rate, FIRMWARE_SAMPLE_RATE);
        assert!(audio.duration_ms > 500, "时长 {} ms 太短（错位解析会得到几十毫秒）", audio.duration_ms);
        assert_eq!(audio.pcm.len() as u64 * 1000 / 48_000, audio.duration_ms);

        let out = std::env::temp_dir().join("easyop_tts_ws_live.wav");
        std::fs::write(&out, audio.to_wav_bytes()).unwrap();
        println!("已写出 {}（{} ms）", out.display(), audio.duration_ms);
    }

    /// **真打**双向流式端点，证明握手 + 会话 + 喂文本 + 收尾整条链路正确。
    ///
    /// ```text
    /// EASYOP_TTS_KEY=<key> \
    /// EASYOP_TTS_BIDIR_ENDPOINT=wss://openspeech.bytedance.com/api/v3/plan/tts/bidirection \
    ///     cargo test --lib tts::tests::live_volc_bidirection -- --ignored --nocapture
    /// ```
    ///
    /// ⚠️ 端点**必须带 `/plan/`**（本机账号的 key 是方舟那把，裸路径 401）。
    ///
    /// 断言与单向那条完全一致，因为两条路径对外必须**不可区分**：
    /// 采样率 48 kHz、时长 > 500 ms、`pcm.len()` 与 `duration_ms` 自洽。
    /// 时长那条尤其关键 —— 把 event 号当 payload 长度读的错位，就表现为"短一百倍"，
    /// 而它**不会**报错，只会听起来像念得飞快。
    #[tokio::test]
    #[ignore = "需要真实 API Key（EASYOP_TTS_KEY）与网络"]
    async fn live_volc_bidirection_synthesis_produces_playable_48k_pcm() {
        let key = std::env::var("EASYOP_TTS_KEY").expect("需要环境变量 EASYOP_TTS_KEY");
        let mut cfg = TtsConfig::default();
        cfg.api_key = key;
        cfg.endpoint = std::env::var("EASYOP_TTS_BIDIR_ENDPOINT").unwrap_or_else(|_| {
            "wss://openspeech.bytedance.com/api/v3/plan/tts/bidirection".to_string()
        });
        cfg.resource_id =
            std::env::var("EASYOP_TTS_RESOURCE").unwrap_or_else(|_| "seed-tts-2.0".to_string());
        cfg.voice = std::env::var("EASYOP_TTS_SPEAKER")
            .unwrap_or_else(|_| "zh_female_xiaohe_uranus_bigtts".to_string());
        assert!(
            cfg.is_bidirection(),
            "端点末段必须是 bidirection，否则这条测试会去测单向路径"
        );

        let client = TtsClient::new(cfg);
        let audio = client
            .synthesize("任务台已经接通，这是双向流式合成测试。")
            .await
            .expect("真实双向流式合成不应失败");

        assert_eq!(audio.sample_rate, FIRMWARE_SAMPLE_RATE);
        assert!(
            audio.duration_ms > 500,
            "时长 {} ms 太短（错位解析会得到几十毫秒）",
            audio.duration_ms
        );
        assert_eq!(audio.pcm.len() as u64 * 1000 / 48_000, audio.duration_ms);

        let out = std::env::temp_dir().join("easyop_tts_bidirection_live.wav");
        std::fs::write(&out, audio.to_wav_bytes()).unwrap();
        println!("已写出 {}（{} ms）", out.display(), audio.duration_ms);
    }
}
