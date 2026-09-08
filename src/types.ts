// 与 Rust 后端模型一一对应的 TypeScript 类型。
// 序列化规则来自 `model.rs` 的 serde 标签：
//   ActionSpec  -> { kind, value }   （Disabled 只有 kind）
//   AppTarget   -> { kind, ... }
//   Profile     -> { id, name, keys, encoder }

/** 一个输入（键）上的动作，kind + value 判别式。 */
export type ActionSpec =
  | { kind: "disabled" }
  | { kind: "named"; value: string }
  | { kind: "hotkey"; value: string }
  | { kind: "text"; value: string }
  | { kind: "host_action"; value: string };

/** 本机应用库的打开目标。 */
export type AppTarget =
  | { kind: "app"; path: string; args: string; work_dir: string }
  | { kind: "file"; path: string }
  | { kind: "url"; url: string }
  | { kind: "folder"; path: string };

/** 应用库条目。 */
export interface AppEntry {
  uuid: string;
  title: string;
  target: AppTarget;
}

/** 后端 `list_system_apps` 枚举到的本机已注册应用。 */
export interface SystemApp {
  name: string;
  path: string;
  /** "app" = 直接启动；"shell" = 用系统默认方式打开（.lnk/.url）。 */
  kind: "app" | "shell";
}

/** 一个按键方案（profile）。keys: KEY1..KEY8，encoder: left/right/press。 */
export interface Profile {
  id: string;
  name: string;
  keys: Record<string, ActionSpec>;
  encoder: Record<string, ActionSpec>;
}

/** 固件配置下发回执（0x11，kind 0x03）。 */
export interface ConfigAck {
  ok: boolean;
  saved: boolean;
  phase: number;
  bytes: number;
  crc16: number;
}

/** 后端 `get_vocabulary` 返回的词表与常量。 */
export interface Vocabulary {
  named_actions: string[];
  modifiers: string[];
  named_keys: string[];
  text_max_utf8_bytes: number;
  host_action_prefix: string;
  schema: string;
  keys: string[];
  encoder_inputs: string[];
}

/** 后端 `host-action` 事件负荷。 */
export interface HostActionEvent {
  uuid: string;
  message: string;
}

/** 后端日志缓冲区的一条日志（供「日志」面板实时调试）。 */
export interface LogEntry {
  seq: number;
  ts: string;
  level: "info" | "warn" | "err" | string;
  msg: string;
}

/** LLM 后端类型。 */
export type LlmBackend = "doubao" | "openai" | "anthropic" | "ollama" | "custom";

/** LLM 配置（持久化到 config.json）。 */
export interface LlmConfig {
  backend: LlmBackend;
  endpoint: string;
  model: string;
  api_key: string;
  timeout_secs: number;
}

/** 语音编辑阶段。 */
export type EditPhase = "idle" | "listening" | "processing";
