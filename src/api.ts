// 与后端 Tauri 命令的封装层。前端仅通过这里的函数与 Rust 交互。
import { invoke } from "@tauri-apps/api/core";
import type { AppEntry, ConfigAck, DeviceFirmwareStatus, LicenseInfo, LogEntry, Profile, SystemApp, Vocabulary, LlmConfig, EditPhase, UsageEntry, DailyCount, VocabWord, DeskConfig, DeskSnapshot, DriverProbe, TtsConfig, TtsTestReport, RuntimeSnapshot, DispatchOutcome, Lease, ConsumeOutcome, ApiEnvelope, NoticeKind, NoticeStatusReport, NoticePrepareReport, NoticePlayReport, RenderResultReport, PasteStyle, SessionView, CliTerminalConfig } from "./types";

/** 检测开发板 HID 是否在线。 */
export function deviceStatus(): Promise<string> {
  return invoke<string>("device_status");
}

/**
 * 读开发板固件版本（USB Feature Report `0x13` → `0x11` / kind `0x04` 分块响应）。
 *
 * ⚠ 与 `deviceStatus()` 是**两件事**，别合并：那条是"枚举即返回"的在线检测（会被轮询），
 * 这条要走一次完整的状态回读（实机约 100 ms）。失败时抛出的是**具体原因**
 * （没插 USB / 载荷被固件拒 / 分块 CRC 不过），不是一句笼统的"读取失败"。
 */
export function deviceFirmwareStatus(): Promise<DeviceFirmwareStatus> {
  return invoke<DeviceFirmwareStatus>("device_firmware_status");
}

/**
 * 读取当前授权（注册）状态：从本地配置加载已保存的激活码并重新校验。
 * 返回统一信封——`ok` 字段判断成败，`data` 拿 `LicenseInfo`。
 */
export function licenseStatus(): Promise<ApiEnvelope<LicenseInfo>> {
  return invoke<ApiEnvelope<LicenseInfo>>("license_status");
}

/** 激活（注册）一个本地授权码：后端校验签名通过后持久化。 */
export function activateLicense(code: string): Promise<ApiEnvelope<LicenseInfo>> {
  return invoke<ApiEnvelope<LicenseInfo>>("activate_license", { code });
}

/** 下发一个完整方案到开发板。 */
export function pushConfig(profile: Profile): Promise<ConfigAck> {
  return invoke<ConfigAck>("push_config", { profile });
}

/** 拉取词表与常量。 */
export function getVocabulary(): Promise<Vocabulary> {
  return invoke<Vocabulary>("get_vocabulary");
}

/** 启动 Host Action 常驻监听器。 */
export function startListener(): Promise<boolean> {
  return invoke<boolean>("start_listener");
}

/** 停止 Host Action 常驻监听器。 */
export function stopListener(): Promise<boolean> {
  return invoke<boolean>("stop_listener");
}

/** 查询监听器运行状态。 */
export function listenerStatus(): Promise<boolean> {
  return invoke<boolean>("listener_status");
}

/** 手动「测试打开」一个应用库条目。 */
export function testOpen(entry: AppEntry): Promise<string> {
  return invoke<string>("test_open", { entry });
}

/** 读取应用库。 */
export function listApps(): Promise<AppEntry[]> {
  return invoke<AppEntry[]>("list_apps");
}

/** 枚举本机已注册/可见的应用（开始菜单可启动项）。 */
export function listSystemApps(): Promise<SystemApp[]> {
  return invoke<SystemApp[]>("list_system_apps");
}

/** 新增应用库条目。 */
export function addApp(entry: AppEntry): Promise<AppEntry> {
  return invoke<AppEntry>("add_app", { entry });
}

/** 更新应用库条目。 */
export function updateApp(entry: AppEntry): Promise<AppEntry> {
  return invoke<AppEntry>("update_app", { entry });
}

/** 删除应用库条目（按 uuid）。 */
export function removeApp(uuid: string): Promise<void> {
  return invoke<void>("remove_app", { uuid });
}

