//! 集中常量，与固件协议真源对齐（`config_payload.cpp` / `usb_hid.cpp` / `host_action_protocol.cpp`）。

/// 固件 USB 设备标识（usb_hid.cpp）。
pub const VID: u16 = 0x303A;
pub const PID: u16 = 0x1006;

/// 配置通道（APP → 开发板）。
pub const REPORT_ID_CONFIG: u8 = 0x10;
/// 回执/命令通道（开发板 → APP）。
pub const REPORT_ID_APP_COMMAND: u8 = 0x11;

/// 任务台逐像素帧（APP → 开发板，63 字节 Feature Report）。
///
/// 这是**新增**通道（Phase 3.1），一个字节都不动既有 `0x12`：一路 Frame 携带 5 颗灯的
/// RGB 与 `(sequence, ttl_ms)`，固件收到即整帧生效、ttl 到期自动回落到既有状态机。
/// 帧布局与"为什么必须新增而不是复用 `0x12`"详见 `agent_desk::device_led` 的模块文档。
///
/// # 其它已占用的 Report ID（登记备查）
///
/// 真源是固件 `main/platform/usb_hid.cpp` 的 HID 描述符与各协议头文件。它们**不在本
/// 进程里使用**，所以只以注释登记、不定义常量 —— 定义了没人用的常量只会变成
/// 一批 dead code 警告，那对下一个人是噪音，不是文档。
///
/// | ID | 方向 | 用途 |
/// |---|---|---|
/// | `0x10` | APP → 板 | 配置下发（上面的 `REPORT_ID_CONFIG`） |
/// | `0x11` | 板 → APP | 回执 / 命令（上面的 `REPORT_ID_APP_COMMAND`） |
/// | `0x12` | APP → 板 | Agent 状态（16B）。固件渲染成**整条灯带一个状态色** |
/// | `0x13` | APP → 板 | 状态请求（即下方的 `REPORT_ID_STATUS_REQUEST`） |
/// | `0x14` | 板 → APP | 音库资产请求（63B） |
/// | `0x15` | APP → 板 | 音库资产响应（63B） |
/// | `0x16` | APP → 板 | **任务台逐像素帧**（本常量） |
pub const REPORT_ID_DESK_LED: u8 = 0x16;

/// 状态回读请求通道（APP → 板 Feature Report）。
///
/// 发出一条 17 字节 Feature Report（1 字节 Report ID + 16 字节载荷），载荷必须
/// 以 `b"S3R"` + 协议版本开头，并带一个**非 0** 的 `request_id`。固件是 fail-closed：
/// 任一项不符就**静默丢弃**（连错误回包都没有）—— 所以"发了没反应"首先怀疑载荷
/// 写错，而不是板子坏了。真源见固件 `components/keyboard/src/status_hid_protocol.cpp`。
pub const REPORT_ID_STATUS_REQUEST: u8 = 0x13;

/// 0x13 载荷（**不含** Report ID）的固定长度与握手字节。
pub const STATUS_REQUEST_PAYLOAD_LEN: usize = 16;
pub const STATUS_REQUEST_MAGIC: [u8; 3] = *b"S3R";
pub const STATUS_HID_PROTOCOL_VERSION: u8 = 1;
/// 请求 flag：置位表示"跳过缓存，重建一份新的状态 JSON"。
pub const STATUS_REQUEST_FLAG_FRESH: u8 = 0x01;
/// 已知 flag 位集合；出现未知位固件会拒绝整条请求。
pub const STATUS_REQUEST_KNOWN_FLAGS: u8 = STATUS_REQUEST_FLAG_FRESH;

/// 0x11 命令 kind：**状态响应**。
///
/// 与 `KIND_CONFIG_ACK = 0x03`、`KIND_HOST_ACTION = 0x05` 是三个互不重叠的编号，
/// 不得互换（固件侧有 `static_assert` 钉住 `0x05 != 0x04`）。
///
/// ⚠ 2026-09-30 之前本文件只有 0x03 / 0x05 —— 缺 `0x04` 的后果就是"**即使固件回了
/// 状态，上位机也会把它当未知报文丢掉**"，表现为"板子不支持读版本"。
pub const KIND_STATUS_RESPONSE: u8 = 0x04;

