// 前端静态常量：输入定义、具名动作中文标签、动作类型元数据。

/** 可配置的输入（键）及其中文名。 */
export const KEY_UI: { id: string; label: string; group: "keys" | "encoder" }[] = [
  { id: "KEY1", label: "按键 1", group: "keys" },
  { id: "KEY2", label: "按键 2", group: "keys" },
  { id: "KEY3", label: "按键 3", group: "keys" },
  { id: "KEY4", label: "按键 4", group: "keys" },
  { id: "KEY5", label: "按键 5", group: "keys" },
  { id: "KEY6", label: "按键 6", group: "keys" },
  { id: "KEY7", label: "按键 7", group: "keys" },
  { id: "KEY8", label: "按键 8", group: "keys" },
  { id: "left", label: "编码器 · 左旋", group: "encoder" },
  { id: "right", label: "编码器 · 右旋", group: "encoder" },
  { id: "press", label: "编码器 · 按下", group: "encoder" },
];

/** 具名动作 → 中文标签（未列出的回退到原名）。 */
export const NAMED_ACTION_LABELS: Record<string, string> = {
  disabled: "禁用",
  voice_ptt_hold: "语音 · 按下说话",
  edit_ptt_hold: "编辑 · 按下说话",
  paste_last: "粘贴最近",
  history: "剪贴板历史",
  toggle_profile: "切换方案",
  settings: "打开设置",
  previous_profile: "上一个方案",
  next_profile: "下一个方案",
  scroll_axis_toggle: "切换滚动方向",
  text_caret_select: "光标选择",
  mouse_drag_select: "鼠标拖动选择",
  select_all: "全选",
  copy: "复制",
  paste: "粘贴",
  undo: "撤销",
  // Phase 3 提示词库浮窗（easyop 主机端专有动作）
  workbench: "提示词库",
  workbench_prev: "提示词库 · 上一项",
  workbench_next: "提示词库 · 下一项",
  workbench_confirm: "提示词库 · 确认",
  // Phase 4A ⌘SS 命令面板（easyop 主机端专有动作）
  ss_palette: "⌘SS 命令面板",
  // 启动终端并预置命令（easyop 主机端专有动作）
  cli_terminal: "启动终端",

};

/** 提示词库相关的具名动作 ID，界面分组与提示文案都以此为准。 */
export const WORKBENCH_ACTIONS: string[] = [
  "workbench",
  "workbench_prev",
  "workbench_next",
  "workbench_confirm",
];

/** ⌘SS 命令面板相关的具名动作 ID。 */
export const SS_PALETTE_ACTIONS: string[] = [
  "ss_palette",
];

/** 「启动终端」的具名动作 ID（`cli_terminal`：开/复用终端 + 预置命令前缀）。 */
export const CLI_TERMINAL_ACTION = "cli_terminal";

/** 上者的数组形式，供分组判断复用同一份 ID。 */
export const CLI_TERMINAL_ACTIONS: string[] = [CLI_TERMINAL_ACTION];

/** 内容台旋钮导航相关的具名动作 ID（旧 Named kind 的残留，仅用于分组判断兼容）。 */
export const STUDIO_ACTIONS: string[] = [
  "studio_prev",
  "studio_next",
  "studio_confirm",
];

/** 内容台旋钮导航的子动作选项（新 Studio kind 的下拉用）。 */
export const STUDIO_KIND_OPTIONS: { value: string; label: string }[] = [
  { value: "prev", label: "上一项（左旋切换焦点）" },
  { value: "next", label: "下一项（右旋切换焦点）" },
  { value: "confirm", label: "确认（按下执行）" },
];

/** 动作类型的中文名，用于类型选择下拉。 */
export const ACTION_KIND_LABELS: Record<string, string> = {
  disabled: "禁用",
  named: "功能动作",
  hotkey: "组合键",
  text: "输入文本",
  host_action: "打开应用",
  // Agent 任务台：把这一键归入某个槽位（S1–S4），说不说、播不播由任务台决定。
  // 固件侧它被编码成 `host_action:<保留 UUID>`，所以不需要固件改动。
  task_desk: "任务台",
  // 内容台旋钮导航：编码器旋转切换焦点、按下确认执行。
  // 固件侧同样编码成 `host_action:<保留 UUID>`。
  // 注意：内容台 *插件* 的命令菜单在动作类型下拉里另称「内容台」，与旋钮区分。
  studio: "内容台 · 旋钮",
};

/**
 * Agent 任务台的槽位选项。
 *
 * 槽位号就是字符串本身，直接存进 `ActionSpec.value`——**不做 0/1 基转换**：
 * 固件侧保留 UUID 表、UI 显示、用户口语（"S1"）三处一旦有人用不同基数，
 * 就会出现"配了 S1 结果语音进了槽 2"这种对不上号的问题。
 */
export const DESK_SLOT_OPTIONS: { value: string; label: string }[] = [
  { value: "1", label: "S1（槽位 1）" },
  { value: "2", label: "S2（槽位 2）" },
  { value: "3", label: "S3（槽位 3）" },
  { value: "4", label: "S4（槽位 4）" },
];

/** 修饰键 + 普通键的候选，用于组合键便捷下拉（单词，使用规范英文名）。 */
export const KEY_CANDIDATES: string[] = [
  "ctrl",
  "shift",
  "alt",
  "meta",
  "Return",
  "Escape",
  "Backspace",
  "Tab",
  "Space",
  "a",
  "b",
  "c",
  "d",
  "e",
  "f",
  "g",
  "h",
  "i",
  "j",
  "k",
  "l",
  "m",
  "n",
  "o",
  "p",
  "q",
  "r",
  "s",
  "t",
  "u",
  "v",
  "w",
  "x",
  "y",
  "z",
  "0",
  "1",
  "2",
  "3",
  "4",
  "5",
  "6",
  "7",
  "8",
  "9",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "F1",
  "F2",
  "F3",
  "F4",
  "F5",
  "F6",
  "F7",
  "F8",
  "F9",
  "F10",
  "F11",
  "F12",
];
