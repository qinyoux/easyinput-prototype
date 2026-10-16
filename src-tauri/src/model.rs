//! 领域数据模型：按键动作、按键方案（含提示词库）、本机应用库、配置回执。
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
/// {"kind": "task_desk", "value": "1"}        // Agent 任务台槽位 1
/// {"kind": "studio", "value": "prev"}        // 内容台旋钮 · 上一项
/// ```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ActionSpec {
    Disabled,
    Named(String),
    Hotkey(String),
    Text(String),
    HostAction(String),
    /// **Agent 任务台槽位按键**。`value` 是槽位号字符串 `"1"`–`"4"`。
    ///
    /// # 为什么单列一个 kind，而不复用 `HostAction`
    ///
    /// 对固件而言两者**完全等价**，`payload::to_firmware_value` 都会把它编码成
    /// `host_action:<保留 UUID>`（固件白名单只认这一种携带自定义语义的形式，
    /// 详见 `constants::UUID_DESK_SLOT1` 的说明）。
    ///
    /// 分开的理由在**主机侧**：`HostAction` 的语义是"按 UUID 去应用库里开一个程序"，
    /// 而任务台按键的语义是"把这一键归入某个槽位"。两者共用一个 kind 会让
    /// `handle_host_action` 的分流退化成"先查应用库、查不到再当任务台"——
    /// 一旦用户的某个应用 UUID 恰好撞上保留段就会静默开错东西。
    /// 类型分开，分流就是**互斥**的，不依赖查表顺序。
    TaskDesk(String),
    /// **内容台旋钮导航**。`value` 是 `"prev"` / `"next"` / `"confirm"`。
    ///
    /// 与 `TaskDesk` 同理：对固件而言等价于 `host_action:<保留 UUID>`，
    /// 分开 kind 是为了在主机侧 UI 上独立成"内容台"选项，
    /// 不与普通功能动作（copy / paste 等）混在下拉里。
    Studio(String),
}

impl ActionSpec {
    /// 槽位号（`TaskDesk` 专用）。非法/非本类型返回 `None`。
    ///
    /// **不在这里兜底成 1**：写进配置的槽位号只可能来自 UI 的四个选项，
    /// 出现别的值说明配置被人手改坏了，应当显式失败而不是猜一个。
    pub fn desk_slot(&self) -> Option<u8> {
        match self {
            ActionSpec::TaskDesk(v) => v.trim().parse::<u8>().ok().filter(|s| (1..=4).contains(s)),
            _ => None,
        }
    }
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

/// 提示词库条目：一条可以用浮窗一键上屏的常用提示词。
///
/// 与「固定文本动作」（`ActionSpec::Text`）是**两回事**：
/// - 固定文本动作挂在某个按键上，会被 `payload::build_firmware_payload` 下发到固件；
/// - 提示词只是主机端的一份常用语清单，**完全不下发固件**，只服务于 KEY4 浮窗。
///
/// `id` 由主机端生成（v4 UUID），仅用于界面上定位「改哪一条 / 删哪一条」，
/// 不参与任何协议，也没必要在下发前做规范化校验。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    /// 条目唯一 ID。
    pub id: String,
    /// 提示词正文（面板预览与上屏都用它）。
    pub text: String,
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
    /// 该方案专属的提示词库（KEY4 浮窗的内容，随方案切换）。
    ///
    /// `#[serde(default)]` 是**必需**的：加字段之前的 `profiles.json` 里没有这个键，
    /// 缺了它会让整份配置反序列化失败，用户的全部方案会被静默重置成出厂默认。
    #[serde(default)]
    pub prompts: Vec<Prompt>,
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
        Profile { id: id.into(), name: name.into(), keys, encoder, prompts: Vec::new() }
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

/// 开发板固件状态（0x13 状态请求 → 0x11 / kind 0x04 分块响应）的摘要。
///
/// 只摘取界面上真正要看的几项。**不**做全字段结构体反序列化：固件的状态 JSON
/// 会随版本增删字段（`capabilities` 那一段就一直在长），用固定 struct 接会在
/// 固件升级时整条解析失败 —— 那时连 `firmware` 都读不出来了。
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct DeviceStatus {
    /// 固件版本串，如 `0.4.53-idf-v2-ble-store-v1`（固件 `kFirmwareVersion`）。
    ///
    /// 串里带 `idf-v2-…` 这种构建标记，能直接看出板子上跑的是哪一支构建 ——
    /// 这正是"烧没烧进去"这个问题唯一可信的回答。
    pub firmware: String,
    /// 状态 JSON 的 schema，如 `ai_keyboard.config_status.v1`（判断协议代际）。
    pub schema: String,
    /// 电池电压（mV）；固件没报就是 `None`。
    pub battery_mv: Option<u16>,
    /// 电池电量（%）；固件没报就是 `None`。
    pub battery_percent: Option<u8>,
}