/** 读取方案库。 */
export function listProfiles(): Promise<Profile[]> {
  return invoke<Profile[]>("list_profiles");
}

/** 新建一个默认方案。 */
export function newProfile(name: string): Promise<Profile> {
  return invoke<Profile>("new_profile", { name });
}

/** 保存（新增或按 id 更新）方案。 */
export function saveProfile(profile: Profile): Promise<Profile> {
  return invoke<Profile>("save_profile", { profile });
}

/** 删除方案（按 id）。 */
export function deleteProfile(id: string): Promise<void> {
  return invoke<void>("delete_profile", { id });
}

/** 设置当前激活方案 ID（同步主机端状态，供固件按键切换时使用）。 */
export function setActiveProfile(id: string | null): Promise<void> {
  return invoke<void>("set_active_profile", { id });
}

/** 增量拉取日志：返回序号严格大于 `since` 的所有条目。 */
export function getLogs(since: number): Promise<LogEntry[]> {
  return invoke<LogEntry[]>("get_logs", { since });
}

/** 清空后端日志缓冲区。 */
export function clearLogs(): Promise<void> {
  return invoke<void>("clear_logs");
}

/**
 * 返回日志目录（落盘文件所在）。
 *
 * 日志现在**同时写盘**（`logs/easyop-YYYY-MM-DD.log`）：内存那份一关程序就没了，
 * 而排障恰恰发生在"刚出过事"的时候。这个接口给界面用，让用户能一键找到当天那份。
 */
export function getLogDir(): Promise<string> {
  return invoke<string>("get_log_dir");
}

/** 在资源管理器中打开日志目录。 */
export function openLogDir(): Promise<string> {
  return invoke<string>("open_log_dir");
}

/** 查询开发板 Wi-Fi 音频是否在线。 */
export function keyboardAudioOnline(): Promise<boolean> {
  return invoke<boolean>("keyboard_audio_online");
}

/**
 * 一次 Wi-Fi 下发的如实结果。
 *
 * ⚠️ 字段名是 **snake_case**：本仓库 Rust 侧只有少数结构体用 `rename_all`，
 * 这里与 `wifi_link.rs` 的字段名保持一致（照搬会得到一堆 `undefined`）。
 */
export interface WifiConnectOutcome {
  ssid: string;
  /** `usb-hid` / `wifi-udp` / `usb-hid + wifi-udp` */
  channel: string;
  /** 板子是否已把端点写进 NVS ⇒ 下次上电它能自己连，不需要上位机在场。 */
  saved_to_board: boolean;
  lan_ipv4: string | null;
  /** 还没连上时给用户的一句话（连上了是 null）。 */
  hint: string | null;
}

export interface KnownWifiView {
  ssid: string;
  last_success_unix: number;
  success_count: number;
  has_password: boolean;
  last_error: string;
}

/** Wi-Fi 链路看护线程的实时状态。 */
export interface WifiLinkStatus {
  /** disabled | no-history | idle | provisioning | waiting-heartbeat | online | blocked */
  state: string;
  state_text: string;
  board_online: boolean;
  board_hid_present: boolean;
  current_ssid: string | null;
  lan_ipv4: string | null;
  target_ssid: string | null;
  target_reason: string | null;
  saved_to_board: boolean;
  attempts: number;
  next_retry_secs: number;
  last_error: string | null;
  last_channel: string | null;
  known: KnownWifiView[];
}

/**
 * 下发 Wi-Fi 端点并持久化。
 *
 * ⛔ 别把这个 `await` 当成「连上了」：它只说明**端点已经发出去了**（并且带回了板子的
 * 存盘回执）。真正连上的判据是 `wifiLinkStatus().board_online`（收到的板子心跳）。
 */
export function wifiConnect(ssid: string, password: string): Promise<WifiConnectOutcome> {
  return invoke<WifiConnectOutcome>("wifi_connect", { ssid, password });
}

/** 断开开发板 Wi-Fi 音频（下发空端点，板子会释放 Wi-Fi 省电）。 */
export function wifiDisconnect(): Promise<string> {
  return invoke<string>("wifi_disconnect");
}

