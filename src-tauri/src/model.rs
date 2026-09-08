//! 领域数据模型：按键动作、按键方案、本机应用库、配置回执。
//!
//! 这些类型同时被前端（经 Tauri 命令 JSON 序列化）与后端各模块共享。
//! 动作的「本地存储表示」是 `ActionSpec` 判别枚举；向固件下发时由
//! `payload::to_firmware_value` 转换为固件 schema（`ai_keyboard.v1`）要求的
//! 裸字符串 / `{"hotkey":...}` / `{"text":...}` JSON 值。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 一个输入（键）上的动作定义，用 kind + value 的判别式表示。
///
/// 序列化到本地 profile JSON：
/// ```json
/// {"kind": "disabled"}
/// {"kind": "named", "value": "copy"}
/// {"kind": "hotkey", "value": "Ctrl+Alt+A"}
/// {"kind": "text", "value": "你好"}          // 固定文本，≤ 960 字节
/// {"kind": "host_action", "value": "host_action:<uuid>"}
/// ```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ActionSpec {
    Disabled,
    Named(String),
    Hotkey(String),
    Text(String),
    HostAction(String),
}

/// 固定可配置的按键输入 ID 及其固件 JSON 键名。
///
/// 固件要求 `keys` 必须含 KEY1..KEY8 全部 8 项（每项包 `{press:...}`），
/// `encoder` 必须含 left/right/press 全部 3 项（裸值）。因此方案始终携带
/// 全部 11 个输入，用户仅编辑关心的部分，其余保持默认。
pub const KEY_INPUTS: &[&str] = &["KEY1", "KEY2", "KEY3", "KEY4", "KEY5", "KEY6", "KEY7", "KEY8"];
pub const ENCODER_INPUTS: &[&str] = &["left", "right", "press"];
pub const ALL_INPUTS: &[&str] = &[
    "KEY1", "KEY2", "KEY3", "KEY4", "KEY5", "KEY6", "KEY7", "KEY8", "left", "right", "press",
];

/// 本机应用库项：一个可被 Host Action v1 触发的打开目标。
///
/// `uuid` 会出现在固件上下发的 `host_action:<uuid>` 动作中；开发板回发该
/// UUID 时，应用据此查找并打开目标。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppTarget {
    /// 启动一个可执行程序（可带参数、工作目录）。
    App { path: String, args: String, work_dir: String },
    /// 用系统默认应用打开一个文件/文档。
    File { path: String },
    /// 用默认浏览器打开一个 URL。
    Url { url: String },
    /// 打开一个文件夹。
    Folder { path: String },
}

/// 应用库条目。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AppEntry {
    /// 规范小写 hex UUID（Host Action v1 要求 36 字符、连字符位 8/13/18/23、仅 a-f0-9）。
    pub uuid: String,
    /// 展示名。
    pub title: String,
    /// 打开目标。
    pub target: AppTarget,
}

/// 一个按键方案（profile）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Profile {
    /// 方案唯一 ID。
    pub id: String,
    /// 方案名。
    pub name: String,
    /// KEY1..KEY8 → 动作。
    pub keys: BTreeMap<String, ActionSpec>,
    /// left/right/press → 动作。
    pub encoder: BTreeMap<String, ActionSpec>,
}

impl Profile {
    /// 用固件默认键映射构造一个全新方案（`keymap.h` `DefaultKeymap`）。
    pub fn factory_default(name: impl Into<String>, id: impl Into<String>) -> Self {
        let mut keys = BTreeMap::new();
        keys.insert("KEY1".into(), ActionSpec::Named("voice_ptt_hold".into()));
        keys.insert("KEY2".into(), ActionSpec::Hotkey("Return".into()));
        keys.insert("KEY3".into(), ActionSpec::Named("edit_ptt_hold".into()));
        keys.insert("KEY4".into(), ActionSpec::Hotkey("Backspace".into()));
        keys.insert("KEY5".into(), ActionSpec::Named("select_all".into()));
        keys.insert("KEY6".into(), ActionSpec::Named("copy".into()));
        keys.insert("KEY7".into(), ActionSpec::Named("paste".into()));
        keys.insert("KEY8".into(), ActionSpec::Named("undo".into()));
        let mut encoder = BTreeMap::new();
        encoder.insert("left".into(), ActionSpec::Disabled);
        encoder.insert("right".into(), ActionSpec::Disabled);
        encoder.insert("press".into(), ActionSpec::Named("scroll_axis_toggle".into()));
        Profile { id: id.into(), name: name.into(), keys, encoder }
    }

    /// 取出某个输入的动作；缺失时回退到「禁用」。
    pub fn action_for(&self, input: &str) -> ActionSpec {
        if KEY_INPUTS.contains(&input) {
            self.keys.get(input).cloned().unwrap_or(ActionSpec::Disabled)
        } else {
            self.encoder.get(input).cloned().unwrap_or(ActionSpec::Disabled)
        }
    }
}

/// 固件配置下发回执（0x11，kind 0x03），经 `payload::parse_ack` 解析。
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ConfigAck {
    pub ok: bool,
    pub saved: bool,
    pub phase: u8,
    pub bytes: u16,
    pub crc16: u16,
}
