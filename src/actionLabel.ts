// 动作标签与分组：把 ActionSpec 翻译成界面可读的名称与类型分组。
// 磁贴工作台、侧栏筛选、⌘SS 命令面板都从这里取标签，避免各处重复一份映射。
import { ACTION_KIND_LABELS, DESK_SLOT_OPTIONS, NAMED_ACTION_LABELS, WORKBENCH_ACTIONS, SS_PALETTE_ACTIONS, CLI_TERMINAL_ACTIONS, STUDIO_ACTIONS, KEY_UI } from "./constants";
import type { ActionSpec, AppEntry, Profile } from "./types";

/** 侧栏的动作类型筛选维度。「all」是聚合视图，其余各自对应一种动作类型。 */
export type ActionGroup =
  | "all"
  | "named"
  | "workbench"
  | "ss_palette"
  | "terminal"
  | "studio"
  | "desk"
  | "voice"
  | "hotkey"
  | "text"
  | "app"
  | "disabled";

export const ACTION_GROUPS: { id: ActionGroup; label: string }[] = [
  { id: "all", label: "全部" },
  { id: "named", label: "功能动作" },
  { id: "desk", label: "任务台" },
  { id: "workbench", label: "提示词库" },
  { id: "ss_palette", label: "⌘SS 命令面板" },
  { id: "terminal", label: "启动终端" },
  { id: "studio", label: "内容台" },
  { id: "voice", label: "语音" },
  { id: "hotkey", label: "组合键" },
  { id: "text", label: "文本" },
  { id: "app", label: "打开应用" },
  { id: "disabled", label: "未设置" },
];

/** 语音类功能动作：按下说话系列走的是语音链路，单独成组便于集中查看。 */
const VOICE_NAMED: string[] = ["voice_ptt_hold", "edit_ptt_hold"];

/** 取单个动作所属的类型分组（不含 all）。 */
export function groupOf(action: ActionSpec): ActionGroup {
  switch (action.kind) {
    case "named":
      if (WORKBENCH_ACTIONS.includes(action.value)) return "workbench";
      if (SS_PALETTE_ACTIONS.includes(action.value)) return "ss_palette";
      if (CLI_TERMINAL_ACTIONS.includes(action.value)) return "terminal";
      if (STUDIO_ACTIONS.includes(action.value)) return "studio";
      return VOICE_NAMED.includes(action.value) ? "voice" : "named";
    case "hotkey":
      return "hotkey";
    case "text":
      return "text";
    case "host_action":
      return "app";
    case "task_desk":
      return "desk";
    case "disabled":
      return "disabled";
  }
}

/** 槽位号 → 人类可读短名（`"1"` → `S1`）。非法值原样退回，便于看出配置坏了。 */
export function deskSlotLabel(value: string): string {
  return DESK_SLOT_OPTIONS.find((o) => o.value === value)?.label ?? `槽位 ${value}`;
}

/** 判断动作是否落在某个筛选分组内。 */
export function matchesGroup(action: ActionSpec, group: ActionGroup): boolean {
  return group === "all" || groupOf(action) === group;
}

/** 动作类型标签（磁贴右上角的类型徽标）。 */
export function actionKindLabel(action: ActionSpec): string {
  return ACTION_KIND_LABELS[action.kind] ?? action.kind;
}

/** 从 host_action 动作值里取出应用 UUID（值形如 `host_action:<uuid>`）。 */
export function hostActionUuid(value: string, prefix = "host_action:"): string {
  return value.startsWith(prefix) ? value.slice(prefix.length) : "";
}

/** 动作摘要：磁贴主体文案。 */
export function actionSummary(action: ActionSpec, apps: AppEntry[]): string {
  switch (action.kind) {
    case "disabled":
      return "未设置";
    case "named":
      return NAMED_ACTION_LABELS[action.value] ?? action.value;
    case "hotkey":
      return action.value || "未设置";
    case "text":
      return action.value
        ? `「${action.value.slice(0, 12)}${action.value.length > 12 ? "…" : ""}」`
        : "未设置";
    case "host_action": {
      const uuid = hostActionUuid(action.value);
      const app = apps.find((a) => a.uuid === uuid);
      return app ? app.title : "打开应用";
    }
    case "task_desk":
      return deskSlotLabel(action.value);
  }
}

/** 磁贴/搜索项的副标题：补充一点上下文，避免只看到动作名而不知落到哪个键。 */
export function actionDetail(action: ActionSpec, apps: AppEntry[]): string {
  switch (action.kind) {
    case "disabled":
      return "按下后不触发任何动作";
    case "named":
      // 提示词库是主机端动作：固件只负责回传触发，实际界面由 easyop 渲染。
      if (WORKBENCH_ACTIONS.includes(action.value)) {
        return `提示词库 · ${action.value}（由 easyop 处理）`;
      }
      if (SS_PALETTE_ACTIONS.includes(action.value)) {
        return `⌘SS 命令面板 · 开合搜索面板`;
      }
      if (CLI_TERMINAL_ACTIONS.includes(action.value)) {
        return `启动终端 · 开（或复用）终端并预置命令，由 easyop 处理`;
      }
      return `功能动作 · ${action.value}`;
    case "hotkey":
      return action.value ? `组合键 · ${action.value}` : "尚未录入组合键";
    case "text":
      return action.value ? `输入 ${action.value.length} 个字符` : "尚未填写文本";
    case "host_action": {
      const uuid = action.value.startsWith("host_action:")
        ? action.value.slice("host_action:".length)
        : "";
      const app = apps.find((a) => a.uuid === uuid);
      return app ? `启动 ${app.title}` : "未绑定应用";
    }
    case "task_desk":
      return `Agent 任务台 · ${deskSlotLabel(action.value)}（由 easyop 处理）`;
  }
}

/** 一个可筛选/可搜索的输入项：8 个按键与编码器的 3 个方向。 */
export interface InputEntry {
  id: string;
  label: string;
  group: "keys" | "encoder";
  action: ActionSpec;
}

/** 把 Profile 摊平成输入项列表，顺序沿用 KEY_UI（KEY1-8 → 编码器左旋/右旋/按下）。 */
export function inputsOf(profile: Profile): InputEntry[] {
  return KEY_UI.map((k) => ({
    id: k.id,
    label: k.label,
    group: k.group,
    action:
      (k.group === "keys" ? profile.keys[k.id] : profile.encoder[k.id]) ?? {
        kind: "disabled" as const,
      },
  }));
}

/** 输入项的中文短名：磁贴角标与编码器状态栏都用它。 */
export function inputLabel(inputId: string): string {
  const found = KEY_UI.find((k) => k.id === inputId);
  return found ? found.label : inputId;
}

/** 输入项的短标识：磁贴上用 KEY1 / 左旋 这类紧凑写法。 */
export function inputShort(inputId: string): string {
  switch (inputId) {
    case "left":
      return "左旋";
    case "right":
      return "右旋";
    case "press":
      return "按下";
    default:
      return inputId;
  }
}
