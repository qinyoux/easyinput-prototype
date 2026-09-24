// 插件宿主前端层。
// 监听 Rust emit 的 `plugin:activate` 事件，在主窗执行插件 JS。
// 逻辑等价于原 plugin_host.rs::build_host_js，但改为 TS 原生实现，
// 绕过 Tauri 2 eval / initialization_script 的时序问题。
//
// 流程：收到 { pluginId, commandId, code } → 定义 eo.* API →
// blob import 插件代码 → activate(context) → 执行初始命令。
//
// 性能 profiling：activate 耗时 / 命令执行耗时 / eo.* 调用计数，
// 超阈值（SLOW_THRESHOLD_MS）记 warn。通过 getPluginStats() 可查运行时统计。

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

type PluginActivatePayload = {
  pluginId: string;
  commandId: string;
  code: string;
  /** 插件目录绝对路径；老版本 host 不传。插件自带脚本/资源时靠它定位。 */
  dir?: string;
};

// ── 性能 profiling ──

/** 慢操作阈值（毫秒）。超过此值的 activate / 命令执行记 warn。 */
const SLOW_THRESHOLD_MS = 5000;

/** 单个插件的运行时统计。 */
type PluginStats = {
  /** activate() 耗时（毫秒），undefined 表示尚未激活。 */
  activateMs?: number;
  /** 每命令的调用次数与累计耗时（毫秒）。 */
  commandCalls: Record<string, { count: number; totalMs: number }>;
  /** 每个 eo.* API 的调用次数。 */
  apiCalls: Record<string, number>;
};

/** 全局插件统计表：pluginId → stats。 */
const pluginStats = new Map<string, PluginStats>();

function ensureStats(pluginId: string): PluginStats {
  let s = pluginStats.get(pluginId);
  if (!s) {
    s = { commandCalls: {}, apiCalls: {} };
    pluginStats.set(pluginId, s);
  }
  return s;
}

/**
 * 查询插件运行时统计（调试 / 监控用）。
 * 返回各插件的 activate 耗时、命令调用次数 / 累计耗时、eo.* API 调用次数。
 */
export function getPluginStats(): Map<string, PluginStats> {
  return new Map(pluginStats);
}

/**
 * 包装一个 async 函数，自动计时 + 计数。
 * 超过 SLOW_THRESHOLD_MS 时记 warn 日志。
 */
function timed<T>(
  pluginId: string,
  label: string,
  fn: () => Promise<T>,
  onSlow?: (ms: number) => void,
): Promise<T> {
  const stats = ensureStats(pluginId);
  const key = label;
  stats.apiCalls[key] = (stats.apiCalls[key] ?? 0) + 1;
  const start = performance.now();
  return fn().finally(() => {
    const ms = performance.now() - start;
    if (ms > SLOW_THRESHOLD_MS) {
      invoke<void>("eo_log_warn", {
        msg: `[plugin-profile] ${pluginId}.${label} 耗时 ${ms.toFixed(0)}ms（超阈值 ${SLOW_THRESHOLD_MS}ms）`,
      }).catch(() => {});
      onSlow?.(ms);
    }
  });
}

// 已激活插件的命令注册表，避免重复 activate。
const activatedPlugins = new Map<string, Record<string, () => Promise<unknown>>>();

// 页签渲染函数注册表：`${pluginId}:${pageId}` → renderFn
type PageLifecycle = { refresh: () => void; dispose: () => void };
const pageRenderers = new Map<string, (container: HTMLElement) => PageLifecycle>();

// 激活等待队列：pluginId → resolve fn
const pendingActivations = new Map<string, () => void>();

/**
 * 初始化插件宿主：监听 `plugin:activate` 事件。
 * 在 main.tsx 中调用一次。
 */
export async function initPluginHost(): Promise<UnlistenFn> {
  return listen<PluginActivatePayload>("plugin:activate", (ev) => {
    const { pluginId, commandId, code, dir } = ev.payload;
    runPlugin(pluginId, commandId, code, dir).catch((e) => {
      console.error(`[plugin-host] 插件 ${pluginId} 执行失败:`, e);
    });
  });
}

/**
 * 执行插件：定义 eo.* → blob import → activate → exec command。
 * 幂等：已激活的插件直接执行命令，不重新 activate。
 */
