// 判别实验：正文完全一样，只改「收尾那条 asr-state 的措辞」，
// 看浮层是留还是被收 —— 以此判定凶手是不是 VoicePanel 里那条
// 「status 文案 → phase → hide_overlay」的推理链。
//
// V1：收尾状态 =「识别完成，正在派给 S1」  → VoicePanel 判 phase=idle → 应收浮层
// V3：收尾状态 =「正在听，请说话…」        → VoicePanel 判 phase=listening → 应留浮层
//
// 两段都发了正文，所以「有没有字」不是变量，唯一的变量是状态行措辞。

const { emit } = window.__TAURI__.event;
const { invoke } = window.__TAURI__.core;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ── V1 ──
await invoke("show_overlay");
await emit("asr-state", "正在听，请说话…");
await sleep(400);
await emit("asr-final", "V1正文：含识别完成的收尾");
await sleep(200);
await emit("asr-state", "识别完成，正在派给 S1");
await sleep(2200);

// ── V3 ──
await invoke("show_overlay");
await emit("asr-state", "正在听，请说话…");
await sleep(400);
await emit("asr-final", "V3正文：收尾仍是正在听");
await sleep(200);
await emit("asr-state", "正在听，请说话…");
await sleep(2200);

await invoke("hide_overlay");
return "done";
