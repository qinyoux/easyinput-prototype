// 在所有窗口里挂钩 Tauri 的底层 IPC 入口，把每一次
// `show_overlay` / `hide_overlay` 调用连**调用栈**一起记下来。
// 用来回答一个必须靠证据回答的问题：到底是谁把浮层收掉的。
//
// Tauri 2 的前端 `invoke` 最终都走 `window.__TAURI_INTERNALS__.invoke`，
// 在这里包一层，无论是界面代码调的还是页面里别处调的，都躲不掉。

window.__ipc = window.__ipc || [];

if (!window.__ipcHooked) {
  window.__ipcHooked = 1;
  const orig = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
  window.__TAURI_INTERNALS__.invoke = function (cmd, args, opts) {
    if (cmd === "show_overlay" || cmd === "hide_overlay") {
      let st = "";
      try {
        st = (new Error().stack || "")
          .split("\n")
          .slice(1, 5)
          .map((s) => s.trim())
          .join("  <<<  ");
      } catch (e) {}
      window.__ipc.push([Date.now(), location.pathname, cmd, st.slice(0, 400)]);
    }
    return orig(cmd, args, opts);
  };
}

return JSON.stringify({ hooked: true, path: location.pathname });
