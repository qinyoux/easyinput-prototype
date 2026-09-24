// 语音输入页签：PTT / 手动录音 → 云端流式识别 → 上屏。
// 与后端会话状态机（session.rs）通过事件与命令交互。
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import * as api from "../api";

type Mode = "hold" | "call";
type Phase = "idle" | "listening" | "calling" | "error";

export default function VoicePanel() {
  const [status, setStatus] = useState("准备就绪");
  const [liveText, setLiveText] = useState("");
  // 编辑改写阶段的流式输出。**必须与 `liveText` 分开**：
  // `liveText` 是识别阶段的字段（覆盖语义，服务端回的是累积全文），
  // 而 LLM 的增量是**追加**语义。两者共用一个状态，就会把 ASR 原话
  // 和改写结果接成一段 —— 用户看到的就是"字幕里冒出了别的字"。
  const [editText, setEditText] = useState("");
  const [lastResult, setLastResult] = useState("");
  // 最近**真的**进了输入框的一句话。
  //
  // ⛔ 不能拿 `lastResult` 代替它：`asr-final` 在**注入之前**就发了（后端先让用户
  // 看到识别结果，再做注入），那一刻这句话还没进任何输入框。用它显示"最近上屏"，
  // 就是在替用户宣布一件还没发生的事 —— 而"进没进输入框"恰恰是他最需要看准的。
  // 后端为此单独发 `voice-injected`（只在注入成功后）。
  const [lastInjected, setLastInjected] = useState("");
  // 注入失败、后端留了底的那一句话（**异常路径专用**）。
  //
  // ⛔ 它**不是** v0.29.0 那个「待确认」：待确认是流程里的一个正常站点（代价是同一件事
  // 要按三次），而它只在"注入这一步失败"时出现 —— 正常路径永远碰不到它，
  // 所以它不会给正常操作多加任何一次按键。
  // 它存在的唯一理由：识别已经成功，不能因为"送进输入框"失败就把用户刚说的话丢掉。
  const [failedText, setFailedText] = useState("");
  const [phase, setPhase] = useState<Phase>("idle");
  const [mode, setMode] = useState<Mode>("hold");
  const [elapsed, setElapsed] = useState(0);
  const [kbAudioOnline, setKbAudioOnline] = useState(false);
  const transcriptRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];
    let mounted = true;
    (async () => {
      unlisteners.push(
        await listen<string>("asr-state", (e) => {
          if (!mounted) return;
          setStatus(e.payload);
        }),
        await listen<string>("asr-partial", (e) => { if (mounted) setLiveText(e.payload); }),
        await listen<string>("asr-final", (e) => {
          if (!mounted) return;
          setLastResult(e.payload);
          setLiveText("");
        }),
        // 注入失败：后端把那一句留了底，并把内容一起发过来（界面得给它一个出口）。
        // ⛔ 新一轮录音**不清它**：留底在会话之外，用户没处理就不该消失。
        await listen<{ text: string }>("voice-failed-inject", (e) => {
          if (!mounted) return;
          setFailedText(e.payload?.text ?? "");
        }),
        // 「最近上屏」的唯一可信来源：只有真进过输入框才会收到它。
        await listen<string>("voice-injected", (e) => {
          if (mounted) setLastInjected(e.payload);
        }),
        // ⛔ 编辑模式**没有** `edit-partial` 这个事件：后端的编辑链路复用
        // `asr::run_recognition`，它发的就是上面那条全局 `asr-partial`。
        // 这里曾挂过一个永远收不到的 `edit-partial` 监听，浮层那边更是因为它
        // 去写了一条"兜底"分支、把字幕锁死在第一段（见 `edit_overlay.html`）。
        // **死通道比缺通道更危险：它看起来像有一条路。** 统一走 `asr-partial`。
        await listen<string>("edit-state", (e) => {
          if (!mounted) return;
          setStatus(e.payload);
          // 新一轮编辑从"聆听"重新开始 ⇒ 上一轮的改写结果必须让位，
          // 否则它会一直挂在字幕区，盖住这一轮刚识别出来的内容。
          if (e.payload.includes("编辑聆听")) setEditText("");
        }),
        // 改写流开始：清空上一段，后面收到的都是这一轮的改写结果。
        await listen("edit-llm-start", () => { if (mounted) setEditText(""); }),
        await listen<string>("edit-llm-chunk", (e) => {
          if (!mounted) return;
          // ⛔ 写 `editText`，不写 `liveText` —— 见上面 editText 的说明。
          setEditText((prev) => prev + e.payload);
        }),
      );
    })();
    return () => { mounted = false; unlisteners.forEach((u) => u()); };
  }, []);

  useEffect(() => {
    let mounted = true;
    (async () => {
      // 这里**不再**读 `get_overlay_enabled`：本面板不显示、也不控制字幕浮层
      // （开关在「设置 → 界面选项」，浮层显隐整体归后端，理由见下面那段说明）。
      try {
        const m = await invoke<string>("get_mode");
        if (mounted && (m === "hold" || m === "call")) setMode(m);
      } catch { /* ignore */ }
      // 对齐"注入失败的留底"：`voice-failed-inject` 是事件，面板挂载前发出的那条收不到。
      // 少了这一步，刷新页面 / 切回本页签之后，一句真留着底的话在界面上看不见 ——
      // 用户会以为它丢了，然后重新说一遍（而它其实一直等着被重新注入）。
      try {
        const p = await invoke<string | null>("get_failed_inject");
        if (mounted && p) setFailedText(p);
      } catch { /* ignore */ }
    })();
    return () => { mounted = false; };
  }, []);

  // 浮层显隐**完全由后端决定**，前端一个字节都不碰。这不是"少写点代码"，
  // 而是这里连着踩过两次的坑留下的结论：
  //
  // ① 后端在识别成功后先发正文（`asr-final`），再发状态行「识别完成，正在派给 S{slot}」，
  //    并按 `DESK_SUBTITLE_LINGER` 让浮层多留 1.8 秒。前端曾拿 `status` 的子串判"活儿干完了"
  //    而把浮层收掉 —— 实测正文只亮约 200 毫秒，用户看到的就是"小窗弹了一下，里面没字"。
  // ② 只把"收"去掉、留着"显示"也不行，而且更隐蔽：本效果的声明**顺序**在这儿是致命的 ——
  //    它排在 `status → phase` 那条之前，所以 status 变化的那一帧它会先带着**上一轮的 phase**
  //    跑一次（`listening`），在后端已经把浮层收掉之后又把它叫回来。实测：一轮空录音结束后
  //    浮层仍可见 4.7 秒，直到手动收才消失。
  //
  // `status` 是**给人看的字符串**，不是给机器判的状态位：它由 `asr-state` 与 `edit-state`
  // 两个通道轮流写入，拿它的子串去推导控制流，注定要被下一句文案绊倒。
  //
  // 后端才是唯一知道"什么时候开始、什么时候算完"的一方：录音开始它 show，每条终态路径它 hide
  // （注入成功 / 空结果 / 识别失败 / 任务台停留结束 / 通话挂断 / 转编辑），开关关掉也由它收
  // （`set_overlay_enabled`）。前端再加一手，就只是多一个抢方向盘的人。

  useEffect(() => {
    let mounted = true;
    const poll = async () => {
      try {
        const online = await api.keyboardAudioOnline();
        if (mounted) setKbAudioOnline(online);
      } catch {
        if (mounted) setKbAudioOnline(false);
      }
    };
    poll();
    const timer = setInterval(poll, 3000);
    return () => { mounted = false; clearInterval(timer); };
  }, []);

  useEffect(() => {
    if (status.includes("通话中")) setPhase("calling");
    else if (status.includes("正在听") || status.includes("正在上屏") || status.includes("编辑聆听") || status.includes("正在编辑") || status.includes("正在提取") || status.includes("正在替换")) setPhase("listening");
    else if (status.includes("失败") || status.includes("请先") || status.includes("错误")) setPhase("error");
    else if (status.includes("编辑完成") || status.includes("识别完成")) setPhase("idle");
    else setPhase("idle");
  }, [status]);

  useEffect(() => {
    if (transcriptRef.current) {
      transcriptRef.current.scrollTop = transcriptRef.current.scrollHeight;
    }
  }, [liveText, editText, lastResult, status, phase, failedText]);

  useEffect(() => {
    const active = phase === "listening" || phase === "calling";
    if (!active) { setElapsed(0); return; }
    const id = setInterval(() => setElapsed((v) => v + 1), 1000);
    return () => clearInterval(id);
  }, [phase]);

  const fmtTime = (s: number) => {
    const m = Math.floor(s / 60);
    const sec = s % 60;
    return `${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}`;
  };

  async function applyMode(next: Mode) {
    try {
      await invoke("set_mode", { mode: next });
      setMode(next);
    } catch (e) {
      setStatus(`切换失败: ${e}`);
    }
  }

  // ● 按钮与 KEY1 **完全等价**：只报"按了一下"，是开始还是结束由后端按当前状态决定
  // （`start_recording` → `ptt_down`）。
  // ⛔ 前端**不再自己记**"我是不是在录"：那份镜像必然会和后端的真实状态漂移
  //    （例如用户按了物理 KEY1 而前端不知道），而漂移的表现正是最难受的那种
  //    "按钮写着停止、按下去却开始了一段新录音"。
  //    按钮的标签本来就来自后端事件（`asr-state` → `phase`），根本不需要第二份状态。
  async function toggleRecord() {
    try {
      await invoke("start_recording");
    } catch (e) {
      setStatus(`操作失败: ${e}`);
    }
  }

  // 重新注入 / 丢弃「注入失败留底」的那一句话。
  //
  // ⛔ 失败必须说出来：后端在重试失败时会把内容**再放回去**（保证），
  // 所以这里把错误显示出来就行 —— 内容不会因为"试了一次没成"而消失。
  async function retryInject() {
    try {
      await invoke<string>("retry_failed_inject");
    } catch (e) {
      setStatus(`重新注入失败: ${e}`);
    }
  }

  async function dismissFailed() {
    try {
      await invoke<string | null>("clear_failed_inject");
      // 用户明确说"不要了"，界面就别再举着它 —— 否则看起来像还在等处理。
      setFailedText("");
    } catch (e) {
      setStatus(`丢弃失败: ${e}`);
    }
  }

  const badgeText =
    phase === "calling" ? "通话中" :
    phase === "listening" ? "聆听中" :
    phase === "error" ? "需注意" : "就绪";
  const badgeClass =
    phase === "calling" ? "calling" :
    phase === "listening" ? "listening" :
    phase === "error" ? "error" : "idle";

  const isRecording = phase === "listening" || phase === "calling";
  const statusText =
    phase === "calling" ? "正在通话 · 自动断句上屏" :
    phase === "listening" ? "正在聆听" :
    phase === "error" ? status :
    "准备就绪";

  const MicIcon = () => (
    <svg aria-hidden="true" focusable="false" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z" />
      <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
      <line x1="12" y1="19" x2="12" y2="23" />
      <line x1="8" y1="23" x2="16" y2="23" />
    </svg>
  );
  const StopIcon = () => (
    <svg aria-hidden="true" focusable="false" viewBox="0 0 24 24" fill="currentColor">
      <rect x="6" y="6" width="12" height="12" rx="2" />
    </svg>
  );

  // 录音按钮的无障碍名称随状态变化（开始/停止），属于动作型按钮，
  // 因此不再叠加 aria-pressed —— 标签本身已表达当前可执行的动作。
  const micLabel = isRecording
    ? (phase === "calling" ? "停止通话录音" : "停止录音")
    : (mode === "call" ? "开始通话录音" : "开始录音");

  return (
    <div className="voice-panel">
      <div className="voice-head">
        <span className={`badge ${badgeClass}`} role="status" aria-live="polite">
          <span className="dot" aria-hidden="true" />
          {badgeText}
        </span>
        <span className="voice-status-text">{statusText}</span>
        <span className={`kb-audio-badge ${kbAudioOnline ? "online" : "offline"}`}>
          {kbAudioOnline ? "键盘麦克风" : "电脑麦克风"}
        </span>
      </div>

      <div className={`status-indicator ${isRecording ? "active" : "idle"}`} aria-hidden="true">
        <span className="s-dot" />
        {statusText}
      </div>

      <div className="mic-wrap">
        <button
          className={`mic-btn${isRecording ? " recording" : ""}`}
          onClick={toggleRecord}
          aria-label={micLabel}
          title={micLabel}
        >
          {isRecording ? <StopIcon /> : <MicIcon />}
        </button>
      </div>

      <div className={`timer${isRecording ? "" : " muted"}`}>
        {fmtTime(elapsed)}
      </div>

      <div
        className="transcript"
        ref={transcriptRef}
        role="log"
        aria-live="polite"
        aria-label="识别文本"
      >
        {phase === "error" && status ? (
          <span className="placeholder">{status}</span>
        ) : editText ? (
          <>{editText}<span className="cursor" /></>
        ) : liveText ? (
          <>{liveText}<span className="cursor" /></>
        ) : lastResult ? (
          <>{lastResult}</>
        ) : (
          <span className="placeholder">
            {mode === "call"
              ? "点击录音开始通话，说完一句自动上屏"
              : "点击录音开始说话；再点一次（或按语音键）注入输入框，录音时按回车 = 注入并发送"}
          </span>
        )}
      </div>

      {failedText && (
        // 注入失败条：**只在异常路径上出现**（正常路径永远不会看到它）。
        // 两个出口：重新注入（后端保证重试失败也不会把内容弄丢）/ 丢弃（用户说不要了）。
        <div className="fail-row">
          <span className="fail-hint">
            注入失败（{failedText.length} 字）· 内容已保留，可
            <strong>重新注入</strong>
          </span>
          <button className="fail-btn primary" onClick={retryInject}>重新注入</button>
          <button className="fail-btn" onClick={dismissFailed}>丢弃</button>
        </div>
      )}

      <div className="mode-row" role="radiogroup" aria-label="录音模式">
        <button
          role="radio"
          aria-checked={mode === "hold"}
          className={`mode-btn${mode === "hold" ? " active" : ""}`}
          onClick={() => applyMode("hold")}
          title="按一下开始说话 → 再按一下注入输入框（不发送）；录音时按回车 = 注入并发送"
        >单次按键</button>
        <button
          role="radio"
          aria-checked={mode === "call"}
          className={`mode-btn${mode === "call" ? " active" : ""}`}
          onClick={() => applyMode("call")}
          title="持续通话，说完一句自动上屏（此模式下回车不参与）"
        >连续通话</button>
      </div>

      {lastInjected && (
        <div className="last-line">最近上屏：{lastInjected.slice(0, 50)}{lastInjected.length > 50 ? "…" : ""}</div>
      )}
    </div>
  );
}