/// 0x11 kind 0x04 的 63 字节消息容器布局（不含 Report ID）。
///
/// ```text
/// [0]       kind = 0x04
/// [1]       chunk_index
/// [2]       total_chunks
/// [3]       = STATUS_RESPONSE_METADATA_LEN + 本块 JSON 字节数
/// [4]       协议版本（STATUS_HID_PROTOCOL_VERSION）
/// [5..9]    request_id u32 LE（回显请求里的值，用于丢弃过期响应）
/// [9..11]   JSON 总长 u16 LE
/// [11..13]  JSON 的 crc16-ccitt u16 LE
/// [13..63]  JSON 分片（每块最多 50 字节）
/// ```
pub const STATUS_MESSAGE_PAYLOAD_LEN: usize = 63;
pub const STATUS_RESPONSE_HEADER_LEN: usize = 4;
pub const STATUS_RESPONSE_METADATA_LEN: usize = 9;
/// 单块能携带的 JSON 字节数（50）。
pub const STATUS_RESPONSE_JSON_PER_CHUNK: usize =
    STATUS_MESSAGE_PAYLOAD_LEN - STATUS_RESPONSE_HEADER_LEN - STATUS_RESPONSE_METADATA_LEN;
/// 状态 JSON 的字节上限（固件 `kConfigStatusGattSafeLen`）。
pub const STATUS_RESPONSE_MAX_JSON_LEN: usize = 512;

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

/// 主机端浮窗工作台的保留 UUID（Phase 3）。
///
/// 与方案切换同构：固件不认识这些语义，只当作普通 host_action 回发；
/// 主机拦截后驱动浮窗的开关、上下移动高亮与确认执行。
/// 这样编码器的左旋/右旋/按下也能直接操作浮窗，无需固件侧改动。
pub const UUID_WORKBENCH: &str = "00000000-0000-4000-8000-000000000004";
pub const UUID_WORKBENCH_PREV: &str = "00000000-0000-4000-8000-000000000005";
pub const UUID_WORKBENCH_NEXT: &str = "00000000-0000-4000-8000-000000000006";
pub const UUID_WORKBENCH_CONFIRM: &str = "00000000-0000-4000-8000-000000000007";

/// 主机端 ⌘SS 命令面板的保留 UUID（Phase 4A）。
///
/// 与浮窗工作台同构：固件不认识，只当作普通 host_action 回发；
/// 主机拦截后开合命令面板窗口。
pub const UUID_SS_PALETTE: &str = "00000000-0000-4000-8000-000000000008";

/// 主机端「启动终端」的保留 UUID。
///
/// # 它解决的是什么
///
/// 一键把终端**准备好**：开一个命令行窗口，并把配置好的命令（如
/// `codex.cmd exec --skip-git-repo-check`）打进输入框、**不回车**。
/// 随后按语音键说出的话接在这条命令后面，回车即执行 ——
/// 于是「启动环境」与「输入内容」成为两个独立步骤。
///
/// # 为什么不是"用参数把命令喂给 cmd"
///
/// `cmd /k <命令>` 是**直接执行**；要"只打进去、等用户补完"，唯一的路子是
/// 先开窗口、再往里面按键（见 `cli_terminal.rs`）。而「往窗口里按键」正是
/// `text_delivery` 已有的能力，复用即可。
///
/// 编号沿用上面同一段 hex 计数（01–0c 已用），不要换段：主机端按精确字符串
/// 匹配，换段只在运行期表现为"按键没反应"。
pub const UUID_CLI_TERMINAL: &str = "00000000-0000-4000-8000-00000000000d";

/// 主机端「内容台」旋钮导航的保留 UUID。
///
/// 与浮窗工作台同构：旋钮左旋/右旋在功能列表间切换焦点，
/// 按下确认执行当前高亮的功能。焦点状态由主机端 `host_action.rs` 维护。
/// 固件不认识这些语义，只当作普通 host_action 回发。
pub const UUID_STUDIO_PREV: &str = "00000000-0000-4000-8000-00000000000e";
pub const UUID_STUDIO_NEXT: &str = "00000000-0000-4000-8000-00000000000f";
pub const UUID_STUDIO_CONFIRM: &str = "00000000-0000-4000-8000-000000000010";