/** 读 Wi-Fi 链路实时状态（含历史网络、环境、重试倒计时）。 */
export function wifiLinkStatus(): Promise<WifiLinkStatus> {
  return invoke<WifiLinkStatus>("wifi_link_status");
}

/** 下发蓝牙启用命令到开发板。 */
export function bleEnable(): Promise<void> {
  return invoke<void>("ble_enable");
}

/** 下发蓝牙关闭命令到开发板。 */
export function bleDisable(): Promise<void> {
  return invoke<void>("ble_disable");
}

/** 下发蓝牙配对命令到开发板。 */
export function blePair(): Promise<void> {
  return invoke<void>("ble_pair");
}

/** 查询开发板蓝牙连接状态。 */
export function bleStatus(): Promise<string> {
  return invoke<string>("ble_status");
}

/** 设置麦克风来源。 */
export function setMicSource(source: "auto" | "pc" | "wifi" | "bluetooth"): Promise<string> {
  return invoke<string>("set_mic_source", { source });
}

/** 查询当前麦克风来源设置。 */
export function getMicSource(): Promise<string> {
  return invoke<string>("get_mic_source");
}

/** 设置 Wi-Fi 配置并持久化到配置文件。 */
export function setWifiConfig(ssid: string, password: string): Promise<void> {
  return invoke<void>("set_wifi_config", { ssid, password });
}

/** 读取 Wi-Fi 配置（从配置文件）。 */
export function getWifiConfig(): Promise<{ ssid: string; password: string }> {
  return invoke<{ ssid: string; password: string }>("get_wifi_config");
}

/** 设置蓝牙启用状态并持久化。 */
export function setBleEnabled(enabled: boolean): Promise<void> {
  return invoke<void>("set_ble_enabled", { enabled });
}

/** 读取蓝牙启用状态。 */
export function getBleEnabled(): Promise<boolean> {
  return invoke<boolean>("get_ble_enabled");
}

// ── 语音编辑 ──

/** 设置 LLM 配置。 */
export function setLlmConfig(config: LlmConfig): Promise<void> {
  return invoke<void>("set_llm_config", { config });
}

/** 读取当前 LLM 配置。 */
export function getLlmConfig(): Promise<LlmConfig> {
  return invoke<LlmConfig>("get_llm_config");
}

/** 测试 LLM 连接。 */
export function testLlmConnection(): Promise<string> {
  return invoke<string>("test_llm_connection");
}

/** 设置编辑 PTT 键（空字符串清除）。 */
export function setEditPttKey(spec: string): Promise<string> {
  return invoke<string>("set_edit_ptt_key", { spec });
}

/** 获取当前编辑阶段。 */
export function getEditPhase(): Promise<EditPhase> {
  return invoke<EditPhase>("get_edit_phase");
}

// ── 提示词库浮窗（KEY4）──
//
// 这里只封装「主窗口能做的事」：开合与查态。列表快照（workbench_list）和
// 上屏（workbench_commit）由浮窗页面自己用 window.__TAURI__ 调用——它们属于
// 提示词库浮窗的私有流程，主窗口代劳没有语义（confirm 会收起浮窗再往原窗口注入正文）。

/** 开或收提示词库浮窗，返回操作后是否可见（与硬件 KEY4 等价）。 */
export function workbenchToggle(): Promise<boolean> {
  return invoke<boolean>("workbench_toggle");
}

/** 收起提示词库浮窗，并把焦点还给呼出前的窗口。 */
export function workbenchHide(): Promise<void> {
  return invoke<void>("workbench_hide");
}

/** 查询提示词库浮窗当前是否可见，用于同步入口按钮的状态。 */
export function workbenchIsOpen(): Promise<boolean> {
  return invoke<boolean>("workbench_is_open");
}
// ── 使用记录（工作台"使用记录中心"）──

/** 查询使用记录（按日期范围）。 */
export function usageLogQuery(from: string, to: string): Promise<UsageEntry[]> {
  return invoke<UsageEntry[]>("usage_log_query", { query: { from, to } });
}

