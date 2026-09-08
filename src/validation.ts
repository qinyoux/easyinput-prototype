// 前端预校验，严格镜像后端 `validate.rs` / 固件判定；后端仍是最终真源。
import type { ActionSpec, AppTarget } from "./types";

/** 去掉空格/下划线/连字符并转小写，与固件 `normalize` 一致。 */
export function normalizeToken(token: string): string {
  return token
    .split("")
    .filter((c) => c !== " " && c !== "_" && c !== "-")
    .join("")
    .toLowerCase();
}

/** 规范小写 hex UUID：36 字符，连字符位 8/13/18/23，其余为 0-9a-f。 */
export function isCanonicalUuid(uuid: string): boolean {
  if (uuid.length !== 36) return false;
  for (let i = 0; i < uuid.length; i++) {
    const ch = uuid[i];
    if (i === 8 || i === 13 || i === 18 || i === 23) {
      if (ch !== "-") return false;
    } else {
      if (!/[0-9a-f]/.test(ch)) return false;
    }
  }
  return true;
}

const NAMED_KEYS = [
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

const MODIFIERS = [
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

function isNamedKey(token: string): boolean {
  return NAMED_KEYS.includes(token);
}

function isPlainKey(token: string): boolean {
  if (token.length === 1) {
    const ch = token;
    if (/[a-z0-9]/.test(ch)) return true;
  }
  if (token.length >= 2 && token.startsWith("f")) {
    const n = Number(token.slice(1));
    if (Number.isFinite(n)) return n >= 1 && n <= 12;
  }
  return false;
}

/** 校验组合键。返回中文错误或 null。 */
export function validateHotkey(hotkey: string): string | null {
  const trimmed = hotkey.trim();
  if (!trimmed) return "组合键不能为空";
  let sawToken = false;
  for (const part of trimmed.split("+")) {
    const token = normalizeToken(part);
    if (!token) continue;
    sawToken = true;
    if (token === "fn" || token === "function") return "组合键不支持 Fn 键";
    if (MODIFIERS.includes(token)) continue;
    if (!isNamedKey(token) && !isPlainKey(token)) {
      return `组合键包含固件不支持的键「${part}」（支持 a-z、0-9、Enter/Escape/Backspace/Tab/Space、方向键、F1-F12）`;
    }
  }
  if (!sawToken) return "组合键不能只含分隔符";
  return null;
}

/** 校验固定文本字节数。 */
export function validateText(
  text: string,
  maxBytes: number,
): string | null {
  if (!text) return "固定文本不能为空";
  const bytes = new TextEncoder().encode(text).length;
  if (bytes > maxBytes) return `固定文本过长：${bytes} 字节（上限 ${maxBytes}）`;
  return null;
}

/** 校验打开应用值（形如 host_action:<uuid>）。 */
export function validateHostAction(
  value: string,
  prefix: string,
): string | null {
  if (!value.startsWith(prefix)) return `打开应用值必须以 ${prefix} 开头`;
  if (!isCanonicalUuid(value.slice(prefix.length)))
    return "打开应用 UUID 格式非法";
  return null;
}

/** 校验动作。prefix 用于 host_action 校验。 */
export function validateAction(
  action: ActionSpec,
  namedActions: string[],
  prefix: string,
  maxTextBytes: number,
): string | null {
  switch (action.kind) {
    case "disabled":
      return null;
    case "named":
      return namedActions.includes(action.value)
        ? null
        : `具名动作「${action.value}」不在固件词表中`;
    case "hotkey":
      return validateHotkey(action.value);
    case "text":
      return validateText(action.value, maxTextBytes);
    case "host_action":
      return validateHostAction(action.value, prefix);
    default:
      return "未知动作类型";
  }
}

/** 校验打开目标。 */
export function isValidAppTarget(target: AppTarget): string | null {
  switch (target.kind) {
    case "app":
      return target.path.trim() ? null : "可执行文件路径不能为空";
    case "file":
      return target.path.trim() ? null : "文件路径不能为空";
    case "url":
      return target.url.startsWith("http://") || target.url.startsWith("https://")
        ? null
        : "URL 需以 http:// 或 https:// 开头";
    case "folder":
      return target.path.trim() ? null : "文件夹路径不能为空";
    default:
      return "未知打开目标类型";
  }
}

/** 统计 UTF-8 字节数。 */
export function utf8Bytes(text: string): number {
  return new TextEncoder().encode(text).length;
}