/// Agent 任务台的槽位按键保留 UUID。
///
/// # 为什么任务台按键必须借 `host_action` 通道
///
/// 固件 `config_payload.cpp::parse_action` 对按键动作是**白名单制**：一个字符串
/// 要么命中 `named_action_kind()` 里的固定词表，要么形如 `host_action:<规范 UUID>`；
/// 两者都不满足就返回 `UnknownAction`，**整份配置被拒**。
///
/// 所以「这个键属于任务台第 N 槽」这种自定义语义，唯一能落地的载体就是
/// `host_action:<uuid>` —— 固件不认识它，但会原样回发 0x11 kind 0x05 报文，
/// 主机端按 UUID 分流。这与 `workbench` / `ss_palette` 是同一套做法。
///
/// # 按下即触发，没有按住语义
///
/// 固件 `keymap.cpp::event_for_action`：`ActionKind::HostAction` 在
/// `InputPhase::Released` 时返回空事件，**只在按下时回发一次**。
/// 因此槽位按键是"点按触发"，不是"按住说话"——录音由主机侧发 EICC start
/// 命令驱动（固件 `keyboard_audio.cpp:1245`），固件不参与按住判定。
///
/// 编号沿用上面同一段 hex 计数（01–08 已用），不要改成别的 UUID 段：
/// 主机端拦截逻辑按精确字符串匹配，换段只会在运行期表现为"按键无反应"。
pub const UUID_DESK_SLOT1: &str = "00000000-0000-4000-8000-000000000009";
pub const UUID_DESK_SLOT2: &str = "00000000-0000-4000-8000-00000000000a";
pub const UUID_DESK_SLOT3: &str = "00000000-0000-4000-8000-00000000000b";
pub const UUID_DESK_SLOT4: &str = "00000000-0000-4000-8000-00000000000c";

/// 槽位（从 1 起）→ 保留 UUID。越界返回 `None`（调用方必须显式失败，不许静默兜底）。
///
/// `const fn`：这样槽位数量的**唯一真源**就是这张表本身
/// （`agent_desk::config::SLOT_COUNT` 直接由 `desk_slot_count()` 推导），
/// 加一个槽位只需改这里一处，两处对不上的可能性从结构上消掉。
pub const fn desk_slot_uuid(slot: u8) -> Option<&'static str> {
    match slot {
        1 => Some(UUID_DESK_SLOT1),
        2 => Some(UUID_DESK_SLOT2),
        3 => Some(UUID_DESK_SLOT3),
        4 => Some(UUID_DESK_SLOT4),
        _ => None,
    }
}

/// 保留 UUID → 槽位（从 1 起）。主机端拦截用。
pub fn desk_slot_of_uuid(uuid: &str) -> Option<u8> {
    match uuid {
        UUID_DESK_SLOT1 => Some(1),
        UUID_DESK_SLOT2 => Some(2),
        UUID_DESK_SLOT3 => Some(3),
        UUID_DESK_SLOT4 => Some(4),
        _ => None,
    }
}

/// 槽位数量的**唯一真源**：`desk_slot_uuid` 认哪些槽位，这里就到几。
///
/// 写成函数而不是常量，是为了让"表里有几项"这件事由**表本身**回答。
/// 常量写 4、表里只有 3 项，常量自己是不会报错的。
pub const fn desk_slot_count() -> u8 {
    let mut n = 0u8;
    while desk_slot_uuid(n + 1).is_some() {
        n += 1;
    }
    n
}

#[cfg(test)]
mod desk_slot_tests {
    use super::*;

    #[test]
    fn desk_slot_table_is_contiguous() {
        // 表必须从 1 连续排到 N，中间不能有洞：有洞意味着某个槽位按键
        // 会被编码成 `disabled`，用户只会看到"这个键没反应"。
        assert_eq!(desk_slot_count(), 4);
        for slot in 1..=desk_slot_count() {
            let uuid = desk_slot_uuid(slot).expect("连续区间内必须有 UUID");
            assert_eq!(desk_slot_of_uuid(uuid), Some(slot), "反查必须回到同一槽位");
            assert_eq!(uuid.len(), HOST_ACTION_V1_UUID_LEN);
            assert!(crate::validate::is_canonical_uuid(uuid), "{uuid} 不是规范 UUID");
        }
        // 越界必须返回 None，不能回绕到槽位 1。
        assert!(desk_slot_uuid(0).is_none());
        assert!(desk_slot_uuid(desk_slot_count() + 1).is_none());
    }

