// 验证「后台字幕浮层」开关在显隐收归后端之后仍然正确：
//   ① 关掉开关 → 已经挂在屏幕上的浮层要立刻消失
//   ② 录音中把开关打开 → 浮层要立刻补上（后端只在"录音开始"那一刻读过开关）
//   ③ 这一轮结束时，照旧由后端收掉
// 结束时把开关恢复成「开」，不留下被改过的用户配置。
const { invoke } = window.__TAURI__.core;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const w = window.__TAURI__.window.getCurrentWindow();
const out = [];
const snap = async (label) => out.push([label, await w.isVisible()]);

await invoke("show_overlay");
await sleep(400);
await snap("① 手动显示后");

await invoke("set_overlay_enabled", { enabled: false });
await sleep(500);
await snap("② 关掉开关后（应为 false）");

await invoke("start_recording");
await sleep(1200);
await snap("③ 录音中·开关关（应为 false）");

await invoke("set_overlay_enabled", { enabled: true });
await sleep(600);
await snap("④ 录音中途打开开关（应为 true）");

await invoke("start_recording"); // 结束录音 → 空结果 → 后端收尾
await sleep(4500);
await snap("⑤ 这一轮结束后（应为 false）");

return JSON.stringify(out);
