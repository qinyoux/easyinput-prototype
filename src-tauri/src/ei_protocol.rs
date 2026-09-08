//! EasyInput 音频/控制协议编解码（EIAU / EIHB / EICC / EICA）。
//!
//! 与固件 `audio_packet_wire.h` / `audio-control-v1.md` 一一对应。
//! 所有多字节字段均为小端。

use std::convert::TryInto;

// ---------- 魔数 ----------

pub const MAGIC_EIAU: &[u8; 4] = b"EIAU";
pub const MAGIC_EIHB: &[u8; 4] = b"EIHB";
pub const MAGIC_EICC: &[u8; 4] = b"EICC";
pub const MAGIC_EICA: &[u8; 4] = b"EICA";

// ---------- EIAU v2 音频包 ----------

/// EIAU v2 头部长度（固定 32 字节）。
pub const EIAU_HEADER_LEN: usize = 32;

/// EIAU v2 音频包解析结果。
pub struct EiauPacket<'a> {
    pub session_id: u64,
    pub capture_seq: u32,
    pub sample_rate: u32,
    pub timestamp_ms: u32,
    pub frame_samples: u16,
    pub payload_bytes: u16,
    pub pcm: &'a [u8],
}

impl<'a> EiauPacket<'a> {
    /// 从原始字节解析 EIAU v2 包，返回 PCM 数据切片（引用原 buffer）。
    pub fn parse(buf: &'a [u8]) -> Result<Self, &'static str> {
        if buf.len() < EIAU_HEADER_LEN {
            return Err("EIAU 包过短");
        }
        if &buf[0..4] != MAGIC_EIAU {
            return Err("EIAU 魔数不匹配");
        }
        if buf[4] != 2 {
            return Err("EIAU 版本不支持");
        }
        let header_len = buf[5] as usize;
        if header_len < EIAU_HEADER_LEN || buf.len() < header_len {
            return Err("EIAU 头部长度非法");
        }
        let session_id = u64::from_le_bytes(buf[8..16].try_into().unwrap());
        let capture_seq = u32::from_le_bytes(buf[16..20].try_into().unwrap());
        let sample_rate = u32::from_le_bytes(buf[20..24].try_into().unwrap());
        let timestamp_ms = u32::from_le_bytes(buf[24..28].try_into().unwrap());
        let frame_samples = u16::from_le_bytes(buf[28..30].try_into().unwrap());
        let payload_bytes = u16::from_le_bytes(buf[30..32].try_into().unwrap());
        let pcm = &buf[header_len..];
        if pcm.len() < payload_bytes as usize {
            return Err("EIAU 负载截断");
        }
        Ok(Self {
            session_id,
            capture_seq,
            sample_rate,
            timestamp_ms,
            frame_samples,
            payload_bytes,
            pcm: &pcm[..payload_bytes as usize],
        })
    }
}

// ---------- EIHB 心跳包 ----------

/// EIHB 心跳包解析结果。
pub struct EihbHeartbeat {
    pub flags: u8,
    pub session_id: u64,
    pub seq: u32,
}

impl EihbHeartbeat {
    /// 从原始字节解析 EIHB 心跳包（20 字节）。
    pub fn parse(buf: &[u8]) -> Result<Self, &'static str> {
        if buf.len() < 20 {
            return Err("EIHB 包过短");
        }
        if &buf[0..4] != MAGIC_EIHB {
            return Err("EIHB 魔数不匹配");
        }
        Ok(Self {
            flags: buf[5],
            session_id: u64::from_le_bytes(buf[8..16].try_into().unwrap()),
            seq: u32::from_le_bytes(buf[16..20].try_into().unwrap()),
        })
    }

    /// bit0: 正在流式传输音频。
    pub fn is_streaming(&self) -> bool {
        self.flags & 0x01 != 0
    }

    /// bit1: 音频设备就绪。
    pub fn audio_ready(&self) -> bool {
        self.flags & 0x02 != 0
    }
}

// ---------- EICC 控制命令 ----------

/// EICC action 值。
pub const EICC_ACTION_START: u8 = 1;
pub const EICC_ACTION_STOP: u8 = 2;
pub const EICC_ACTION_KEEPALIVE: u8 = 3;

/// 构造 EICC 控制命令包（36 字节）。
pub fn build_eicc(action: u8, session_id: u64, seq: u32) -> [u8; 36] {
    let mut buf = [0u8; 36];
    buf[0..4].copy_from_slice(MAGIC_EICC);
    buf[4] = 1; // version
    buf[5] = action;
    // buf[6..8] reserved
    buf[8..16].copy_from_slice(&session_id.to_le_bytes());
    buf[16..20].copy_from_slice(&seq.to_le_bytes());
    // buf[20..36] token (reserved, all zero)
    buf
}