    #[test]
    fn desk_slot_uuids_do_not_collide_with_other_reserved_uuids() {
        // 保留段是精确字符串匹配，撞车 = 一个键同时被两处认领。
        for slot in 1..=desk_slot_count() {
            let uuid = desk_slot_uuid(slot).unwrap();
            for other in [
                UUID_TOGGLE_PROFILE,
                UUID_PREVIOUS_PROFILE,
                UUID_NEXT_PROFILE,
                UUID_WORKBENCH,
                UUID_WORKBENCH_PREV,
                UUID_WORKBENCH_NEXT,
                UUID_WORKBENCH_CONFIRM,
                UUID_SS_PALETTE,
                UUID_CLI_TERMINAL,
            ] {
                assert_ne!(uuid, other, "任务台槽位 UUID 与既有保留 UUID 撞车");
            }
        }
    }
}

/// 配置 schema（固件 `parse_config_payload` 校验 `schema == "ai_keyboard.v1"`）。
pub const SCHEMA_VERSION: &str = "ai_keyboard.v1";

/// 固定文本上限（`kFixedTextMaxUtf8Bytes = 960`，以代码为准，非文档的 64）。
pub const FIXED_TEXT_MAX_UTF8_BYTES: usize = 960;

/// 单条提示词的字节上限。
///
/// 提示词**不下发固件**，因此不受 `FIXED_TEXT_MAX_UTF8_BYTES` 约束（那是固件 JSON
/// 报文的限制）。这里的上限只用于挡住「误粘贴一整篇文档」这类输入：上屏走剪贴板，
/// 内容再长也能贴出去，但面板只有两行预览，超过这个量级对用户没有意义。
pub const PROMPT_MAX_UTF8_BYTES: usize = 4096;

/// 单个方案最多保留的提示词条数。
///
/// 面板靠编码器一条条转过去，条数过多会让「旋钮选择」事实上不可用；
/// 这个上限取的是一个仍然可以用旋钮逐个翻完的量级。
pub const PROMPT_MAX_COUNT: usize = 60;

/// 具名 action 词表（与 `config_payload.cpp` `named_action_kind` 一致）。
///
/// 末尾四项 `workbench*` 是 **easyop 主机端专有动作**：固件不认识它们，
/// `payload::to_firmware_value` 会在下发前把它们改写成 `host_action:<保留 UUID>`，
/// 因此固件的 `named_action_kind` 永远不会看到这些名字。
/// 登记在此是为了让本地 `validate_action` 与前端词表能识别它们。
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
    // ---- easyop 主机端专有（Phase 3 浮窗工作台）----
    "workbench",
    "workbench_prev",
    "workbench_next",
    "workbench_confirm",
    // ---- easyop 主机端专有（Phase 4A ⌘SS 命令面板）----
    "ss_palette",
    // ---- easyop 主机端专有（启动终端并预置命令）----
    "cli_terminal",

];

/// 内容台旋钮导航的功能列表（按焦点顺序排列）。
///
/// 旋钮左旋/右旋在此列表间循环切换焦点，按下确认时，
/// 主机端按当前焦点索引 emit `studio-action` 事件，
/// 前端收到后调 studio 插件的 `studio.quick_action` 命令执行对应功能。
pub const STUDIO_ACTION_IDS: &[&str] = &[
    "studio_new",
    "studio_pipeline",
    "studio_advance",
    "studio_export",
    "studio_wechat",
];

/// 内容台功能列表的中文标签（与 `STUDIO_ACTION_IDS` 一一对应）。
pub const STUDIO_ACTION_LABELS: &[&str] = &[
    "新建选题",
    "一条龙",
    "推进阶段",
    "导出",
    "+公众号",
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
