//! 构造固件配置 JSON（`ai_keyboard.v1`）与解析 0x11 回执。
//!
//! `build_firmware_payload_with_wifi` 把本地 `Profile` 转换成固件 `config_payload.cpp` 严格声称的
//! 结构：`keys` 条目包 `{press: ...}`，`encoder` 条目用裸值，两者都必须含全部字段。
//! 它与固件 JSON 一并产出两张反向索引表（UUID → 正文 / UUID → 输入项），
//! 因为含非ASCII的固定文本每次下发都会拿到新的随机 UUID，索引必须与本次下发同源。

use crate::constants::*;
use crate::model::{ActionSpec, ConfigAck, Profile, ENCODER_INPUTS, KEY_INPUTS};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use uuid::Uuid;

/// 具名动作 → 主机端保留 UUID。
///
/// 这些动作的语义完全在主机侧实现（方案切换、浮窗工作台），固件只需要把触发
/// 原样回传。命中者在下发时被改写成 `host_action:<uuid>` 裸字符串。
pub fn reserved_uuid_for_named(name: &str) -> Option<&'static str> {
    match name {
        "toggle_profile" => Some(UUID_TOGGLE_PROFILE),
        "previous_profile" => Some(UUID_PREVIOUS_PROFILE),
        "next_profile" => Some(UUID_NEXT_PROFILE),
        "workbench" => Some(UUID_WORKBENCH),
        "workbench_prev" => Some(UUID_WORKBENCH_PREV),
        "workbench_next" => Some(UUID_WORKBENCH_NEXT),
        "workbench_confirm" => Some(UUID_WORKBENCH_CONFIRM),
        "ss_palette" => Some(UUID_SS_PALETTE),
        "cli_terminal" => Some(UUID_CLI_TERMINAL),

        // 插件声明的命令（`<plugin>.<command>`）：启动时已注册保留 UUID。
        _ => crate::plugin_registry::plugin_action_uuid(name),
    }
}

/// 保留 UUID → 具名动作（`reserved_uuid_for_named` 的逆映射）。
///
/// 用于把固件回发的 UUID 反查回「是哪个语义」，以及输入来源反查时比对固件值。
pub fn named_for_reserved_uuid(uuid: &str) -> Option<&'static str> {
    match uuid {
        UUID_TOGGLE_PROFILE => Some("toggle_profile"),
        UUID_PREVIOUS_PROFILE => Some("previous_profile"),
        UUID_NEXT_PROFILE => Some("next_profile"),
        UUID_WORKBENCH => Some("workbench"),
        UUID_WORKBENCH_PREV => Some("workbench_prev"),
        UUID_WORKBENCH_NEXT => Some("workbench_next"),
        UUID_WORKBENCH_CONFIRM => Some("workbench_confirm"),
        UUID_SS_PALETTE => Some("ss_palette"),
        UUID_CLI_TERMINAL => Some("cli_terminal"),

        // 插件声明的命令（`<plugin>.<command>`）：逆查得到全限定命令 id。
        _ => crate::plugin_registry::resolve_action_uuid(uuid).map(|(_, c)| c),
    }
}

/// 把单个动作转成固件可接受的 JSON 值。
///
/// - `Disabled` / `Named` → 裸字符串
/// - `Hotkey` → `{"hotkey": "..."}`
/// - `Text` → `{"text": "..."}`
/// - `HostAction` / `TaskDesk` → 裸字符串 `"host_action:<uuid>"`
///
/// 主机端保留具名动作（方案切换三件套、浮窗工作台四件套）被转为保留 UUID 的
/// `host_action` 裸字符串，使固件回发 0x11 kind 0x05 报文，主机端拦截后执行。
/// 任务台槽位按键走同一条通道（固件对按键动作是白名单制，这是唯一能携带
/// 自定义语义的形式）。
pub fn to_firmware_value(action: &ActionSpec) -> Value {
    match action {
        ActionSpec::Disabled => Value::String("disabled".into()),
        ActionSpec::Named(name) => {
            if let Some(uuid) = reserved_uuid_for_named(name) {
                Value::String(format!("{HOST_ACTION_PREFIX}{uuid}"))
            } else {
                Value::String(name.clone())
            }
        }
        ActionSpec::Hotkey(hk) => json!({ "hotkey": hk }),
        ActionSpec::Text(text) => json!({ "text": text }),
        ActionSpec::HostAction(value) => Value::String(value.clone()),
        ActionSpec::TaskDesk(_) => match action.desk_slot().and_then(crate::constants::desk_slot_uuid)
        {
            Some(uuid) => Value::String(format!("{HOST_ACTION_PREFIX}{uuid}")),
            None => {
                // 失败要看得见：这里没法返回 Err（下发路径的签名是 Value），
                // 所以显式记一条错误日志，再降级成 disabled——**绝不**静默发一个
                // 固件会拒收的字符串（那会让整份配置下发失败，用户只看到"按键失灵"）。
                crate::log_buffer::err(format!(
                    "任务台按键槽位号非法（{action:?}），本次下发已降级为 disabled"
                ));
                Value::String("disabled".into())
            }
        },
        ActionSpec::Studio(v) => {
            let uuid = match v.as_str() {
                "prev" => UUID_STUDIO_PREV,
                "next" => UUID_STUDIO_NEXT,
                "confirm" => UUID_STUDIO_CONFIRM,
                _ => {
                    crate::log_buffer::err(format!(
                        "内容台动作值非法（{action:?}），本次下发已降级为 disabled"
                    ));
                    return Value::String("disabled".into());
                }
            };
            Value::String(format!("{HOST_ACTION_PREFIX}{uuid}"))
        }
    }
}

