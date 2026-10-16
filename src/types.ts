// 与 Rust 后端模型一一对应的 TypeScript 类型。
// 序列化规则来自 `model.rs` 的 serde 标签：
//   ActionSpec  -> { kind, value }   （Disabled 只有 kind）
//   AppTarget   -> { kind, ... }
//   Profile     -> { id, name, keys, encoder, prompts }

/** 一个输入（键）上的动作，kind + value 判别式。 */
export type ActionSpec =
  | { kind: "disabled" }
  | { kind: "named"; value: string }
  | { kind: "hotkey"; value: string }
  | { kind: "text"; value: string }
  | { kind: "host_action"; value: string }
  /** Agent 任务台槽位按键。value 是槽位号字符串 "1"–"4"。 */
  | { kind: "task_desk"; value: string }
  /** 内容台旋钮导航。value 是 "prev" / "next" / "confirm"。 */
  | { kind: "studio"; value: string };

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

/**
 * 一条提示词（KEY4 浮窗提示词库的条目）。
 *
 * 与「固定文本动作」（`ActionSpec` 的 `text`）是两回事：固定文本动作挂在某个按键上，
 * 会被下发到固件；提示词只是主机端的一份常用语清单，不下发固件，只服务于浮窗。
 * `id` 由主机端生成，用于定位「改哪一条 / 删哪一条」。
 */
export interface Prompt {
  id: string;
  text: string;
}

/** 一个按键方案（profile）。keys: KEY1..KEY8，encoder: left/right/press。 */
export interface Profile {
  id: string;
  name: string;
  keys: Record<string, ActionSpec>;
  encoder: Record<string, ActionSpec>;
  /** 该方案专属的提示词库，随方案切换。旧配置里没有这个字段，读到时按空数组处理。 */
  prompts: Prompt[];
}

/** 固件配置下发回执（0x11，kind 0x03）。 */
export interface ConfigAck {
  ok: boolean;
  saved: boolean;
  phase: number;
  bytes: number;
  crc16: number;
}

/**
 * 开发板固件状态（后端 `device_firmware_status` 回读的摘要）。
 *
 * `firmware` 是板子**自己报出**的固件版本串（如 `0.4.53-idf-v2-ble-store-v1`）——
 * 这是"板子上到底烧了哪一版"唯一可信的回答，不依赖任何本地文件。
 */
export interface DeviceFirmwareStatus {
  firmware: string;
  /** 状态 JSON 的 schema，如 `ai_keyboard.config_status.v1`（用于判断协议代际）。 */
  schema: string;
  /** 电池电压 mV；固件没报就是 null。 */
  battery_mv: number | null;
  /** 电池电量 %；固件没报就是 null。 */
  battery_percent: number | null;
}

/** 授权（注册）状态——本地激活码校验结果。 */
export interface LicenseInfo {
  /** 是否有效（已注册且未过期/未篡改）。 */
  valid: boolean;
  /** 版次标识，如 trial / pro / team。 */
  edition: string;
  /** 过期时间（Unix 毫秒）；null = 永不过期。 */
  exp: number | null;
  /** 备注（授权对象、订单号等）。 */
  note: string;
  /** 是否绑定了机器。 */
  machineBound: boolean;
  /** 给用户的提示文案。 */
  message: string;
}

/** 一组可绑定到按键的「具名动作」，前端以独立下拉菜单呈现（按插件归类）。 */
export interface ActionGroup {
  /** 分组 id：插件 id（如 `live-translate`）。 */
  id: string;
  /** 分组展示名（下拉菜单标题），如「实时翻译」「内容台」「风格调色盘」。 */
  label: string;
  /** 该分组下的动作 id 列表（顺序即下拉顺序）。 */
  actions: string[];
}

