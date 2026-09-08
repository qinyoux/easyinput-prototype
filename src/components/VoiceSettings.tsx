import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import * as api from "../api";

const DEFAULT_URL = "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async";
const DEFAULT_RESOURCE = "volc.seedasr.sauc.duration";

export default function VoiceSettings() {
  const [asrUrl, setAsrUrl] = useState(DEFAULT_URL);
  const [resourceId, setResourceId] = useState(DEFAULT_RESOURCE);
  const [apiKey, setApiKey] = useState("");
  const [apiStatus, setApiStatus] = useState<{ ok: boolean; msg: string } | null>(null);
  const [pttSpec, setPttSpec] = useState("Ctrl+Shift+Space");
  const [pttStatus, setPttStatus] = useState<{ ok: boolean; msg: string } | null>(null);
  const [overlayEnabled, setOverlayEnabled] = useState(true);
  const [kbAudioOnline, setKbAudioOnline] = useState(false);
  const [micSource, setMicSource] = useState<"auto" | "pc" | "wifi" | "bluetooth">("auto");
  const [wifiSsid, setWifiSsid] = useState("");
  const [wifiPassword, setWifiPassword] = useState("");
  const [wifiConnected, setWifiConnected] = useState(false);
  const [bleEnabled, setBleEnabled] = useState(false);
  const [bleStatus, setBleStatus] = useState("未连接");
  const [llmBackend, setLlmBackend] = useState<"doubao" | "openai" | "anthropic" | "ollama" | "custom">("doubao");
  const [llmEndpoint, setLlmEndpoint] = useState("https://ark.cn-beijing.volces.com/api/v3/chat/completions");
  const [llmModel, setLlmModel] = useState("");
  const [llmApiKey, setLlmApiKey] = useState("");
  const [llmTimeout, setLlmTimeout] = useState(15);
  const [llmTestStatus, setLlmTestStatus] = useState<{ ok: boolean; msg: string } | null>(null);
  const [editPttSpec, setEditPttSpec] = useState("Ctrl+Shift+E");
  const [editPttStatus, setEditPttStatus] = useState<{ ok: boolean; msg: string } | null>(null);
  const [hint, setHint] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [showLlmApiKey, setShowLlmApiKey] = useState(false);
  const [showWifiPassword, setShowWifiPassword] = useState(false);

  useEffect(() => {
    let mounted = true;
    (async () => {
      try {
        const [u, r, k, ov] = await Promise.all([
          invoke<string>("get_asr_url"),
          invoke<string>("get_resource_id"),
          invoke<string>("get_api_key"),
          invoke<boolean>("get_overlay_enabled"),
        ]);
        if (!mounted) return;
        if (u) setAsrUrl(u);
        if (r) setResourceId(r);
        if (k) setApiKey(k);
        if (typeof ov === "boolean") setOverlayEnabled(ov);
      } catch { /* ignore */ }
      try {
        const cfg = await api.getLlmConfig();
        if (!mounted) return;
        setLlmBackend(cfg.backend);
        setLlmEndpoint(cfg.endpoint);
        setLlmModel(cfg.model);
        setLlmApiKey(cfg.api_key);
        setLlmTimeout(cfg.timeout_secs);
      } catch { /* ignore */ }
      try {
        const src = await api.getMicSource();
        if (mounted && src) setMicSource(src as typeof micSource);
      } catch { /* ignore */ }
      try {
        const wc = await api.getWifiConfig();
        if (mounted) {
          setWifiSsid(wc.ssid ?? "");
          setWifiPassword(wc.password ?? "");
        }
      } catch { /* ignore */ }
      try {
        const ble = await api.getBleEnabled();
        if (mounted) setBleEnabled(ble);
      } catch { /* ignore */ }
    })();
    return () => { mounted = false; };
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
    return () => { mounted = false; clearInterval(timer); };
  }, []);

  async function saveApiConfig() {
    try {
      await invoke("set_asr_url", { url: asrUrl });
      await invoke("set_resource_id", { id: resourceId });
      await invoke("set_api_key", { key: apiKey });
      setApiStatus({ ok: true, msg: "已保存" });
    } catch (e) {
      setApiStatus({ ok: false, msg: `失败: ${e}` });
    }
  }

  async function applyPtt() {
    try {
      const label = await invoke<string>("set_ptt_key", { spec: pttSpec });
      setPttStatus({ ok: true, msg: `已生效：${label}` });
    } catch (e) {
      setPttStatus({ ok: false, msg: `失败: ${e}` });
    }
  }

  function quickPtt(spec: string) {
    setPttSpec(spec);
    invoke<string>("set_ptt_key", { spec })
      .then((label) => setPttStatus({ ok: true, msg: `已生效：${label}` }))
      .catch((e) => setPttStatus({ ok: false, msg: `失败: ${e}` }));
  }

  return (
    <div className="settings-page">
      <div className="settings-section">
        <h3 className="settings-section-title">PTT 键（键盘快捷触发）</h3>
        <div className="keychips">
          <button onClick={() => quickPtt("Ctrl+Shift+Space")}>Ctrl+Shift+Space</button>
          <button onClick={() => quickPtt("F13")}>F13</button>
          <button onClick={() => quickPtt("RAlt")}>右 Alt</button>
        </div>
        <div className="row">
          <input value={pttSpec} onChange={(e) => setPttSpec(e.target.value)} placeholder="如 F13、RAlt" />
          <button onClick={applyPtt}>应用</button>
        </div>
        {pttStatus && <p className={`hint ${pttStatus.ok ? "ok" : "bad"}`}>{pttStatus.msg}</p>}
      </div>

      <div className="settings-section">
        <h3 className="settings-section-title">语音识别服务</h3>
        <div className="field">
          <label>服务地址</label>
          <div className="row">
            <input value={asrUrl} onChange={(e) => setAsrUrl(e.target.value)} placeholder="wss://..." spellCheck={false} />
          </div>
        </div>
        <div className="field">
          <label>模型 ID</label>
          <div className="row">
            <input value={resourceId} onChange={(e) => setResourceId(e.target.value)} placeholder="如 volc.seedasr.sauc.duration" spellCheck={false} />
          </div>
        </div>
        <div className="field">
          <label>API Key</label>
          <div className="row">
            <input type={showApiKey ? "text" : "password"} value={apiKey} onChange={(e) => setApiKey(e.target.value)} placeholder="粘贴 Key" spellCheck={false} />
            <button className="toggle-visibility" onClick={() => setShowApiKey(!showApiKey)} title={showApiKey ? "隐藏" : "显示"}>{showApiKey ? "🙈" : "👁"}</button>
            <button onClick={saveApiConfig}>保存</button>
          </div>
          {apiStatus && <p className={`hint ${apiStatus.ok ? "ok" : "bad"}`}>{apiStatus.msg}</p>}
        </div>
      </div>

      <div className="settings-section">
        <h3 className="settings-section-title">LLM 编辑配置</h3>
        <div className="field">
          <label>LLM 后端</label>
          <div className="row">
            <select className="select" value={llmBackend} onChange={(e) => setLlmBackend(e.target.value as typeof llmBackend)}>
              <option value="doubao">豆包（火山引擎）</option>
              <option value="openai">OpenAI</option>
              <option value="anthropic">Anthropic</option>
              <option value="ollama">Ollama（本地）</option>
              <option value="custom">自定义</option>
            </select>
          </div>
        </div>
        <div className="field">
          <label>LLM 端点</label>
          <div className="row">
            <input value={llmEndpoint} onChange={(e) => setLlmEndpoint(e.target.value)} placeholder="https://..." spellCheck={false} />
          </div>
        </div>
        <div className="field">
          <label>LLM 模型</label>
          <div className="row">
            <input value={llmModel} onChange={(e) => setLlmModel(e.target.value)} placeholder="如 ep-20240903141530-8x6s4" spellCheck={false} />
          </div>
        </div>
        <div className="field">
          <label>LLM API Key</label>
          <div className="row">
            <input type={showLlmApiKey ? "text" : "password"} value={llmApiKey} onChange={(e) => setLlmApiKey(e.target.value)} placeholder="粘贴 LLM Key" spellCheck={false} />
            <button className="toggle-visibility" onClick={() => setShowLlmApiKey(!showLlmApiKey)} title={showLlmApiKey ? "隐藏" : "显示"}>{showLlmApiKey ? "🙈" : "👁"}</button>
            <button onClick={async () => {
              try {
                await api.setLlmConfig({ backend: llmBackend, endpoint: llmEndpoint, model: llmModel, api_key: llmApiKey, timeout_secs: llmTimeout });
                setLlmTestStatus({ ok: true, msg: "已保存" });
              } catch (e) { setLlmTestStatus({ ok: false, msg: `保存失败: ${e}` }); }
            }}>保存</button>
            <button onClick={async () => {
              setLlmTestStatus({ ok: true, msg: "测试中…" });
              try {
                const msg = await api.testLlmConnection();
                setLlmTestStatus({ ok: true, msg });
              } catch (e) { setLlmTestStatus({ ok: false, msg: `失败: ${e}` }); }
            }}>测试</button>
          </div>
          {llmTestStatus && <p className={`hint ${llmTestStatus.ok ? "ok" : "bad"}`}>{llmTestStatus.msg}</p>}
        </div>
        <div className="field">
          <label>LLM 超时（秒）</label>
          <div className="row">
            <input type="number" value={llmTimeout} onChange={(e) => setLlmTimeout(Number(e.target.value) || 15)} min={5} max={60} />
          </div>
        </div>
      </div>

      <div className="settings-section">
        <h3 className="settings-section-title">语音编辑键</h3>
        <div className="keychips">
          <button onClick={() => { setEditPttSpec("Ctrl+Shift+E"); api.setEditPttKey("Ctrl+Shift+E").then((l) => setEditPttStatus({ ok: true, msg: `已生效：${l}` })).catch((e) => setEditPttStatus({ ok: false, msg: `失败: ${e}` })); }}>Ctrl+Shift+E</button>
          <button onClick={() => { setEditPttSpec("Ctrl+Shift+V"); api.setEditPttKey("Ctrl+Shift+V").then((l) => setEditPttStatus({ ok: true, msg: `已生效：${l}` })).catch((e) => setEditPttStatus({ ok: false, msg: `失败: ${e}` })); }}>Ctrl+Shift+V</button>
        </div>
        <div className="row">
          <input value={editPttSpec} onChange={(e) => setEditPttSpec(e.target.value)} placeholder="如 Ctrl+Shift+E" />
          <button onClick={async () => {
            try { const label = await api.setEditPttKey(editPttSpec); setEditPttStatus({ ok: true, msg: `已生效：${label}` }); }
            catch (e) { setEditPttStatus({ ok: false, msg: `失败: ${e}` }); }
          }}>应用</button>
        </div>
        {editPttStatus && <p className={`hint ${editPttStatus.ok ? "ok" : "bad"}`}>{editPttStatus.msg}</p>}
        <p className="hint">用法：选中文本 → 按编辑键说修改要求 → 再按编辑键确认 → LLM 编辑后自动替换。</p>
      </div>

      <div className="settings-section">
        <h3 className="settings-section-title">通信配置</h3>
        <div className="field">
          <label>麦克风来源</label>
          <div className="row">
            <select className="select" value={micSource} onChange={(e) => {
              const v = e.target.value as typeof micSource;
              setMicSource(v);
              api.setMicSource(v).catch((err) => setHint(`麦克风切换失败: ${err}`));
            }}>
              <option value="auto">自动（优先键盘，回退电脑）</option>
              <option value="pc">电脑本机麦克风</option>
              <option value="wifi">Wi-Fi 键盘麦克风</option>
              <option value="bluetooth">蓝牙键盘麦克风</option>
            </select>
          </div>
          <p className="hint">
            当前：{kbAudioOnline ? "键盘麦克风在线" : "键盘麦克风离线"}
            {micSource === "auto" && (kbAudioOnline ? "（使用键盘）" : "（使用电脑）")}
          </p>
        </div>

        <div className="field">
          <label>Wi-Fi 配置（开发板连接的无线网络）</label>
          <div className="row">
            <input value={wifiSsid} onChange={(e) => setWifiSsid(e.target.value)} placeholder="Wi-Fi SSID" />
            <input type={showWifiPassword ? "text" : "password"} value={wifiPassword} onChange={(e) => setWifiPassword(e.target.value)} placeholder="密码" />
            <button className="toggle-visibility" onClick={() => setShowWifiPassword(!showWifiPassword)} title={showWifiPassword ? "隐藏" : "显示"}>{showWifiPassword ? "🙈" : "👁"}</button>
          </div>
          <div className="row">
            <span className={`kb-audio-badge ${wifiConnected ? "online" : "offline"}`}>
              {wifiConnected ? "Wi-Fi 已连接" : "Wi-Fi 未连接"}
            </span>
            <button onClick={async () => {
              try {
                await api.setWifiConfig(wifiSsid, wifiPassword);
                await api.wifiConnect(wifiSsid, wifiPassword);
                setWifiConnected(true);
              }
              catch (e) { setWifiConnected(false); setHint(`Wi-Fi 连接失败: ${e}`); }
            }}>连接</button>
            <button onClick={async () => {
              try { await api.wifiDisconnect(); setWifiConnected(false); }
              catch (e) { setHint(`Wi-Fi 断开失败: ${e}`); }
            }}>断开</button>
          </div>
          <p className="hint">配置下发到开发板后，开发板将连接此 Wi-Fi 并通过 UDP 发送音频。SSID 和密码会自动保存到配置文件，下次启动自动回填。</p>
        </div>

        <div className="field toggle-field">
          <label>蓝牙</label>
          <div className="row">
            <span className="toggle-desc">启用蓝牙通信通道（按键事件经 BLE HID 传输）</span>
            <button
              className={`switch${bleEnabled ? " on" : ""}`}
              onClick={async () => {
                const next = !bleEnabled;
                setBleEnabled(next);
                try {
                  if (next) await api.bleEnable();
                  else await api.bleDisable();
                  await api.setBleEnabled(next);
                } catch (e) { setHint(`蓝牙开关失败: ${e}`); }
              }}
              aria-pressed={bleEnabled}
            >
              <span className="knob" />
            </button>
          </div>
          <div className="row">
            <span className={`kb-audio-badge ${bleEnabled ? "online" : "offline"}`}>
              {bleEnabled ? `蓝牙已开启 · ${bleStatus}` : "蓝牙已关闭"}
            </span>
            {bleEnabled && <button onClick={async () => {
              setBleStatus("配对中…");
              try {
                await api.blePair();
                for (let i = 0; i < 10; i++) {
                  await new Promise(r => setTimeout(r, 2000));
                  try {
                    const s = await api.bleStatus();
                    setBleStatus(s);
                    if (s.includes("已连接") || s.includes("配对成功")) break;
                  } catch { /* ignore */ }
                }
              }
              catch (e) { setBleStatus(`配对失败: ${e}`); }
            }}>配对</button>}
          </div>
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
            >
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
