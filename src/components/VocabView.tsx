// ASR 自定义词库视图：管理语音识别热词。
import { useEffect, useState, useCallback } from "react";
import * as api from "../api";
import type { VocabWord } from "../types";

export default function VocabView() {
  const [words, setWords] = useState<VocabWord[]>([]);
  const [newText, setNewText] = useState("");
  const [newWeight, setNewWeight] = useState(5);
  const [status, setStatus] = useState<{ ok: boolean; msg: string } | null>(null);

  const load = useCallback(async () => {
    try {
      setWords(await api.asrVocabList());
    } catch {
      setWords([]);
    }
  }, []);

  useEffect(() => { load(); }, [load]);

  const add = useCallback(async () => {
    const text = newText.trim();
    if (!text) return;
    try {
      setWords(await api.asrVocabAdd(text, newWeight));
      setNewText("");
      setStatus({ ok: true, msg: "已添加" });
    } catch (e) {
      setStatus({ ok: false, msg: `添加失败: ${e}` });
    }
  }, [newText, newWeight]);

  const remove = useCallback(async (text: string) => {
    try {
      setWords(await api.asrVocabRemove(text));
      setStatus({ ok: true, msg: "已删除" });
    } catch (e) {
      setStatus({ ok: false, msg: `删除失败: ${e}` });
    }
  }, []);

  return (
    <div className="vocab-view">
      <p className="vocab-hint">
        自定义热词可在语音识别时优先匹配专业术语和高频词汇，提升识别准确率。权重越高越优先（1-10）。
      </p>

      <div className="vocab-add-row">
        <input
          className="vocab-input"
          value={newText}
          onChange={(e) => setNewText(e.target.value)}
          placeholder="输入热词…"
          onKeyDown={(e) => e.key === "Enter" && add()}
        />
        <label className="vocab-weight-label">
          权重
          <input
            type="number"
            className="vocab-weight-input"
            value={newWeight}
            onChange={(e) => setNewWeight(Number(e.target.value) || 5)}
            min={1}
            max={10}
          />
        </label>
        <button className="vocab-add-btn" onClick={add}>添加</button>
      </div>

      {status && <p className={`hint ${status.ok ? "ok" : "bad"}`}>{status.msg}</p>}

      <div className="vocab-list">
        {words.length === 0 ? (
          <p className="vocab-empty">暂无热词。</p>
        ) : (
          words.map((w) => (
            <div key={w.text} className="vocab-item">
              <span className="vocab-text">{w.text}</span>
              <span className="vocab-weight">权重 {w.weight}</span>
              <button className="vocab-del-btn" onClick={() => remove(w.text)}>删除</button>
            </div>
          ))
        )}
      </div>
    </div>
  );
}