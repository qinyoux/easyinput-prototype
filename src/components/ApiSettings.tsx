// 「模型配置」页签：所有"连到外部模型/服务"的配置集中在这一页。
//
// ## 为什么单独开一页（2026-09-22 第一轮）
//
// 过去这些配置散在两处：「语音识别 / 语音编辑」在「设置」，「语音合成 + 各执行器密钥」
// 在「设置 → Agent 任务台」。后果有两个：
//
// 1. **换一个模型服务商要跨页找**：端点在 A 页、Key 在 B 页，还得在一堆行为开关里翻；
// 2. **强相关的三件套（端点 / 模型 / 密钥）被行为类配置挤开**，改一次要来回滚。
//
// ## 第二轮：执行器也并了进来（钦友提议"统一模型配置入口"）
//
// 评估结论与落地边界写在 `DriverEditor.tsx` 的模块头，一句话概括：
// **入口统一，字段按形态分流。** API 型执行器确实就是"一个模型"，默认只露 4 个字段；
// CLI 型是"一个能改你磁盘的进程"，它的完成判定 / 会话续接留在同一块的「高级」里 ——
// 拍平成一个"模型"表单，代价是把"任务到底做完了没有"这个唯一不能猜的东西藏起来。
//
// 本页因此成为执行器配置的**唯一编辑入口**（「任务台」页不再编辑执行器）。
//
// ## 归属判据（唯一一条，不再逐项讨论）
//
// **一个配置项属于本页，当且仅当它满足其一：**
// **① 决定"怎么连到一个外部模型服务"（端点 / 密钥 / 模型 / 资源 ID / 采样率）；**
// **② 是该服务的触发入口（按键）；**
// **③ 它就是被调度的那一方本身（执行器）。**
//
// 反向的例子，都不进本页、留在「设置」：
// Everything 是本地可执行文件路径；Wi-Fi / 麦克风是设备通信；粘贴方式 / 字幕浮层是行为开关。
//
// ## 本页写两个存储（所以保存按钮也按存储分开）
//
// | 段 | 存储 |
// |---|---|
// | §1 语音识别 / §2 语音编辑 | `%APPDATA%\com.easyop.app\config.json` |
// | §3 语音合成 / §4 执行器 | `%APPDATA%\com.easyop.app\agent_desk.json` |
//
// 不合成一个"保存全部"按钮：那是两个不同的文件、两条不同的后端命令，
// 存到一半失败时你会分不清哪半边生效了 —— 这正是本项目最想避免的那类静默状态。
//
// ⚠️ 密钥（ASR / LLM / TTS / 各执行器）只在本页输入与回填。按键类是**立即生效**的
// （不参与表单保存），所以每一段都会写明"这一项改完要不要点保存"。
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import * as api from "../api";
import DriverEditor from "./DriverEditor";
import type {
  DeskConfig,
  DriverConfig,
  DriverKind,
  LlmBackend,
  TtsConfig,
  TtsProvider,
} from "../types";

const DEFAULT_ASR_URL = "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async";
const DEFAULT_ASR_RESOURCE = "volc.seedasr.sauc.duration";

/** 交付采样率常量——与固件 `kSpeakerPlaybackSampleRate` 一致，不要改。 */
const FIRMWARE_SAMPLE_RATE = 48000;

/** 换服务商时要一起换的字段：音色与端点/资源在两个协议下互不通用。 */
const PROVIDER_DEFAULTS: Record<
  TtsProvider,
  { endpoint: string; voice: string; endpointHint: string; voiceHint: string }
> = {
  volc: {
    endpoint: "https://openspeech.bytedance.com/api/v3/tts/unidirectional",
    voice: "zh_female_cancan_mars_bigtts",
    // 传输由端点前缀决定（后端按 scheme 自动选），所以这里把两种都写出来 ——
    // 用户换了 wss 端点却不知道"协议也跟着换了"，是最容易卡住的一步。
    endpointHint:
      "按前缀自动选传输：`https://` = v3 单向流式（HTTP chunked + NDJSON）；" +
      "`wss://` = 单向流式（WebSocket 二进制帧）",
    voiceHint:
      "豆包 speaker ID，**必须与 resource 同版本**。" +
      "配 seed-tts-1.0：zh_female_cancan_mars_bigtts（1.0 音色）；" +
      "配 seed-tts-2.0：zh_female_vv_uranus_bigtts / zh_female_cancan_uranus_bigtts / " +
      "zh_male_yuanboxiaoshu_uranus_bigtts（2.0 音色，实测可用）。",
  },
  bailian: {
    endpoint:
      "https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation",
    voice: "Cherry",
    endpointHint:
      "DashScope 原生协议（非 OpenAI 兼容），只有 HTTP —— 填 wss:// 会被后端明确拒绝",
    voiceHint: "百炼 voice 名，如 Cherry / Ethan / Serena",
  },
};

