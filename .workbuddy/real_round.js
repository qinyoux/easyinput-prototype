// 真机驱动：不广播任何事件，直接让后端跑一轮完整录音。
// 目的：确认「浮层的显示与收尾现在都由后端负责」——
// 修复后前端已经不收浮层了，所以这一轮里浮层若被收掉，**只可能**是后端收的。
const { invoke } = window.__TAURI__.core;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

await invoke("start_recording"); // 按下：开始录音（后端 show_overlay）
await sleep(5000);
await invoke("start_recording"); // 再按：结束并交付 → 空结果 → 后端 hide_overlay
await sleep(4500);
return "real-round-done";
