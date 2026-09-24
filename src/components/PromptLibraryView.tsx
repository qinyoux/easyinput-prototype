// 提示词库视图：列出当前方案的提示词，支持增删改。
// 复用现有 workbench_prompt_add / update / remove Tauri commands。
import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Prompt } from "../types";

interface Props {
  profileId: string;
  prompts: Prompt[];
  onPromptsChange: (prompts: Prompt[]) => void;
}

export default function PromptLibraryView({ profileId, prompts, onPromptsChange }: Props) {
  const [newText, setNewText] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editText, setEditText] = useState("");
  const [status, setStatus] = useState<{ ok: boolean; msg: string } | null>(null);

  const addPrompt = useCallback(async () => {
    const text = newText.trim();
    if (!text) return;
    try {
      const result = await invoke<{ items: Prompt[] }>("workbench_prompt_add", {
        profileId,
        text,
      });
      onPromptsChange(result.items);
      setNewText("");
      setStatus({ ok: true, msg: "已添加" });
    } catch (e) {
      setStatus({ ok: false, msg: `添加失败: ${e}` });
    }
  }, [newText, profileId, onPromptsChange]);

  const updatePrompt = useCallback(async (id: string) => {
    const text = editText.trim();
    if (!text) return;
    try {
      const result = await invoke<{ items: Prompt[] }>("workbench_prompt_update", {
        profileId,
        id,
        text,
      });
      onPromptsChange(result.items);
      setEditingId(null);
      setStatus({ ok: true, msg: "已更新" });
    } catch (e) {
      setStatus({ ok: false, msg: `更新失败: ${e}` });
    }
  }, [editText, profileId, onPromptsChange]);

  const removePrompt = useCallback(async (id: string) => {
    try {
      const result = await invoke<{ items: Prompt[] }>("workbench_prompt_remove", {
        profileId,
        id,
      });
      onPromptsChange(result.items);
      setStatus({ ok: true, msg: "已删除" });
    } catch (e) {
      setStatus({ ok: false, msg: `删除失败: ${e}` });
    }
  }, [profileId, onPromptsChange]);

  return (
    <div className="prompt-library-view">
      <div className="prompt-add-row">
        <input
          className="prompt-input"
          value={newText}
          onChange={(e) => setNewText(e.target.value)}
          placeholder="输入新提示词…"
          onKeyDown={(e) => e.key === "Enter" && addPrompt()}
        />
        <button className="prompt-add-btn" onClick={addPrompt}>添加</button>
      </div>

      {status && <p className={`hint ${status.ok ? "ok" : "bad"}`}>{status.msg}</p>}

      <div className="prompt-list">
        {prompts.length === 0 ? (
          <p className="prompt-empty">暂无提示词。添加一条常用的提示词，方便日常快速调用。</p>
        ) : (
          prompts.map((p) => (
            <div key={p.id} className="prompt-item">
              {editingId === p.id ? (
                <>
                  <input
                    className="prompt-edit-input"
                    value={editText}
                    onChange={(e) => setEditText(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") updatePrompt(p.id);
                      if (e.key === "Escape") setEditingId(null);
                    }}
                    autoFocus
                  />
                  <button className="prompt-save-btn" onClick={() => updatePrompt(p.id)}>保存</button>
                  <button className="prompt-cancel-btn" onClick={() => setEditingId(null)}>取消</button>
                </>
              ) : (
                <>
                  <span className="prompt-text">{p.text}</span>
                  <button
                    className="prompt-edit-btn"
                    onClick={() => { setEditingId(p.id); setEditText(p.text); }}
                  >编辑</button>
                  <button
                    className="prompt-del-btn"
                    onClick={() => removePrompt(p.id)}
                  >删除</button>
                </>
              )}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
