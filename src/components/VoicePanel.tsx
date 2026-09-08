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
  const [lastResult, setLastResult] = useState("");
  const [phase, setPhase] = useState<Phase>("idle");
  const [mode, setMode] = useState<Mode>("hold");
  const [elapsed, setElapsed] = useState(0);
  const [overlayEnabled, setOverlayEnabled] = useState(true);
  const [kbAudioOnline, setKbAudioOnline] = useState(false);
  const recRef = useRef(false);
  const transcriptRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];
    let mounted = true;
    (async () => {
      unlisteners.push(
        await listen<string>("asr-state", (e) => { if (mounted) setStatus(e.payload); }),
        await listen<string>("asr-partial", (e) => { if (mounted) setLiveText(e.payload); }),
        await listen<string>("asr-final", (e) => {
          if (!mounted) return;
          setLastResult(e.payload);
          setLiveText("");
        }),
        await listen<string>("edit-state", (e) => { if (mounted) setStatus(e.payload); }),
        await listen<string>("edit-partial", (e) => { if (mounted) setLiveText(e.payload); }),
        await listen<string>("edit-llm-chunk", (e) => { if (mounted) setLiveText((prev) => prev + e.payload); }),
      );
    })();
    return () => { mounted = false; unlisteners.forEach((u) => u()); };
  }, []);

  useEffect(() => {
    let mounted = true;
    (async () => {
      try {
        const ov = await invoke<boolean>("get_overlay_enabled");
        if (mounted && typeof ov === "boolean") setOverlayEnabled(ov);
      } catch { /* ignore */ }
      try {
        const m = await invoke<string>("get_mode");
        if (mounted && (m === "hold" || m === "call")) setMode(m);
      } catch { /* ignore */ }
    })();
    return () => { mounted = false; };
  }, []);

  useEffect(() => {
    const active = phase === "listening" || phase === "calling";
    const isEditMode = status.includes("编辑聆听") || status.includes("正在编辑") || status.includes("正在提取") || status.includes("正在替换") || status.includes("识别完成");
    if (overlayEnabled && active && !isEditMode) {
      invoke("show_overlay").catch(() => {});
    } else {
      invoke("hide_overlay").catch(() => {});
    }
  }, [phase, overlayEnabled, status]);

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
  }, [liveText, lastResult, status, phase]);

  useEffect(() => {
    const active = phase === "listening" || phase === "calling";
    if (!active) { setElapsed(0); return; }
    const id = setInterval(() => setElapsed((v) => v + 1), 1000);
    return () => clearInterval(id);
  }, [phase]);

  useEffect(() => {
    if (phase === "idle" || phase === "error") {
      recRef.current = false;
    }
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

  async function toggleRecord() {
    if (recRef.current) {
      recRef.current = false;
      await invoke("stop_recording");
    } else {
      recRef.current = true;
      await invoke("start_recording");
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
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z" />
      <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
      <line x1="12" y1="19" x2="12" y2="23" />
      <line x1="8" y1="23" x2="16" y2="23" />
    </svg>
  );
  const StopIcon = () => (
    <svg viewBox="0 0 24 24" fill="currentColor">
      <rect x="6" y="6" width="12" height="12" rx="2" />
    </svg>
  );

  return (
    <div className="voice-panel">
      <div className="voice-head">
        <span className={`badge ${badgeClass}`}>
          <span className="dot" />
          {badgeText}
        </span>
        <span className="voice-status-text">{statusText}</span>
        <span className={`kb-audio-badge ${kbAudioOnline ? "online" : "offline"}`}>
          {kbAudioOnline ? "键盘麦克风" : "电脑麦克风"}
        </span>
      </div>

      <div className={`status-indicator ${isRecording ? "active" : "idle"}`}>
        <span className="s-dot" />
        {statusText}
      </div>

      <div className="mic-wrap">
        <button
          className={`mic-btn${isRecording ? " recording" : ""}`}
          onClick={toggleRecord}
        >
          {isRecording ? <StopIcon /> : <MicIcon />}
        </button>
      </div>

      <div className={`timer${isRecording ? "" : " muted"}`}>
        {fmtTime(elapsed)}
      </div>

      <div className="transcript" ref={transcriptRef}>
        {phase === "error" && status ? (
          <span className="placeholder">{status}</span>
        ) : liveText ? (
          <>{liveText}<span className="cursor" /></>
        ) : lastResult ? (
          <>{lastResult}</>
        ) : (
          <span className="placeholder">
            {mode === "call"
              ? "点击录音开始通话，说完一句自动上屏"
              : "点击录音开始说话，或按 PTT 键"}
          </span>
        )}
      </div>

      <div className="mode-row">
        <button
          className={`mode-btn${mode === "hold" ? " active" : ""}`}
          onClick={() => applyMode("hold")}
        >单次按键</button>
        <button
          className={`mode-btn${mode === "call" ? " active" : ""}`}
          onClick={() => applyMode("call")}
        >连续通话</button>
      </div>

      {lastResult && (
        <div className="last-line">最近上屏：{lastResult.slice(0, 50)}{lastResult.length > 50 ? "…" : ""}</div>
      )}
    </div>
  );
}
