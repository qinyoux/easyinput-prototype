// 本机应用库：录入可被 Host Action 触发的打开目标。
import { useEffect, useMemo, useState } from "react";
import * as api from "../api";
import type { AppEntry, AppTarget, SystemApp } from "../types";
import { isValidAppTarget } from "../validation";

const TARGET_KIND_LABELS: Record<AppTarget["kind"], string> = {
  app: "启动程序",
  file: "打开文件",
  url: "打开网址",
  folder: "打开文件夹",
};

interface Props {
  apps: AppEntry[];
  refresh: () => Promise<void>;
  notify: (kind: "ok" | "err", text: string) => void;
}

export default function AppLibrary({ apps, refresh, notify }: Props) {
  const [editing, setEditing] = useState<AppEntry | null>(null);
  const [isNew, setIsNew] = useState(false);

  function startNew() {
    setIsNew(true);
    setEditing({
      uuid: "",
      title: "",
      target: { kind: "app", path: "", args: "", work_dir: "" },
    });
  }

  function startEdit(entry: AppEntry) {
    setIsNew(false);
    setEditing({ ...entry, target: { ...entry.target } });
  }

  async function handleRemove(entry: AppEntry) {
    try {
      await api.removeApp(entry.uuid);
      await refresh();
      notify("ok", `已删除「${entry.title}」`);
    } catch (e) {
      notify("err", String(e));
    }
  }

  async function handleTest(entry: AppEntry) {
    try {
      const msg = await api.testOpen(entry);
      notify("ok", msg);
    } catch (e) {
      notify("err", String(e));
    }
  }

  return (
    <div className="app-library">
      <div className="library-head">
        <h3>本机应用</h3>
        <button className="btn-primary" onClick={startNew}>
          + 新增应用
        </button>
      </div>

      {apps.length === 0 && (
        <p className="empty-hint">
          还没有应用。新增一个后，就能在按键上选择「打开应用」。
        </p>
      )}

      <ul className="app-list">
        {apps.map((a) => (
          <li key={a.uuid} className="app-item">
            <div className="app-item-info">
              <div className="app-title">{a.title}</div>
              <div className="app-target-tag">
                {TARGET_KIND_LABELS[a.target.kind]} ·{" "}
                <span className="mono">{targetSummary(a.target)}</span>
              </div>
            </div>
            <div className="app-item-ops">
              <button
                className="btn-ghost"
                onClick={() => handleTest(a)}
                title="测试打开"
              >
                测试
              </button>
              <button className="btn-ghost" onClick={() => startEdit(a)}>
                编辑
              </button>
              <button
                className="btn-ghost danger"
                onClick={() => handleRemove(a)}
              >
                删除
              </button>
            </div>
          </li>
        ))}
      </ul>

      {editing && (
        <AppEditor
          entry={editing}
          isNew={isNew}
          onCancel={() => setEditing(null)}
          onDone={async (saved) => {
            setEditing(null);
            if (saved) {
              await refresh();
              notify("ok", isNew ? "应用已添加" : "应用已更新");
            }
          }}
          notify={notify}
        />
      )}
    </div>
  );
}

function targetSummary(t: AppTarget): string {
  switch (t.kind) {
    case "app":
      return t.path;
    case "file":
      return t.path;
    case "url":
      return t.url;
    case "folder":
      return t.path;
  }
}

function AppEditor({
  entry,
  isNew,
  onCancel,
  onDone,
  notify,
}: {
  entry: AppEntry;
  isNew: boolean;
  onCancel: () => void;
  onDone: (saved: boolean) => void;
  notify: (kind: "ok" | "err", text: string) => void;
}) {
  const [title, setTitle] = useState(entry.title);
  const [target, setTarget] = useState<AppTarget>(entry.target);

  async function save() {
    const err = isValidAppTarget(target);
    if (err) {
      notify("err", err);
      return;
    }
    if (!title.trim()) {
      notify("err", "展示名不能为空");
      return;
    }
    const payload: AppEntry = { uuid: entry.uuid, title: title.trim(), target };
    try {
      if (isNew) {
        await api.addApp(payload);
      } else {
        await api.updateApp(payload);
      }
      onDone(true);
    } catch (e) {
      notify("err", String(e));
    }
  }

  return (
    <div className="modal-backdrop">
      <div className="modal">
        <h3>{isNew ? "新增应用" : "编辑应用"}</h3>

        <label className="field-label">展示名</label>
        <input
          className="text-input"
          value={title}
          placeholder="如：记事本"
          onChange={(e) => setTitle(e.target.value)}
        />

        <label className="field-label">打开类型</label>
        <select
          className="select"
          value={target.kind}
          onChange={(e) => {
            const kind = e.target.value as AppTarget["kind"];
            const base: AppTarget =
              kind === "app"
                ? { kind, path: "", args: "", work_dir: "" }
                : kind === "file"
                  ? { kind, path: "" }
                  : kind === "url"
                    ? { kind, url: "" }
                    : { kind, path: "" };
            setTarget(base);
          }}
        >
          {(["app", "file", "url", "folder"] as const).map((k) => (
            <option key={k} value={k}>
              {TARGET_KIND_LABELS[k]}
            </option>
          ))}
        </select>

        {target.kind === "app" && (
          <SystemAppPicker
            onPick={(name, path) => {
              setTitle(name);
              setTarget({ kind: "app", path, args: "", work_dir: "" });
            }}
            notify={notify}
          />
        )}

        <TargetFields target={target} onChange={setTarget} />

        <div className="modal-ops">
          <button className="btn-ghost" onClick={onCancel}>
            取消
          </button>
          <button className="btn-primary" onClick={save}>
            保存
          </button>
        </div>
      </div>
    </div>
  );
}

