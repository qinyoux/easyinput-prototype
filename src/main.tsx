import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { initTheme } from "./theme";
import { initPluginHost } from "./pluginHost";
import "./styles.css";

// 落主题 + 挂系统主题监听（index.html 中的内联脚本已抢先执行一次，这里保证状态一致）
initTheme();

// 初始化插件宿主：监听 Rust emit 的 plugin:activate 事件，在主窗执行插件 JS。
initPluginHost().catch((e) => console.error("[plugin-host] 初始化失败:", e));

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