/** 日历统计：每天的记录数与类型分布。 */
export function usageLogDailyCounts(from: string, to: string): Promise<DailyCount[]> {
  return invoke<DailyCount[]>("usage_log_daily_counts", { query: { from, to } });
}

// ── ASR 自定义词库 ──

/** 列出所有热词。 */
export function asrVocabList(): Promise<VocabWord[]> {
  return invoke<VocabWord[]>("asr_vocab_list");
}

/** 添加热词（去重：已有相同 text 则更新权重）。 */
export function asrVocabAdd(text: string, weight?: number): Promise<VocabWord[]> {
  return invoke<VocabWord[]>("asr_vocab_add", { text, weight });
}

/** 删除热词（按文本匹配）。 */
export function asrVocabRemove(text: string): Promise<VocabWord[]> {
  return invoke<VocabWord[]>("asr_vocab_remove", { text });
}

// ── Agent 任务台 ──
//
// 全部返回 `ApiEnvelope<T>`：Rust 侧用统一信封，`ok/code/message` 三字段就能
// 精确分类失败原因（NotConfigured / InvalidInput / ProviderError …），
// 不必靠解析错误字符串。

/** 读取任务台配置。 */
export function deskGetConfig(): Promise<ApiEnvelope<DeskConfig>> {
  return invoke<ApiEnvelope<DeskConfig>>("desk_get_config");
}

/** 写入任务台配置（含 TTS / 执行器 / 槽位）。 */
export function deskSetConfig(config: DeskConfig): Promise<ApiEnvelope<DeskConfig>> {
  return invoke<ApiEnvelope<DeskConfig>>("desk_set_config", { config });
}

/** 单独保存 TTS 配置。 */
export function deskSetTtsConfig(tts: TtsConfig): Promise<ApiEnvelope<TtsConfig>> {
  return invoke<ApiEnvelope<TtsConfig>>("desk_set_tts_config", { tts });
}

/** 任务台总览快照。 */
export function deskSnapshot(): Promise<ApiEnvelope<DeskSnapshot>> {
  return invoke<ApiEnvelope<DeskSnapshot>>("desk_snapshot");
}

/** 探测一个执行器（当前仅静态配置检查）。 */
export function deskProbeDriver(id: string): Promise<ApiEnvelope<DriverProbe>> {
  return invoke<ApiEnvelope<DriverProbe>>("desk_probe_driver", { id });
}

/** TTS 试合成（真调一次服务，落盘 wav 供试听）。 */
export function deskTtsTest(text: string): Promise<ApiEnvelope<TtsTestReport>> {
  return invoke<ApiEnvelope<TtsTestReport>>("desk_tts_test", { text });
}

// ---------------------------------------------------------------------------
// 任务台运行时（Phase 2：队列 / 未读 / 播放租约）
//
// 参数名一律用单词（`slot` / `task` / `token`）：Tauri 命令参数在 JS 侧是 camelCase，
// 单字命名就不存在"Rust 叫 task_id、JS 要写 taskId"这种隐形转换。
// ---------------------------------------------------------------------------

/** 运行时快照（队列 + 未读）。事件 `agent-desk-state` 的载荷与此同构。 */
export function deskRuntimeState(): Promise<ApiEnvelope<RuntimeSnapshot>> {
  return invoke<ApiEnvelope<RuntimeSnapshot>>("desk_runtime_state");
}

/** 手动派发一条任务到指定槽位（`slot` 为 0 基）。 */
export function deskDispatchTask(slot: number, prompt: string): Promise<ApiEnvelope<DispatchOutcome>> {
  return invoke<ApiEnvelope<DispatchOutcome>>("desk_dispatch_task", { slot, prompt });
}

/** 撤掉某槽排队中的一条任务（不影响正在执行的那条）。 */
export function deskCancelPending(slot: number, task: number): Promise<ApiEnvelope<number>> {
  return invoke<ApiEnvelope<number>>("desk_cancel_pending", { slot, task });
}

