//! 构造固件配置 JSON（`ai_keyboard.v1`）与解析 0x11 回执。
//!
//! `to_firmware_json` 把本地 `Profile` 转换成固件 `config_payload.cpp` 严格声称的
//! 结构：`keys` 条目包 `{press: ...}`，`encoder` 条目用裸值，两者都必须含全部字段。

use crate::constants::*;
use crate::model::{ActionSpec, ConfigAck, Profile, ENCODER_INPUTS, KEY_INPUTS};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use uuid::Uuid;

/// 把单个动作转成固件可接受的 JSON 值。
///
/// - `Disabled` / `Named` → 裸字符串
/// - `Hotkey` → `{"hotkey": "..."}`
/// - `Text` → `{"text": "..."}`
/// - `HostAction` → 裸字符串 `"host_action:<uuid>"`
///
/// 方案切换命名动作（`toggle_profile` / `previous_profile` / `next_profile`）
/// 被转为保留 UUID 的 `host_action` 裸字符串，使固件回发 0x11 kind 0x05 报文，
/// 主机端拦截后执行方案切换并重新下发当前方案。
pub fn to_firmware_value(action: &ActionSpec) -> Value {
    match action {
        ActionSpec::Disabled => Value::String("disabled".into()),
        ActionSpec::Named(name) => {
            if name == "toggle_profile" {
                Value::String(format!("host_action:{UUID_TOGGLE_PROFILE}"))
            } else if name == "previous_profile" {
                Value::String(format!("host_action:{UUID_PREVIOUS_PROFILE}"))
            } else if name == "next_profile" {
                Value::String(format!("host_action:{UUID_NEXT_PROFILE}"))
            } else {
                Value::String(name.clone())
            }
        }
        ActionSpec::Hotkey(hk) => json!({ "hotkey": hk }),
        ActionSpec::Text(text) => json!({ "text": text }),
        ActionSpec::HostAction(value) => Value::String(value.clone()),
    }
}

/// 把一个方案序列化为固件配置 JSON 字符串。
///
/// `text_map` 接收含非ASCII字符的固定文本动作映射（UUID → 文本内容），
/// 供主机端在固件回发 UUID 时用剪贴板+Ctrl+V 注入。
pub fn to_firmware_json(profile: &Profile, text_map: &mut HashMap<String, String>) -> String {
    let mut keys = Map::new();
    for input in KEY_INPUTS {
        let action = profile.action_for(input);
        keys.insert(input.to_string(), json!({ "press": action_to_firmware(&action, text_map) }));
    }
    let mut encoder = Map::new();
    for input in ENCODER_INPUTS {
        let action = profile.action_for(input);
        encoder.insert(input.to_string(), action_to_firmware(&action, text_map));
    }
    let payload = json!({
        "schema": SCHEMA_VERSION,
        "target_platform": "windows",
        "profiles": [ { "keys": keys, "encoder": encoder } ],
    });
    serde_json::to_string(&payload).unwrap_or_else(|_| {
        // 理论上不会失败；兜底返回最简 schema。
        format!(r#"{{"schema":"{SCHEMA_VERSION}","target_platform":"windows","profiles":[{{}}]}}"#)
    })
}

/// 把单个动作转成固件可接受的 JSON 值，含非ASCII的 Text 转为 host_action。
fn action_to_firmware(action: &ActionSpec, text_map: &mut HashMap<String, String>) -> Value {
    if let ActionSpec::Text(text) = action {
        if text.chars().any(|c| !c.is_ascii()) {
            let uuid = Uuid::new_v4().to_string();
            text_map.insert(uuid.clone(), text.clone());
            return Value::String(format!("host_action:{uuid}"));
        }
    }
    to_firmware_value(action)
}

/// 从 0x11（kind 0x03）回执缓冲（前置 Report ID 首字节）解析确认信息。
/// 对齐固件回执布局：`[id, kind, chunk, total, len, phase, ok, bytes_lo, bytes_hi, crc_lo, crc_hi, saved]`。
pub fn parse_ack(report: &[u8]) -> ConfigAck {
    let u8_at = |i: usize| report.get(i).copied().unwrap_or(0);
    let u16_le_at = |i: usize| u16::from_le_bytes([u8_at(i), u8_at(i + 1)]);
    ConfigAck {
        phase: u8_at(ACK_OFF_PHASE),
        ok: u8_at(ACK_OFF_OK) != 0,
        bytes: u16_le_at(ACK_OFF_BYTES),
        crc16: u16_le_at(ACK_OFF_CRC),
        saved: u8_at(ACK_OFF_SAVED) != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firmware_json_shape() {
        let p = Profile::factory_default("默认", "p1");
        let mut text_map = HashMap::new();
        let json = to_firmware_json(&p, &mut text_map);
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["schema"], SCHEMA_VERSION);
        let prof = &v["profiles"][0];
        assert!(prof["keys"]["KEY1"]["press"].is_string());
        assert!(prof["keys"]["KEY5"]["press"].is_string());
        // encoder 为裸值
        assert_eq!(prof["encoder"]["left"], "disabled");
        assert_eq!(prof["encoder"]["press"], "scroll_axis_toggle");
        // 全部字段都在位
        for input in crate::model::ALL_INPUTS {
            if KEY_INPUTS.contains(&input) {
                assert!(prof["keys"][input].is_object());
            } else {
                assert!(prof["encoder"][input].is_string());
            }
        }
        // 默认方案无含非ASCII的文本动作
        assert!(text_map.is_empty());
    }

    #[test]
    fn custom_values() {
        let mut p = Profile::factory_default("默认", "p1");
        p.keys.insert("KEY2".into(), ActionSpec::Hotkey("Ctrl+Alt+A".into()));
        p.keys.insert("KEY3".into(), ActionSpec::Text("你好".into()));
        p.keys.insert("KEY4".into(), ActionSpec::HostAction("host_action:12345678-1234-4abc-8def-1234567890ab".into()));
        let mut text_map = HashMap::new();
        let v: Value = serde_json::from_str(&to_firmware_json(&p, &mut text_map)).unwrap();
        assert_eq!(v["profiles"][0]["keys"]["KEY2"]["press"]["hotkey"], "Ctrl+Alt+A");
        // 含非ASCII的 Text 被转为 host_action 裸字符串
        let key3_val = v["profiles"][0]["keys"]["KEY3"]["press"].as_str().unwrap();
        assert!(key3_val.starts_with("host_action:"));
        assert_eq!(v["profiles"][0]["keys"]["KEY4"]["press"], "host_action:12345678-1234-4abc-8def-1234567890ab");
        // text_map 中应有一条映射
        assert_eq!(text_map.len(), 1);
        assert!(text_map.values().any(|v| v == "你好"));
    }

    #[test]
    fn ascii_text_stays_text() {
        let mut p = Profile::factory_default("默认", "p1");
        p.keys.insert("KEY1".into(), ActionSpec::Text("hello 123".into()));
        let mut text_map = HashMap::new();
        let v: Value = serde_json::from_str(&to_firmware_json(&p, &mut text_map)).unwrap();
        assert_eq!(v["profiles"][0]["keys"]["KEY1"]["press"]["text"], "hello 123");
        assert!(text_map.is_empty());
    }
}
