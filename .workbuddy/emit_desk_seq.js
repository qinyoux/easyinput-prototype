// 在主窗口里按「任务台派发成功」的真实顺序广播一遍事件序列，
// 用来回答：浮层收到 `asr-final` 时到底有没有把正文渲染出来。
//
// 顺序照抄 src-tauri/src/session.rs：
//   ① show_overlay + emit("asr-state", "正在听，请说话…")
//   ② deliver_hold_result 的 desk 分支 → desk_subtitle_events()
//        emit("asr-final", <正文>)
//        emit("asr-state", "识别完成，正在派给 S{slot}")
//   ③ dispatch_to_desk 末尾 → emit("desk-slot-key", {...})
//
// 每一步之间留足间隔，让浮层的 200ms tick 能各采到一次。

const { emit } = window.__TAURI__.event;
const { invoke } = window.__TAURI__.core;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const log = [];

// ① 开始聆听：浮层弹出 + 清空正文
await invoke("show_overlay");
await emit("asr-state", "正在听，请说话…");
log.push(["t+0", "show_overlay + asr-state 正在听"]);
await sleep(700);

// ② 收尾：正文在前、状态行在后
await emit("asr-final", "把昨天的日志按小时汇总一下");
log.push(["t+700", "asr-final 正文"]);
await sleep(300);

await emit("asr-state", "识别完成，正在派给 S1");
log.push(["t+1000", "asr-state 识别完成，正在派给 S1"]);
await sleep(300);

// ③ 派发结果
await emit("desk-slot-key", {
  slot: 1,
  message: "S1 已开始执行（任务 #999）—— 过程在后台跑，跑完会播报结果",
  dispatched: true,
});
log.push(["t+1300", "desk-slot-key"]);

// 停留 1.8 秒后由后端收尾，这里等久一点，看浮层是否被正确收起
await sleep(2600);
await invoke("hide_overlay");
log.push(["t+3900", "hide_overlay"]);

return JSON.stringify(log);