/** 从本机已安装/注册应用中快速选择，自动填充展示名与程序路径。 */
function SystemAppPicker({
  onPick,
  notify,
}: {
  onPick: (name: string, path: string) => void;
  notify: (kind: "ok" | "err", text: string) => void;
}) {
  const [apps, setApps] = useState<SystemApp[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [query, setQuery] = useState("");

  async function load() {
    setLoading(true);
    try {
      const list = await api.listSystemApps();
      setApps(list);
      setLoaded(true);
      notify("ok", `发现 ${list.length} 个应用`);
    } catch (e) {
      notify("err", String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return apps;
    return apps.filter(
      (a) =>
        a.name.toLowerCase().includes(q) || a.path.toLowerCase().includes(q),
    );
  }, [apps, query]);

  return (
    <div className="sys-app-picker">
      <label className="field-label">从本机已安装应用选择</label>
      <div className="picker-row">
        <input
          className="text-input"
          value={query}
          placeholder="搜索应用名称或路径…"
          onChange={(e) => setQuery(e.target.value)}
        />
        <button
          className="btn-ghost"
          onClick={load}
          disabled={loading}
          title="重新扫描本机应用"
        >
          {loading ? "扫描中…" : "刷新"}
        </button>
      </div>
      {loaded && apps.length > 0 && (
        <select
          className="select"
          value=""
          onChange={(e) => {
            const idx = Number(e.target.value);
            const app = filtered[idx];
            if (app) onPick(app.name, app.path);
          }}
        >
          <option value="" disabled>
            选择要打开的应用…
          </option>
          {filtered.map((a, i) => (
            <option key={a.path} value={i}>
              {a.name}
              {a.kind === "shell" ? "（快捷方式）" : ""}
            </option>
          ))}
        </select>
      )}
      {loaded && apps.length === 0 && (
        <p className="empty-hint">未在本机开始菜单发现可用应用。</p>
      )}
    </div>
  );
}

function TargetFields({
  target,
  onChange,
}: {
  target: AppTarget;
  onChange: (t: AppTarget) => void;
}) {
  switch (target.kind) {
    case "app":
      return (
        <>
          <label className="field-label">程序路径</label>
          <input
            className="text-input"
            value={target.path}
            placeholder="C:\\Windows\\notepad.exe"
            onChange={(e) => onChange({ ...target, path: e.target.value })}
          />
          <label className="field-label">启动参数（可空）</label>
          <input
            className="text-input"
            value={target.args}
            placeholder="如：--new-window file.txt"
            onChange={(e) => onChange({ ...target, args: e.target.value })}
          />
          <label className="field-label">工作目录（可空）</label>
          <input
            className="text-input"
            value={target.work_dir}
            placeholder="如：C:\\Users\\me"
            onChange={(e) => onChange({ ...target, work_dir: e.target.value })}
          />
        </>
      );
    case "file":
      return (
        <>
          <label className="field-label">文件路径</label>
          <input
            className="text-input"
            value={target.path}
            placeholder="C:\\docs\\报告.docx"
            onChange={(e) => onChange({ ...target, path: e.target.value })}
          />
        </>
      );
    case "url":
      return (
        <>
          <label className="field-label">网址</label>
          <input
            className="text-input"
            value={target.url}
            placeholder="https://example.com"
            onChange={(e) => onChange({ ...target, url: e.target.value })}
          />
        </>
      );
    case "folder":
      return (
        <>
          <label className="field-label">文件夹路径</label>
          <input
            className="text-input"
            value={target.path}
            placeholder="C:\\Users\\me\\Documents"
            onChange={(e) => onChange({ ...target, path: e.target.value })}
          />
        </>
      );
  }
}
