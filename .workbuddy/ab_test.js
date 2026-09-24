// A/B 判别：把「正文」和「状态行」拆开单独发，看究竟哪一条把浮层收掉。
//
// A 段：只发 asr-final（正文），然后安静等 2.5 秒 —— 期间没有任何理由收浮层。
// B 段：再发 asr-state「识别完成，正在派给 S1」，等 1.5 秒看浮层是否被收。
//
// 预期（若凶手是状态行）：A 段 text 有字且 visible 全程 true；B 段 visible 立刻 false。

const { emit } = window.__TAURI__.event;
const { invoke } = window.__TAURI__.core;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

await invoke("show_overlay");
await emit("asr-state", "正在听，请说话…");
await sleep(400);

// ── A 段：只有正文 ──
await emit("asr-final", "A段：只发正文");
await sleep(2500);

// ── B 段：加上那条含「识别完成」的状态行 ──
await emit("asr-state", "识别完成，正在派给 S1");
await sleep(1500);

await invoke("hide_overlay");
return "done";
