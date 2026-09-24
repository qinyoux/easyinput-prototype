// 「设置」页签：**不连外部模型服务**的其余配置。
//
// 2026-09-22 拆分：原先这个文件名叫 VoiceSettings，里面混着三类东西 ——
// ① 语音识别 / 语音编辑（API 凭据）；② 设备通信（Wi-Fi / 麦克风 / 蓝牙）；
// ③ 与语音无关的界面与外设开关（主题 / Everything / 文本注入 / 终端启动 / 字幕浮层）。
//
// ① 已整体迁到独立的「模型配置」页（`ApiSettings.tsx`，含 PTT 与编辑键）；
// 本文件保留 ② ③，因此改名为 `GeneralSettings.tsx` —— 名字要如实描述它管什么，
// 否则下一个打开这个文件的人会以为语音配置还在这里。
//
// 归属判据（与 模型配置页共用同一条）：**只有"决定怎么连外部模型服务"的项才进 API 页**；
// 本地程序路径、设备通信、行为开关一律留在本页。
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import * as api from "../api";
import { THEME_MODE_LABELS, useTheme, type ThemeMode } from "../theme";
import {
  PASTE_STYLE_OPTIONS,
  TERMINAL_KIND_OPTIONS,
  type PasteStyle,
  type TerminalKind,
  type CliTerminalConfig,
} from "../types";