async function runPlugin(
  pluginId: string,
  commandId: string,
  code: string,
  pluginDir?: string,
): Promise<void> {
  // 已激活则直接执行命令
  const existing = activatedPlugins.get(pluginId);
  if (existing) {
    await execCommand(pluginId, existing, commandId);
    return;
  }

  // ── eo.* API（每个方法自动计时 + 计数） ──
  const eo = {
    get_selection: () =>
      timed(pluginId, "get_selection", () => invoke<string>("eo_get_selection")),
    clipboard_read: () =>
      timed(pluginId, "clipboard_read", () => invoke<string>("eo_clipboard_read")),
    replace_selection: (text: string) =>
      timed(pluginId, "replace_selection", () => invoke<void>("eo_replace_selection", { text })),
    inject: (text: string) =>
      timed(pluginId, "inject", () => invoke<void>("eo_inject", { text })),
    storage: {
      get: (key: string) =>
        timed(pluginId, "storage.get", () => invoke<unknown>("eo_storage_get", { pluginId, key })),
      set: (key: string, value: unknown) =>
        timed(pluginId, "storage.set", () =>
          invoke<void>("eo_storage_set", { pluginId, key, value }),
        ),
    },
    log: {
      info: (msg: string) => invoke<void>("eo_log_info", { msg: String(msg) }),
      warn: (msg: string) => invoke<void>("eo_log_warn", { msg: String(msg) }),
      error: (msg: string) => invoke<void>("eo_log_error", { msg: String(msg) }),
    },
    chat: (messages: unknown[], opts?: unknown) =>
      timed(pluginId, "chat", () => invoke<string>("eo_chat", { messages, opts: opts ?? {} })),
    run: (program: string, args?: string[], opts?: unknown) =>
      timed(pluginId, "run", () =>
        invoke<unknown>("eo_run", { program, args: args ?? [], opts: opts ?? {} }),
      ),
  };

  // ── 命令注册表 + context ──
  const commands: Record<string, () => Promise<unknown>> = {};
  const context = {
    api: eo,
    pluginId,
    pluginDir,
    registerCommand: (id: string, fn: () => Promise<unknown>) => {
      commands[id] = fn;
    },
    registerPage: (pageId: string, renderFn: (container: HTMLElement) => PageLifecycle) => {
      pageRenderers.set(`${pluginId}:${pageId}`, renderFn);
    },
  };

  // ── blob import 插件代码 → activate（计时） ──
  try {
    await invoke<void>("eo_log_info", { msg: `plugin-host: 开始加载插件 ${pluginId}` });
    const blob = new Blob([code], { type: "text/javascript" });
    const url = URL.createObjectURL(blob);
    const mod = await import(url);
    URL.revokeObjectURL(url);

    if (typeof mod.activate === "function") {
      const activateStart = performance.now();
      await mod.activate(context);
      const activateMs = performance.now() - activateStart;
      ensureStats(pluginId).activateMs = activateMs;

      if (activateMs > SLOW_THRESHOLD_MS) {
        await invoke<void>("eo_log_warn", {
          msg: `[plugin-profile] ${pluginId} activate() 耗时 ${activateMs.toFixed(0)}ms（超阈值 ${SLOW_THRESHOLD_MS}ms）`,
        });
      }
      await invoke<void>("eo_log_info", {
        msg: `plugin-host: activate() 完成（${activateMs.toFixed(0)}ms），已注册命令: ${Object.keys(commands).join(", ")}`,
      });
    } else {
      await invoke<void>("eo_log_error", { msg: "插件入口未导出 activate 函数" });
      return;
    }
  } catch (e) {
    console.error(`[plugin-host] 插件 ${pluginId} 激活失败:`, e);
    try {
      await invoke<void>("eo_log_error", { msg: `插件激活失败: ${String(e)}` });
    } catch {
      // ignore
    }
    return;
  }

  activatedPlugins.set(pluginId, commands);
  pendingActivations.get(pluginId)?.();
  pendingActivations.delete(pluginId);
  if (commandId !== "__page__") {
    await execCommand(pluginId, commands, commandId);
  }
}

/** 执行已注册的命令（计时 + 计数）。 */
async function execCommand(
  pluginId: string,
  commands: Record<string, () => Promise<unknown>>,
  commandId: string,
): Promise<void> {
  const fn = commands[commandId];
  if (!fn) {
    await invoke<void>("eo_log_error", { msg: `命令未注册：${commandId}` });
    return;
  }

  const stats = ensureStats(pluginId);
  const entry = (stats.commandCalls[commandId] ??= { count: 0, totalMs: 0 });
  entry.count++;

  const start = performance.now();
  try {
    await fn();
  } catch (e) {
    console.error(`[plugin-host] 插件 ${pluginId} 命令 ${commandId} 执行失败:`, e);
    await invoke<void>("eo_log_error", { msg: `命令执行失败: ${String(e)}` });
  } finally {
    const ms = performance.now() - start;
    entry.totalMs += ms;
    if (ms > SLOW_THRESHOLD_MS) {
      await invoke<void>("eo_log_warn", {
        msg: `[plugin-profile] ${pluginId} 命令 ${commandId} 耗时 ${ms.toFixed(0)}ms（超阈值 ${SLOW_THRESHOLD_MS}ms）`,
      }).catch(() => {});
    }
  }
}
/**
 * 确保插件已激活（供 PluginPageContainer 调用）。
 * 已激活则立即返回；否则触发激活并等待完成。
 */
export async function ensurePluginActivated(pluginId: string): Promise<void> {
  if (activatedPlugins.has(pluginId)) return;
  const promise = new Promise<void>((resolve) => {
    pendingActivations.set(pluginId, resolve);
  });
  await invoke("plugin_trigger_activate", { pluginId });
  return promise;
}

/** 获取页签渲染函数（插件 activate 后 registerPage 注册）。 */
export function getPluginPageRenderer(
  pluginId: string,
  pageId: string,
): ((container: HTMLElement) => PageLifecycle) | undefined {
  return pageRenderers.get(`${pluginId}:${pageId}`);
}
/**
 * 从外部调用已激活插件的已注册命令（供事件监听器等使用）。
 *
 * 与 `execCommand` 不同：不走初始激活流程，直接查 `activatedPlugins` 表。
 * 命令函数签名是 `(args?) => Promise<unknown>`，args 透传给插件。
 */
export async function invokePluginCommand(
  pluginId: string,
  commandId: string,
  args?: unknown,
): Promise<unknown | undefined> {
  const commands = activatedPlugins.get(pluginId);
  if (!commands) return undefined;
  const fn = commands[commandId] as ((args?: unknown) => Promise<unknown>) | undefined;
  if (!fn) return undefined;
  return fn(args);
}
