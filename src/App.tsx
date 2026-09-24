// 主应用。
// 布局分四层：顶栏（品牌 · 方案切换 · 全局操作）→ 状态条（设备连接 · 编码器绑定）
// → 侧栏（动作类型筛选 / 主导航）→ 主区（按 tab 分发到各页面）。
// Ctrl+K / ⌘SS 唤出 ⌘SS 命令面板（独立浮窗，Super StartUp 移植）。
import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import * as api from "./api";
import type { ActionSpec, AppEntry, ConfigAck, Profile, Prompt, Vocabulary } from "./types";
import Sidebar from "./components/Sidebar";
import type { Tab, PluginPageInfo } from "./components/Sidebar";
import ProfileSwitcher from "./components/ProfileSwitcher";
import Workbench from "./components/Workbench";
import PluginPageContainer from "./components/PluginPageContainer";
import { ensurePluginActivated, invokePluginCommand } from "./pluginHost";
import EncoderStatus from "./components/EncoderStatus";
import ProfileManager from "./components/ProfileManager";
import RemapEditor from "./components/RemapEditor";
import AppLibrary from "./components/AppLibrary";
import VoicePanel from "./components/VoicePanel";
import GeneralSettings from "./components/GeneralSettings";
import ApiSettings from "./components/ApiSettings";
import DeskView from "./components/DeskView";
import DeskSettings from "./components/DeskSettings";
import LogPanel from "./components/LogPanel";
import ThemeToggle from "./components/ThemeToggle";
import { inputsOf } from "./actionLabel";
import { validateAction } from "./validation";

type Toast = { kind: "ok" | "err"; text: string };

const PAGE_TITLES: Record<Tab, string> = {
  workbench: "工作台",
  remap: "按键重映射",
  apps: "应用管理",
  voice: "语音",
  desk: "Agent 任务台",
  api: "模型配置",
  profiles: "方案",
  settings: "设置",
};