/// 一次方案下发的产物：固件 JSON + 主机端需要的文本动作反查表。
///
/// `text_map` 必须与本次下发的 JSON 来自同一次遍历：含非ASCII的固定文本在每次
/// 下发时都会拿到一个新的 v4 UUID，脱离本次遍历独立推导出来的 UUID 与开发板上
/// 正在生效的那一份对不上。
///
/// 改造前这里还有第二张表 `text_sources`（UUID → 硬件输入），它只服务于浮窗的
/// 「按键动作列表」做反查高亮。浮窗改成提示词库后不再列按键动作，该表已无消费方，
/// 故一并移除 —— 少一张没有读者的表，也少一处将来会被人误以为「在用」的假接口。
pub struct FirmwarePayload {
    /// 固件配置 JSON。
    pub json: String,
    /// UUID → 文本正文（固件回传该 UUID 时用于剪贴板上屏）。
    pub text_map: HashMap<String, String>,
}

/// 开发板 Wi-Fi 音频端点（随配置一起下发的四个顶层字段）。
///
/// ⛔ **为什么它必须跟着每一次下发走 —— 这是 2026-09-24 查出来的真 bug。**
///
/// 固件 `ConfigState::apply_json` 是**整体替换**：`current_ = parsed.config`，
/// 而 `ParsedKeyboardConfig` 里没被 JSON 提到的字段一律回到**默认值**
/// （`wifi_ssid` / `audio_host` 默认都是空串）。于是"下发方案"这件事的副作用是
/// **把开发板上已经配好的 Wi-Fi 端点清空** —— 板子随即按 `audio_enabled == false`
/// 关掉 Wi-Fi，而界面上一切正常、没有任何报错。
///
/// 本仓库过去只发 `schema` / `target_platform` / `profiles`（见 git 历史），
/// 所以「按一次方案切换，板子麦克风就掉线」是必然发生的，不是偶发。
///
/// 修法就是把端点字段补全，让**每一次**下发都携带当前端点 ——
/// 而不是"在下发之后补一条 Wi-Fi 命令"（那样中间有一段窗口是坏的，
/// 而且补发的那条又得是完整载荷，等于把同一件事做两遍）。
#[derive(Clone, Copy, Debug, Default)]
pub struct WifiEndpoint<'a> {
    pub ssid: &'a str,
    pub password: &'a str,
    /// 上位机的 IPv4（固件 `config_ready` 要求非空，否则连 Wi-Fi 都不会尝试）。
    pub host: &'a str,
    pub port: u16,
}

