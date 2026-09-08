//! 集中常量，与固件协议真源对齐（`config_payload.cpp` / `usb_hid.cpp` / `host_action_protocol.cpp`）。

/// 固件 USB 设备标识（usb_hid.cpp）。
pub const VID: u16 = 0x303A;
pub const PID: u16 = 0x1006;

/// 配置通道（APP → 开发板）。
pub const REPORT_ID_CONFIG: u8 = 0x10;
/// 回执/命令通道（开发板 → APP）。
pub const REPORT_ID_APP_COMMAND: u8 = 0x11;

/// 0x11 命令 kind。
pub const KIND_CONFIG_ACK: u8 = 0x03;
pub const KIND_HOST_ACTION: u8 = 0x05;

/// 0x10 配置分块。
pub const CONFIG_HEADER_LEN: usize = 11;
pub const CONFIG_MAX_CHUNK_DATA: usize = 52;
pub const CONFIG_MAX_JSON_LEN: usize = 2048;
pub const MAGIC: [u8; 3] = *b"S3C";
pub const CONFIG_VERSION: u8 = 1;

/// 0x11 回执的 host 侧缓冲偏移（缓冲前置 Report ID 字节）。
pub const ACK_OFF_PHASE: usize = 5;
pub const ACK_OFF_OK: usize = 6;
pub const ACK_OFF_BYTES: usize = 7;
pub const ACK_OFF_CRC: usize = 9;
pub const ACK_OFF_SAVED: usize = 11;

/// ERROR_IO_PENDING 的 HRESULT（HRESULT_FROM_WIN32(997)）。
pub const HRESULT_ERROR_IO_PENDING: u32 = 0x8007_03E5;

/// Host Action v1。
pub const HOST_ACTION_PREFIX: &str = "host_action:";
pub const HOST_ACTION_PREFIX_LEN: usize = 12;
pub const HOST_ACTION_V1_UUID_LEN: usize = 36;
pub const HOST_ACTION_V1_HEADER_LEN: usize = 4;

/// 主机端方案切换的保留 UUID（与固件无关，仅借 host_action 通道回传）。
/// 固件将这些视为普通 host_action，回发 0x11 kind 0x05 报文，主机拦截后执行方案切换。
pub const UUID_TOGGLE_PROFILE: &str = "00000000-0000-4000-8000-000000000001";
pub const UUID_PREVIOUS_PROFILE: &str = "00000000-0000-4000-8000-000000000002";
pub const UUID_NEXT_PROFILE: &str = "00000000-0000-4000-8000-000000000003";

/// 配置 schema（固件 `parse_config_payload` 校验 `schema == "ai_keyboard.v1"`）。
pub const SCHEMA_VERSION: &str = "ai_keyboard.v1";

/// 固定文本上限（`kFixedTextMaxUtf8Bytes = 960`，以代码为准，非文档的 64）。
pub const FIXED_TEXT_MAX_UTF8_BYTES: usize = 960;

/// 具名 action 词表（与 `config_payload.cpp` `named_action_kind` 一致）。
pub const NAMED_ACTIONS: &[&str] = &[
    "disabled",
    "voice_ptt_hold",
    "edit_ptt_hold",
    "paste_last",
    "history",
    "toggle_profile",
    "settings",
    "previous_profile",
    "next_profile",
    "scroll_axis_toggle",
    "text_caret_select",
    "mouse_drag_select",
    "select_all",
    "copy",
    "paste",
    "undo",
];

/// 修饰键 token 白名单（`normalize` 后：去掉空格/下划线/连字符、转小写）。
pub const HOTKEY_MODIFIER_TOKENS: &[&str] = &[
    "ctrl",
    "control",
    "commandorcontrol",
    "leftctrl",
    "leftcontrol",
    "shift",
    "leftshift",
    "alt",
    "option",
    "leftalt",
    "leftoption",
    "meta",
    "gui",
    "cmd",
    "command",
    "leftmeta",
    "leftgui",
    "leftcmd",
    "leftcommand",
    "altgr",
    "rightalt",
    "rightoption",
    "rightmeta",
    "rightgui",
    "rightcmd",
    "rightcommand",
    "rightctrl",
    "rightcontrol",
    "rightshift",
];

/// 非修饰键的具名键（`normalize` 后）。
pub const HOTKEY_NAMED_KEYS: &[&str] = &[
    "enter",
    "return",
    "escape",
    "esc",
    "backspace",
    "tab",
    "space",
    "spacebar",
    "arrowright",
    "right",
    "arrowleft",
    "left",
    "arrowdown",
    "down",
    "arrowup",
    "up",
];
