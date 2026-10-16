//! 固件状态回读：`0x13` 状态请求 → `0x11` / kind `0x04` 分块响应。
//!
//! # 协议真源（**照源码核对过，不是推测**）
//!
//! - `easy-input-maker/components/keyboard/include/keyboard/status_hid_protocol.h`
//! - `easy-input-maker/components/keyboard/src/status_hid_protocol.cpp`
//! - `easy-input-maker/components/keyboard/src/config_status.cpp`（`build_full_status_json`）
//!
//! ```text
//! 请求：Feature Report 0x13，17 字节 = 1 Report ID + 16 字节载荷
//!   [0]      0x13（Report ID，不进载荷）
//!   [1..4]   'S' '3' 'R'
//!   [4]      协议版本 = 1
//!   [5..9]   request_id u32 LE（**必须非 0**）
//!   [9]      flags（0 / 0x01=fresh）
//!   [10..17] 必须全 0
//!
//! 响应：Input Report 0x11，64 字节 = 1 Report ID + 63 字节载荷
//!   payload[0]      kind = 0x04
//!   payload[1]      chunk_index
//!   payload[2]      total_chunks
//!   payload[3]      9 + 本块 JSON 字节数
//!   payload[4]      协议版本
//!   payload[5..9]   request_id（回显）
//!   payload[9..11]  JSON 总长 u16 LE
//!   payload[11..13] JSON 的 crc16 u16 LE
//!   payload[13..63] JSON 分片（每块 ≤ 50 字节；JSON 总长上限 512）
//! ```
//!
//! # 两个"静默失效"陷阱（都踩过）
//!
//! 1. **固件对畸形请求是 fail-closed 且不回错**。第一版探针把 16 字节载荷写成全零，
//!    结果一块响应都收不到 —— 看起来像"固件不支持这个功能"。实际是 `data[0] != 'S'`
//!    被直接丢掉。所以：**"没有响应"的第一嫌疑永远是请求载荷，而不是板子**。
//! 2. **缺 `KIND_STATUS_RESPONSE = 0x04` 时，即使固件回了也会被当未知报文丢掉**。
//!    本模块与 `constants.rs` 一起把 0x04 补上，两侧才闭合。
//!
//! # 为什么纯逻辑与 IO 分家
//!
//! `build_request` / `decode_chunk` / `StatusAssembler` 都是纯函数或纯状态机，
//! 可以用**按固件算法写的测试编码器**做 round-trip（见文件尾 `test_fixtures`）——
//! 偏移写错一个字节就会当场露馅，不必插板子试。真机 IO 只剩薄薄一层。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::constants::*;
use crate::crc16::crc16_ccitt;
use crate::device;
use crate::model::DeviceStatus;
use crate::transport;

/// 一次状态回读的整体超时。
///
/// 实机实测（2026-09-30，JSON 451 字节 / 10 块）：从发出请求到收齐全部块 **不到 100 ms**。
/// 1.2 s 是给"上位机同时在忙别的事"留的余量；再放大只会让"读不到"这件事更晚被说出来，
/// 而慢本身就是一种失败。
pub const STATUS_QUERY_TIMEOUT_MS: u64 = 1200;

/// 单块读取的切片超时；外层整体超时兜底。
const STATUS_CHUNK_READ_MS: u32 = 400;

// ---------------------------------------------------------------------------
// 请求
// ---------------------------------------------------------------------------

/// 构造 `0x13` 请求 Feature Report（17 字节 = Report ID + 16 字节载荷）。
pub fn build_request(request_id: u32, fresh: bool) -> [u8; 1 + STATUS_REQUEST_PAYLOAD_LEN] {
    let mut report = [0u8; 1 + STATUS_REQUEST_PAYLOAD_LEN];
    report[0] = REPORT_ID_STATUS_REQUEST;
    report[1..4].copy_from_slice(&STATUS_REQUEST_MAGIC);
    report[4] = STATUS_HID_PROTOCOL_VERSION;
    report[5..9].copy_from_slice(&request_id.to_le_bytes());
    report[9] = if fresh { STATUS_REQUEST_FLAG_FRESH } else { 0 };
    // [10..17] 保持 0：固件要求这些字节全 0，否则整条请求被拒。
    report
}

