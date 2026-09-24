// 侧栏：主导航 + 插件页签（动态追加）。
export type Tab = string;

export interface PluginPageInfo {
  pluginId: string;
  pageId: string;
  title: string;
  icon: string;
  order: number;
}

const NAV: { id: string; label: string }[] = [
  { id: "workbench", label: "工作台" },
  { id: "remap", label: "按键重映射" },
  { id: "apps", label: "应用管理" },
  { id: "voice", label: "语音" },
  { id: "desk", label: "任务台" },
  // 紧跟「语音 / 任务台」：它管的就是这两页要用的外部服务与执行器（ASR / 语音编辑 LLM /
  // TTS / 模型与 CLI 执行器），放在被服务的功能旁边才好找。
  // 2026-09-22：由「API 配置」改名 —— 它现在同时是**执行器的唯一编辑入口**，
  // 而"执行器"不只是 API（还有 CLI Agent），旧名字已经如实不了它管什么。
  { id: "api", label: "模型配置" },
  { id: "profiles", label: "方案" },
  { id: "settings", label: "设置" },
];

interface Props {
  tab: Tab;
  onTabChange: (tab: Tab) => void;
  pluginPages: PluginPageInfo[];
}

export default function Sidebar({ tab, onTabChange, pluginPages }: Props) {
  return (
    <aside className="sidebar">
      <nav className="side-nav" aria-label="主导航">
        {NAV.map((n) => (
          <button
            key={n.id}
            type="button"
            className={`side-nav-item ${tab === n.id ? "active" : ""}`}
            aria-current={tab === n.id ? "page" : undefined}
            onClick={() => onTabChange(n.id)}
          >
            {n.label}
          </button>
        ))}
        {pluginPages.length > 0 && <div className="side-nav-sep" />}
        {pluginPages.map((pp) => {
          const tabId = `plugin:${pp.pluginId}:${pp.pageId}`;
          return (
            <button
              key={tabId}
              type="button"
              className={`side-nav-item ${tab === tabId ? "active" : ""}`}
              aria-current={tab === tabId ? "page" : undefined}
              onClick={() => onTabChange(tabId)}
            >
              {pp.title}
            </button>
          );
        })}
      </nav>
    </aside>
  );
}
