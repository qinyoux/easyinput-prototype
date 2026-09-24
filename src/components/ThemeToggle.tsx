// 顶栏主题快捷开关：一键在深色 / 浅色之间切换（「跟随系统」可在设置页选择）。
import { THEME_MODE_LABELS, useTheme } from "../theme";

function IconMoon() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z" />
    </svg>
  );
}

function IconSun() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
    </svg>
  );
}

export default function ThemeToggle() {
  const { mode, resolved, setMode } = useTheme();
  const next = resolved === "dark" ? "light" : "dark";

  return (
    <button
      className="btn-ghost theme-toggle"
      onClick={() => setMode(next)}
      title={`切换为${THEME_MODE_LABELS[next]}主题（当前：${THEME_MODE_LABELS[mode]}）`}
      aria-label={`切换为${THEME_MODE_LABELS[next]}主题`}
    >
      {resolved === "dark" ? <IconMoon /> : <IconSun />}
    </button>
  );
}