interface Props {
  notify: (kind: "ok" | "err", text: string) => void;
}

type Status = { ok: boolean; msg: string } | null;

const LLM_BACKENDS: { value: LlmBackend; label: string }[] = [
  { value: "doubao", label: "豆包（火山引擎）" },
  { value: "openai", label: "OpenAI" },
  { value: "anthropic", label: "Anthropic" },
  { value: "ollama", label: "Ollama（本地）" },
  { value: "custom", label: "自定义" },
];

export default function ApiSettings({ notify }: Props) {
  // ---- §1 语音识别（ASR）：config.json ----
  const [asrUrl, setAsrUrl] = useState(DEFAULT_ASR_URL);
  const [resourceId, setResourceId] = useState(DEFAULT_ASR_RESOURCE);
  const [asrKey, setAsrKey] = useState("");
  const [showAsrKey, setShowAsrKey] = useState(false);
  const [asrStatus, setAsrStatus] = useState<Status>(null);
  // 语音识别的入口按键（PTT）——与 ASR 是"同一个功能的两个面"，所以放在同一段。
  const [pttSpec, setPttSpec] = useState("Ctrl+Shift+Space");
  const [pttStatus, setPttStatus] = useState<Status>(null);

  // ---- §2 语音编辑（LLM）：config.json ----
  const [llmBackend, setLlmBackend] = useState<LlmBackend>("doubao");
  const [llmEndpoint, setLlmEndpoint] = useState(
    "https://ark.cn-beijing.volces.com/api/v3/chat/completions",
  );
  const [llmModel, setLlmModel] = useState("");
  const [llmKey, setLlmKey] = useState("");
  const [llmTimeout, setLlmTimeout] = useState(15);
  const [showLlmKey, setShowLlmKey] = useState(false);
  const [llmStatus, setLlmStatus] = useState<Status>(null);
  const [editPttSpec, setEditPttSpec] = useState("Ctrl+Shift+E");
  const [editPttStatus, setEditPttStatus] = useState<Status>(null);

  // ---- §3 语音合成 + §4 执行器（API）：同一份 agent_desk.json ----
  const [cfg, setCfg] = useState<DeskConfig | null>(null);
  const [cfgError, setCfgError] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [showTtsKey, setShowTtsKey] = useState(false);
  const [ttsText, setTtsText] = useState("任务台已接通，这是一段语音合成测试。");
  const [ttsResult, setTtsResult] = useState<string | null>(null);
  const [ttsBusy, setTtsBusy] = useState(false);

  useEffect(() => {
    let mounted = true;
    (async () => {
      try {
        const [u, r, k] = await Promise.all([
          invoke<string>("get_asr_url"),
          invoke<string>("get_resource_id"),
          invoke<string>("get_api_key"),
        ]);
        if (!mounted) return;
        if (u) setAsrUrl(u);
        if (r) setResourceId(r);
        if (k) setAsrKey(k);
      } catch {
        /* 读不到就保持默认值，保存时会覆盖 */
      }
      try {
        const c = await api.getLlmConfig();
        if (!mounted) return;
        setLlmBackend(c.backend);
        setLlmEndpoint(c.endpoint);
        setLlmModel(c.model);
        setLlmKey(c.api_key);
        setLlmTimeout(c.timeout_secs);
      } catch {
        /* 同上 */
      }
    })();
    return () => {
      mounted = false;
    };
  }, []);

  useEffect(() => {
    let mounted = true;
    (async () => {
      try {
        const env = await api.deskGetConfig();
        if (!mounted) return;
        if (env.ok && env.data) {
          setCfg(env.data);
          setDirty(false);
        } else {
          setCfgError(env.message ?? env.code);
        }
      } catch (e) {
        if (mounted) setCfgError(String(e));
      }
    })();
    return () => {
      mounted = false;
    };
  }, []);

  // ---- §1 语音识别 ----

  async function saveAsr() {
    try {
      await invoke("set_asr_url", { url: asrUrl });
      await invoke("set_resource_id", { id: resourceId });
      await invoke("set_api_key", { key: asrKey });
      setAsrStatus({ ok: true, msg: "已保存到 config.json" });
    } catch (e) {
      setAsrStatus({ ok: false, msg: `失败: ${e}` });
    }
  }

  async function applyPtt(spec: string) {
    try {
      const label = await invoke<string>("set_ptt_key", { spec });
      setPttStatus({ ok: true, msg: `已生效：${label}` });
    } catch (e) {
      setPttStatus({ ok: false, msg: `失败: ${e}` });
    }
  }

  function quickPtt(spec: string) {
    setPttSpec(spec);
    applyPtt(spec);
  }

  // ---- §2 语音编辑 ----

  async function saveLlm() {
    try {
      await api.setLlmConfig({
        backend: llmBackend,
        endpoint: llmEndpoint,
        model: llmModel,
        api_key: llmKey,
        timeout_secs: llmTimeout,
      });
      setLlmStatus({ ok: true, msg: "已保存到 config.json" });
    } catch (e) {
      setLlmStatus({ ok: false, msg: `保存失败: ${e}` });
    }
  }

  /** 测试的是**盘上的配置**，所以先存再测 —— 否则测的是旧 Key，白得一个失败。 */
  async function testLlm() {
    setLlmStatus({ ok: true, msg: "测试中…" });
    try {
      await saveLlm();
      const msg = await api.testLlmConnection();
      setLlmStatus({ ok: true, msg });
    } catch (e) {
      setLlmStatus({ ok: false, msg: `失败: ${e}` });
    }
  }

  async function applyEditPtt(spec: string) {
    try {
      const label = await api.setEditPttKey(spec);
      setEditPttStatus({ ok: true, msg: `已生效：${label}` });
    } catch (e) {
      setEditPttStatus({ ok: false, msg: `失败: ${e}` });
    }
  }

  function quickEditPtt(spec: string) {
    setEditPttSpec(spec);
    applyEditPtt(spec);
  }

  // ---- §3 / §4 任务台侧（TTS + 执行器）----

  function patchTts(p: Partial<TtsConfig>) {
    setCfg((c) => (c ? { ...c, tts: { ...c.tts, ...p } } : c));
    setDirty(true);
  }

  /**
   * 换服务商：**端点与音色一并切到该 provider 的默认值**。
   *
   * 不这么做的话，从豆包切到百炼会留下豆包端点 + 豆包音色 ID，表现为
   * "点了试合成报了一串看不懂的上游错误"——两个协议的音色命名完全不通用，
   * 留着只会制造一个必然失败的组合。用户真要用自建网关，改完端点再存即可。
   */
  function switchTtsProvider(provider: TtsProvider) {
    const d = PROVIDER_DEFAULTS[provider];
    patchTts({ provider, endpoint: d.endpoint, voice: d.voice });
  }

  function patchDriver(id: string, p: Partial<DriverConfig>) {
    setCfg((c) =>
      c ? { ...c, drivers: c.drivers.map((x) => (x.id === id ? { ...x, ...p } : x)) } : c,
    );
    setDirty(true);
  }

  /**
   * 按需新增一个执行器 —— 这是"任意模型随时加"的落点，**不需要改 Rust 代码**。
   *
   * # 两个必须在这里做对的细节
   *
   * 1. **id 要唯一且稳定**。它是槽位绑定的引用键，`normalize()` 补预置时也按它比。
   *    这里用 `model-N` / `cli-N` 自增，跳过已占用的号。
   * 2. **完成信号必须与形态匹配**。后端的 `validate_driver` 对
   *    「Api + 进程退出码」是**硬错误**（Api 没有子进程，永远判不出完成），
   *    新建时若用 `CompletionSignal::default()`（`process_exit_zero`）就会
   *    造出一个开箱即红、且用户看不出为什么的执行器。
   */
  function addDriver(kind: DriverKind) {
    if (!cfg) return;
    const base = kind === "api" ? "model" : "cli";
    let n = 1;
    while (cfg.drivers.some((d) => d.id === `${base}-${n}`)) n += 1;
    const id = `${base}-${n}`;
    const fresh: DriverConfig = {
      id,
      label: kind === "api" ? `新模型 ${n}` : `新 CLI 执行器 ${n}`,
      kind,
      // 默认**不启用**：新条目往往还没填完，先启用只会让任务台多一条"未配置完整"的阻塞。
      enabled: false,
      endpoint: kind === "api" ? "https://" : "",
      model: "",
      api_key: "",
      command: "",
      // CLI 预填 `{prompt}`：忘了它后端会直接拒绝调度（任务文本传不进去）。
      args: kind === "cli" ? ["{prompt}"] : [],
      env: [],
      cwd: "",
      timeout_ms: kind === "cli" ? 900_000 : 0,
      completion: {
        kind: kind === "api" ? "http_stream_end" : "process_exit_zero",
        pattern: "",
        evidence_cmd: "",
      },
      continuation: null,
      capabilities: ["text"],
      // 产物发现：**默认行为与后端一致**（后端是 `default_true`）。
      // ⚠️ 这里写 `true` 不是随手填的：写成 `false` 会让"新建的执行器"与
      // "老配置补默认值"两条路得到**不同的默认**，而界面上看不出区别。
      artifact_glob: "",
      artifact_open: true,
      note: "",
    };
    setCfg({ ...cfg, drivers: [...cfg.drivers, fresh] });
    setDirty(true);
  }

  /**
   * 删除执行器，**并把引用它的槽位绑定一起清空**。
   *
   * 不清理的话，坏配置会一直躺在磁盘上，直到用户按下那个键才炸
   * （`desk_dispatch_task` 会报"槽位 S1 绑定的执行器 xxx 不存在"）——
   * 那时离"我删了个没用的执行器"已经很远了，很难联想到一起。
   * 清空而不是"顺手改绑到另一个"，理由同后端 `retire_drivers`：猜错就是把任务
   * 发给了错误的 Agent，代价远高于"这个键暂时没反应"。
   */
  function removeDriver(id: string) {
    if (!cfg) return;
    const target = cfg.drivers.find((d) => d.id === id);
    setCfg({
      ...cfg,
      drivers: cfg.drivers.filter((d) => d.id !== id),
      slots: cfg.slots.map((s) => (s.mode === "exact" && s.driver === id ? { mode: "empty" } : s)),
    });
    setDirty(true);
    notify("ok", `已移除「${target?.label ?? id}」，记得点下面的保存`);
  }

  async function saveDesk() {
    if (!cfg) return;
    setBusy(true);
    try {
      const env = await api.deskSetConfig(cfg);
      if (env.ok && env.data) {
        setCfg(env.data);
        setDirty(false);
        notify("ok", "语音合成与执行器配置已保存");
      } else {
        notify("err", env.message ?? env.code);
      }
    } catch (e) {
      notify("err", String(e));
    } finally {
      setBusy(false);
    }
  }

  async function runTtsTest() {
    setTtsBusy(true);
    setTtsResult("合成中…");
    try {
      // 先保存再试合成：否则用的是盘上的旧 key，会出现"刚填的 key 说未配置"。
      if (dirty) await saveDesk();
      const env = await api.deskTtsTest(ttsText);
      if (env.ok && env.data) {
        const d = env.data;
        setTtsResult(
          `成功：${d.sample_rate} Hz · ${d.samples} 采样 · ${d.duration_ms} ms` +
            (d.characters != null ? ` · 计费 ${d.characters} 字符` : "") +
            `\n已落盘：${d.wav_path}`,
        );
        notify("ok", "TTS 合成成功，已落盘 wav");
      } else {
        const msg = `[${env.code}] ${env.message ?? "未知错误"}`;
        setTtsResult(`失败：${msg}`);
        notify("err", msg);
      }
    } catch (e) {
      setTtsResult(`失败：${e}`);
      notify("err", String(e));
    } finally {
      setTtsBusy(false);
    }
  }

  return (
    <div className="settings-page">
      {/* ---------- 页首：本页边界 ---------- */}
      <div className="settings-section">
        <h3 className="settings-section-title">这一页管什么</h3>
        <p className="hint">
          放三类东西：<strong>① 连到外部模型服务的配置</strong>（端点 / 密钥 / 模型 / 资源 ID）、
          <strong>② 这些功能的触发按键</strong>、<strong>③ 被调度的一方本身</strong>（执行器）。
          <br />
          反向的都不在这里 —— Everything 是本地程序路径、Wi-Fi 与麦克风是设备通信、
          粘贴方式与字幕浮层是行为开关，它们留在「<strong>设置</strong>」页。
        </p>
        <p className="hint">
          <strong>两个存储、两个保存按钮</strong>：
          §1 语音识别 / §2 语音编辑 写入 <code>config.json</code>；
          §3 语音合成 / §4 执行器 写入 <code>agent_desk.json</code>。
          <br />
          ⚠️ <strong>按键类（触发键 / 编辑键）是立即生效</strong>的，不参与"保存"；
          <strong>执行器的改动（含新增 / 删除）也必须点保存</strong>才算落盘。
        </p>
      </div>

      {/* ---------- §1 语音识别 ---------- */}
      <div className="settings-section">
        <h3 className="settings-section-title">① 语音识别（ASR）</h3>
        <p className="hint">
          语音输入的识别通道（火山引擎流式 ASR）。与它配套的触发键放在紧下面 ——
          识别配置和"怎么开始录"是同一件事的两面。
        </p>
        <div className="field">
          <label>服务地址</label>
          <div className="row">
            <input
              value={asrUrl}
              onChange={(e) => setAsrUrl(e.target.value)}
              placeholder="wss://..."
              spellCheck={false}
            />
          </div>
        </div>
        <div className="field">
          <label>模型 ID（X-Api-Resource-Id）</label>
          <div className="row">
            <input
              value={resourceId}
              onChange={(e) => setResourceId(e.target.value)}
              placeholder="如 volc.seedasr.sauc.duration"
              spellCheck={false}
            />
          </div>
        </div>
        <div className="field">
          <label>API Key</label>
          <div className="row">
            <input
              type={showAsrKey ? "text" : "password"}
              value={asrKey}
              onChange={(e) => setAsrKey(e.target.value)}
              placeholder="粘贴 Key"
              spellCheck={false}
            />
            <button
              className="toggle-visibility"
              onClick={() => setShowAsrKey(!showAsrKey)}
              title={showAsrKey ? "隐藏" : "显示"}
              aria-label={showAsrKey ? "隐藏识别 API Key" : "显示识别 API Key"}
            >
              {showAsrKey ? "🙈" : "👁"}
            </button>
          </div>
        </div>
        <div className="field">
          <div className="row">
            <button onClick={saveAsr}>保存语音识别配置</button>
          </div>
          {asrStatus && (
            <p className={`hint ${asrStatus.ok ? "ok" : "bad"}`}>{asrStatus.msg}</p>
          )}
        </div>

        <div className="field">
          <label>识别触发键（PTT：按住说话）</label>
          <div className="keychips">
            <button onClick={() => quickPtt("Ctrl+Shift+Space")}>Ctrl+Shift+Space</button>
            <button onClick={() => quickPtt("F13")}>F13</button>
            <button onClick={() => quickPtt("RAlt")}>右 Alt</button>
          </div>
          <div className="row">
            <input
              value={pttSpec}
              onChange={(e) => setPttSpec(e.target.value)}
              placeholder="如 F13、RAlt"
            />
            <button onClick={() => applyPtt(pttSpec)}>应用</button>
          </div>
          <p className="hint">点「应用」或上面任一快捷键<strong>立刻生效</strong>，不需要保存。</p>
          {pttStatus && (
            <p className={`hint ${pttStatus.ok ? "ok" : "bad"}`}>{pttStatus.msg}</p>
          )}
        </div>
      </div>

      {/* ---------- §2 语音编辑 ---------- */}
      <div className="settings-section">
        <h3 className="settings-section-title">② 语音编辑（LLM）</h3>
        <p className="hint">
          轻量云端小模型，用于提示词优化、文本拓展、快速响应。选中文本 → 按编辑键说修改要求
          → 再按编辑键确认 → LLM 编辑后自动替换。
          <br />
          它与「语音合成」是<strong>两套独立配置</strong>：一个把话说成文字、一个把文字说成声音，
          服务商和密钥都各填各的。
        </p>
        <div className="field">
          <label>LLM 后端</label>
          <div className="row">
            <select
              className="select"
              value={llmBackend}
              onChange={(e) => setLlmBackend(e.target.value as LlmBackend)}
            >
              {LLM_BACKENDS.map((b) => (
                <option key={b.value} value={b.value}>
                  {b.label}
                </option>
              ))}
            </select>
          </div>
        </div>
        <div className="field">
          <label>LLM 端点</label>
          <div className="row">
            <input
              value={llmEndpoint}
              onChange={(e) => setLlmEndpoint(e.target.value)}
              placeholder="https://..."
              spellCheck={false}
            />
          </div>
          <p className="hint">
            要写到 <code>/chat/completions</code> 这一级。只填到 <code>/v1</code> 或
            <code>/api/v3</code> 会在调用时返回 404 / <code>Invalid URL</code>，
            而这个错在界面上完全看不出来。
          </p>
        </div>
        <div className="field">
          <label>LLM 模型</label>
          <div className="row">
            <input
              value={llmModel}
              onChange={(e) => setLlmModel(e.target.value)}
              placeholder="如 doubao-seed-2-0-mini-260428"
              spellCheck={false}
            />
          </div>
        </div>
        <div className="field">
          <label>LLM API Key</label>
          <div className="row">
            <input
              type={showLlmKey ? "text" : "password"}
              value={llmKey}
              onChange={(e) => setLlmKey(e.target.value)}
              placeholder="粘贴 LLM Key"
              spellCheck={false}
            />
            <button
              className="toggle-visibility"
              onClick={() => setShowLlmKey(!showLlmKey)}
              title={showLlmKey ? "隐藏" : "显示"}
              aria-label={showLlmKey ? "隐藏大模型 API Key" : "显示大模型 API Key"}
            >
              {showLlmKey ? "🙈" : "👁"}
            </button>
          </div>
        </div>
        <div className="field">
          <label>LLM 超时（秒）</label>
          <div className="row">
            <input
              type="number"
              value={llmTimeout}
              onChange={(e) => setLlmTimeout(Number(e.target.value) || 15)}
              min={5}
              max={60}
            />
          </div>
        </div>
        <div className="field">
          <div className="row">
            <button onClick={saveLlm}>保存语音编辑配置</button>
            <button onClick={testLlm}>测试连接</button>
          </div>
          <p className="hint">「测试连接」会<strong>先把当前值存盘再发起一次真实调用</strong>，测的就是盘上那份。</p>
          {llmStatus && (
            <p className={`hint ${llmStatus.ok ? "ok" : "bad"}`}>{llmStatus.msg}</p>
          )}
        </div>

        <div className="field">
          <label>语音编辑键</label>
          <div className="keychips">
            <button onClick={() => quickEditPtt("Ctrl+Shift+E")}>Ctrl+Shift+E</button>
            <button onClick={() => quickEditPtt("Ctrl+Shift+V")}>Ctrl+Shift+V</button>
          </div>
          <div className="row">
            <input
              value={editPttSpec}
              onChange={(e) => setEditPttSpec(e.target.value)}
              placeholder="如 Ctrl+Shift+E"
            />
            <button onClick={() => applyEditPtt(editPttSpec)}>应用</button>
          </div>
          <p className="hint">同样是<strong>立即生效</strong>，不需要保存。</p>
          {editPttStatus && (
            <p className={`hint ${editPttStatus.ok ? "ok" : "bad"}`}>{editPttStatus.msg}</p>
          )}
        </div>
      </div>

      {/* ---------- §3 语音合成 ---------- */}
      {!cfg ? (
        <div className="settings-section">
          <h3 className="settings-section-title">③ 语音合成（TTS）· ④ 执行器</h3>
          <p className={`hint ${cfgError ? "bad" : ""}`}>
            {cfgError ? `读取任务台配置失败：${cfgError}` : "读取任务台配置中…"}
          </p>
        </div>
      ) : (
        <>
          <div className="settings-section">
            <h3 className="settings-section-title">③ 语音合成（TTS）</h3>
            <p className="hint">
              用于把任务总结念出来。默认走<strong>豆包语音合成大模型</strong>（火山引擎 v3
              单向流式）。
              <br />
              <strong>两种传输按端点前缀自动选</strong>：<code>https://</code> 走 HTTP
              chunked + NDJSON（一次 POST 读整条流），<code>wss://</code> 走 WebSocket
              二进制帧（发一帧、收一串音频帧）。同一份配置、同一个服务，只是"这句话怎么送出去"不同。
              <br />
              豆包两条路都直接要 48 kHz，所以<strong>不需要重采样</strong>；百炼只支持 HTTP，
              输出 24 kHz，由后端转成 48 kHz。
            </p>
            <div className="field">
              <label>服务商</label>
              <div className="row">
                <select
                  className="select"
                  value={cfg.tts.provider}
                  onChange={(e) => switchTtsProvider(e.target.value as TtsProvider)}
                >
                  <option value="volc">豆包语音合成大模型（火山引擎）</option>
                  <option value="bailian">阿里云百炼</option>
                </select>
              </div>
            </div>
            <div className="field">
              <label>端点</label>
              <div className="row">
                <input
                  value={cfg.tts.endpoint}
                  onChange={(e) => patchTts({ endpoint: e.target.value })}
                  spellCheck={false}
                />
              </div>
              <p className="hint">
                {PROVIDER_DEFAULTS[cfg.tts.provider].endpointHint}。留空 = 用官方默认端点。
              </p>
            </div>

            {cfg.tts.provider === "volc" ? (
              <>
                <div className="field">
                  <label>资源 ID（X-Api-Resource-Id）</label>
                  <div className="row">
                    <input
                      value={cfg.tts.resource_id}
                      onChange={(e) => patchTts({ resource_id: e.target.value })}
                      placeholder="seed-tts-1.0"
                      spellCheck={false}
                    />
                  </div>
                  <p className="hint">
                    决定可用音色与计费条目，因此<strong>也决定端点能不能连上</strong> ——
                    账号未授权的 resource_id 会在 <strong>WebSocket 握手阶段就被 403 拒掉</strong>。
                    <strong>音色与 resource 必须同版本</strong>：
                    `seed-tts-1.0` 只认 1.0 音色，`seed-tts-2.0` 只认 2.0 音色，混用会在建连之后
                    返回 `55000000 resource ID is mismatched`。留空会明确报错，不替你猜。
                  </p>
                </div>
                <div className="field">
                  <label>音色（speaker）</label>
                  <div className="row">
                    <input
                      value={cfg.tts.voice}
                      onChange={(e) => patchTts({ voice: e.target.value })}
                      placeholder={PROVIDER_DEFAULTS.volc.voice}
                      spellCheck={false}
                    />
                  </div>
                  <p className="hint">{PROVIDER_DEFAULTS.volc.voiceHint}</p>
                </div>
                <div className="field">
                  <label>明确语种（explicit_language）</label>
                  <div className="row">
                    <input
                      value={cfg.tts.explicit_language}
                      onChange={(e) => patchTts({ explicit_language: e.target.value })}
                      placeholder="zh-cn"
                      spellCheck={false}
                    />
                  </div>
                  <p className="hint">
                    中文为主填 `zh-cn`（支持中英混），仅英文填 `en`。<strong>留空则不传</strong>
                    ，由服务端按中英混处理。
                  </p>
                </div>
              </>
            ) : (
              <>
                <div className="field">
                  <label>模型</label>
                  <div className="row">
                    <input
                      value={cfg.tts.model}
                      onChange={(e) => patchTts({ model: e.target.value })}
                      placeholder="qwen3-tts-flash"
                      spellCheck={false}
                    />
                  </div>
                </div>
                <div className="field">
                  <label>音色（voice）</label>
                  <div className="row">
                    <input
                      value={cfg.tts.voice}
                      onChange={(e) => patchTts({ voice: e.target.value })}
                      placeholder={PROVIDER_DEFAULTS.bailian.voice}
                      spellCheck={false}
                    />
                  </div>
                  <p className="hint">{PROVIDER_DEFAULTS.bailian.voiceHint}</p>
                </div>
                <div className="field">
                  <label>语种（language_type）</label>
                  <div className="row">
                    <input
                      value={cfg.tts.language_type}
                      onChange={(e) => patchTts({ language_type: e.target.value })}
                      placeholder="Chinese"
                      spellCheck={false}
                    />
                  </div>
                  <p className="hint">单一语种时显式指定（Chinese / English），效果优于 Auto。</p>
                </div>
              </>
            )}

            <div className="field">
              <label>API Key</label>
              <div className="row">
                <input
                  type={showTtsKey ? "text" : "password"}
                  value={cfg.tts.api_key}
                  onChange={(e) => patchTts({ api_key: e.target.value })}
                  placeholder={
                    cfg.tts.provider === "volc"
                      ? "粘贴火山引擎控制台的 API Key（X-Api-Key）"
                      : "粘贴百炼 API Key"
                  }
                  spellCheck={false}
                />
                <button
                  className="toggle-visibility"
                  onClick={() => setShowTtsKey(!showTtsKey)}
                  title={showTtsKey ? "隐藏" : "显示"}
                  aria-label={showTtsKey ? "隐藏 TTS API Key" : "显示 TTS API Key"}
                >
                  {showTtsKey ? "🙈" : "👁"}
                </button>
              </div>
            </div>
            <div className="field">
              <label>交付采样率 / 文本上限</label>
              <div className="row">
                <input
                  type="number"
                  value={cfg.tts.target_sample_rate}
                  onChange={(e) => patchTts({ target_sample_rate: Number(e.target.value) || 0 })}
                />
                <input
                  type="number"
                  value={cfg.tts.max_chars}
                  onChange={(e) => patchTts({ max_chars: Number(e.target.value) || 0 })}
                />
              </div>
              <p
                className={`hint ${cfg.tts.target_sample_rate === FIRMWARE_SAMPLE_RATE ? "" : "bad"}`}
              >
                采样率必须是 {FIRMWARE_SAMPLE_RATE} Hz —— 固件 `max98357a_supports_lrclk()` 不接受
                24 kHz。豆包直接按 {FIRMWARE_SAMPLE_RATE} Hz 请求；百炼输出的 24 kHz
                由后端重采样。改这个值必须同步改固件并重新烧录。
              </p>
              <p className="hint">
                文本上限是<strong>客户端自保</strong>：豆包服务端实测接受 1500 字，
                真正卡时长的是固件「单条音频不超过 150 秒」——总结侧按 180 字左右生成足够。
              </p>
            </div>

            <div className="field">
              <label>试合成</label>
              <div className="row">
                <input
                  value={ttsText}
                  onChange={(e) => setTtsText(e.target.value)}
                  placeholder="要合成的文本"
                />
                <button onClick={runTtsTest} disabled={ttsBusy}>
                  {ttsBusy ? "合成中…" : "试合成"}
                </button>
              </div>
              {ttsResult && (
                <p
                  className={`hint ${ttsResult.startsWith("成功") ? "ok" : "bad"}`}
                  style={{ whiteSpace: "pre-wrap" }}
                >
                  {ttsResult}
                </p>
              )}
            </div>
          </div>

          {/* ---------- §4 执行器（模型 + CLI Agent）---------- */}
          <div className="settings-section">
            <h3 className="settings-section-title">④ 执行器（被调度的一方）</h3>
            <p className="hint">
              「任务台」派发任务时挑的就是这里的东西。<strong>这是执行器唯一的编辑入口</strong> ——
              命令 / 参数 / 超时 / 完成判定 / 能力声明全在它自己的展开块里，
              不用再去「任务台」页翻第二遍。
              <br />
              <strong>按需添加</strong>：右上角两个按钮分别加"直连模型"和"CLI 执行器"，
              <strong>不需要改代码</strong>。分组不是分类癖 —— 这两类东西的风险完全不同：
              API 型是<strong>一次 HTTP 往返</strong>（快、便宜、只出文本），
              CLI 型是<strong>一个能改你磁盘的进程</strong>（慢、贵、有副作用）。
              <br />
              所以<strong>字段按形态分流</strong>：API 型只给你 4 个输入框；
              CLI 型的完成判定 / 会话续接收在「高级选项」里，默认收起但不删 ——
              那几项决定"任务到底算不算做完"，藏丢了就会静默判错。
              <br />
              ⚠️ 预置执行器只能<strong>停用</strong>不能删除（后端会在启动时补回缺失的预置，
              删了会"自己回来"）；自建的不受此限。
            </p>
            <DriverEditor
              drivers={cfg.drivers}
              slots={cfg.slots}
              onPatch={patchDriver}
              onAdd={addDriver}
              onRemove={removeDriver}
              flush={saveDesk}
              notify={notify}
            />
          </div>

          <div className="settings-save-bar">
            <span className={`hint ${dirty ? "bad" : "ok"}`}>
              {dirty ? "有未保存的修改（③ ④）" : "已保存"}
            </span>
            <button className="btn-primary" onClick={saveDesk} disabled={busy || !dirty}>
              {busy ? "保存中…" : "保存合成与执行器配置"}
            </button>
          </div>
        </>
      )}
    </div>
  );
}