/** 后端 `get_vocabulary` 返回的词表与常量。 */
export interface Vocabulary {
  named_actions: string[];
  /** 插件声明的动作 id → 中文标签（来自各插件 commands[].title）。 */
  action_labels: Record<string, string>;
  /** 按插件归类的动作分组（每个可见插件 => 一个顶层菜单）。 */
  action_groups: ActionGroup[];
  /** 主机内置功能动作（「功能动作」菜单用，不含插件命令）。 */
  core_actions: string[];
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
// ── 使用记录（工作台"使用记录中心"）──

/** 一条使用记录。 */
export interface UsageEntry {
  ts: string;
  kind: "asr" | "ss_exec" | "prompt_use" | "key_action" | string;
  text: string;
  source: string;
  profile?: string;
}

/** 日历统计：某天的记录数与类型分布。 */
export interface DailyCount {
  date: string;
  count: number;
  by_kind: Record<string, number>;
}

// ── ASR 自定义词库 ──

/** 一条热词。 */
export interface VocabWord {
  text: string;
  weight: number;
}

// ── Agent 任务台 ──
//
// 与 Rust 侧 `agent_desk::config` 一一对应。改动任一侧都要同步另一侧，
// 因为 serde 的字段名就是这里的字段名（没有中间层做转译）。

/** 执行器形态。 */
export type DriverKind = "api" | "cli";

/** 任务能力分类——"路由"要判的就是它。 */
export type Capability = "text" | "code" | "image" | "video" | "search" | "vision";

/** 完成信号来源。铁律：`process_exit_zero` 单独出现时可信度只有「未校验」。 */
export type CompletionKind =
  | "http_stream_end"
  | "log_pattern"
  | "process_exit_zero"
  | "async_task_poll";

/** 完成判定配置。 */
export interface CompletionSignal {
  kind: CompletionKind;
  /**
   * `log_pattern` 用的匹配串。
   *
   * 支持 **多个候选，`;` 分隔，任一命中即完成**（不含分隔符时就是单串，与旧值兼容）。
   * 上游工具的日志格式会随版本变，绑死一个串等于上游一改名，全部任务集体判失败。
   */
  pattern: string;
  /** 世界门禁命令：非空时只有它退出码 0 才算真完成，可信度升为「已校验」。 */
  evidence_cmd: string;
}

/** 一个可被调度的执行器（模型 API 或 Agent CLI）。 */
export interface DriverConfig {
  id: string;
  label: string;
  kind: DriverKind;
  enabled: boolean;
  /** Api 形态：端点。 */
  endpoint: string;
  /** Api 形态：模型名。 */
  model: string;
  /** Api 形态：密钥（默认空，由用户填写）。 */
  api_key: string;
  /** Cli 形态：可执行文件。 */
  command: string;
  /** Cli 形态：参数模板，必须含 `{prompt}`。 */
  args: string[];
  /** Cli 形态：环境变量 `KEY=VALUE`，CLI 必须显式给 `HOME`。 */
  env: string[];
  /** Cli 形态：工作目录。 */
  cwd: string;
  /**
   * 单次执行超时（毫秒）。**0 = 用后端默认**（180000，即 180 秒）。
   *
   * ⚠️ 后端是 `#[serde(default)]` 反序列化的：**前端不认识这个字段就会在
   * 回存配置时把它静默清零**（读进来时是 900000，点保存后变 0）。所以两侧必须同改。
   */
  timeout_ms: number;
  completion: CompletionSignal;
  /**
   * 会话续接配置。让多轮对话**接着上一轮**，而不是每次都重开会话。
   *
   * - `null` = 这个执行器不支持 / 未配置续接；
   * - `{}` 里的三个字段说明"会话 id 藏在哪、怎么带回去"。
   *
   * ⚠️ 与 `timeout_ms` 同一个坑：后端是 `#[serde(default)]` 反序列化，
   * **前端不认识这个字段就会在回存时把它静默清零** —— 表现为
   * "保存一次之后 codex 就不再续接了"。所以前端表单必须原样带着它。
   */
  continuation: ContinuationSpec | null;
  capabilities: Capability[];
  /**
   * 产物过滤（可选 glob，如 `out/*.png`）。空 = 只按时间窗判定，不做名字过滤。
   *
   * ⚠️ **相对「工作目录」匹配**，不是相对磁盘根：
   * `*.md` 只命中工作目录**根下**的 markdown；`**` 才跨层，`*` 不跨分隔符。
   * 没配工作目录时**不会扫盘**（拿 easyop 自己的目录顶替会打开一堆无关文件）。
   */
  artifact_glob: string;
  /**
   * 任务成功后是否自动打开产物。
   *
   * 打开策略：只扫到 1 个 ⇒ 直接打开它；多个 ⇒ 打开所在文件夹并选中**最新**那个。
   * 只在**任务成功**且**确实扫到**时才动作 —— 它会抢焦点，抢错了比不打开烦人得多。
   *
   * ⚠️ 与 `timeout_ms` 同一个坑：后端是 `#[serde(default = "default_true")]`，
   * **前端不认识这个字段就会在回存时把它静默清零**。而且这个字段的"零"是 `false`，
   * 不是后端默认的 `true` —— 表现就是"保存一次之后自动打开产物就悄悄关了"。
   */
  artifact_open: boolean;
  note: string;
}

/**
 * 会话续接配置。
 *
 * 三个字段回答同一件事的三个分段：**id 从哪来 / 叫什么 / 怎么带回去**。
 * 不同工具的形态不同（codex 用子命令 `resume <id>`，别的可能是 `--session <id>`），
 * 所以它是数据而不是代码。
 */
export interface ContinuationSpec {
  /** 会话 id 出现在哪种 stdout 事件里（比对 JSONL 的 `"type"` 字段）。空 = 不启用。 */
  id_event_type: string;
  /** 会话 id 在那个事件里的键名。 */
  id_field: string;
  /** 续接时追加的参数模板，含 `{id}` 的元素会被替换。 */
  args: string[];
}

/**
 * 某槽正在续接的会话摘要（界面显示用，**不含完整 id**）。
 */
export interface SessionView {
  driver_id: string;
  /** id 前 8 位 —— 够用来确认"这两轮是不是同一段对话"。 */
  id_short: string;
  at_ms: number;
}

/** 槽位绑定——三种取值就是"人选执行器 / 模型选能力 / 全自动"三层。 */
export type SlotBinding =
  | { mode: "empty" }
  | { mode: "exact"; driver: string }
  | { mode: "capability"; capability: Capability; prefer: string[] }
  | { mode: "auto" };

/** 任务完成后的总结配置。 */
export interface SummaryConfig {
  /** 用哪个执行器做总结。空 = 复用语音编辑的 LLM 通道。 */
  driver_id: string;
  prompt_template: string;
  max_chars: number;
  fallback_truncate: boolean;
}

/** 状态播报配置。Notice 不产生未读，与 Result 严格分离。 */
export interface NoticeConfig {
  /** 总开关。**只管喇叭，不管灯。** */
  enabled: boolean;
  on_start: boolean;
  /** 进度类（正在推进 / 已运行 N 分钟）。单独一档，方便"只关吵的那一类"。 */
  on_progress: boolean;
  on_complete: boolean;
  on_fail: boolean;
  /** 同类 Notice 最小间隔（秒）。0 = 不限。只约束进度类。 */
  min_interval_secs: number;
  /**
   * 「已运行 N 分钟」的固定档位（秒），升序。
   *
   * ⚠️ **档数上限由后端语料闭集决定**（当前 3 档 = 1/5/15 分钟）。多写的会被
   * 后端静默截断，因为每档都要有一句固定语料并预合成成音频。
   */
  progress_marks_secs: number[];
}

/** TTS 服务商。`volc` = 豆包语音合成大模型（火山引擎），`bailian` = 阿里云百炼。 */
export type TtsProvider = "volc" | "bailian";

/**
 * TTS 配置。交付采样率恒为 48000（固件要求）。
 *
 * 两个 provider 协议不同，字段分两套（见 Rust 侧 `tts::TtsConfig` 的表格）：
 * `resource_id` / `explicit_language` 属豆包，`model` / `language_type` 属百炼。
 */
export interface TtsConfig {
  provider: TtsProvider;
  endpoint: string;
  /** 豆包专用：`X-Api-Resource-Id`（seed-tts-1.0 / seed-tts-2.0）。 */
  resource_id: string;
  /** 百炼专用：模型名。 */
  model: string;
  api_key: string;
  /** 音色名——豆包填 speaker ID，百炼填 voice 名，两者不通用。 */
  voice: string;
  /** 豆包专用：`additions.explicit_language`，空则不传。 */
  explicit_language: string;
  /** 百炼专用：`input.language_type`。 */
  language_type: string;
  target_sample_rate: number;
  max_chars: number;
  timeout_secs: number;
}

/** Agent 任务台总配置。 */
export interface DeskConfig {
  schema: number;
  enabled: boolean;
  /** 产物交付目录。空 = 用配置目录旁的 agent_desk_out。 */
  output_dir: string;
  tts: TtsConfig;
  summary: SummaryConfig;
  notice: NoticeConfig;
  /** 外部会话监工（C：盯 codex 的 rollout 日志）。 */
  probe: ProbeConfig;
  drivers: DriverConfig[];
  slots: SlotBinding[];
}

/** 外部会话监工（C 方案）。 */
export interface ProbeConfig {
  /** 总开关。默认开：没装 codex 时根目录不存在 ⇒ 零副作用。 */
  enabled: boolean;
  /** rollout 根目录。留空 = 用 `%USERPROFILE%\.codex\sessions`。 */
  root: string;
}

// ⚠️ 原 `DriverView`（执行器的 UI 视图）已于 v0.39.2 移除，后端 `DeskSnapshot.drivers` 同步删除。
// 原因：任务台那个「执行器」板块是重复的第三份——配置与「探测」在「模型配置 → ④ 执行器」，
// 不可调度的原因在页首 `blockers` 里已按槽位逐条列出。⛔ 不要再加回来。
// 注意：`DriverProbe`（探测返回体）**仍然保留**，它属于模型配置页，勿一并删除。

/** 槽位视图。 */
export interface SlotView {
  index: number;
  mode: string;
  detail: string;
  /** 需要注意但不阻塞的事（如"能力路由尚未实现"）。 */
  notes: string[];
}

/** 任务台总览。 */
export interface DeskSnapshot {
  enabled: boolean;
  ready: boolean;
  /** 未就绪的具体原因，逐条列。 */
  blockers: string[];
  tts_configured: boolean;
  tts_model: string;
  output_dir: string;
  config_path: string;
  slots: SlotView[];
  /**
   * 喇叭上**正在播**的内容（v0.43.4 新增；`null` = 空闲）。
   *
   * 没有它，前端只能显示"正在播报"四个字 —— 排队等待的不可见正是
   * "S2 怎么没声音"这类反馈的温床（等待感来自不可见，不来自等待）。
   */
  playing: PlayingView | null;
  /**
   * **排队等补播**的通知（队首在前，v0.43.4 新增）。
   *
   * 与 `playing` 合起来回答：现在在播什么、我还有几条没播、分别是什么。
   */
  notice_queue: NoticeQueueItemView[];
}

/** 当前喇叭上正在播的内容（`DeskSnapshot.playing`）。 */
export interface PlayingView {
  /** `null` = 正在播某槽的**结果**（用户按下的）；否则为状态播报种类。 */
  notice_kind: NoticeKind | null;
  text: string;
  /** 槽位标签（"S1"…）；无槽位为 `null`。 */
  slot_label: string | null;
}

/** 排队中的一条通知（`DeskSnapshot.notice_queue` 的元素）。 */
export interface NoticeQueueItemView {
  /** 排队位置（1 起，1 = 下一个就轮到它）。 */
  position: number;
  kind: NoticeKind;
  /** 展示文本：有动态正文用正文，否则用固定语料。 */
  text: string;
  /** 槽位标签（"S1"…）；无槽位为 `null`。 */
  slot_label: string | null;
  /** 是否终态播报（前端据此配色：终态=该听完的承诺，进度=状态更新）。 */
  terminal: boolean;
}

// ---------------------------------------------------------------------------
// 任务台运行时（Phase 2：队列 + 未读 + 播放租约）
//
// 与 Rust 侧 `agent_desk::{queue,unread,state}` 的视图结构一一对应。
// 字段名一律保持 snake_case，跟其它接口一致，不做驼峰转换。
// ---------------------------------------------------------------------------

/** 队列里的一条任务。 */
export interface QueuedTask {
  id: number;
  slot: number;
  prompt: string;
  driver_id: string;
  enqueued_at_ms: number;
}

/** 正在执行的一条任务。 */
export interface RunningTask {
  task: QueuedTask;
  started_at_ms: number;
}

export interface SlotQueueView {
  index: number;
  running?: RunningTask | null;
  pending: QueuedTask[];
  depth: number;
}

export interface QueueSnapshot {
  /** 正在执行的**槽位数**，不是任务条目数。第 5 颗灯的输入。 */
  running_count: number;
  total_pending: number;
  active_drivers: string[];
  completed: number;
  failed: number;
  slots: SlotQueueView[];
}

export interface SlotUnreadView {
  index: number;
  pending: number;
  generation: number;
  leased: boolean;
}

export interface UnreadSnapshot {
  total_pending: number;
  /** 正在持有播放租约的槽位；null/undefined = 喇叭空闲。 */
  playing_slot?: number | null;
  slots: SlotUnreadView[];
}

/**
 * 运行时三态灯。**档位由后端给**（`RuntimeSnapshot.light`），前端只做颜色与文案映射。
 *
 * ⛔ 不要在前端从 `busy` / `idle` / `queue.total_pending` 自己推档位 ——
 * 那会让"什么算积压"有了两份实现：后端一改判据，这里不会跟着动，
 * 界面就会**静默地**显示错误颜色。本项目在"字幕浮层显隐"上已经吃过一次这个亏。
 */
export type LightState = "idle" | "running" | "backlog";

/** 运行时总快照（事件 `agent-desk-state` 的载荷）。 */
export interface RuntimeSnapshot {
  queue: QueueSnapshot;
  unread: UnreadSnapshot;
  /** 三态灯档位，语义见 {@link LightState}。 */
  light: LightState;
  busy: boolean;
  /** 完全空闲（没在跑、也没排队）。注意与 `!busy` 不等价。 */
  idle: boolean;
  any_unread: boolean;
}

/** 派发结果：立刻开始 或 排队等待。 */
export type DispatchOutcome =
  | { state: "started"; task_id: number; depth: number }
  | { state: "queued"; task_id: number; ahead: number };

/** 播放租约。 */
export interface Lease {
  slot: number;
  token: number;
  granted_at_ms: number;
  generation: number;
  granted_pending: number;
}

/** 一次成功回执的结果。 */
export interface ConsumeOutcome {
  slot: number;
  remaining: number;
}

/** 统一返回信封（对应 Rust 的 `ApiEnvelope`）。 */
export interface ApiEnvelope<T> {
  ok: boolean;
  code: string;
  data?: T;
  message?: string;
}

/** TTS 试合成报告。 */
export interface TtsTestReport {
  sample_rate: number;
  samples: number;
  duration_ms: number;
  characters?: number;
  /** 落盘的 WAV 绝对路径。 */
  wav_path: string;
}

/** 执行器探测报告。`scope` 说明本次只做了哪一层检查。 */
export interface DriverProbe {
  id: string;
  ready: boolean;
  scope: string;
  detail: string;
}

// ---------------------------------------------------------------------------
// 任务台播报（Phase 3 第二段「后声」）
//
// ⛔ 一条要记住的边界：状态播报（Notice）**不产生未读**。
// 收到 Notice 只说明"发生了什么"，不代表"有结果等你听" ——
// 后者看 RuntimeSnapshot.unread。两者是两条独立通道，别混。
// ---------------------------------------------------------------------------

/** 状态播报的固定语料（闭合集，与 Rust 的 `NoticeKind` 一一对应）。 */
export type NoticeKind =
  | "task_started"
  | "progressing"
  | "running_1min"
  | "running_5min"
  | "completed"
  | "completed_artifact_failed"
  | "failed"
  | "needs_confirmation"
  | "timed_out"
  | "lost_contact"
  // ⚠️ 追加在末尾：Rust 侧 `NoticeKind::code()` 即缓存文件名，顺序是契约。
  | "running_15min"
  | "results_waiting";

/** 一条语料的缓存状态。 */
export interface NoticeItemView {
  kind: NoticeKind;
  code: number;
  /** 固定语料文本。 */
  text: string;
  /** 是否已有可用缓存（存在且长度合理）⇒ 能离线播。 */
  cached: boolean;
  path?: string;
  bytes?: number;
}

export interface NoticeStatusReport {
  dir: string;
  total: number;
  cached: number;
  /** 全部就绪 ⇒ **离线播报可用**（拔网线也能播状态）。 */
  offline_ready: boolean;
  /** TTS 未配置 ⇒ 无法预合成，必须让用户看到原因。 */
  tts_configured: boolean;
  /** 上一条真的播出去的通知（诊断"为什么刚才没响"的第一站）。 */
  last_spoken?: NoticeKind;
  items: NoticeItemView[];
}

export interface NoticePrepareFailure {
  kind: NoticeKind;
  reason: string;
}

export interface NoticePrepareReport {
  synthesized: NoticeKind[];
  already_cached: NoticeKind[];
  failed: NoticePrepareFailure[];
  offline_ready: boolean;
  duration_ms: number;
}

export interface NoticePlayReport {
  kind: NoticeKind;
  text: string;
  /** 是否完整播完（被抢占则为 false）。 */
  completed: boolean;
  duration_ms: number;
  played_samples: number;
  total_samples: number;
}

export interface RenderResultReport {
  slot: number;
  task_id?: number;
  /** 真正送给 TTS 的文本 —— 回传是为了可核对"它到底念了什么"。 */
  spoken: string;
  success: boolean;
  result_at_ms: number;
  completed: boolean;
  lease_token: number;
  /** ⭐ 只有完整播完才为 true。被中断时未读必须留着。 */
  consumed: boolean;
  unread_left: number;
  duration_ms: number;
}

/** `agent-desk-notice` 事件的载荷。 */
export interface NoticeEventPayload {
  slot?: number;
  kind: NoticeKind;
  text: string;
  /** 是否终态（值得打断的判据）。 */
  terminal: boolean;
  audio_path?: string;
  /** 补充文本（C：外部会话的最终回复摘要）。**不参与语音**，只用于字幕。 */
  detail?: string;
}

/**
 * `agent-desk-channel` 事件载荷：喇叭正在播什么 + 后面还排着谁。
 *
 * ⚠️ 为什么不塞进 `agent-desk-state`：**触发时机不同**。
 * `agent-desk-state` 由任务状态驱动，而喇叭的占用与排队由**播放动作**驱动 ——
 * 一次播完、一次抢占、一次入队，都可能发生在任务状态完全没变的时刻。
 * 等任务状态再变时才通知，队列等待期就是一片死寂（遗留项第 3 条）。
 */
export interface ChannelStatePayload {
  playing: PlayingView | null;
  notice_queue: NoticeQueueItemView[];
}

// ---------------------------------------------------------------------------
// 文本注入（A″：语音打进终端 / TUI 场景）
// ---------------------------------------------------------------------------

/**
 * 模拟粘贴时按哪一组键。
 *
 * 为什么必须可配：`Ctrl+V` 是**图形界面**的约定；终端里的 TUI 程序（codex 等）
 * 把终端置于 raw mode 直接读按键字节，`Ctrl+V` 在那里是字面量 0x16，会被 TUI
 * 当成自己的快捷键吃掉 —— **粘不进去**。
 *
 * ⚠️ 终端该用哪一组键**没有统一答案**，而且**人手能按 ≠ 程序模拟能生效**。
 * 本机用程序模拟逐个实测的结论（`text_delivery.rs` 的 `resolve_for_window`）：
 * Windows Terminal → `Ctrl+Shift+V`；Git Bash（mintty）→ **`Ctrl+V`**。
 * ⚠️ mintty 那一行是人手 `Shift+Insert` 好好的、但**程序模拟一个键都到不了**
 * （它读修饰键用泛用 `VK_SHIFT`，注入的 shift 置不上那个位）；mintty 为注入专门留了
 * `Ctrl+V` 分支。所以**「自动」给 Git Bash 选的是 `Ctrl+V`，不是 `Shift+Insert`**。
 *
 * `auto` 是默认值：由**焦点窗口的类名**自动决定（按实测表选键），不必手动切。
 */
export type PasteStyle = "auto" | "ctrl_v" | "shift_insert" | "ctrl_shift_v";

/** 每种方式的界面说明（顺序 = 下拉框顺序）。 */
export const PASTE_STYLE_OPTIONS: { value: PasteStyle; label: string; hint: string }[] = [
  {
    value: "auto",
    label: "自动（推荐）",
    hint: "看焦点窗口：Windows Terminal → Ctrl+Shift+V，Git Bash → Ctrl+V，其余 → Ctrl+V",
  },
  { value: "ctrl_v", label: "Ctrl+V", hint: "图形界面约定；也是程序模拟在 Git Bash 里唯一有效的那一组" },
  {
    value: "shift_insert",
    label: "Shift+Insert",
    hint: "人手动按在 Git Bash 里有效，但程序模拟发过去无反应 —— 慎选",
  },
  {
    value: "ctrl_shift_v",
    label: "Ctrl+Shift+V",
    hint: "Windows Terminal 的粘贴键（程序模拟实测可用）",
  },
];

// ---------------------------------------------------------------------------
// 「启动终端并预置命令」
// ---------------------------------------------------------------------------

/**
 * 用哪个终端承载预置命令。
 *
 * `auto` 的判定是"**看文件在不在**"（优先 Windows Terminal，没有则 cmd），
 * 而不是"试着起一个、失败再换" —— 后者会先弹一个空窗口再弹一个，用户看到两个窗口闪。
 */
export type TerminalKind = "auto" | "wt" | "cmd" | "powershell";

/** 终端选项的界面说明（顺序 = 下拉框顺序）。 */
export const TERMINAL_KIND_OPTIONS: { value: TerminalKind; label: string; hint: string }[] = [
  {
    value: "auto",
    label: "自动检测（推荐）",
    hint: "优先 Windows Terminal（实测粘贴键 Ctrl+Shift+V），没有则用 cmd.exe",
  },
  { value: "wt", label: "Windows Terminal", hint: "强制用 wt.exe 开新窗口" },
  { value: "cmd", label: "cmd.exe", hint: "经典命令提示符（ConsoleWindowClass，粘贴键回落 Ctrl+V）" },
  { value: "powershell", label: "Windows PowerShell", hint: "powershell.exe 新窗口" },
];

/**
 * 「启动终端」配置（后端 `cli_terminal.json`）。
 *
 * ⚠️ `command` 是**命令前缀**，不是完整命令 —— 语音说出的内容会接在它后面。
 * 所以界面文案必须说清这一点，否则用户会以为"按一下就跑了"。
 */
export interface CliTerminalConfig {
  /** 要预置到输入框里的命令（不含回车；末尾的空格由后端补）。 */
  command: string;
  /** 工作目录（空 = 用户主目录）。 */
  cwd: string;
  terminal: TerminalKind;
  /** 打完是否直接回车执行。默认 false（预置前缀，等语音补全）。 */
  auto_run: boolean;
}