export default function GeneralSettings() {
  const [overlayEnabled, setOverlayEnabled] = useState(true);
  const { mode: themeMode, setMode: applyThemeMode } = useTheme();
  const [kbAudioOnline, setKbAudioOnline] = useState(false);
  const [micSource, setMicSource] = useState<"auto" | "pc" | "wifi" | "bluetooth">("auto");
  const [wifiSsid, setWifiSsid] = useState("");
  const [wifiPassword, setWifiPassword] = useState("");
  /** 链路实时状态（来自后端看护线程）。`null` = 还没拿到。 */
  const [wifiLink, setWifiLink] = useState<api.WifiLinkStatus | null>(null);
  // 蓝牙的"状态"文案由后端给（见 commands.rs 的 `ble_status`）——它现在是一句
  // **如实的话**："读不到（开发板只在蓝牙 GATT 通道上发布蓝牙状态…）"。
  // 过去这个字段装的是 Wi-Fi 音频心跳的替身结论（在线/离线），与蓝牙无关。
  const [bleStatus, setBleStatus] = useState("读取中…");
  const [hint, setHint] = useState("");
  const [showWifiPassword, setShowWifiPassword] = useState(false);
  const [everythingPath, setEverythingPath] = useState("");
  const [everythingStatus, setEverythingStatus] = useState<{ ok: boolean; msg: string } | null>(
    null,
  );
  const [compactContextMenu, setCompactContextMenu] = useState(false);
  // ---- 文本注入（A″：语音打进终端 / TUI 场景）----
  const [pasteStyle, setPasteStyleState] = useState<PasteStyle>("auto");
  // 初值取后端默认（v0.28.2 起为 **false**）：在真正读到配置前的那一帧也显示得跟生效值一致，
  // 否则开关会先显示"开"再跳成"关"，看着像被改了两次。
  const [submitAfterPaste, setSubmitAfterPasteState] = useState(false);
  const [pasteStyleStatus, setPasteStyleStatus] = useState<{ ok: boolean; msg: string } | null>(
    null,
  );
  // ---- 「启动终端并预置命令」----
  // 初值刻意留空并由后端填：命令默认值只有后端一个真源（`DEFAULT_COMMAND`），
  // 前端再抄一份的话，将来改了后端默认值，界面会一直显示那个过期的旧默认值。
  // 空着的那一瞬有 placeholder 顶着，不会看着像坏了。
  const [cliCfg, setCliCfg] = useState<CliTerminalConfig>({
    command: "",
    cwd: "",
    terminal: "auto",
    auto_run: false,
  });
  const [cliStatus, setCliStatus] = useState<{ ok: boolean; msg: string } | null>(null);

  useEffect(() => {
    let mounted = true;
    api
      .getCliTerminalConfig()
      .then((c) => {
        if (mounted) setCliCfg(c);
      })
      .catch((e) => {
        if (mounted) setCliStatus({ ok: false, msg: `读不到终端启动配置：${e}` });
      });
    return () => {
      mounted = false;
    };
  }, []);

  /** 保存终端启动配置。`syncBack` 时用后端规范化后的值回填（命令/目录已 trim）。 */
  async function persistCli(next: CliTerminalConfig, syncBack: boolean) {
    setCliCfg(next);
    try {
      const saved = await api.setCliTerminalConfig(next);
      if (syncBack) setCliCfg(saved);
      setCliStatus({ ok: true, msg: "已保存" });
    } catch (e) {
      setCliStatus({ ok: false, msg: `保存失败: ${e}` });
    }
  }

  /** 先落盘再试跑：否则"试跑"验证的是磁盘上的旧配置，改了没保存会以为改了没用。 */
  async function testCliLaunch() {
    try {
      const saved = await api.setCliTerminalConfig(cliCfg);
      setCliCfg(saved);
    } catch (e) {
      setCliStatus({ ok: false, msg: `保存失败: ${e}` });
      return;
    }
    setCliStatus({ ok: true, msg: "已保存，正在启动终端…（最多等 6 秒）" });
    try {
      const msg = await api.launchCliTerminal();
      setCliStatus({ ok: true, msg });
    } catch (e) {
      // 这里是**明确的失败**（比如"没等到终端窗口成为前台"），不能显示成成功。
      setCliStatus({ ok: false, msg: String(e) });
    }
  }

  useEffect(() => {
    let mounted = true;
    (async () => {
      try {
        const ov = await invoke<boolean>("get_overlay_enabled");
        if (mounted && typeof ov === "boolean") setOverlayEnabled(ov);
      } catch {
        /* ignore */
      }
      try {
        const src = await api.getMicSource();
        if (mounted && src) setMicSource(src as typeof micSource);
      } catch {
        /* ignore */
      }
      try {
        const wc = await api.getWifiConfig();
        if (mounted) {
          setWifiSsid(wc.ssid ?? "");
          setWifiPassword(wc.password ?? "");
        }
      } catch {
        /* ignore */
      }
      try {
        const s = await api.bleStatus();
        if (mounted) setBleStatus(s);
      } catch {
        /* ignore */
      }
      try {
        const ep = await invoke<string>("get_everything_path");
        if (mounted) setEverythingPath(ep);
      } catch {
        /* ignore */
      }
      try {
        const ccm = await invoke<boolean>("get_compact_context_menu");
        if (mounted) setCompactContextMenu(ccm);
      } catch {
        /* ignore */
      }
      try {
        const [ps, sa] = await Promise.all([api.getPasteStyle(), api.getSubmitAfterPaste()]);
        if (!mounted) return;
        if (ps) setPasteStyleState(ps);
        setSubmitAfterPasteState(!!sa);
      } catch {
        /* ignore */
      }
    })();
    return () => {
      mounted = false;
    };
  }, []);

  useEffect(() => {
    let mounted = true;
    const poll = async () => {
      try {
        const online = await api.keyboardAudioOnline();
        if (mounted) setKbAudioOnline(online);
      } catch {
        if (mounted) setKbAudioOnline(false);
      }
    };
    poll();
    const timer = setInterval(poll, 3000);
    return () => {
      mounted = false;
      clearInterval(timer);
    };
  }, []);

  // Wi-Fi 链路状态轮询。
  //
  // ⛔ 以前这张卡片的「Wi-Fi 已连接」是**本地硬写**的：点一下「连接」就
  // `setWifiConnected(true)`，跟板子到底连没连上毫无关系。于是"点了成功、麦克风
  // 却不出声"这种状态可以一直挂着。现在的唯一判据是后端的 `board_online`
  // （= 上位机收到了板子的 EIHB 心跳），前端只负责显示它。
  useEffect(() => {
    let mounted = true;
    const poll = async () => {
      try {
        const st = await api.wifiLinkStatus();
        if (mounted) setWifiLink(st);
      } catch {
        if (mounted) setWifiLink(null);
      }
    };
    poll();
    const timer = setInterval(poll, 2000);
    return () => {
      mounted = false;
      clearInterval(timer);
    };
  }, []);

  return (
    <div className="settings-page">
      <div className="settings-section">
        <h3 className="settings-section-title">通信配置</h3>
        <div className="field">
          <label>麦克风来源</label>
          <div className="row">
            <select
              className="select"
              value={micSource}
              onChange={(e) => {
                const v = e.target.value as typeof micSource;
                setMicSource(v);
                api.setMicSource(v).catch((err) => setHint(`麦克风切换失败: ${err}`));
              }}
            >
              <option value="auto">自动（优先键盘，回退电脑）</option>
              <option value="pc">电脑本机麦克风</option>
              <option value="wifi">Wi-Fi 键盘麦克风</option>
              {/* 开发板只有 Wi-Fi UDP 一条音频通路（对固件源码核实：AudioTransport
                  只有 Unavailable / WifiUdp）。选项保留是为了兼容老配置，但必须
                  把"选了不生效"写在选项里 —— 否则用户会以为是自己没配对成功。 */}
              <option value="bluetooth">蓝牙键盘麦克风（开发板无此通路，不生效）</option>
            </select>
          </div>
          <p className="hint">
            当前：{kbAudioOnline ? "键盘麦克风在线" : "键盘麦克风离线"}
            {micSource === "auto" && (kbAudioOnline ? "（使用键盘）" : "（使用电脑）")}
          </p>
          {micSource === "bluetooth" && (
            <p className="hint bad">
              开发板的音频<strong>只能走 Wi-Fi</strong>（蓝牙在这台设备上只传按键，不传音频），
              所以这一项不会生效、实际用电脑麦克风。要脱离电脑麦克风请选「Wi-Fi 键盘麦克风」
              并把开发板接到同一网络。
            </p>
          )}
        </div>

        <div className="field">
          <label>Wi-Fi 配置（开发板连接的无线网络）</label>
          <div className="row">
            <input
              value={wifiSsid}
              onChange={(e) => setWifiSsid(e.target.value)}
              placeholder="Wi-Fi SSID"
            />
            <input
              type={showWifiPassword ? "text" : "password"}
              value={wifiPassword}
              onChange={(e) => setWifiPassword(e.target.value)}
              placeholder="密码"
            />
            <button
              className="toggle-visibility"
              onClick={() => setShowWifiPassword(!showWifiPassword)}
              title={showWifiPassword ? "隐藏" : "显示"}
              aria-label={showWifiPassword ? "隐藏 WiFi 密码" : "显示 WiFi 密码"}
            >
              {showWifiPassword ? "🙈" : "👁"}
            </button>
          </div>
          <div className="row">
            <span
              className={`kb-audio-badge ${wifiLink?.board_online ? "online" : "offline"}`}
            >
              {wifiLink?.board_online ? "Wi-Fi 已连接" : "Wi-Fi 未连接"}
            </span>
            <button
              onClick={async () => {
                setHint("");
                try {
                  await api.setWifiConfig(wifiSsid, wifiPassword);
                  const r = await api.wifiConnect(wifiSsid, wifiPassword);
                  // ⚠️ 这里**不写"已连接"**：下发成功 ≠ 连上了。
                  // 徽标由 `wifi_link_status` 的 `board_online` 决定。
                  setHint(
                    `端点已下发（通道 ${r.channel}${r.saved_to_board ? "，板子已存 NVS：下次上电它会自己连" : ""}）。` +
                      (r.hint ? ` ${r.hint}` : " 正在等板子心跳确认…"),
                  );
                } catch (e) {
                  setHint(`Wi-Fi 连接失败: ${e}`);
                }
              }}
            >
              连接
            </button>
            <button
              onClick={async () => {
                setHint("");
                try {
                  const msg = await api.wifiDisconnect();
                  setHint(msg);
                } catch (e) {
                  setHint(`Wi-Fi 断开失败: ${e}`);
                }
              }}
            >
              断开
            </button>
            <button
              onClick={async () => {
                try {
                  setWifiLink(await api.wifiLinkStatus());
                } catch (e) {
                  setHint(`读取链路状态失败: ${e}`);
                }
              }}
            >
              刷新状态
            </button>
          </div>

          {/* 链路实时状态：把"自动检测 / 自动连接 / 重试倒计时"如实摊开。
              这四行是**唯一**能让"板子麦克风不出声"这类问题当场定位的地方 ——
              没有它，用户只能看到一个"未连接"和一堆猜测。 */}
          {wifiLink && (
            <div className="hint" style={{ lineHeight: 1.7 }}>
              <div>
                <strong>链路状态：</strong>
                {wifiLink.state_text}
                {wifiLink.attempts > 0 && wifiLink.next_retry_secs > 0 && (
                  <> · 第 {wifiLink.attempts + 1} 次重试还有 {wifiLink.next_retry_secs} 秒</>
                )}
              </div>
              <div>
                <strong>上位机所处网络：</strong>
                {wifiLink.current_ssid ?? "未连 Wi-Fi"} · 本机地址{" "}
                {wifiLink.lan_ipv4 ?? "未知"}
              </div>
              {wifiLink.target_ssid && (
                <div>
                  <strong>本轮目标：</strong>
                  {wifiLink.target_ssid}
                  {wifiLink.target_reason ? `（${wifiLink.target_reason}）` : ""}
                  {wifiLink.last_channel ? ` · 通道 ${wifiLink.last_channel}` : ""}
                  {wifiLink.saved_to_board ? " · 板子已存 NVS" : ""}
                </div>
              )}
              {wifiLink.last_error && (
                <div style={{ color: "var(--danger, #d33)" }}>
                  <strong>最近一次失败：</strong>
                  {wifiLink.last_error}
                </div>
              )}
              {wifiLink.known.length > 0 && (
                <div>
                  <strong>历史网络（按成功证据排序）：</strong>
                  {wifiLink.known
                    .map((k) =>
                      k.last_success_unix > 0
                        ? `${k.ssid}（成功 ${k.success_count} 次）`
                        : `${k.ssid}（尚未成功过${k.has_password ? "" : "，无密码"}）`,
                    )
                    .join(" · ")}
                </div>
              )}
            </div>
          )}

          <p className="hint">
            配置下发到开发板后，开发板将连接此 Wi-Fi 并通过 UDP 发送音频。SSID 和密码会自动保存到配置文件，下次启动自动回填。
            <br />
            <strong>接好一次之后就不必再手动连了</strong>：端点由上位机写进板子的 NVS，
            板子<strong>上电即自行连接</strong>（每约 10 秒重试一轮）；上位机只在你
            <strong>换了网络、或本机 IP 变了</strong>时自动重下发。历史网络会一直留着，
            下次回到那张网会直接命中。
          </p>
        </div>

        <div className="field">
          <label>蓝牙（BLE HID）</label>
          {/* ⛔ 这里以前是一个"启用蓝牙通信通道"的开关 + 一个"配对"按钮，还带一个
              "蓝牙已开启 · 离线"的状态标签。三样东西**没有一样是真的**（对固件源码核实，09-21）：

              1. 开关下发的 `{"cmd":"ble_enable"}` 固件**不认识** —— 它的配置载荷解析器
                 只接受一组固定的顶层键（schema / profiles / ptt_hotkey / audio_* /
                 wifi_* / target_platform…），**没有 cmd 字段**；而这个命令走的是
                 不等回执的下发通道，所以"按了没反应"从来不会报错。
              2. 状态标签读的是 **Wi-Fi UDP 音频通道的心跳**，和蓝牙毫无关系 ——
                 板子 Wi-Fi 音频没上来时它就写"离线"，用户于是去查蓝牙。
              3. 开发板的蓝牙**本来就没有开关**：BLE HID 在固件里常开（广播、连接身份、
                 HID 上报都在跑），配对由 Windows 侧完成。

              按「失败不静默」：把这三样假东西换成一个说真话的说明块。
              开关与配对按钮移除，蓝牙开关/配对命令在后端改为如实报错（见 commands.rs）。 */}
          <p className="hint">
            <strong>开发板的蓝牙是常开的，不需要（也没有）开关。</strong>
            按键事件由固件<strong>同时</strong>经 USB 和蓝牙发出；要在无线状态下用它，
            请到 Windows「设置 → 蓝牙和其他设备 → 添加设备」里与开发板配对，
            配对成功后拔掉 USB 线，按键依然可用。
          </p>
          <p className="hint">
            此前这里的开关与「配对」按钮<strong>都不生效</strong>——下发的命令开发板固件
            并不认识（它的配置协议只接受固定的配置字段，没有指令字段），所以按了等于没按。
            已如实改为不提供这两个按钮，而不是继续假装它们有用。
          </p>
          <div className="row">
            <span className="kb-audio-badge offline">连接状态：{bleStatus}</span>
          </div>
        </div>
      </div>

      <div className="settings-section">
        <h3 className="settings-section-title">界面主题</h3>
        <div className="field toggle-field">
          <div className="row">
            <span className="toggle-desc">深色 / 浅色 / 跟随系统</span>
            <div className="segmented" role="group" aria-label="界面主题">
              {(["dark", "light", "system"] as ThemeMode[]).map((m) => (
                <button
                  key={m}
                  className={`mode-btn${themeMode === m ? " active" : ""}`}
                  onClick={() => applyThemeMode(m)}
                  aria-pressed={themeMode === m}
                >
                  {THEME_MODE_LABELS[m]}
                </button>
              ))}
            </div>
          </div>
          <p className="hint">「跟随系统」会随 Windows 的深浅色设置自动切换；也可用顶栏右侧的按钮快速在深色 / 浅色之间切换。</p>
        </div>
      </div>

      <div className="settings-section">
        <h3 className="settings-section-title">Everything 搜索</h3>
        <div className="field">
          <label>Everything 可执行文件路径</label>
          <div className="row">
            <input
              value={everythingPath}
              onChange={(e) => setEverythingPath(e.target.value)}
              placeholder="如 C:\Program Files\Everything\Everything.exe 或 es.exe"
              spellCheck={false}
            />
            <button
              onClick={async () => {
                try {
                  const ok = await invoke<boolean>("everything_available");
                  setEverythingStatus({
                    ok,
                    msg: ok ? "Everything 可用" : "路径无效或 Everything 未运行",
                  });
                } catch (e) {
                  setEverythingStatus({ ok: false, msg: `验证失败: ${e}` });
                }
              }}
            >
              验证
            </button>
            <button
              onClick={async () => {
                try {
                  await invoke("set_everything_path", { path: everythingPath });
                  setEverythingStatus({ ok: true, msg: "已保存" });
                } catch (e) {
                  setEverythingStatus({ ok: false, msg: `保存失败: ${e}` });
                }
              }}
            >
              保存
            </button>
          </div>
          {everythingStatus && (
            <p className={`hint ${everythingStatus.ok ? "ok" : "bad"}`}>{everythingStatus.msg}</p>
          )}
          <p className="hint">
            配置后 ⌘SS 面板可通过 Everything 快速搜索全盘文件。
            需先安装 <a href="https://www.voidtools.com/zh-cn/" target="_blank" rel="noreferrer">Everything</a> 并保持运行。
          </p>
        </div>
        <div className="field toggle-field">
          <label>精简右键菜单</label>
          <div className="row">
            <span className="toggle-desc">开启后只显示常用的 6 项（打开 / 打开方式 / 打开所在位置 / 复制路径 / 属性 / 更多选项），更接近 Everything 风格；关闭则显示完整原生 Shell 菜单（含所有第三方扩展）</span>
            <button
              className={`switch${compactContextMenu ? " on" : ""}`}
              onClick={() => {
                const next = !compactContextMenu;
                setCompactContextMenu(next);
                invoke("set_compact_context_menu", { enabled: next }).catch(() => {});
              }}
              aria-pressed={compactContextMenu}
              aria-label="精简右键菜单"
            >
              <span className="switch-text">{compactContextMenu ? "开" : "关"}</span>
              <span className="knob" />
            </button>
          </div>
          <div className="row">
            <span className={`switch-state${compactContextMenu ? " on" : ""}`}>
              {compactContextMenu ? "已开启 · 右键显示精简菜单" : "已关闭 · 右键显示系统完整菜单"}
            </span>
          </div>
        </div>
      </div>

      <div className="settings-section">
        <h3 className="settings-section-title">文本注入</h3>
        <div className="field">
          <label>粘贴方式</label>
          <div className="row">
            <select
              className="select"
              value={pasteStyle}
              onChange={async (e) => {
                const next = e.target.value as PasteStyle;
                setPasteStyleState(next);
                try {
                  // 用后端回传的规范化值回填：用户手写的 "Ctrl+V" 会被规整成 "ctrl_v"，
                  // 界面上必须显示后端真正存下的那个，否则"显示的"和"生效的"会不一致。
                  const saved = await api.setPasteStyle(next);
                  setPasteStyleState(saved as PasteStyle);
                  setPasteStyleStatus({ ok: true, msg: "已保存" });
                } catch (err) {
                  setPasteStyleStatus({ ok: false, msg: `保存失败: ${err}` });
                }
              }}
            >
              {PASTE_STYLE_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>{o.label}</option>
              ))}
            </select>
          </div>
          <p className="hint">{PASTE_STYLE_OPTIONS.find((o) => o.value === pasteStyle)?.hint}</p>
          <p className="hint">
            <strong>为什么需要这一项</strong>：<code>Ctrl+V</code> 在终端里的 TUI 程序
            （codex / vim / htop 等）中<strong>粘不进去</strong> —— TUI 会把终端置于
            raw mode 直接读按键字节，<code>Ctrl+V</code> 在那里是字面量 <code>0x16</code>，
            会被 TUI 当成自己的快捷键吃掉。终端级粘贴由<strong>终端模拟器</strong>处理，
            而不同终端的键位<strong>不一样</strong>（本机实测）：
            <code>Windows Terminal</code> 用 <code>Ctrl+Shift+V</code>（或鼠标右键），
            <code>Git Bash</code> 用 <code>Shift+Insert</code>。选「自动」时按焦点窗口
            类名选对应那组键；认不出的窗口回落 <code>Ctrl+V</code>。
          </p>
          {pasteStyleStatus && (
            <p className={`hint ${pasteStyleStatus.ok ? "ok" : "bad"}`}>{pasteStyleStatus.msg}</p>
          )}
        </div>
        <div className="field toggle-field">
          <label>粘贴后自动回车</label>
          <div className="row">
            <span className="toggle-desc">
              管的是<strong>其他注入路径</strong>（命令注入、调试用注入）：注入文本后
              再按一次回车（提交）。<strong>默认关</strong> —— 回车是
              <strong>不可撤销</strong>的提交，而"该不该现在提交"只有你知道。
              <br />
              ⛔ <strong>语音输入不受这个开关影响</strong>：它按"你怎么结束录音"决定
              —— 再按一次语音键 = <strong>只注入</strong>；录音期间按
              <strong>回车</strong> = <strong>注入并发送</strong>。
              两者都是按次生效，不会去改这个全局开关（也不要改成"临时改全局再改回来"，
              那会让共享状态在一次并发里留下脏值）。
            </span>
            <button
              className={`switch${submitAfterPaste ? " on" : ""}`}
              onClick={() => {
                const next = !submitAfterPaste;
                setSubmitAfterPasteState(next);
                api.setSubmitAfterPaste(next).catch(() => {});
              }}
              aria-pressed={submitAfterPaste}
              aria-label="粘贴后自动回车"
            >
              <span className="switch-text">{submitAfterPaste ? "开" : "关"}</span>
              <span className="knob" />
            </button>
          </div>
        </div>
      </div>

      <div className="settings-section">
        <h3 className="settings-section-title">终端启动</h3>
        <p className="hint">
          把「启动环境」和「输入内容」拆成两步：<strong>按一下绑了这个动作的键</strong>
          → 开一个终端窗口并把下面这条命令打进输入框（<strong>不回车</strong>）
          → 再<strong>按语音键说话</strong>，识别到的内容接在命令后面 → 回车即执行。
          <br />
          为什么不能把命令当参数交给终端：<code>cmd /k &lt;命令&gt;</code> 是<strong>直接执行</strong>；
          要「只打进去、等你说完」，只能先开窗口、再往窗口里打键。
          <br />
          焦点已经在一个终端上时<strong>不会再开新窗口</strong>，而是直接往那个终端里预置 ——
          于是「上次没等到窗口」时，点一下终端、再按一次键就能接上。
        </p>
        <div className="field">
          <label>要预置的命令（前缀，不含回车）</label>
          <div className="row">
            <input
              className="text-input mono"
              value={cliCfg.command}
              onChange={(e) => setCliCfg({ ...cliCfg, command: e.target.value })}
              placeholder="codex exec --skip-git-repo-check"
              spellCheck={false}
            />
          </div>
          <p className="hint">
            这是<strong>前缀</strong>，语音说出的内容会接在它后面。中间那个空格由 easyop 自动补 ——
            少了它，两段会黏成一个参数（<code>--skip-git-repo-check打开计算器</code>），
            然后你会看到一个像是「codex 参数写错了」的报错。
          </p>
        </div>
        <div className="field">
          <label>工作目录</label>
          <div className="row">
            <input
              className="text-input mono"
              value={cliCfg.cwd}
              onChange={(e) => setCliCfg({ ...cliCfg, cwd: e.target.value })}
              placeholder="留空 = 用户主目录"
              spellCheck={false}
            />
          </div>
        </div>
        <div className="field">
          <label>终端程序</label>
          <div className="row">
            <select
              className="select"
              value={cliCfg.terminal}
              onChange={(e) =>
                setCliCfg({ ...cliCfg, terminal: e.target.value as TerminalKind })
              }
            >
              {TERMINAL_KIND_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
          </div>
          <p className="hint">
            {TERMINAL_KIND_OPTIONS.find((o) => o.value === cliCfg.terminal)?.hint}
          </p>
        </div>
        <div className="field toggle-field">
          <label>打完直接回车执行</label>
          <div className="row">
            <span className="toggle-desc">
              <strong>默认关</strong>，而且应该保持关：命令只是前缀，回车等于把它当一条完整命令跑掉。
              只有「一键跑固定命令」那种用法才需要打开
            </span>
            <button
              className={`switch${cliCfg.auto_run ? " on" : ""}`}
              onClick={() => setCliCfg({ ...cliCfg, auto_run: !cliCfg.auto_run })}
              aria-pressed={cliCfg.auto_run}
              aria-label="打完直接回车执行"
            >
              <span className="switch-text">{cliCfg.auto_run ? "开" : "关"}</span>
              <span className="knob" />
            </button>
          </div>
          <p className="hint">这一项会跟上面的输入框一起保存，所以改完记得点「保存」。</p>
        </div>
        <div className="field">
          <div className="row">
            <button onClick={() => persistCli(cliCfg, true)}>保存</button>
            <button onClick={testCliLaunch}>试一下（立刻启动终端）</button>
          </div>
          {cliStatus && (
            <p className={`hint ${cliStatus.ok ? "ok" : "bad"}`}>{cliStatus.msg}</p>
          )}
        </div>
      </div>

      <div className="settings-section">
        <h3 className="settings-section-title">界面选项</h3>
        <div className="field toggle-field">
          <label>后台字幕浮层</label>
          <div className="row">
            <span className="toggle-desc">语音输入时在输入窗口下方实时显示识别文字</span>
            <button
              className={`switch${overlayEnabled ? " on" : ""}`}
              onClick={() => {
                const next = !overlayEnabled;
                setOverlayEnabled(next);
                invoke("set_overlay_enabled", { enabled: next }).catch(() => {});
              }}
              aria-pressed={overlayEnabled}
              aria-label="后台字幕浮层"
            >
              <span className="switch-text">{overlayEnabled ? "开" : "关"}</span>
              <span className="knob" />
            </button>
          </div>
          <p className="hint">后台运行 + 最小化到托盘后，跟随输入窗口的语音字幕依然可见。</p>
        </div>
      </div>

      {hint && <p className="hint bad">{hint}</p>}
    </div>
  );
}
