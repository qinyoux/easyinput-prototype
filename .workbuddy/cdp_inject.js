// 往浮层页面里装一个「事件 + 状态」记录器。
// 只在第一次注入时注册监听器，之后重复调用只返回当前记录，不重复叠加。
window.__rec = window.__rec || [];
window.__tick = window.__tick || [];

if (!window.__installed) {
  window.__installed = 1;
  const names = ["asr-final", "asr-state", "asr-partial", "voice-injected", "voice-alert", "desk-slot-key"];
  for (const n of names) {
    await window.__TAURI__.event.listen(n, (e) => {
      window.__rec.push([
        Date.now(),
        n,
        typeof e.payload === "string" ? e.payload : JSON.stringify(e.payload),
      ]);
    });
  }
  setInterval(async () => {
    try {
      const w = window.__TAURI__.window.getCurrentWindow();
      const vis = await w.isVisible();
      const t = document.getElementById("text");
      const s = document.getElementById("status-text");
      window.__tick.push([
        Date.now(),
        vis,
        t.className,
        (t.textContent || "").slice(0, 80),
        s.textContent,
      ]);
      if (window.__tick.length > 600) window.__tick.shift();
    } catch (err) {
      /* 窗口 API 可能短暂不可用，忽略 */
    }
  }, 200);
}

return JSON.stringify({
  installed: true,
  rec: window.__rec.length,
  tick: window.__tick.length,
  nowText: document.getElementById("text").textContent,
  nowStatus: document.getElementById("status-text").textContent,
});