/** 取消某槽**正在执行**的任务。返回 false = 该槽当前没有在跑的任务。 */
export function deskCancelRunning(slot: number): Promise<ApiEnvelope<boolean>> {
  return invoke<ApiEnvelope<boolean>>("desk_cancel_running", { slot });
}

/** 清空所有槽的待执行任务，返回清掉的条数。 */
export function deskClearPending(): Promise<ApiEnvelope<number>> {
  return invoke<ApiEnvelope<number>>("desk_clear_pending");
}

/** 索取播放租约（模拟按下播放键）。**不消费未读**——只有回执才消费。 */
export function deskGrantPlayback(slot: number): Promise<ApiEnvelope<Lease>> {
  return invoke<ApiEnvelope<Lease>>("desk_grant_playback", { slot });
}

/** 回执：这段内容**完整播完了**，消费一条未读。迟到回执会被拒绝且不消费。 */
export function deskAckPlayback(slot: number, token: number): Promise<ApiEnvelope<ConsumeOutcome>> {
  return invoke<ApiEnvelope<ConsumeOutcome>>("desk_ack_playback", { slot, token });
}

/** 归还租约，不消费未读（用户抢占 / 播放流程放弃）。 */
export function deskReleasePlayback(slot: number, token: number): Promise<ApiEnvelope<null>> {
  return invoke<ApiEnvelope<null>>("desk_release_playback", { slot, token });
}

/**
 * 丢弃未读：明确「放弃收听」，把未读清零。`slot` 省略 = 清全部。
 *
 * ⚠️ 与 `deskAckPlayback` 不是一回事：那个是"听完了"（已读），这个是"放弃了"（删掉）。
 * 返回值是被丢掉的条数。
 */
export function deskDiscardUnread(slot?: number): Promise<ApiEnvelope<number>> {
  // `Option<u8>` 侧省略参数要显式传 null，不能传 undefined（会被当成缺参报错）。
  return invoke<ApiEnvelope<number>>("desk_discard_unread", { slot: slot ?? null });
}

// ---------------------------------------------------------------------------
// 播报（Phase 3 第二段「后声」）
//
// 播放发生在**后端**（Rust 侧 cpal 直接写系统默认输出设备），不是前端。
// 这不是实现偏好：**"听完才消费未读"的判定必须落在进程内** ——
// 放前端的话，刷一次页面、webview 崩一次，这条承诺就断了。
// 所以这里只需要"发命令"和"听事件"。
// ---------------------------------------------------------------------------

/** 语音语料的缓存状态（离线可用性的唯一判据）。 */
export function deskNoticeStatus(): Promise<ApiEnvelope<NoticeStatusReport>> {
  return invoke<ApiEnvelope<NoticeStatusReport>>("desk_notice_status");
}

/**
 * 预合成固定语料。
 *
 * `force = false` 只补缺失的（日常）；`force = true` 全部重合成
 * （改了措辞 / 换了音色之后）。单条失败不中断，结果里逐条汇报。
 */
export function deskPrepareNotices(force = false): Promise<ApiEnvelope<NoticePrepareReport>> {
  return invoke<ApiEnvelope<NoticePrepareReport>>("desk_prepare_notices", { force });
}

/** 播一条状态语料（**不涉及未读**）。 */
export function deskPlayNotice(kind: NoticeKind): Promise<ApiEnvelope<NoticePlayReport>> {
  return invoke<ApiEnvelope<NoticePlayReport>>("desk_play_notice", { kind });
}

/**
 * 每个槽位当前正在续接的会话摘要。
 *
 * 用来回答"它现在是在接着聊，还是在开新话题"—— 没有这个信息，
 * 用户无法判断自己那句"刚才那个呢"到底能不能被听懂。
 */
export function deskSessionInfo(): Promise<ApiEnvelope<(SessionView | null)[]>> {
  return invoke<ApiEnvelope<(SessionView | null)[]>>("desk_session_info");
}

/**
 * 丢掉某槽的会话 ⇒ 下一次任务新起一轮对话。
 *
 * 返回是否**确实**清掉了一个已存在的会话（本来就没有则为 false）。
 */