/// 构造一次完整下发产物，可选携带 Wi-Fi 音频端点。
///
/// `wifi` 为 `Some` 时，四个顶层字段（`wifi_ssid` / `wifi_password` /
/// `audio_host` / `audio_port`）会被写进 JSON —— 固件据此连接并持久化到 NVS。
/// 传入空 SSID 是**合法且有语义**的：它表示"关掉板子的 Wi-Fi 音频"。
pub fn build_firmware_payload_with_wifi(
    profile: &Profile,
    wifi: Option<WifiEndpoint<'_>>,
) -> FirmwarePayload {
    let mut text_map = HashMap::new();

    let mut keys = Map::new();
    for input in KEY_INPUTS {
        let action = profile.action_for(input);
        let value = action_to_firmware(&action, &mut text_map);
        keys.insert(input.to_string(), json!({ "press": value }));
    }
    let mut encoder = Map::new();
    for input in ENCODER_INPUTS {
        let action = profile.action_for(input);
        let value = action_to_firmware(&action, &mut text_map);
        encoder.insert(input.to_string(), value);
    }

    let mut payload = json!({
        "schema": SCHEMA_VERSION,
        "target_platform": "windows",
        "profiles": [ { "keys": keys, "encoder": encoder } ],
    });
    // 端点字段按需追加。`wifi_password` 为空串也照发（开放网络 / guest 网络都合法），
    // 判据是"这次下发要不要谈 Wi-Fi"，而不是"密码非空才发"。
    if let Some(endpoint) = wifi {
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("wifi_ssid".to_string(), json!(endpoint.ssid));
            obj.insert("wifi_password".to_string(), json!(endpoint.password));
            obj.insert("audio_host".to_string(), json!(endpoint.host));
            obj.insert("audio_port".to_string(), json!(endpoint.port));
        }
    }
    let json = serde_json::to_string(&payload).unwrap_or_else(|_| {
        // 理论上不会失败；兜底返回最简 schema。
        format!(r#"{{"schema":"{SCHEMA_VERSION}","target_platform":"windows","profiles":[{{}}]}}"#)
    });

    FirmwarePayload { json, text_map }
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
        let built = build_firmware_payload_with_wifi(&p, None);
        let v: Value = serde_json::from_str(&built.json).unwrap();
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
        assert!(built.text_map.is_empty());
    }

    #[test]
    fn custom_values() {
        let mut p = Profile::factory_default("默认", "p1");
        p.keys.insert("KEY2".into(), ActionSpec::Hotkey("Ctrl+Alt+A".into()));
        p.keys.insert("KEY3".into(), ActionSpec::Text("你好".into()));
        p.keys.insert("KEY4".into(), ActionSpec::HostAction("host_action:12345678-1234-4abc-8def-1234567890ab".into()));
        let built = build_firmware_payload_with_wifi(&p, None);
        let v: Value = serde_json::from_str(&built.json).unwrap();
        assert_eq!(v["profiles"][0]["keys"]["KEY2"]["press"]["hotkey"], "Ctrl+Alt+A");
        // 含非ASCII的 Text 被转为 host_action 裸字符串
        let key3_val = v["profiles"][0]["keys"]["KEY3"]["press"].as_str().unwrap();
        assert!(key3_val.starts_with("host_action:"));
        assert_eq!(v["profiles"][0]["keys"]["KEY4"]["press"], "host_action:12345678-1234-4abc-8def-1234567890ab");
        // text_map 中应有一条映射
        assert_eq!(built.text_map.len(), 1);
        assert!(built.text_map.values().any(|v| v == "你好"));
    }

    #[test]
    fn ascii_text_stays_text() {
        let mut p = Profile::factory_default("默认", "p1");
        p.keys.insert("KEY1".into(), ActionSpec::Text("hello 123".into()));
        let built = build_firmware_payload_with_wifi(&p, None);
        let v: Value = serde_json::from_str(&built.json).unwrap();
        assert_eq!(v["profiles"][0]["keys"]["KEY1"]["press"]["text"], "hello 123");
        assert!(built.text_map.is_empty());
    }

    #[test]
    fn workbench_named_maps_to_reserved_uuid() {
        let mut p = Profile::factory_default("默认", "p1");
        p.keys.insert("KEY4".into(), ActionSpec::Named("workbench".into()));
        p.encoder.insert("left".into(), ActionSpec::Named("workbench_prev".into()));
        p.encoder.insert("right".into(), ActionSpec::Named("workbench_next".into()));
        p.encoder.insert("press".into(), ActionSpec::Named("workbench_confirm".into()));
        let built = build_firmware_payload_with_wifi(&p, None);
        let v: Value = serde_json::from_str(&built.json).unwrap();
        let prof = &v["profiles"][0];
        assert_eq!(
            prof["keys"]["KEY4"]["press"],
            format!("host_action:{UUID_WORKBENCH}")
        );
        assert_eq!(
            prof["encoder"]["left"],
            format!("host_action:{UUID_WORKBENCH_PREV}")
        );
        assert_eq!(
            prof["encoder"]["right"],
            format!("host_action:{UUID_WORKBENCH_NEXT}")
        );
        assert_eq!(
            prof["encoder"]["press"],
            format!("host_action:{UUID_WORKBENCH_CONFIRM}")
        );
        // 浮窗动作不消耗 text_map
        assert!(built.text_map.is_empty());
    }

    #[test]
    fn firmware_payload_text_map() {
        let mut p = Profile::factory_default("默认", "p1");
        // KEY3 与 encoder.left 各放一条含非ASCII文本
        p.keys.insert("KEY3".into(), ActionSpec::Text("帮我润色这段文字".into()));
        p.encoder.insert("left".into(), ActionSpec::Text("总结要点".into()));
        // KEY5 是纯 ASCII 文本：留在固件侧自行键入，不进 text_map
        p.keys.insert("KEY5".into(), ActionSpec::Text("hello".into()));

        let built = build_firmware_payload_with_wifi(&p, None);
        assert_eq!(built.text_map.len(), 2);
        assert!(built.text_map.values().any(|v| v == "帮我润色这段文字"));
        assert!(built.text_map.values().any(|v| v == "总结要点"));

        // 固件 JSON 里的 host_action 值应与 text_map 的键一一对应
        let v: Value = serde_json::from_str(&built.json).unwrap();
        let key3 = v["profiles"][0]["keys"]["KEY3"]["press"].as_str().unwrap();
        let left = v["profiles"][0]["encoder"]["left"].as_str().unwrap();
        assert!(built.text_map.contains_key(key3.trim_start_matches("host_action:")));
        assert!(built.text_map.contains_key(left.trim_start_matches("host_action:")));
        // ASCII 文本不产生 UUID
        assert_eq!(v["profiles"][0]["keys"]["KEY5"]["press"]["text"], "hello");
    }

    /// ⛔ 这条钉住的是 2026-09-24 查出来的真 bug：固件 `apply_json` **整体替换**
    /// 配置，载荷里没提到的字段回默认值（`wifi_ssid` / `audio_host` 默认空串）。
    /// 所以"下发方案"必须**带上** Wi-Fi 端点，否则每下发一次就把板子的 Wi-Fi 清空。
    #[test]
    fn wifi_endpoint_fields_are_emitted() {
        let p = Profile::factory_default("默认", "p1");
        // ⛔ 测试一律用**假**凭据。这里曾经填的是真实 SSID/密码，
        //    差点随代码入库 —— 单测里的字符串和注释一样会被永久保存。
        let built = build_firmware_payload_with_wifi(
            &p,
            Some(WifiEndpoint {
                ssid: "example-ssid",
                password: "example-pass",
                host: "192.168.31.24",
                port: 17333,
            }),
        );
        let v: Value = serde_json::from_str(&built.json).unwrap();
        // 四个顶层字段都在（固件就是按这四个名字读的）
        assert_eq!(v["wifi_ssid"], "example-ssid");
        assert_eq!(v["wifi_password"], "example-pass");
        assert_eq!(v["audio_host"], "192.168.31.24");
        assert_eq!(v["audio_port"], 17333);
        // 键位方案仍在（缺了会被固件判 MissingProfile 拒整包）
        assert!(v["profiles"][0]["keys"]["KEY1"]["press"].is_string());
        // 必须在固件 2048 字节上限内 —— 超了会被整包丢弃
        assert!(
            built.json.len() <= crate::constants::CONFIG_MAX_JSON_LEN,
            "载荷 {} 字节，超过固件上限",
            built.json.len()
        );
    }

    /// 不带端点时**不出现**这组字段（而不是出现空值）—— 两者在固件侧同义
    /// （都是回默认值的空串），但显式区分能让"这次下发有没有谈 Wi-Fi"可读。
    #[test]
    fn wifi_endpoint_omitted_when_not_requested() {
        let p = Profile::factory_default("默认", "p1");
        let built = build_firmware_payload_with_wifi(&p, None);
        let v: Value = serde_json::from_str(&built.json).unwrap();
        assert!(v.get("wifi_ssid").is_none());
        assert!(v.get("audio_host").is_none());
        assert!(v.get("audio_port").is_none());
    }

    #[test]
    fn reserved_uuid_roundtrip() {
        for name in [
            "toggle_profile",
            "previous_profile",
            "next_profile",
            "workbench",
            "workbench_prev",
            "workbench_next",
            "workbench_confirm",
        ] {
            let uuid = reserved_uuid_for_named(name).expect("保留动作应有 UUID");
            assert!(crate::validate::is_canonical_uuid(uuid), "{uuid} 非法");
            assert_eq!(named_for_reserved_uuid(uuid), Some(name));
        }
        // 普通具名动作不产生 UUID
        assert!(reserved_uuid_for_named("copy").is_none());
    }
}
