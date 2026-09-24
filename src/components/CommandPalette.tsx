// ⌘K / Ctrl+K 命令面板：一处搜索「动作 · 方案 · 页面」，避免为找东西而在导航里翻。
// 自实现而非引库：只是一个受控输入框 + 过滤 + 方向键导航，够用且无额外依赖。
import { useEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import { actionKindLabel, actionSummary, inputLabel, inputShort } from "../actionLabel";
import type { AppEntry, Profile } from "../types";
import type { Tab } from "./Sidebar";

interface Props {
  open: boolean;
  onClose: () => void;
  profile: Profile | null;
  profiles: Profile[];
  activeId: string | null;
  apps: AppEntry[];
  /** 选中动作项：跳转工作台并展开该输入的编辑器 */
  onRunAction: (inputId: string) => void;
  onSelectProfile: (id: string) => void;
  onNavigate: (tab: Tab) => void;
}

interface Item {
  key: string;
  group: "动作" | "方案" | "页面";
  title: string;
  sub: string;
  /** 参与模糊匹配的补充关键词（键位短名等） */
  keywords: string;
  run: () => void;
}

const NAV_TARGETS: { tab: Tab; title: string; sub: string }[] = [
  { tab: "workbench", title: "工作台", sub: "页面 · 磁贴总览" },
  { tab: "remap", title: "按键重映射", sub: "页面 · 可视化键盘" },
  { tab: "apps", title: "应用管理", sub: "页面 · 本机应用库" },
  { tab: "voice", title: "语音", sub: "页面 · 语音输入与听写" },
  { tab: "profiles", title: "方案", sub: "页面 · 新建 / 重命名 / 删除" },
  { tab: "settings", title: "设置", sub: "页面 · 语音与主题设置" },
];

export default function CommandPalette({
  open,
  onClose,
  profile,
  profiles,
  activeId,
  apps,
  onRunAction,
  onSelectProfile,
  onNavigate,
}: Props) {
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState(0);
  const listRef = useRef<HTMLUListElement>(null);

  useEffect(() => {
    if (open) {
      setQuery("");
      setCursor(0);
    }
  }, [open]);

  const items = useMemo<Item[]>(() => {
    const out: Item[] = [];

    if (profile) {
      const ids = [...Object.keys(profile.keys), ...Object.keys(profile.encoder)];
      for (const id of ids) {
        const action = profile.keys[id] ?? profile.encoder[id];
        if (!action) continue;
        out.push({
          key: `action:${id}`,
          group: "动作",
          title: `${inputLabel(id)} → ${actionSummary(action, apps)}`,
          sub: `动作 · ${actionKindLabel(action)}`,
          keywords: `${id} ${inputShort(id)}`,
          run: () => onRunAction(id),
        });
      }
    }

    for (const p of profiles) {
      out.push({
        key: `profile:${p.id}`,
        group: "方案",
        title: p.name,
        sub: p.id === activeId ? "方案 · 当前使用中" : "方案 · 切换",
        keywords: "profile 方案 切换",
        run: () => onSelectProfile(p.id),
      });
    }

    for (const n of NAV_TARGETS) {
      out.push({
        key: `nav:${n.tab}`,
        group: "页面",
        title: n.title,
        sub: n.sub,
        keywords: `nav 页面 ${n.tab}`,
        run: () => onNavigate(n.tab),
      });
    }

    return out;
  }, [profile, profiles, activeId, apps, onRunAction, onSelectProfile, onNavigate]);

  const filtered = useMemo(() => {
    const tokens = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    if (tokens.length === 0) return items;
    return items.filter((it) => {
      const hay = `${it.title} ${it.sub} ${it.group} ${it.keywords}`.toLowerCase();
      return tokens.every((t) => hay.includes(t));
    });
  }, [items, query]);

  useEffect(() => {
    setCursor((c) => (c >= filtered.length ? 0 : c));
  }, [filtered.length]);

  useEffect(() => {
    const el = listRef.current?.querySelector<HTMLElement>(".cmdk-item.active");
    el?.scrollIntoView({ block: "nearest" });
  }, [cursor, filtered]);

  if (!open) return null;

  function runAt(index: number) {
    const it = filtered[index];
    if (!it) return;
    it.run();
    onClose();
  }

  function handleKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setCursor((c) => (filtered.length === 0 ? 0 : (c + 1) % filtered.length));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setCursor((c) => (filtered.length === 0 ? 0 : (c - 1 + filtered.length) % filtered.length));
    } else if (e.key === "Enter") {
      e.preventDefault();
      runAt(cursor);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  }

  const activeOptionId = filtered[cursor] ? `cmdk-opt-${cursor}` : undefined;

  return (
    <div className="cmdk-backdrop" onClick={onClose}>
      <div
        className="cmdk"
        role="dialog"
        aria-modal="true"
        aria-label="命令面板"
        onClick={(e) => e.stopPropagation()}
      >
        <input
          className="cmdk-input"
          type="text"
          autoFocus
          value={query}
          placeholder="搜索动作、方案或页面…"
          aria-label="搜索动作、方案或页面"
          role="combobox"
          aria-expanded="true"
          aria-controls="cmdk-list"
          aria-autocomplete="list"
          aria-activedescendant={activeOptionId}
          onChange={(e) => {
            setQuery(e.target.value);
            setCursor(0);
          }}
          onKeyDown={handleKeyDown}
        />
        <ul className="cmdk-list" id="cmdk-list" role="listbox" aria-label="搜索结果" ref={listRef}>
          {filtered.length === 0 && <li className="cmdk-empty">没有匹配项</li>}
          {filtered.map((it, i) => (
            <li
              key={it.key}
              id={`cmdk-opt-${i}`}
              role="option"
              aria-selected={i === cursor}
              className={`cmdk-item ${i === cursor ? "active" : ""}`}
              onMouseEnter={() => setCursor(i)}
              onClick={() => runAt(i)}
            >
              <span className="cmdk-item-title">{it.title}</span>
              <span className="cmdk-item-sub">{it.sub}</span>
            </li>
          ))}
        </ul>
        <div className="cmdk-foot">↑↓ 选择 · Enter 执行 · Esc 关闭</div>
      </div>
    </div>
  );
}