export function deskResetSession(slot: number): Promise<ApiEnvelope<boolean>> {
  return invoke<ApiEnvelope<boolean>>("desk_reset_session", { slot });
}

/**
 * 播报某槽的未读结果：取租约 → 合成 → 播放 → **只有完整播完才消费未读**。
 *
 * 这是 Phase 3.5 的核心语义在 PC 侧的落地：播到一半被中断，未读会**保留**。
 */
export function deskRenderResult(slot: number): Promise<ApiEnvelope<RenderResultReport>> {
  return invoke<ApiEnvelope<RenderResultReport>>("desk_render_result", { slot });
}

/** 一条任务历史（`TaskRecord` 的投影）。 */
export interface TaskRecordView {
  task_id: number;
  /** 0 基槽位；显示时 +1。 */
  slot: number;
  driver_id: string;
  /** 用户当时说的原话。 */
  prompt: string;
  /** `已完成` / `失败` / `超时` / `已取消`。 */
  status: string;
  reason: string | null;
  /** `high` / `medium`（medium = 完成判定未经世界门禁校验）。 */
  confidence: string;
  started_at_ms: number;
  finished_at_ms: number;
  duration_ms: number;
  session_id: string | null;
  /** 全量日志文件名（相对任务目录）；null = 日志没写成。 */
  log_file: string | null;
  artifacts: string[];
  /** 输出尾部，列表页直接显示用。 */
  output_tail: string;
}

/** 最近的任务历史（**最新在前**）。 */
export function deskTaskHistory(limit = 50): Promise<ApiEnvelope<TaskRecordView[]>> {
  return invoke<ApiEnvelope<TaskRecordView[]>>("desk_task_history", { limit });
}

/**
 * 某条任务的**全量**日志。
 *
 * 单开一条命令（而不是塞进列表）是因为它可能几 MB —— 见后端同名命令的文档。
 */
export function deskTaskLog(task: number): Promise<ApiEnvelope<string | null>> {
  return invoke<ApiEnvelope<string | null>>("desk_task_log", { task });
}

// ---------------------------------------------------------------------------
// 文本注入（A″：语音打进终端 / TUI 场景）
// ---------------------------------------------------------------------------

/** 读当前粘贴方式。 */
export function getPasteStyle(): Promise<PasteStyle> {
  return invoke<PasteStyle>("get_paste_style");
}

/**
 * 设置粘贴方式。**无法识别的值后端会拒绝**（返回 reject），不静默回落默认值。
 * 返回规范化后的字符串，用它回填界面可避免"显示的与存的不一致"。
 */
export function setPasteStyle(style: PasteStyle): Promise<string> {
  return invoke<string>("set_paste_style", { style });
}

/** 读"粘贴后是否自动回车"。 */
export function getSubmitAfterPaste(): Promise<boolean> {
  return invoke<boolean>("get_submit_after_paste");
}

/** 设置"粘贴后是否自动回车"。 */
export function setSubmitAfterPaste(enabled: boolean): Promise<void> {
  return invoke<void>("set_submit_after_paste", { enabled });
}

// ---------------------------------------------------------------------------
// 「启动终端并预置命令」
// ---------------------------------------------------------------------------

/** 读终端启动配置。 */
export function getCliTerminalConfig(): Promise<CliTerminalConfig> {
  return invoke<CliTerminalConfig>("cli_terminal_get_config");
}

/** 写终端启动配置。返回**保存后的规范化配置**（命令/目录已 trim），用它回填界面。 */
export function setCliTerminalConfig(
  config: CliTerminalConfig,
): Promise<CliTerminalConfig> {
  return invoke<CliTerminalConfig>("cli_terminal_set_config", { config });
}

/**
 * 手动触发一次"启动终端并预置命令"，等价于按下绑定键。
 *
 * 有这个入口，用户才不必"去板子上按一下"才能验证命令配得对不对 ——
 * 那等于把最容易出错的一步放在最难试的地方。
 */
export function launchCliTerminal(): Promise<string> {
  return invoke<string>("cli_terminal_launch");
}