/// 上一次发出去的 request_id，用来保证新 id **严格大于**它。
static LAST_REQUEST_ID: AtomicU32 = AtomicU32::new(0);

/// 生成一个**非 0 且严格递增**的请求 id。
///
/// - 非 0 是协议硬要求（固件 `decode_status_hid_request` 见到 0 直接拒绝）。
/// - 严格递增是为了让"上一次没发完的残留响应"必然对不上新 id，从而被丢弃；
///   若用自增计数器，上位机重启后计数回到 1，就可能与板子上还挂着的旧块撞号。
fn next_request_id() -> u32 {
    let base = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0);
    loop {
        let last = LAST_REQUEST_ID.load(Ordering::Relaxed);
        // base 可能是 0（时钟异常）或比 last 小（校时回拨）—— 都必须仍严格递增。
        let mut next = if base > last { base } else { last.wrapping_add(1) };
        if next == 0 {
            next = 1;
        }
        if LAST_REQUEST_ID
            .compare_exchange_weak(last, next, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            return next;
        }
    }
}

// ---------------------------------------------------------------------------
// 响应
// ---------------------------------------------------------------------------

/// 一块已解析的 `0x11` / kind `0x04` 响应分块。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusChunk {
    pub request_id: u32,
    pub index: u8,
    pub total: u8,
    pub protocol_version: u8,
    /// 整份 JSON 的字节数（每块都带，可交叉校验）。
    pub json_len: usize,
    /// 整份 JSON 的 CRC16（每块都带）。
    pub crc16: u16,
    /// 本块携带的 JSON 分片。
    pub data: Vec<u8>,
}

/// 固件侧同一个算法：给定 JSON 总长，应分成几块。
///
/// 与 `status_hid_protocol.cpp::status_hid_response_chunk_count` 逐行对应。
/// 用来交叉校验响应里声明的 `total_chunks` —— 声明值与算出来的不一致，
/// 说明对面不是这套协议（或 JSON 长度越界），必须显式失败而不是"尽力拼一下"。
pub fn chunk_count_for(json_len: usize) -> Option<u8> {
    if json_len == 0 || json_len > STATUS_RESPONSE_MAX_JSON_LEN {
        return None;
    }
    let chunks = (json_len + STATUS_RESPONSE_JSON_PER_CHUNK - 1) / STATUS_RESPONSE_JSON_PER_CHUNK;
    if chunks == 0 || chunks > 255 {
        return None;
    }
    Some(chunks as u8)
}

/// 解析一条 Input Report（**含** Report ID 首字节）。
///
/// - 不是状态响应（别的 kind、别的 Report ID）→ `Ok(None)`，交回调用方忽略；
/// - 是状态响应但自身畸形（长度/编号/协议版本越界）→ `Err`，**不猜、不兜底**。
pub fn decode_chunk(report: &[u8]) -> Result<Option<StatusChunk>, String> {
    if report.len() < 2 || report[0] != REPORT_ID_APP_COMMAND || report[1] != KIND_STATUS_RESPONSE {
        return Ok(None);
    }
    // 去掉 Report ID，后面全部按固件的 63 字节容器口径处理。
    let payload = &report[1..];
    let header = STATUS_RESPONSE_HEADER_LEN + STATUS_RESPONSE_METADATA_LEN;
    if payload.len() < header {
        return Err(format!("状态响应过短：{} 字节（至少 {header}）", payload.len()));
    }

    let index = payload[1];
    let total = payload[2];
    let declared = payload[3] as usize;
    let protocol_version = payload[4];
    let request_id = u32::from_le_bytes([payload[5], payload[6], payload[7], payload[8]]);
    let json_len = u16::from_le_bytes([payload[9], payload[10]]) as usize;
    let crc = u16::from_le_bytes([payload[11], payload[12]]);

    if protocol_version != STATUS_HID_PROTOCOL_VERSION {
        return Err(format!(
            "状态响应协议版本不支持：{protocol_version}（本机支持 {STATUS_HID_PROTOCOL_VERSION}）"
        ));
    }
    if request_id == 0 {
        return Err("状态响应的 request_id 为 0".to_string());
    }
    if total == 0 {
        return Err("状态响应声明 total_chunks = 0".to_string());
    }
    if index >= total {
        return Err(format!("状态分块编号越界：index {index} >= total {total}"));
    }
    if declared < STATUS_RESPONSE_METADATA_LEN {
        return Err(format!("状态分块长度字段非法：{declared}"));
    }
    let chunk_len = declared - STATUS_RESPONSE_METADATA_LEN;
    if chunk_len > STATUS_RESPONSE_JSON_PER_CHUNK {
        return Err(format!(
            "状态分块超过单块上限：{chunk_len} > {STATUS_RESPONSE_JSON_PER_CHUNK}"
        ));
    }
    if payload.len() < header + chunk_len {
        return Err(format!(
            "状态分块被截断：声明 {chunk_len} 字节，实际只剩 {}",
            payload.len() - header
        ));
    }

    let expected_total = chunk_count_for(json_len)
        .ok_or_else(|| format!("状态 JSON 长度越界：{json_len}（上限 {STATUS_RESPONSE_MAX_JSON_LEN}）"))?;
    if expected_total != total {
        return Err(format!(
            "状态分块数与 JSON 长度不自洽：声明 {total} 块，按 {json_len} 字节应为 {expected_total} 块"
        ));
    }

    Ok(Some(StatusChunk {
        request_id,
        index,
        total,
        protocol_version,
        json_len,
        crc16: crc,
        data: payload[header..header + chunk_len].to_vec(),
    }))
}

