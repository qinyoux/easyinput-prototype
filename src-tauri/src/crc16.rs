//! CRC-16/CCITT-FALSE：与固件 `crc16_ccitt` 一致（init 0xFFFF、poly 0x1021、MSB-first）。

/// 计算 `crc16_ccitt`，对完整配置 JSON 字节计算。
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}
