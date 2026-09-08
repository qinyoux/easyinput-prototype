// 与后端 Tauri 命令的封装层。前端仅通过这里的函数与 Rust 交互。
import { invoke } from "@tauri-apps/api/core";
import type { AppEntry, ConfigAck, LogEntry, Profile, SystemApp, Vocabulary, LlmConfig, EditPhase } from "./types";

/** 检测开发板 HID 是否在线。 */
export function deviceStatus(): Promise<string> {
  return invoke<string>("device_status");
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

/** 查询开发板 Wi-Fi 音频是否在线。 */
export function keyboardAudioOnline(): Promise<boolean> {
  return invoke<boolean>("keyboard_audio_online");
}

/** 下发 Wi-Fi 连接命令到开发板。 */
export function wifiConnect(ssid: string, password: string): Promise<void> {
  return invoke<void>("wifi_connect", { ssid, password });
}

/** 下发 Wi-Fi 断开命令到开发板。 */
export function wifiDisconnect(): Promise<void> {
  return invoke<void>("wifi_disconnect");
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