/// 收块 → 校验 → 拼出 JSON 的状态机。
///
/// # 为什么每块都要看元数据
///
/// `json_len` / `crc16` / `total` 在**每一块**里都重复出现。它们彼此不一致，只有一种
/// 解释：这些块不属于同一份响应。此时必须整体作废，而不是"用最后一块的元数据去拼"。
#[derive(Debug)]
pub struct StatusAssembler {
    request_id: u32,
    json_len: Option<usize>,
    crc16: Option<u16>,
    total: Option<u8>,
    parts: BTreeMap<u8, Vec<u8>>,
    /// 因 request_id 不符被丢弃的块数（**区分"板子没回"与"回的是上一轮的"**）。
    pub stale_chunks: u32,
    /// 因畸形被丢弃的块数。
    pub malformed_chunks: u32,
}

impl StatusAssembler {
    pub fn new(request_id: u32) -> Self {
        Self {
            request_id,
            json_len: None,
            crc16: None,
            total: None,
            parts: BTreeMap::new(),
            stale_chunks: 0,
            malformed_chunks: 0,
        }
    }

    /// 已收到的块数 / 声明的总块数（后者在收到第一块前是 `None`）。
    pub fn progress(&self) -> (usize, Option<u8>) {
        (self.parts.len(), self.total)
    }

    /// 喂进一条原始 Input Report。
    ///
    /// 返回 `Ok(Some(json))` 表示整份响应已收齐、CRC 校验通过。
    /// 非本响应的报文（其它 kind / 过期 request_id）返回 `Ok(None)` 并被计数。
    pub fn push(&mut self, report: &[u8]) -> Result<Option<String>, String> {
        let chunk = match decode_chunk(report) {
            Ok(Some(c)) => c,
            Ok(None) => return Ok(None),
            Err(e) => {
                self.malformed_chunks += 1;
                return Err(e);
            }
        };
        if chunk.request_id != self.request_id {
            self.stale_chunks += 1;
            return Ok(None);
        }
        // 元数据一致性：先到者定调，后来者必须完全一致。
        match (self.json_len, self.crc16, self.total) {
            (Some(len), Some(crc), Some(total)) => {
                if len != chunk.json_len || crc != chunk.crc16 || total != chunk.total {
                    return Err(format!(
                        "状态分块元数据不一致：已按 {total} 块 / {len} 字节 / CRC 0x{crc:04X} 收块，\
                         本块声明 {} 块 / {} 字节 / CRC 0x{:04X}",
                        chunk.total, chunk.json_len, chunk.crc16
                    ));
                }
            }
            _ => {
                self.json_len = Some(chunk.json_len);
                self.crc16 = Some(chunk.crc16);
                self.total = Some(chunk.total);
            }
        }
        // 同一块号重复到达：内容相同就容忍（重传），不同就是矛盾。
        if let Some(prev) = self.parts.get(&chunk.index) {
            if *prev != chunk.data {
                return Err(format!("状态分块 #{} 重复且内容不同", chunk.index));
            }
            return Ok(None);
        }
        self.parts.insert(chunk.index, chunk.data);
        self.try_finish()
    }

