// 实时日志面板：500ms 增量轮询后端日志缓冲区，滚动显示调试信息。
import { useEffect, useRef, useState } from "react";
import * as api from "../api";
import type { LogEntry } from "../types";

interface Props {
  onClose: () => void;
}

export default function LogPanel({ onClose }: Props) {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [autoscroll, setAutoscroll] = useState(true);
  const [pollError, setPollError] = useState<string | null>(null);
  const lastSeqRef = useRef(0);
  const boxRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let alive = true;
    const poll = async () => {
      try {
        const entries = await api.getLogs(lastSeqRef.current);
        if (!alive) return;
        setPollError(null);
        if (entries.length > 0) {
          lastSeqRef.current = entries[entries.length - 1].seq;
          setLogs((prev) => [...prev, ...entries].slice(-2000));
        }
      } catch (e) {
        if (alive) setPollError(String(e));
      }
    };
    void poll();
    const id = setInterval(poll, 500);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  useEffect(() => {
    if (autoscroll && boxRef.current) {
      boxRef.current.scrollTop = boxRef.current.scrollHeight;
    }
  }, [logs, autoscroll]);

  async function handleClear() {
    try {
      await api.clearLogs();
      setLogs([]);
      lastSeqRef.current = 0;
    } catch {
      // ignore
    }
  }

  return (
    <div className="modal-backdrop log-backdrop" onClick={onClose}>
      <div className="modal log-panel" onClick={(e) => e.stopPropagation()}>
        <div className="log-head">
          <h3>实时日志</h3>
          <div className="log-ops">
            <label className="log-autoscroll">
              <input
                type="checkbox"
                checked={autoscroll}
                onChange={(e) => setAutoscroll(e.target.checked)}
              />
              自动滚动
            </label>
            <button className="btn-ghost" onClick={handleClear}>
              清空
            </button>
            <button className="btn-ghost" onClick={onClose}>
              关闭
            </button>
          </div>
        </div>
        <div className="log-body" ref={boxRef}>
          {pollError && (
            <div className="log-line err">
              <span className="log-ts">--:--:--.---</span>
              <span className="log-level">ERR</span>
              <span className="log-msg">轮询失败：{pollError}</span>
            </div>
          )}
          {logs.length === 0 && !pollError && (
            <p className="empty-hint">
              暂无日志。按开发板按键或在「本机应用」点击「测试」打开应用，日志会实时显示在这里。
            </p>
          )}
          {logs.map((l) => (
            <div key={l.seq} className={`log-line ${l.level}`}>
              <span className="log-ts">{l.ts}</span>
              <span className="log-level">{l.level.toUpperCase()}</span>
              <span className="log-msg">{l.msg}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
