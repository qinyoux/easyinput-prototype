/**
 * 主题状态模块
 *
 * - 三种模式：dark（深色）/ light（浅色）/ system（跟随系统）
 * - 通过 localStorage 持久化，key 为 `easyop.theme`
 * - system 模式下监听 `prefers-color-scheme` 变化并实时切换
 * - 最终只做一件事：把解析结果写到 <html data-theme="dark|light">
 *   （styles.css 里的 [data-theme="..."] 令牌块据此生效）
 *
 * 注意：index.html 中有一段等价的防闪烁内联脚本，用于首屏渲染前抢先落主题。
 * 二者共享同一个 storage key 与同一套解析逻辑，修改时必须同步。
 */

import { useCallback, useEffect, useState } from "react";

export type ThemeMode = "dark" | "light" | "system";
export type ResolvedTheme = "dark" | "light";

/** 模式的中文标签，设置页与顶栏复用。 */
export const THEME_MODE_LABELS: Record<ThemeMode, string> = {
  dark: "深色",
  light: "浅色",
  system: "跟随系统",
};

/** localStorage 键名，必须与 index.html 内联脚本保持一致。 */
export const THEME_STORAGE_KEY = "easyop.theme";

/** 未设置过时的默认模式：跟随系统。 */
export const DEFAULT_THEME_MODE: ThemeMode = "system";

const SYSTEM_QUERY = "(prefers-color-scheme: dark)";

type ThemeListener = (mode: ThemeMode, resolved: ResolvedTheme) => void;

const listeners = new Set<ThemeListener>();
let systemWatcherAttached = false;

export function isThemeMode(value: unknown): value is ThemeMode {
  return value === "dark" || value === "light" || value === "system";
}

/** 读取已保存的模式；缺失或非法时回退到默认模式。 */
export function getStoredThemeMode(): ThemeMode {
  try {
    const raw = localStorage.getItem(THEME_STORAGE_KEY);
    if (isThemeMode(raw)) return raw;
  } catch {
    // localStorage 不可用（隐私模式等）时静默回退
  }
  return DEFAULT_THEME_MODE;
}

/** 当前操作系统的深浅色偏好。 */
export function getSystemTheme(): ResolvedTheme {
  try {
    if (typeof window !== "undefined" && typeof window.matchMedia === "function") {
      return window.matchMedia(SYSTEM_QUERY).matches ? "dark" : "light";
    }
  } catch {
    // matchMedia 异常时按深色处理（与旧版纯深色界面一致）
  }
  return "dark";
}

/** 把模式解析为最终生效的深/浅色。 */
export function resolveTheme(mode: ThemeMode): ResolvedTheme {
  return mode === "system" ? getSystemTheme() : mode;
}

/** 将解析结果写入 <html data-theme="...">。 */
export function applyTheme(resolved: ResolvedTheme): void {
  if (typeof document === "undefined") return;
  document.documentElement.dataset.theme = resolved;
}

function emit(mode: ThemeMode, resolved: ResolvedTheme): void {
  listeners.forEach((fn) => fn(mode, resolved));
}

/** 挂载系统主题监听（幂等，仅首次生效）。 */
function ensureSystemWatcher(): void {
  if (systemWatcherAttached) return;
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return;
  systemWatcherAttached = true;

  const media = window.matchMedia(SYSTEM_QUERY);
  const onChange = () => {
    // 仅在「跟随系统」模式下响应系统变化
    if (getStoredThemeMode() !== "system") return;
    const resolved = getSystemTheme();
    applyTheme(resolved);
    emit("system", resolved);
  };

  if (typeof media.addEventListener === "function") {
    media.addEventListener("change", onChange);
  } else if (typeof media.addListener === "function") {
    // 兼容旧版 WebView
    media.addListener(onChange);
  }
}

/** 切换主题模式：立即生效、持久化、并通知订阅者。返回解析后的主题。 */
export function setThemeMode(mode: ThemeMode): ResolvedTheme {
  const resolved = resolveTheme(mode);
  applyTheme(resolved);
  try {
    localStorage.setItem(THEME_STORAGE_KEY, mode);
  } catch {
    // 持久化失败不影响本次切换
  }
  ensureSystemWatcher();
  emit(mode, resolved);
  return resolved;
}

/** 订阅主题变化，返回取消订阅函数。 */
export function onThemeChange(listener: ThemeListener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** 应用启动时调用一次：落主题 + 挂系统监听。返回当前模式。 */
export function initTheme(): ThemeMode {
  const mode = getStoredThemeMode();
  applyTheme(resolveTheme(mode));
  ensureSystemWatcher();
  return mode;
}

/**
 * React 绑定：读取当前模式与生效主题，并随全局主题变化自动同步。
 * 设置页与顶栏共用同一份状态，避免各组件自行维护。
 */
export function useTheme(): {
  mode: ThemeMode;
  resolved: ResolvedTheme;
  setMode: (mode: ThemeMode) => void;
} {
  const [mode, setLocalMode] = useState<ThemeMode>(() => getStoredThemeMode());
  const [resolved, setResolved] = useState<ResolvedTheme>(() => resolveTheme(getStoredThemeMode()));

  useEffect(
    () =>
      onThemeChange((nextMode, nextResolved) => {
        setLocalMode(nextMode);
        setResolved(nextResolved);
      }),
    [],
  );

  const setMode = useCallback((next: ThemeMode) => {
    setThemeMode(next);
  }, []);

  return { mode, resolved, setMode };
}
