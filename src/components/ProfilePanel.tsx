// 方案库侧栏：切换 / 新建 / 保存 / 删除方案。
import { useState } from "react";
import type { Profile } from "../types";

interface Props {
  profiles: Profile[];
  activeId: string | null;
  dirty: boolean;
  onSelect: (id: string) => void;
  onNew: (name: string) => void;
  onSave: () => void;
  onDelete: (id: string) => void;
}

export default function ProfilePanel({
  profiles,
  activeId,
  dirty,
  onSelect,
  onNew,
  onSave,
  onDelete,
}: Props) {
  const [newName, setNewName] = useState("");
  const active = profiles.find((p) => p.id === activeId);

  function handleNew() {
    const name = newName.trim() || "未命名方案";
    onNew(name);
    setNewName("");
  }

  return (
    <aside className="sidebar">
      <div className="panel-head">
        <h3>按键方案</h3>
        <button className="btn-primary" onClick={handleNew}>
          + 新建
        </button>
      </div>

      <input
        className="text-input new-name-input"
        value={newName}
        placeholder="新方案名称…"
        onChange={(e) => setNewName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") handleNew();
        }}
      />

      <ul className="profile-list">
        {profiles.map((p) => (
          <li
            key={p.id}
            className={`profile-item ${p.id === activeId ? "active" : ""}`}
            onClick={() => onSelect(p.id)}
          >
            <span className="profile-name">{p.name}</span>
            {p.id === activeId && dirty && <span className="dirty-dot">●</span>}
            {p.id === activeId && (
              <button
                className="row-op"
                title="删除方案"
                onClick={(e) => {
                  e.stopPropagation();
                  onDelete(p.id);
                }}
              >
                ×
              </button>
            )}
          </li>
        ))}
      </ul>

      {active && (
        <div className="profile-actions">
          <button className="btn-primary block" onClick={onSave}>
            保存方案
          </button>
        </div>
      )}
    </aside>
  );
}
