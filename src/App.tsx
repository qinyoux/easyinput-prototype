// 主应用：顶部状态栏 + 侧栏（方案库）+ 主体（按键映射 / 本机应用 / 语音输入 三页签）。
import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import * as api from "./api";
import type { AppEntry, ConfigAck, Profile, Vocabulary } from "./types";
import ProfilePanel from "./components/ProfilePanel";
import RemapEditor from "./components/RemapEditor";
import AppLibrary from "./components/AppLibrary";
import VoicePanel from "./components/VoicePanel";
import VoiceSettings from "./components/VoiceSettings";
import LogPanel from "./components/LogPanel";
import { validateAction } from "./validation";

type Toast = { kind: "ok" | "err"; text: string };
type Tab = "remap" | "apps" | "voice" | "settings";

function clone<T>(v: T): T {
  return JSON.parse(JSON.stringify(v));
}

export default function App() {
  const [vocabulary, setVocabulary] = useState<Vocabulary | null>(null);
  const [apps, setApps] = useState<AppEntry[]>([]);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [editing, setEditing] = useState<Profile | null>(null);
  const [dirty, setDirty] = useState(false);
  const [tab, setTab] = useState<Tab>("remap");
  const [deviceText, setDeviceText] = useState<string>("检测中…");
  const [deviceOnline, setDeviceOnline] = useState<boolean>(false);
  const [listenerRunning, setListenerRunning] = useState(false);
  const [pushing, setPushing] = useState(false);
  const [toast, setToast] = useState<Toast | null>(null);
  const [exitDialog, setExitDialog] = useState(false);
  const [rememberExit, setRememberExit] = useState(false);
  const [showLog, setShowLog] = useState(false);
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const updatedProfileRef = useRef<Profile | null>(null);
  const profilesRef = useRef<Profile[]>([]);

  const notify = useCallback((kind: "ok" | "err", text: string) => {
    setToast({ kind, text });
    if (toastTimer.current) clearTimeout(toastTimer.current);
    toastTimer.current = setTimeout(() => setToast(null), 5000);
  }, []);

  const refreshDevice = useCallback(async () => {
    try {
      const msg = await api.deviceStatus();
      setDeviceText(msg);
      setDeviceOnline(true);
    } catch (e) {
      setDeviceText(String(e));
      setDeviceOnline(false);
    }
  }, []);

  const refreshListener = useCallback(async () => {
    try {
      setListenerRunning(await api.listenerStatus());
    } catch {
      setListenerRunning(false);
    }
  }, []);

  const refreshApps = useCallback(async () => {
    setApps(await api.listApps());
  }, []);

  const refreshProfiles = useCallback(async () => {
    const list = await api.listProfiles();
    setProfiles(list);
    return list;
  }, []);

  const loadProfileIntoEditor = useCallback((p: Profile | null) => {
    setEditing(p ? clone(p) : null);
    setDirty(false);
  }, []);

  const loadAll = useCallback(async () => {
    try {
      setVocabulary(await api.getVocabulary());
    } catch (e) {
      notify("err", String(e));
    }
    await refreshApps();
    const list = await refreshProfiles();
    await refreshListener();
    if (list.length > 0) {
      const defaultId = updatedProfileRef.current?.id ?? list[0].id;
      const target = list.find((p) => p.id === defaultId) ?? list[0];
      setActiveId(target.id);
      loadProfileIntoEditor(target);
      api.setActiveProfile(target.id).catch(() => {});
    }
  }, [loadProfileIntoEditor, notify, refreshApps, refreshProfiles]);

  useEffect(() => {
    profilesRef.current = profiles;
  }, [profiles]);

  useEffect(() => {
    loadAll();
    refreshDevice();
    const deviceTimer = setInterval(refreshDevice, 3000);
    let unlisten: UnlistenFn | undefined;
    let unlistenClose: UnlistenFn | undefined;
    let unlistenSwitch: UnlistenFn | undefined;
    listen<{ uuid: string; message: string }>("host-action", (ev) => {
      notify("ok", `Host Action：${ev.payload.message}`);
    }).then((fn) => {
      unlisten = fn;
    });
    listen<{ profile_id: string; profile_name: string; message: string }>("profile-switched", (ev) => {
      const { profile_id, profile_name, message } = ev.payload;
      setActiveId(profile_id);
      const p = profilesRef.current.find((x) => x.id === profile_id);
      if (p) loadProfileIntoEditor(p);
      notify("ok", message || `已切换到方案「${profile_name}」`);
    }).then((fn) => {
      unlistenSwitch = fn;
    });
    listen("close-requested", () => {
      if (localStorage.getItem("easyop.exitRemember") === "1") {
        const pref = localStorage.getItem("easyop.exitPref");
        if (pref === "minimize") invoke("hide_to_tray").catch(() => {});
        else if (pref === "quit") invoke("quit_app").catch(() => {});
      } else {
        setExitDialog(true);
      }
    }).then((fn) => {
      unlistenClose = fn;
    });
    return () => {
      clearInterval(deviceTimer);
      unlisten?.();
      unlistenClose?.();
      unlistenSwitch?.();
    };
  }, [loadAll, notify, refreshDevice, refreshListener]);

  function handleSelect(id: string) {
    if (id === activeId) return;
    const p = profiles.find((x) => x.id === id);
    if (!p) return;
    setActiveId(id);
    loadProfileIntoEditor(p);
    api.setActiveProfile(id).catch(() => {});
  }

  async function handleNew(name: string) {
    if (
      dirty &&
      !window.confirm("当前方案有未保存修改，切换将继续并丢失这些修改。继续？")
    ) {
      return;
    }
    try {
      const p = await api.newProfile(name);
      await refreshProfiles();
      setActiveId(p.id);
      loadProfileIntoEditor(p);
      api.setActiveProfile(p.id).catch(() => {});
      notify("ok", `已新建「${p.name}」`);
    } catch (e) {
      notify("err", String(e));
    }
  }

  async function handleSave() {
    if (!editing) return;
    try {
      const saved = await api.saveProfile(editing);
      const list = await refreshProfiles();
      setEditing(clone(saved));
      updatedProfileRef.current = saved;
      setDirty(false);
      if (list.some((p) => p.id === saved.id)) {
        setActiveId(saved.id);
        api.setActiveProfile(saved.id).catch(() => {});
      }
      notify("ok", `已保存方案「${saved.name}」`);
    } catch (e) {
      notify("err", String(e));
    }
  }

  async function handleDelete(id: string) {
    try {
      await api.deleteProfile(id);
      const list = await refreshProfiles();
      const next = list.find((p) => p.id !== id) ?? list[0] ?? null;
      if (activeId === id) {
        setActiveId(next?.id ?? null);
        loadProfileIntoEditor(next);
      }
      api.setActiveProfile(next?.id ?? null).catch(() => {});
      notify("ok", "方案已删除");
    } catch (e) {
      notify("err", String(e));
    }
  }

  function handleActionChange(inputId: string, action: Profile["keys"][string]) {
    if (!editing) return;
    const next = clone(editing);
    if (inputId in next.keys) {
      next.keys[inputId] = action;
    } else {
      next.encoder[inputId] = action;
    }
    setEditing(next);
    setDirty(true);
  }

  function handleNameChange(name: string) {
    if (!editing) return;
    setEditing({ ...editing, name });
    setDirty(true);
  }

  function validateCurrent(): string | null {
    if (!editing) return null;
    if (!editing.name.trim()) return "方案名不能为空";
    if (!vocabulary) return null;
    const all = [...vocabulary.keys, ...vocabulary.encoder_inputs];
    for (const input of all) {
      const action = editing.keys[input] ?? editing.encoder[input];
      if (action) {
        const err = validateAction(
          action,
          vocabulary.named_actions,
          vocabulary.host_action_prefix,
          vocabulary.text_max_utf8_bytes,
        );
        if (err) return `「${input}」${err}`;
      }
    }
    return null;
  }

  async function handlePush() {
    if (!editing) return;
    const err = validateCurrent();
    if (err) {
      notify("err", err);
      return;
    }
    await refreshDevice();
    setPushing(true);
    try {
      const ack: ConfigAck = await api.pushConfig(editing);
      if (ack.ok) {
        notify(
          "ok",
          `下发成功：${ack.bytes} 字节 · CRC16=0x${ack.crc16.toString(16).toUpperCase().padStart(4, "0")}${ack.saved ? " · 已保存到板载 Flash" : ""}`,
        );
      } else {
        notify("err", `下发出错：phase=${ack.phase}`);
      }
    } catch (e) {
      notify("err", String(e));
    } finally {
      setPushing(false);
    }
  }

  async function toggleListener() {
    try {
      if (listenerRunning) {
        await api.stopListener();
        setListenerRunning(false);
        notify("ok", "Host Action 监听已停止");
      } else {
        await api.startListener();
        setListenerRunning(true);
        notify("ok", "Host Action 监听已启动");
      }
    } catch (e) {
      notify("err", String(e));
    }
  }

  async function chooseExit(action: "minimize" | "quit" | "cancel") {
    if (action !== "cancel" && rememberExit) {
      localStorage.setItem("easyop.exitPref", action);
      localStorage.setItem("easyop.exitRemember", "1");
    }
    setExitDialog(false);
    if (action === "minimize") {
      await invoke("hide_to_tray").catch(() => {});
    } else if (action === "quit") {
      await invoke("quit_app").catch(() => {});
    }
  }

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand">
          <span className="brand-mark">EI</span>
          <span className="brand-name">easyop · 按键配置</span>
        </div>
        <div className="top-status">
          <span className={`dot ${deviceOnline ? "online" : "offline"}`} />
          <span className="status-text">{deviceText}</span>
        </div>
        <button
          className={`btn-ghost listener-toggle ${listenerRunning ? "on" : ""}`}
          onClick={toggleListener}
        >
          {listenerRunning ? "监听中 · 停止" : "启动监听"}
        </button>
        <button
          className="btn-ghost log-btn"
          onClick={() => setShowLog(true)}
          title="打开实时日志调试面板"
        >
          日志
        </button>
        <button
          className="btn-primary push-btn"
          onClick={handlePush}
          disabled={pushing || !editing}
        >
          {pushing ? "下发中…" : "下发到开发板"}
        </button>
      </header>

      <div className="body">
        <ProfilePanel
          profiles={profiles}
          activeId={activeId}
          dirty={dirty}
          onSelect={handleSelect}
          onNew={handleNew}
          onSave={handleSave}
          onDelete={handleDelete}
        />

        <main className="main">
          <div className="tabs">
            <button
              className={`tab ${tab === "remap" ? "active" : ""}`}
              onClick={() => setTab("remap")}
            >
              按键映射
            </button>
            <button
              className={`tab ${tab === "apps" ? "active" : ""}`}
              onClick={() => setTab("apps")}
            >
              本机应用
            </button>
            <button
              className={`tab ${tab === "voice" ? "active" : ""}`}
              onClick={() => setTab("voice")}
            >
              语音输入
            </button>
            <button
              className={`tab ${tab === "settings" ? "active" : ""}`}
              onClick={() => setTab("settings")}
            >
              语音设置
            </button>
          </div>

          {tab === "remap" && editing && (
            <>
              <div className="profile-title-row">
                <input
                  className="text-input profile-name-input"
                  value={editing.name}
                  onChange={(e) => handleNameChange(e.target.value)}
                />
                {dirty && <span className="dirty-tag">未保存</span>}
              </div>
              <RemapEditor
                profile={editing}
                onActionChange={handleActionChange}
                vocabulary={vocabulary!}
                apps={apps}
              />
            </>
          )}

          {tab === "remap" && !editing && (
            <div className="empty-state">请选择一个按键方案，或新建一个。</div>
          )}

          {tab === "apps" && (
            <AppLibrary
              apps={apps}
              refresh={refreshApps}
              notify={notify}
            />
          )}

          <div className="voice-tab" style={{ display: tab === "voice" ? "flex" : "none" }}>
            <VoicePanel />
          </div>

          {tab === "settings" && (
            <div className="settings-tab">
              <VoiceSettings />
            </div>
          )}
        </main>
      </div>

      {toast && (
        <div className={`toast ${toast.kind}`}>
          <span>{toast.text}</span>
          <button className="toast-close" onClick={() => setToast(null)}>
            ×
          </button>
        </div>
      )}

      {exitDialog && (
        <div className="modal-backdrop">
          <div className="modal">
            <h3 className="modal-title">退出 easyop？</h3>
            <p className="modal-desc">
              是否最小化到系统托盘，继续在后台运行语音输入？
              <br />托盘图标可随时重新打开或彻底退出。
            </p>
            <label className="remember-row">
              <input
                type="checkbox"
                checked={rememberExit}
                onChange={(e) => setRememberExit(e.target.checked)}
              />
              记住我的选择，下次直接执行
            </label>
            <div className="modal-actions">
              <button className="btn primary" onClick={() => chooseExit("minimize")}>最小化到托盘</button>
              <button className="btn ghost" onClick={() => setExitDialog(false)}>取消</button>
              <button className="btn danger" onClick={() => chooseExit("quit")}>退出</button>
            </div>
          </div>
        </div>
      )}

      {showLog && <LogPanel onClose={() => setShowLog(false)} />}
    </div>
  );
}