    /// 收齐了就把 JSON 拼出来并校验；没收齐返回 `Ok(None)`。
    fn try_finish(&mut self) -> Result<Option<String>, String> {
        let (Some(total), Some(json_len), Some(crc)) = (self.total, self.json_len, self.crc16) else {
            return Ok(None);
        };
        if self.parts.len() != total as usize {
            return Ok(None);
        }
        let mut blob = Vec::with_capacity(json_len);
        for index in 0..total {
            let part = self
                .parts
                .get(&index)
                .ok_or_else(|| format!("状态分块缺失：#{index}"))?;
            blob.extend_from_slice(part);
        }
        if blob.len() < json_len {
            return Err(format!(
                "状态 JSON 被截断：声明 {json_len} 字节，实际拼出 {}",
                blob.len()
            ));
        }
        // 固件按固定块长切分，最后一块可能带 0 填充 —— 只能按声明长度截。
        blob.truncate(json_len);
        let actual = crc16_ccitt(&blob);
        if actual != crc {
            return Err(format!(
                "状态 JSON CRC 校验失败：声明 0x{crc:04X}，实算 0x{actual:04X}"
            ));
        }
        // 必须显式校验 UTF-8：用 `from_utf8_lossy` 会把损坏的中文静默换成 U+FFFD，
        // 那等于把"响应坏了"伪装成"正常但内容奇怪"。
        let json = String::from_utf8(blob).map_err(|e| format!("状态 JSON 不是合法 UTF-8：{e}"))?;
        Ok(Some(json))
    }
}

// ---------------------------------------------------------------------------
// 摘要
// ---------------------------------------------------------------------------

/// 从状态 JSON 里摘出界面要看的几项。
pub fn parse_device_status(json: &str) -> Result<DeviceStatus, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("状态 JSON 解析失败：{e}"))?;
    let firmware = value
        .get("firmware")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "状态 JSON 里没有 firmware 字段".to_string())?
        .to_string();
    let schema = value
        .get("schema")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    Ok(DeviceStatus {
        firmware,
        schema,
        battery_mv: value
            .get("battery_mv")
            .and_then(|v| v.as_u64())
            .and_then(|v| u16::try_from(v).ok()),
        battery_percent: value
            .get("battery_percent")
            .and_then(|v| v.as_u64())
            .and_then(|v| u8::try_from(v).ok()),
    })
}

// ---------------------------------------------------------------------------
// 真机 IO
// ---------------------------------------------------------------------------

