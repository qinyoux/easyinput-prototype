// 「方案」页：方案的切换 / 新建 / 重命名 / 保存 / 删除都集中在这里。
// 顶栏下拉只负责快速切换，避免把增删改这类低频但有破坏性的操作塞进下拉。
import { useState } from "react";
import type { Profile } from "../types";

interface Props {
  profiles: Profile[];
  activeId: string | null;
  editing: Profile | null;
  dirty: boolean;
  onSelect: (id: string) => void;
  onNew: (name: string) => void;
  onRename: (name: string) => void;
  onSave: () => void;
  onDelete: (id: string) => void;
}

export default function ProfileManager({
  profiles,
  activeId,
  editing,
  dirty,
  onSelect,
  onNew,
  onRename,
  onSave,
  onDelete,
}: Props) {
  const [newName, setNewName] = useState("");

  function handleNew() {
    onNew(newName.trim() || "未命名方案");
    setNewName("");
  }

  return (
    <div className="profile-manager">
      <section className="pm-section">
        <h3 className="pm-head">当前方案</h3>
        {editing ? (
          <div className="pm-current">
            <input
              className="text-input pm-name"
              value={editing.name}
              aria-label="当前方案名称"
              onChange={(e) => onRename(e.target.value)}
            />
            {dirty && <span className="dirty-tag">未保存</span>}
            <button className="btn-primary" onClick={onSave} disabled={!dirty}>
              保存方案
            </button>
          </div>
        ) : (
          <div className="empty-state">尚未选择方案。</div>
        )}
      </section>

      <section className="pm-section">
        <h3 className="pm-head">方案库</h3>
        <ul className="pm-list">
          {profiles.map((p) => {
            const isActive = p.id === activeId;
            // 方案名是可聚焦按钮，删除按钮作为兄弟节点并列在 <li> 内，
            // 避免「不可聚焦的 li 承担点击」与「按钮嵌套在可点击元素里」两种反模式。
            return (
              <li key={p.id} className={`pm-item ${isActive ? "active" : ""}`}>
                <button
                  className="pm-select"
                  aria-current={isActive ? "true" : undefined}
                  onClick={() => onSelect(p.id)}
                >
                  <span className="pm-item-name">{p.name}</span>
                  <span className="pm-item-meta">
                    {isActive ? (dirty ? "当前 · 有未保存修改" : "当前使用中") : "点击切换"}
                  </span>
                </button>
                {isActive && (
                  <button
                    className="row-op"
                    title="删除方案"
                    aria-label={`删除方案 ${p.name}`}
                    onClick={() => onDelete(p.id)}
                  >
                    ×
                  </button>
                )}
              </li>
            );
          })}
          {profiles.length === 0 && <li className="empty-state">还没有任何方案。</li>}
        </ul>
      </section>

      <section className="pm-section">
        <h3 className="pm-head">新建方案</h3>
        <div className="pm-new">
          <input
            className="text-input"
            value={newName}
            placeholder="新方案名称…"
            aria-label="新方案名称"
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleNew();
            }}
          />
          <button className="btn-primary" onClick={handleNew}>
            新建
          </button>
        </div>
      </section>
    </div>
  );
}