/** 磁贴/编码器的高亮时长：够看清是哪个输入被触发，又不至于拖到干扰下一次操作。 */
const FLASH_MS = 1200;

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
  const [tab, setTab] = useState<Tab>("workbench");
  const [pluginPages, setPluginPages] = useState<PluginPageInfo[]>([]);

  const [flash, setFlash] = useState<string | null>(null);
  const [lastEvent, setLastEvent] = useState<string | null>(null);
  const [deviceText, setDeviceText] = useState<string>("检测中…");
  const [deviceOnline, setDeviceOnline] = useState<boolean>(false);
  const [listenerRunning, setListenerRunning] = useState(false);
  const [workbenchOpen, setWorkbenchOpen] = useState(false);
  const [pushing, setPushing] = useState(false);
  /**
   * 正在保存方案。
   *
   * 与 `pushing` 分开是有意义的：保存与下发是**两个不同的失败面**（一个写本地配置、
   * 一个走 HID 到开发板），合成一个忙碌标志会让"保存中"和"下发中"看起来像同一件事，
   * 出问题时也就分不清是哪儿错了。
   */
  const [saving, setSaving] = useState(false);
  const [toast, setToast] = useState<Toast | null>(null);
  const [exitDialog, setExitDialog] = useState(false);
  const [rememberExit, setRememberExit] = useState(false);
  const [showLog, setShowLog] = useState(false);
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const flashTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const updatedProfileRef = useRef<Profile | null>(null);
  const profilesRef = useRef<Profile[]>([]);
  const editingRef = useRef<Profile | null>(null);

  const notify = useCallback((kind: "ok" | "err", text: string) => {
    setToast({ kind, text });
    if (toastTimer.current) clearTimeout(toastTimer.current);
    toastTimer.current = setTimeout(() => setToast(null), 5000);
  }, []);

  /** 点亮某个输入并播报一句话。真实触发与手动选中都走这里，保证反馈一致。 */
  const highlight = useCallback((inputId: string | null, message?: string) => {
    if (message) setLastEvent(message);
    if (!inputId) return;
    setFlash(inputId);
    if (flashTimer.current) clearTimeout(flashTimer.current);
    flashTimer.current = setTimeout(() => setFlash(null), FLASH_MS);
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

  /**
   * 同步浮窗开合状态。
   *
   * 浮窗除了主窗口这个入口，还能被硬件 KEY4 独立唤起、也能自己按 Esc 收起，
   * 主窗口不掌握这些动作，只能轮询查态。命令本身只是查窗口可见性，代价可忽略。
   */
  const refreshWorkbench = useCallback(async () => {
    try {
      setWorkbenchOpen(await api.workbenchIsOpen());
    } catch {
      setWorkbenchOpen(false);
    }
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
  }, [loadProfileIntoEditor, notify, refreshApps, refreshProfiles, refreshListener]);

  useEffect(() => {
    profilesRef.current = profiles;
  }, [profiles]);

  useEffect(() => {
    editingRef.current = editing;
  }, [editing]);

  useEffect(() => {
    loadAll();
    refreshDevice();
    invoke<PluginPageInfo[]>("plugin_pages").then(setPluginPages).catch(() => {});
    const deviceTimer = setInterval(refreshDevice, 10000);
    refreshWorkbench();
    const workbenchTimer = setInterval(refreshWorkbench, 2000);
    let unlisten: UnlistenFn | undefined;
    let unlistenClose: UnlistenFn | undefined;
    let unlistenSwitch: UnlistenFn | undefined;
    let unlistenDesk: UnlistenFn | undefined;
    let unlistenVoice: UnlistenFn | undefined;
    let unlistenTerminal: UnlistenFn | undefined;
    listen<{ uuid: string; message: string }>("host-action", (ev) => {
      const { uuid, message } = ev.payload;
      notify("ok", `Host Action：${message}`);
      // 板子只回报 UUID，不知道落在哪个输入上；在当前方案里反查一次，
      // 若唯一命中就把对应磁点点亮，让「按了键」与「界面变化」对得上。
      const p = editingRef.current;
      const prefix = vocabulary?.host_action_prefix ?? "host_action:";
      const hit = p
        ? inputsOf(p).find(
            (it) => it.action.kind === "host_action" && it.action.value === `${prefix}${uuid}`,
          )
        : undefined;
      highlight(hit ? hit.id : null, `Host Action：${message}`);
    }).then((fn) => {
      unlisten = fn;
    });
    listen<{ profile_id: string; profile_name: string; message: string }>("profile-switched", (ev) => {
      const { profile_id, profile_name, message } = ev.payload;
      setActiveId(profile_id);
      const p = profilesRef.current.find((x) => x.id === profile_id);
      if (p) loadProfileIntoEditor(p);
      const text = message || `已切换到方案「${profile_name}」`;
      notify("ok", text);
      highlight(null, text);
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
    // 任务台槽位按键（KEY→S1–S4）：走的是**另一条事件**，因为后端在
    // `agent_desk::handle_reserved_uuid` 里就把它认领了，不会再发 `host-action`。
    // 这里把槽位号反查成具体键位点亮，让「按了键」与「界面高亮」同样对得上；
    // 同时按 `dispatched` 决定提示色调——未派发就是**明确的失败**，不是成功。
    listen<{ slot: number; message: string; dispatched: boolean }>("desk-slot-key", (ev) => {
      const { slot, message, dispatched } = ev.payload;
      notify(dispatched ? "ok" : "err", message);
      const p = editingRef.current;
      const hit = p
        ? inputsOf(p).find(
            (it) => it.action.kind === "task_desk" && it.action.value === String(slot),
          )
        : undefined;
      highlight(hit ? hit.id : null, message);
    }).then((fn) => {
      unlistenDesk = fn;
    });
    // 语音链路的告警。为什么单开一条通道：识别相关的提示（连接失败、没收到音频、
    // 会话卡死被强制释放…）过去**只**发到字幕浮层，而浮层只有录音那几秒才显示 ——
    // 于是"按了键什么反应都没有"这类故障在主窗口里一个字都看不到，用户只能猜。
    // 「失败不静默」在这里的落地就是：**没有浮层的时候，也必须有一个地方看得见。**
    listen<{ message: string; source: string }>("voice-alert", (ev) => {
      notify("err", ev.payload.message);
    }).then((fn) => {
      unlistenVoice = fn;
    });
    // 启动终端（按键 → 开/复用终端 → 预置命令前缀）。同样单开一条事件：
    // 后端把它 spawn 到独立线程了（等窗口最多 6 秒，不能钉住 HID 监听线程），
    // 所以结果与"按下那一刻"在时间上已经断开 —— 必须由后端**主动回报**。
    // `ok` 决定色调：'没等到终端窗口'这类情况是明确的失败，不能显示成绿色的成功。
    listen<{ uuid: string; message: string; ok: boolean }>("cli-terminal", (ev) => {
      const { message, ok } = ev.payload;
      notify(ok ? "ok" : "err", message);
      const p = editingRef.current;
      const hit = p
        ? inputsOf(p).find(
            (it) => it.action.kind === "named" && it.action.value === "cli_terminal",
          )
        : undefined;
      highlight(hit ? hit.id : null, message);
    }).then((fn) => {
      unlistenTerminal = fn;
    });
    // 内容台旋钮导航：焦点切换 + 确认执行。
    // 旋钮左旋/右旋 → 后端更新焦点 → emit studio-focus → 前端提示当前选中项。
    // 旋钮按下 → 后端 emit studio-action → 前端调 studio 插件命令执行。
    let unlistenStudioFocus: UnlistenFn | undefined;
    let unlistenStudioAction: UnlistenFn | undefined;
    listen<{ index: number; label: string }>("studio-focus", (ev) => {
      const { label } = ev.payload;
      notify("ok", `内容台 → ${label}`);
    }).then((fn) => {
      unlistenStudioFocus = fn;
    });
    listen<{ action: string; label: string }>("studio-action", async (ev) => {
      const { action, label } = ev.payload;
      notify("ok", `内容台执行：${label}…`);
      try {
        await ensurePluginActivated("studio");
        const r = await invokePluginCommand("studio", "studio.quick_action", { action });
        if (r && typeof r === "object" && "ok" in r) {
          const result = r as { ok: boolean; message?: string; code?: string };
          notify(result.ok ? "ok" : "err", result.ok ? `✔ ${result.message || label}` : `✘ ${result.code || "ERROR"}：${result.message || "未知错误"}`);
        }
      } catch (e) {
        notify("err", `内容台执行失败：${String(e)}`);
      }
    }).then((fn) => {
      unlistenStudioAction = fn;
    });
    return () => {
      clearInterval(deviceTimer);
      clearInterval(workbenchTimer);
      unlisten?.();
      unlistenClose?.();
      unlistenSwitch?.();
      unlistenDesk?.();
      unlistenVoice?.();
      unlistenTerminal?.();
      unlistenStudioFocus?.();
      unlistenStudioAction?.();
    };
  }, [highlight, loadAll, notify, refreshDevice, refreshWorkbench, vocabulary?.host_action_prefix]);

  useEffect(() => {
    return () => {
      if (toastTimer.current) clearTimeout(toastTimer.current);
      if (flashTimer.current) clearTimeout(flashTimer.current);
    };
  }, []);

  // 全局快捷键：Ctrl+K 唤出 ⌘SS 命令面板（独立浮窗）。
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        invoke("toggle_ss_palette").catch(() => {});
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  function handleSelect(id: string) {
    if (!id || id === activeId) return;
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
      setTab("profiles");
      notify("ok", `已新建「${p.name}」`);
    } catch (e) {
      notify("err", String(e));
    }
  }

  async function handleSave() {
    if (!editing || saving) return;
    setSaving(true);
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
    } finally {
      setSaving(false);
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

  function handleActionChange(inputId: string, action: ActionSpec) {
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

  function handlePromptsChange(prompts: Prompt[]) {
    if (!editing) return;
    setEditing({ ...editing, prompts });
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

  /** 手工开合提示词库浮窗：与硬件 KEY4 走同一条命令，无开发板时也能验证。 */
  async function toggleWorkbench() {
    try {
      setWorkbenchOpen(await api.workbenchToggle());
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
        <ProfileSwitcher
          profiles={profiles}
          activeId={activeId}
          dirty={dirty}
          busy={pushing}
          onSelect={handleSelect}
        />
        <div className="topbar-actions">
          <button
            className="btn-ghost cmdk-btn"
            onClick={() => invoke("toggle_ss_palette").catch(() => {})}
            title="打开 ⌘SS 命令面板（Ctrl+K）"
            aria-label="打开 ⌘SS 命令面板"
          >
            ⌘SS
          </button>
          <button
            className={`btn-ghost listener-toggle ${listenerRunning ? "on" : ""}`}
            onClick={toggleListener}
            aria-pressed={listenerRunning}
          >
            {listenerRunning ? "监听中 · 停止" : "启动监听"}
          </button>
          <ThemeToggle />
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
        </div>
      </header>

      <div className="status-strip">
        <div className="top-status">
          <span className={`dot ${deviceOnline ? "online" : "offline"}`} />
          <span className="status-text" title={deviceText}>
            {deviceText}
          </span>
        </div>
        {editing && (
          <EncoderStatus profile={editing} apps={apps} flash={flash} lastEvent={lastEvent} />
        )}
      </div>

      <div className="body">
        <Sidebar
          tab={tab}
          onTabChange={setTab}
          pluginPages={pluginPages}
        />

        <main className="main">
          <div className="page-head">
            <h2 className="page-title">{PAGE_TITLES[tab]}</h2>
            {tab === "workbench" && (
              <button
                className={`btn-ghost workbench-pop ${workbenchOpen ? "on" : ""}`}
                onClick={toggleWorkbench}
                aria-pressed={workbenchOpen}
                title="打开或收起提示词库浮窗（与硬件 KEY4 等价）"
              >
                {workbenchOpen ? "收起提示词库" : "提示词库"}
              </button>
            )}
          </div>

          {tab === "workbench" &&
            (editing && vocabulary ? (
              <Workbench
                profile={editing}
                onPromptsChange={handlePromptsChange}
              />
            ) : (
              <div className="empty-state">请选择一个按键方案，或新建一个。</div>
            ))}

          {tab === "remap" &&
            (editing && vocabulary ? (
              <RemapEditor
                profile={editing}
                onActionChange={handleActionChange}
                vocabulary={vocabulary}
                apps={apps}
                dirty={dirty}
                saving={saving}
                onSave={handleSave}
              />
            ) : (
              <div className="empty-state">请选择一个按键方案，或新建一个。</div>
            ))}

          {tab === "apps" && <AppLibrary apps={apps} refresh={refreshApps} notify={notify} />}

          <div className="voice-tab" style={{ display: tab === "voice" ? "flex" : "none" }}>
            <VoicePanel />
          </div>

          <div className="desk-tab" style={{ display: tab === "desk" ? "flex" : "none" }}>
            <DeskView
              notify={notify}
              onOpenSettings={() => setTab("settings")}
              onOpenApiConfig={() => setTab("api")}
            />
          </div>

          {tab === "profiles" && (
            <ProfileManager
              profiles={profiles}
              activeId={activeId}
              editing={editing}
              dirty={dirty}
              onSelect={handleSelect}
              onNew={handleNew}
              onRename={handleNameChange}
              onSave={handleSave}
              onDelete={handleDelete}
            />
          )}

          {/* 模型配置：把"连外部模型/服务"的配置（ASR / 语音编辑 LLM / TTS / 执行器）
              从 设置 与 任务台 两处抽出来集中到一页，并成为**执行器的唯一编辑入口**。
              用的是与 设置 同一个滚动容器，保证粘性保存栏和页面宽度表现一致。 */}
          {tab === "api" && (
            <div className="settings-tab">
              <ApiSettings notify={notify} />
            </div>
          )}

          {tab === "settings" && (
            <div className="settings-tab">
              <GeneralSettings />
              <DeskSettings notify={notify} />
            </div>
          )}

          {pluginPages.map((pp) => {
            const tabId = `plugin:${pp.pluginId}:${pp.pageId}`;
            return tab === tabId && (
              <PluginPageContainer key={tabId} pluginId={pp.pluginId} pageId={pp.pageId} />
            );
          })}
        </main>
      </div>

      {toast && (
        <div className={`toast ${toast.kind}`}>
          <span>{toast.text}</span>
          <button className="toast-close" onClick={() => setToast(null)} aria-label="关闭提示">
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