/// 发 `0x13` 状态请求并收齐分块，返回状态 JSON 正文。
pub fn query_status_json() -> Result<String, String> {
    let paths = device::enumerate_device_paths();
    if paths.is_empty() {
        return Err(format!(
            "未找到开发板（VID {VID:04X} / PID {PID:04X}），请确认已通过 USB 连接"
        ));
    }
    let request_id = next_request_id();
    let report = build_request(request_id, false);
    let mut last_err = "所有候选 HID 设备都无法完成状态回读".to_string();

    for path in paths {
        let Some(handle) = device::open_matching(&path) else {
            continue;
        };
        // 状态回读走 Feature Report，只有 app-command 集合支持；键盘集合上必然失败。
        if !device::has_feature_report(handle) {
            device::close_handle(handle);
            continue;
        }
        if let Err(e) = transport::send_feature(handle, &report) {
            last_err = format!("状态请求下发失败：{e}");
            device::close_handle(handle);
            continue;
        }

        let deadline = Instant::now() + Duration::from_millis(STATUS_QUERY_TIMEOUT_MS);
        let mut assembler = StatusAssembler::new(request_id);
        let mut outcome: Result<Option<String>, String> = Ok(None);
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let slice = (remaining.as_millis() as u32).min(STATUS_CHUNK_READ_MS).max(1);
            match transport::read_input_report_once(handle, slice) {
                Ok(Some(raw)) => match assembler.push(&raw) {
                    Ok(Some(json)) => {
                        outcome = Ok(Some(json));
                        break;
                    }
                    Ok(None) => continue,
                    Err(e) => {
                        outcome = Err(e);
                        break;
                    }
                },
                // 切片超时：整体 deadline 还早，继续等下一片。
                Ok(None) => continue,
                Err(e) => {
                    outcome = Err(format!("读取状态响应失败：{e}"));
                    break;
                }
            }
        }
        device::close_handle(handle);

        match outcome {
            Ok(Some(json)) => return Ok(json),
            Ok(None) => {
                let (got, total) = assembler.progress();
                let total = total.map(|t| t.to_string()).unwrap_or_else(|| "?".into());
                last_err = format!(
                    "已发出状态请求（request_id={request_id}），{STATUS_QUERY_TIMEOUT_MS}ms 内只收到 {got}/{total} 块\
                     （过期块 {}，畸形块 {}）",
                    assembler.stale_chunks, assembler.malformed_chunks
                );
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// 查询并摘要成 [`DeviceStatus`]。
pub fn query_device_status() -> Result<DeviceStatus, String> {
    let json = query_status_json()?;
    parse_device_status(&json)
}

// ---------------------------------------------------------------------------
// 测试脚手架：按**固件的算法**写一遍编码器
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod test_fixtures {
    use super::*;

    /// 固件 `encode_status_hid_response_chunk` 的等价实现（含 Report ID 首字节）。
    ///
    /// 这个"第二实现"是刻意的：如果我只按自己理解的偏移写解码器、再用手搓字节去测，
    /// 那测的是"我的理解与我的一致"。用固件的算法编码、用我的算法解码，才能抓到偏移错。
    pub fn encode_chunk(report_id: u32, json: &[u8], index: u8, total: u8) -> Vec<u8> {
        let mut payload = vec![0u8; STATUS_MESSAGE_PAYLOAD_LEN];
        payload[0] = KIND_STATUS_RESPONSE;
        payload[1] = index;
        payload[2] = total;
        let offset = index as usize * STATUS_RESPONSE_JSON_PER_CHUNK;
        let chunk_len = STATUS_RESPONSE_JSON_PER_CHUNK.min(json.len() - offset);
        payload[3] = (STATUS_RESPONSE_METADATA_LEN + chunk_len) as u8;
        payload[4] = STATUS_HID_PROTOCOL_VERSION;
        payload[5..9].copy_from_slice(&report_id.to_le_bytes());
        payload[9..11].copy_from_slice(&(json.len() as u16).to_le_bytes());
        payload[11..13].copy_from_slice(&crc16_ccitt(json).to_le_bytes());
        payload[13..13 + chunk_len].copy_from_slice(&json[offset..offset + chunk_len]);
        let mut out = vec![REPORT_ID_APP_COMMAND];
        out.extend_from_slice(&payload);
        out
    }

    /// 一份贴近真机的状态 JSON（2026-09-30 实机回读的正文）。
    pub const SAMPLE_JSON: &str = concat!(
        r#"{"schema":"ai_keyboard.config_status.v1","firmware":"0.4.53-idf-v2-ble-store-v1","#,
        r#""phase":"status","status":"cached","target_platform":"windows","#,
        r#""capabilities":{"semantic_actions":true,"offline_platform_switch":true,"#,
        r#""config_max_bytes":2048,"usb_management_v1":true,"host_action_v1":true},"#,
        r#""saved":true,"bytes":599,"crc16":40258,"ptt_hotkey":"Ctrl+Shift+Space","#,
        r#""edit_ptt_hotkey":"Ctrl+Shift+E","hotkey_mode":"hold","battery_mv":4171,"#,
        r#""battery_percent":97}"#
    );

    /// 把整份 JSON 按固件算法切成一组报告。
    pub fn encode_all(report_id: u32, json: &str) -> Vec<Vec<u8>> {
        let bytes = json.as_bytes();
        let total = chunk_count_for(bytes.len()).expect("测试 JSON 长度必须合法");
        (0..total).map(|i| encode_chunk(report_id, bytes, i, total)).collect()
    }

    /// 收齐一份完整的报告序列。
    pub fn assemble_all(report_id: u32, json: &str) -> Result<String, String> {
        let mut asm = StatusAssembler::new(report_id);
        for report in encode_all(report_id, json) {
            if let Some(done) = asm.push(&report)? {
                return Ok(done);
            }
        }
        Err("测试里没拼出完整 JSON".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::test_fixtures::*;
    use super::*;

    /// 按"载荷索引"改一个字节（`+1` 是把 Report ID 首字节让过去）。
    ///
    /// 载荷索引：0 = kind，1 = chunk_index，2 = total_chunks，3 = 长度，
    /// 4 = 协议版本，5..9 = request_id，9..11 = JSON 总长，11..13 = CRC，13.. = 数据。
    fn poke(report: &mut [u8], payload_index: usize, value: u8) {
        report[1 + payload_index] = value;
    }

    /// 样本 JSON 按协议该切成几块。
    ///
    /// 不写死 10：样本改一个字就全线崩的测试，只会让人去改断言，而不是去改代码。
    fn sample_total() -> u8 {
        chunk_count_for(SAMPLE_JSON.len()).unwrap()
    }

    // ---------- 请求帧 ----------

    #[test]
    fn request_layout_matches_firmware() {
        let r = build_request(0x0102_0304, false);
        assert_eq!(r.len(), 17, "17 = 1 字节 Report ID + 16 字节载荷");
        assert_eq!(r[0], REPORT_ID_STATUS_REQUEST);
        assert_eq!(&r[1..4], b"S3R");
        assert_eq!(r[4], STATUS_HID_PROTOCOL_VERSION);
        assert_eq!(&r[5..9], &[0x04, 0x03, 0x02, 0x01], "request_id 必须小端");
        assert_eq!(r[9], 0);
        assert!(r[10..].iter().all(|&b| b == 0), "尾部必须全 0，否则固件拒收");
    }

    #[test]
    fn request_fresh_flag_sets_only_bit0() {
        let r = build_request(7, true);
        assert_eq!(r[9], STATUS_REQUEST_FLAG_FRESH);
        assert_eq!(r[9] & !STATUS_REQUEST_KNOWN_FLAGS, 0);
    }

    #[test]
    fn request_ids_are_nonzero_and_strictly_increasing() {
        let mut prev = 0u32;
        for _ in 0..1000 {
            let id = next_request_id();
            assert_ne!(id, 0, "0 会被固件当作非法请求");
            assert!(id > prev, "必须严格递增：{id} 不大于 {prev}");
            prev = id;
        }
    }

    // ---------- 分块解码 ----------

    #[test]
    fn decode_rejects_non_status_reports() {
        assert!(decode_chunk(&[]).unwrap().is_none());
        assert!(decode_chunk(&[0x11]).unwrap().is_none());
        // 0x11 但不是 kind 0x04（例如配置回执 0x03）→ 不是状态响应，交给调用方忽略
        let mut ack = vec![0u8; 64];
        ack[0] = REPORT_ID_APP_COMMAND;
        ack[1] = KIND_CONFIG_ACK;
        assert!(decode_chunk(&ack).unwrap().is_none());
        // 别的 Report ID
        let mut other = vec![0u8; 64];
        other[0] = 0x12;
        assert!(decode_chunk(&other).unwrap().is_none());
    }

    #[test]
    fn decode_roundtrip_with_firmware_encoder() {
        // 样本量级要与真机一致（实测 451 字节 / 10 块），否则测出来的分块形态没代表性。
        assert!(
            SAMPLE_JSON.len() > 400 && SAMPLE_JSON.len() <= STATUS_RESPONSE_MAX_JSON_LEN,
            "样本长度 {} 不在真机量级",
            SAMPLE_JSON.len()
        );
        let total = sample_total();
        let reports = encode_all(0xAABB_CCDD, SAMPLE_JSON);
        assert_eq!(reports.len(), total as usize);

        let first = decode_chunk(&reports[0]).unwrap().unwrap();
        assert_eq!(first.request_id, 0xAABB_CCDD);
        assert_eq!(first.index, 0);
        assert_eq!(first.total, total);
        assert_eq!(first.json_len, SAMPLE_JSON.len());
        assert_eq!(first.crc16, crc16_ccitt(SAMPLE_JSON.as_bytes()));
        assert_eq!(first.data.len(), STATUS_RESPONSE_JSON_PER_CHUNK);

        let last = decode_chunk(&reports[total as usize - 1]).unwrap().unwrap();
        let tail = SAMPLE_JSON.len() - (total as usize - 1) * STATUS_RESPONSE_JSON_PER_CHUNK;
        assert_eq!(last.index, total - 1);
        assert_eq!(last.data.len(), tail, "最后一块只带余数，不带 0 填充");
    }

    #[test]
    fn decode_rejects_truncated_and_bad_fields() {
        // 长度不够（连固定头都放不下）
        let mut short = vec![REPORT_ID_APP_COMMAND, KIND_STATUS_RESPONSE];
        short.extend_from_slice(&[0u8; 5]);
        assert!(decode_chunk(&short).is_err());

        let good = encode_chunk(1, SAMPLE_JSON.as_bytes(), 0, sample_total());
        assert!(decode_chunk(&good).unwrap().is_some(), "基准帧必须是合法的");

        // total 与 JSON 长度不自洽（样本必然不是 3 块）
        let mut bad_total = good.clone();
        assert_ne!(sample_total(), 3);
        poke(&mut bad_total, 2, 3);
        assert!(decode_chunk(&bad_total).is_err());

        // chunk_index 越界
        let mut bad_index = good.clone();
        poke(&mut bad_index, 1, sample_total() + 1);
        assert!(decode_chunk(&bad_index).is_err());

        // 协议版本不符
        let mut bad_ver = good.clone();
        poke(&mut bad_ver, 4, 9);
        assert!(decode_chunk(&bad_ver).is_err());

        // request_id = 0
        let mut zero_id = good.clone();
        poke(&mut zero_id, 5, 0);
        assert!(decode_chunk(&zero_id).is_err());

        // JSON 总长越界（65535 字节 > 固件 512 上限）
        let mut huge = good.clone();
        poke(&mut huge, 9, 0xFF);
        poke(&mut huge, 10, 0xFF);
        assert!(decode_chunk(&huge).is_err());

        // 声明长度超过单块上限
        let mut fat = good.clone();
        poke(&mut fat, 3, (STATUS_RESPONSE_METADATA_LEN + STATUS_RESPONSE_JSON_PER_CHUNK + 1) as u8);
        assert!(decode_chunk(&fat).is_err());
    }

    #[test]
    fn chunk_count_matches_firmware_rule() {
        assert_eq!(chunk_count_for(0), None);
        assert_eq!(chunk_count_for(1), Some(1));
        assert_eq!(chunk_count_for(50), Some(1));
        assert_eq!(chunk_count_for(51), Some(2));
        assert_eq!(chunk_count_for(451), Some(10));
        assert_eq!(chunk_count_for(500), Some(10));
        assert_eq!(chunk_count_for(501), Some(11));
        assert_eq!(chunk_count_for(512), Some(11));
        assert_eq!(chunk_count_for(513), None, "超过固件 512 字节上限必须拒绝");
    }

    // ---------- 组装 ----------

    #[test]
    fn assembler_roundtrip() {
        let json = assemble_all(42, SAMPLE_JSON).unwrap();
        assert_eq!(json, SAMPLE_JSON);
    }

    #[test]
    fn assembler_accepts_out_of_order_chunks() {
        let mut reports = encode_all(42, SAMPLE_JSON);
        reports.reverse();
        let mut asm = StatusAssembler::new(42);
        let mut done = None;
        for r in reports {
            if let Some(j) = asm.push(&r).unwrap() {
                done = Some(j);
            }
        }
        assert_eq!(done.unwrap(), SAMPLE_JSON, "分块顺序不应影响结果");
    }

    #[test]
    fn assembler_ignores_other_request_ids_and_counts_them() {
        let total = sample_total();
        let mut asm = StatusAssembler::new(42);
        for r in encode_all(999, SAMPLE_JSON) {
            assert!(asm.push(&r).unwrap().is_none());
        }
        assert_eq!(
            asm.stale_chunks,
            total as u32,
            "过期块要被计数 —— 它是区分「板子没回」和「回的上一轮」的唯一线索"
        );
        assert_eq!(asm.progress(), (0, None));
        // 正确 id 的块仍然能收
        let json = {
            let mut got = None;
            for r in encode_all(42, SAMPLE_JSON) {
                if let Some(j) = asm.push(&r).unwrap() {
                    got = Some(j);
                }
            }
            got
        };
        assert_eq!(json.unwrap(), SAMPLE_JSON);
    }

    #[test]
    fn assembler_tolerates_duplicate_identical_chunk() {
        let mut asm = StatusAssembler::new(42);
        let reports = encode_all(42, SAMPLE_JSON);
        // 第 1 块发两次
        assert!(asm.push(&reports[0]).unwrap().is_none());
        assert!(asm.push(&reports[0]).unwrap().is_none());
        let mut done = None;
        for r in &reports[1..] {
            if let Some(j) = asm.push(r).unwrap() {
                done = Some(j);
            }
        }
        assert_eq!(done.unwrap(), SAMPLE_JSON);
    }

    #[test]
    fn assembler_rejects_conflicting_duplicate_chunk() {
        let mut asm = StatusAssembler::new(42);
        let reports = encode_all(42, SAMPLE_JSON);
        assert!(asm.push(&reports[0]).unwrap().is_none());
        let mut mutated = reports[0].clone();
        let flipped = mutated[14] ^ 0xFF; // 只改数据区第一个字节，元数据（长度/CRC）不动
        poke(&mut mutated, 13, flipped);
        let err = asm.push(&mutated).unwrap_err();
        assert!(err.contains("重复且内容不同"), "实际：{err}");
    }

    #[test]
    fn assembler_rejects_inconsistent_metadata() {
        let mut asm = StatusAssembler::new(42);
        let reports = encode_all(42, SAMPLE_JSON);
        assert!(asm.push(&reports[0]).unwrap().is_none());
        // 换一份不同长度的 JSON 的第 2 块：元数据必然对不上
        let other = encode_all(42, &"x".repeat(60));
        let err = asm.push(&other[1]).unwrap_err();
        assert!(err.contains("元数据不一致"), "实际：{err}");
    }

    #[test]
    fn assembler_fails_on_crc_mismatch() {
        // 手工造一套 CRC 与实际内容不符的块
        let bytes = b"{\"firmware\":\"0.0.0\"}";
        let total = chunk_count_for(bytes.len()).unwrap();
        let mut reports = Vec::new();
        for i in 0..total {
            let mut r = encode_chunk(7, bytes, i, total);
            let broken = r[12] ^ 0xFF; // 破坏 CRC 低字节
            poke(&mut r, 11, broken);
            reports.push(r);
        }
        let mut asm = StatusAssembler::new(7);
        let mut err = None;
        for r in reports {
            match asm.push(&r) {
                Ok(Some(_)) => panic!("CRC 不符时不该判定为收齐"),
                Ok(None) => {}
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }
        assert!(err.unwrap().contains("CRC"), "必须报 CRC 而不是静默返回");
    }

    #[test]
    fn assembler_rejects_invalid_utf8() {
        let bytes: &[u8] = &[b'{', 0xFF, 0xFE, b'}'];
        let total = chunk_count_for(bytes.len()).unwrap();
        let mut asm = StatusAssembler::new(7);
        let mut err = None;
        for i in 0..total {
            match asm.push(&encode_chunk(7, bytes, i, total)) {
                Ok(Some(_)) => panic!("非法 UTF-8 不该被当成合法 JSON 返回"),
                Ok(None) => {}
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }
        assert!(err.unwrap().contains("UTF-8"));
    }

    #[test]
    fn assembler_reports_partial_progress_on_timeout() {
        let mut asm = StatusAssembler::new(42);
        for r in encode_all(42, SAMPLE_JSON).into_iter().take(3) {
            assert!(asm.push(&r).unwrap().is_none());
        }
        assert_eq!(asm.progress(), (3, Some(sample_total())));
    }

    // ---------- 摘要 ----------

    #[test]
    fn parses_real_device_payload() {
        let s = parse_device_status(SAMPLE_JSON).unwrap();
        assert_eq!(s.firmware, "0.4.53-idf-v2-ble-store-v1");
        assert_eq!(s.schema, "ai_keyboard.config_status.v1");
        assert_eq!(s.battery_mv, Some(4171));
        assert_eq!(s.battery_percent, Some(97));
    }

    #[test]
    fn summary_tolerates_missing_optional_fields() {
        let s = parse_device_status(r#"{"schema":"x","firmware":"1.2.3"}"#).unwrap();
        assert_eq!(s.firmware, "1.2.3");
        assert_eq!(s.battery_mv, None);
        assert_eq!(s.battery_percent, None);
    }

    #[test]
    fn summary_requires_non_empty_firmware() {
        assert!(parse_device_status(r#"{"schema":"x"}"#).is_err());
        assert!(parse_device_status(r#"{"firmware":""}"#).is_err());
        assert!(parse_device_status("not json").is_err());
    }
}