// ---------- EICA 确认包 ----------

/// EICA 确认包解析结果。
pub struct EicaAck {
    pub action: u8,
    pub status: u8,
    pub session_id: u64,
    pub seq: u32,
}

impl EicaAck {
    /// 从原始字节解析 EICA 确认包（20 字节）。
    pub fn parse(buf: &[u8]) -> Result<Self, &'static str> {
        if buf.len() < 20 {
            return Err("EICA 包过短");
        }
        if &buf[0..4] != MAGIC_EICA {
            return Err("EICA 魔数不匹配");
        }
        Ok(Self {
            action: buf[5],
            status: buf[6],
            session_id: u64::from_le_bytes(buf[8..16].try_into().unwrap()),
            seq: u32::from_le_bytes(buf[16..20].try_into().unwrap()),
        })
    }

    /// status == 0 表示成功。
    pub fn is_ok(&self) -> bool {
        self.status == 0
    }
}

// ---------- 包类型分发 ----------

/// 收到的 UDP 包类型。
pub enum EiPacket<'a> {
    Eiau(EiauPacket<'a>),
    Eihb(EihbHeartbeat),
    Eica(EicaAck),
    Unknown,
}

/// 按魔数自动分发解析。
pub fn dispatch<'a>(buf: &'a [u8]) -> EiPacket<'a> {
    if buf.len() < 4 {
        return EiPacket::Unknown;
    }
    let magic = &buf[0..4];
    if magic == MAGIC_EIAU {
        match EiauPacket::parse(buf) {
            Ok(p) => EiPacket::Eiau(p),
            Err(_) => EiPacket::Unknown,
        }
    } else if magic == MAGIC_EIHB {
        match EihbHeartbeat::parse(buf) {
            Ok(p) => EiPacket::Eihb(p),
            Err(_) => EiPacket::Unknown,
        }
    } else if magic == MAGIC_EICA {
        match EicaAck::parse(buf) {
            Ok(p) => EiPacket::Eica(p),
            Err(_) => EiPacket::Unknown,
        }
    } else {
        EiPacket::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eicc_roundtrip() {
        let buf = build_eicc(EICC_ACTION_START, 12345, 1);
        assert_eq!(&buf[0..4], MAGIC_EICC);
        assert_eq!(buf[4], 1);
        assert_eq!(buf[5], EICC_ACTION_START);
        assert_eq!(u64::from_le_bytes(buf[8..16].try_into().unwrap()), 12345);
        assert_eq!(u32::from_le_bytes(buf[16..20].try_into().unwrap()), 1);
    }

    #[test]
    fn eihb_parse() {
        let mut buf = [0u8; 20];
        buf[0..4].copy_from_slice(MAGIC_EIHB);
        buf[4] = 1;
        buf[5] = 0x03; // streaming + audio_ready
        buf[8..16].copy_from_slice(&42u64.to_le_bytes());
        buf[16..20].copy_from_slice(&7u32.to_le_bytes());
        let hb = EihbHeartbeat::parse(&buf).unwrap();
        assert!(hb.is_streaming());
        assert!(hb.audio_ready());
        assert_eq!(hb.session_id, 42);
        assert_eq!(hb.seq, 7);
    }

    #[test]
    fn eiau_parse() {
        let pcm = vec![0x01u8, 0x02, 0x03, 0x04];
        let mut buf = vec![0u8; EIAU_HEADER_LEN];
        buf[0..4].copy_from_slice(MAGIC_EIAU);
        buf[4] = 2; // version
        buf[5] = EIAU_HEADER_LEN as u8;
        buf[6] = 1; // codec PCM S16LE
        buf[7] = 1; // mono
        buf[8..16].copy_from_slice(&99u64.to_le_bytes());
        buf[20..24].copy_from_slice(&16000u32.to_le_bytes());
        buf[30..32].copy_from_slice(&(pcm.len() as u16).to_le_bytes());
        buf.extend_from_slice(&pcm);
        let pkt = EiauPacket::parse(&buf).unwrap();
        assert_eq!(pkt.session_id, 99);
        assert_eq!(pkt.sample_rate, 16000);
        assert_eq!(pkt.pcm, &pcm[..]);
    }

    #[test]
    fn dispatch_unknown() {
        assert!(matches!(dispatch(b"XXXX"), EiPacket::Unknown));
        assert!(matches!(dispatch(b""), EiPacket::Unknown));
    }
}
