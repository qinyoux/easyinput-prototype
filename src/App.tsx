// 主应用。
// 布局分四层：顶栏（品牌 · 方案切换 · 全局操作）→ 状态条（设备连接 · 编码器绑定）
// → 侧栏（动作类型筛选 / 主导航）→ 主区（按 tab 分发到各页面）。
// Ctrl+K / ⌘SS 唤出 ⌘SS 命令面板（独立浮窗，Super StartUp 移植）。
import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import * as api from "./api";
import type { ActionSpec, AppEntry, ConfigAck, DeviceFirmwareStatus, LicenseInfo, Profile, Prompt, Vocabulary } from "./types";
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
  // 固件版本：`null` = 还没读到（离线 or 尚未查），`error` 里放**具体原因**。
  // 两者分开是因为"没有版本号"和"读失败"在界面上必须看起来不一样 ——
  // 前者是还没问，后者是问了没答上来。
  const [firmware, setFirmware] = useState<DeviceFirmwareStatus | null>(null);
  const [firmwareError, setFirmwareError] = useState<string | null>(null);
  const [firmwareLoading, setFirmwareLoading] = useState(false);
  // 授权（注册）状态：null = 还没查；否则拿 LicenseInfo。
  const [license, setLicense] = useState<LicenseInfo | null>(null);
  const [licenseLoading, setLicenseLoading] = useState(false);
  const [licenseModalOpen, setLicenseModalOpen] = useState(false);
  const [licenseInput, setLicenseInput] = useState("");
  const [licenseMsg, setLicenseMsg] = useState<string | null>(null);
  const [licenseBusy, setLicenseBusy] = useState(false);
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

  /**
   * 读开发板固件版本（USB Feature Report `0x13` → `0x11`/kind `0x04`）。
   *
   * 这是一次**完整的状态回读**（实机约 100 ms、约 10 个分块），不是枚举式探测，
   * 所以它**不进 10 秒轮询**：只在上线跃迁、应用启动、以及用户手动点击时触发。
   * 放进轮询会每 10 秒打扰一次板子，还会和按键监听器抢同一个 HID 读句柄。
   */
  const refreshFirmware = useCallback(async () => {
    setFirmwareLoading(true);
    try {
      setFirmware(await api.deviceFirmwareStatus());
      setFirmwareError(null);
    } catch (e) {
      // 失败就把上一次的版本号也清掉：留着它等于宣称"板上是那一版"，
      // 而这次我们恰恰没能证明这一点。
      setFirmware(null);
      setFirmwareError(String(e));
    } finally {
      setFirmwareLoading(false);
    }
  }, []);

  /** 读授权状态：从本地配置加载已保存的激活码并重新校验。 */
  const refreshLicense = useCallback(async () => {
    setLicenseLoading(true);
    try {
      const res = await api.licenseStatus();
      if (res.ok && res.data) setLicense(res.data);
      else setLicense(null);
    } catch {
      setLicense(null);
    } finally {
      setLicenseLoading(false);
    }
  }, []);

  /** 上一次的在线状态，用来识别"离线 → 在线"那次跃迁。 */
  const deviceOnlineRef = useRef(false);

  const refreshDevice = useCallback(async () => {
    try {
      const msg = await api.deviceStatus();
      setDeviceText(msg);
      setDeviceOnline(true);
      // 只在**离线 → 在线**这一次跃迁上顺手读一次固件版本。
      if (!deviceOnlineRef.current) {
        deviceOnlineRef.current = true;
        void refreshFirmware();
      }
    } catch (e) {
      setDeviceText(String(e));
      setDeviceOnline(false);
      // 板子不在了，之前读到的版本号就不再是"当前"的事实 —— 一并清掉，
      // 否则界面会一直挂着一个可能已经过时的版本串。
      if (deviceOnlineRef.current) {
        deviceOnlineRef.current = false;
        setFirmware(null);
        setFirmwareError(null);
      }
    }
  }, [refreshFirmware]);

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
    void refreshLicense();
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
    listen<{ index: number; label: string }>("studio-focus", async (ev) => {
      const { index, label } = ev.payload;
      notify("ok", `内容台 → ${label}`);
      try {
        await ensurePluginActivated("studio");
        await invokePluginCommand("studio", "studio.set_focus", { index });
      } catch (_) {}
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
        } else {
          notify("err", `内容台命令未就绪：插件未激活或命令 studio.quick_action 未注册。请先打开侧边栏「内容台」页签激活插件。`);
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
        {/*
          固件版本徽标，紧跟「开发板检测」：两者回答的是同一类问题 ——
          「板子在不在」与「板上跑的是哪一版」。版本串由**板子自己报出**
          （0x13 状态请求 → 0x11/kind 0x04），不是本地文件里的记录，
          所以它也是"固件到底烧没烧进去"唯一可信的回答。
          读不到时如实写"读取失败"并允许点击重试，绝不显示一个可能过期的版本号。
        */}
        <button
          type="button"
          className={`fw-badge${firmware ? " ok" : firmwareError ? " bad" : ""}`}
          onClick={() => void refreshFirmware()}
          disabled={!deviceOnline || firmwareLoading}
          title={
            firmware
              ? `固件 ${firmware.firmware}\n状态协议 ${firmware.schema}` +
                (firmware.battery_mv != null
                  ? `\n电池 ${firmware.battery_mv} mV / ${firmware.battery_percent ?? "?"}%`
                  : "")
              : firmwareError
                ? `${firmwareError}\n\n点击重试`
                : "板子不在线时不读固件版本。插上 USB（或等它重连）会自动读一次，也可以点击重试。"
          }
        >
          {firmwareLoading
            ? "固件版本读取中…"
            : firmware
              ? `固件 ${firmware.firmware}`
              : firmwareError
                ? "固件版本读取失败 · 点击重试"
                : "固件版本 —"}
        </button>
        <button
          type="button"
          className={`license-badge${license?.valid ? " ok" : license?.valid === false ? " bad" : ""}`}
          onClick={() => {
            setLicenseInput("");
            setLicenseMsg(license?.message ?? null);
            setLicenseModalOpen(true);
          }}
          title={license?.valid ? `已注册（${license.edition}）` : "点击输入激活码"}
        >
          {licenseLoading
            ? "注册状态读取中…"
            : license?.valid
              ? `已注册 · ${license.edition}`
              : license?.valid === false
                ? "未注册（试用）"
                : "注册 / 激活"}
        </button>
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

      {licenseModalOpen && (
        <div className="modal-backdrop" onClick={() => setLicenseModalOpen(false)}>
          <div className="modal license-modal" onClick={(e) => e.stopPropagation()}>
            <h3 className="modal-title">激活 easyop</h3>
            <p className="modal-desc">
              输入由作者发放的激活码（本地校验，无需联网）。未激活时使用试用版，功能完整。
            </p>
            <textarea
              className="license-input"
              value={licenseInput}
              placeholder="在此粘贴激活码"
              onChange={(e) => setLicenseInput(e.target.value)}
              rows={3}
            />
            {licenseMsg && (
              <p className={`license-msg${license?.valid ? " ok" : " bad"}`}>{licenseMsg}</p>
            )}
            <div className="modal-actions">
              <button
                className="btn primary"
                disabled={licenseBusy || licenseInput.trim().length === 0}
                onClick={async () => {
                  setLicenseBusy(true);
                  setLicenseMsg(null);
                  try {
                    const res = await api.activateLicense(licenseInput.trim());
                    if (res.ok && res.data) {
                      setLicense(res.data);
                      setLicenseMsg(res.data.message || "激活成功");
                      notify("ok", `激活成功：${res.data.edition}`);
                      setTimeout(() => setLicenseModalOpen(false), 1200);
                    } else {
                      setLicenseMsg(res.message || "激活失败");
                      notify("err", res.message || "激活失败");
                    }
                  } catch (e) {
                    setLicenseMsg(String(e));
                    notify("err", String(e));
                  } finally {
                    setLicenseBusy(false);
                  }
                }}
              >
                {licenseBusy ? "校验中…" : "激活"}
              </button>
              <button className="btn ghost" onClick={() => setLicenseModalOpen(false)}>关闭</button>
            </div>
          </div>
        </div>
      )}

      {showLog && <LogPanel onClose={() => setShowLog(false)} />}
    </div>
  );
}
