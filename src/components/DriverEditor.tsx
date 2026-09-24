// 「模型配置 → 执行器」：**执行器（Driver）配置的唯一编辑入口**。
//
// 它由 `ApiSettings.tsx` 引用。2026-09-22 之前，一个执行器被拆成两处编辑：
// 「模型配置」页管凭据（端点 / 模型 / Key），「任务台」页管调度（启用 / 超时 /
// 完成判定 / 能力）。现在全部收到这里 —— **一个执行器的一切都在它自己的展开块里**。
//
// ---------------------------------------------------------------------------
// 为什么不做成"执行器 = 模型"的单一形态
// ---------------------------------------------------------------------------
//
// 钦友 2026-09-22 的提议（"每个执行器本质上就等同于一个模型，合成一个入口"）
// 对了一半，这一半很重要，另一半会埋雷：
//
// ✅ **对 API 型执行器，一条配置确实就等于一个模型。** 所以本组件给 API 型的
//    默认视野只有 4 个输入框（显示名 / 端点 / 模型 / Key），"完成判定"这类
//    在 Api 形态下恒为 `http_stream_end` 的字段一律不露 —— 不让人有机会填错。
//
// ❌ **CLI 型执行器不是模型，是一个会改你磁盘的自治进程。** `codex-cli` 调哪个
//    模型写在它自己的 `config.toml` 里（本仓库 `DriverConfig.model` 对它一直是
//    空的、从没被读过），这个 harness 真正需要你配的是
//    「命令 / 参数模板 / 工作目录 / 完成判定 / 会话续接」。
//    把它拍平成同一个"模型"表单，代价不是"少几个字段"，而是**完成判定被藏起来**——
//    "进程退出码 ≠ 任务完成"（见 `driver/mod.rs` 模块头），而"任务到底做完了没有"
//    是这套调度里唯一不能猜的东西。
//
// ⇒ 结论：**一个入口、一张列表，但按 `kind` 只渲染真正相关的那几项**。
//    API 型默认 4 个字段；CLI 型默认 3 个；其余（超时 / 完成判定 / 世界门禁 /
//    环境变量 / 会话续接）统一收进「高级」，默认收起 —— 既满足"界面占用更小"，
//    又不把"配错了就静默判错"的那几个字段藏没。
import { useState } from "react";
import * as api from "../api";
import type { Capability, DriverConfig, DriverKind, SlotBinding } from "../types";

/** 驱动超时的出厂默认（毫秒）—— 与 Rust 侧 `driver::DEFAULT_TIMEOUT_MS` 一致。 */
const DEFAULT_TIMEOUT_MS = 180_000;

/**
 * 预置执行器的 id —— **只能停用，不能删除**。
 *
 * ⚠️ 这是与 Rust 侧 `config.rs::default_drivers()` 的**刻意重复**，
 * 与 `CAPABILITY_LABELS` 同一先例。理由：后端 `DeskConfig::normalize()` 会在
 * 每次加载配置时**补回**缺失的预置条目（那是刻意的，否则用户的自定义会被重置）。
 * 所以给预置行渲染"删除"按钮的结果是"删了 → 保存 → 它又回来了"。
 * 与其让人撞一次这个幻觉，不如在前端就不给这个按钮。
 * 代价：改预置清单时两边都要改。
 */
const PRESET_DRIVER_IDS = new Set(["doubao-api", "dsh", "codex-cli"]);

/** 能力中文名（UI 用）。⚠️ 与 Rust 侧 `Capability::label()` 是刻意重复，见其注释。 */
const CAPABILITY_LABELS: Record<Capability, string> = {
  text: "文本",
  code: "代码",
  image: "图片",
  video: "视频",
  search: "检索",
  vision: "读图",
};

const COMPLETION_LABELS: Record<string, string> = {
  http_stream_end: "HTTP 流结束",
  log_pattern: "日志权威串",
  process_exit_zero: "进程退出码 0",
  async_task_poll: "异步任务轮询（尚未实现）",
};

/** CLI 执行器的环境变量里是否声明了 HOME 系变量。规则与后端 `declares_home` 对齐。 */
function declaresHome(env: string[]): boolean {
  return env.some((e) => /^(HOME|USERPROFILE|DSH_HOME)\s*=/i.test(e));
}

/** 是否已具备"能发一次请求"的最小条件。与后端 `DriverConfig::is_configured()` 对齐。 */
function isConfigured(d: DriverConfig): boolean {
  return d.kind === "api"
    ? d.api_key.trim() !== "" && d.model.trim() !== ""
    : d.command.trim() !== "";
}

interface Props {
  drivers: DriverConfig[];
  slots: SlotBinding[];
  onPatch: (id: string, p: Partial<DriverConfig>) => void;
  onAdd: (kind: DriverKind) => void;
  onRemove: (id: string) => void;
  /** 探测读的是**后端已保存的那份配置** ⇒ 调用方需先把当前编辑落盘。 */
  flush: () => Promise<void>;
  notify: (kind: "ok" | "err", text: string) => void;
}

export default function DriverEditor({
  drivers,
  slots,
  onPatch,
  onAdd,
  onRemove,
  flush,
  notify,
}: Props) {
  const [open, setOpen] = useState<string | null>(null);
  const [advanced, setAdvanced] = useState<string | null>(null);
  const [showKey, setShowKey] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [probe, setProbe] = useState<Record<string, string>>({});
  const [probing, setProbing] = useState<string | null>(null);

  const apiDrivers = drivers.filter((d) => d.kind === "api");
  const cliDrivers = drivers.filter((d) => d.kind !== "api");

  /** 该执行器被哪些槽位引用（1-based）。删除前必须看它 —— 见 `onRemove` 的说明。 */
  function boundSlots(id: string): number[] {
    return slots
      .map((s, i) => (s.mode === "exact" && s.driver === id ? i + 1 : null))
      .filter((x): x is number => x !== null);
  }

  async function runProbe(d: DriverConfig) {
    setProbing(d.id);
    setProbe((p) => ({ ...p, [d.id]: "探测中…" }));
    try {
      // 先落盘再探测：`desk_probe_driver` 读的是后端**已保存**的那份配置
      // （Rust 侧 `mgr.store.get()`），不是本页正在编辑的 state —— 不先存就是在测
      // "改之前的旧值"，结果是"刚填的 Key 说未配置"。落盘失败时 `flush` 自己会弹错误提示，
      // 这里的探测结论会跟着是旧的 —— 提示已经发出去了，不重复报警。
      await flush();
      const env = await api.deskProbeDriver(d.id);
      setProbe((p) => ({
        ...p,
        [d.id]:
          env.ok && env.data
            ? `[${env.data.ready ? "✅ 就绪" : "⚠️ 未就绪"}] ${env.data.detail}`
            : `❌ 失败：${env.message ?? env.code}`,
      }));
      if (env.ok && env.data && !env.data.ready) notify("err", `${d.label}：${env.data.detail}`);
    } catch (e) {
      setProbe((p) => ({ ...p, [d.id]: `❌ 失败：${e}` }));
    } finally {
      setProbing(null);
    }
  }

  function renderRow(d: DriverConfig) {
    const isOpen = open === d.id;
    const isAdvanced = advanced === d.id;
    const isPreset = PRESET_DRIVER_IDS.has(d.id);
    const configured = isConfigured(d);
    const homes = boundSlots(d.id);

    return (
      <div key={d.id} className="desk-driver-row">
        <div className="desk-driver-row-head">
          <button className="btn-ghost" onClick={() => setOpen(isOpen ? null : d.id)}>
            {isOpen ? "▾" : "▸"} {d.label || d.id}
          </button>
          {isPreset && <span className="desk-tag">预置</span>}
          <span className={`desk-tag ${configured ? "ok" : "warn"}`}>
            {configured ? "已配置" : "待配置"}
          </span>
          <span className={`desk-tag ${d.enabled ? "ok" : "warn"}`}>
            {d.enabled ? "已启用" : "未启用"}
          </span>
          <button
            className={`switch${d.enabled ? " on" : ""}`}
            onClick={() => onPatch(d.id, { enabled: !d.enabled })}
            aria-pressed={d.enabled}
            aria-label={`启用 ${d.label}`}
          >
            <span className="switch-text">{d.enabled ? "开" : "关"}</span>
            <span className="knob" />
          </button>
          <button
            className="btn-ghost"
            onClick={() => runProbe(d)}
            disabled={probing === d.id}
            title="按盘上配置做一次静态检查（不会真的发起任务）"
          >
            {probing === d.id ? "探测中…" : "探测"}
          </button>
          {isPreset ? (
            <span className="desk-tag" title="预置执行器只能停用；删除后会在下次启动时被补回">
              不可删
            </span>
          ) : confirming === d.id ? (
            <>
              <button
                className="btn-ghost"
                onClick={() => {
                  onRemove(d.id);
                  setConfirming(null);
                  setOpen(null);
                }}
                title="确认删除（不可撤销）"
              >
                确认删除
              </button>
              <button className="btn-ghost" onClick={() => setConfirming(null)}>
                取消
              </button>
            </>
          ) : (
            <button
              className="btn-ghost"
              onClick={() => setConfirming(d.id)}
              title={
                homes.length
                  ? `槽位 S${homes.join("、S")} 正绑定它 —— 删除会连带清空这些绑定`
                  : "删除这个执行器"
              }
            >
              删除
            </button>
          )}
        </div>

        {/* 删除前的"地形报告"：把"这一删会波及谁"摆在按钮旁边，而不是等按下之后才说。 */}
        {confirming === d.id && (
          <p className="hint bad" style={{ padding: "0 12px 8px" }}>
            删除<strong>不可撤销</strong>。
            {homes.length > 0
              ? ` ⚠️ 槽位 S${homes.join("、S")} 正绑着它，保存后这些槽位会被清空 —— 
                 不会自动改绑到别的执行器（猜错就是把任务发给了错的 Agent）。`
              : " 当前没有槽位绑定它。"}
            <br />
            记得点下面的「保存」才算落盘。
          </p>
        )}

        {probe[d.id] && (
          <p className="hint" style={{ padding: "0 12px 8px", whiteSpace: "pre-wrap" }}>
            {probe[d.id]}
          </p>
        )}

        {isOpen && (
          <div className="desk-driver-body">
            {/* ---------- 通用：显示名 ---------- */}
            <div className="field">
              <label>显示名</label>
              <div className="row">
                <input
                  value={d.label}
                  onChange={(e) => onPatch(d.id, { label: e.target.value })}
                  placeholder="给它起个一眼认得出的名字"
                />
              </div>
              <p className="hint">
                id = <code>{d.id}</code>
                {isPreset ? "（预置，不可改）" : "（槽位绑定的引用键，不可改）"}。
              </p>
            </div>

            {/* ---------- 按形态渲染"真正相关"的字段 ---------- */}
            {d.kind === "api" ? (
              <>
                <div className="field">
                  <label>端点</label>
                  <div className="row">
                    <input
                      value={d.endpoint}
                      onChange={(e) => onPatch(d.id, { endpoint: e.target.value })}
                      placeholder="https://…/v1/chat/completions"
                      spellCheck={false}
                    />
                  </div>
                  <p className="hint">
                    要写到 <code>/chat/completions</code> 这一级（后端按原样 POST，少一段就是 404）。
                  </p>
                </div>
                <div className="field">
                  <label>模型</label>
                  <div className="row">
                    <input
                      value={d.model}
                      onChange={(e) => onPatch(d.id, { model: e.target.value })}
                      placeholder="如 deepseek-chat / ep-…（方舟接入点）"
                      spellCheck={false}
                    />
                  </div>
                </div>
                <div className="field">
                  <label>API Key</label>
                  <div className="row">
                    <input
                      type={showKey === d.id ? "text" : "password"}
                      value={d.api_key}
                      onChange={(e) => onPatch(d.id, { api_key: e.target.value })}
                      placeholder="留空 = 该执行器不可用"
                      spellCheck={false}
                    />
                    <button
                      className="btn-ghost"
                      onClick={() => setShowKey(showKey === d.id ? null : d.id)}
                      title={showKey === d.id ? "隐藏" : "显示"}
                      aria-label={`显示或隐藏 ${d.label} 的 API Key`}
                    >
                      {showKey === d.id ? "🙈" : "👁"}
                    </button>
                  </div>
                </div>
              </>
            ) : (
              <>
                <div className="field">
                  <label>命令</label>
                  <div className="row">
                    <input
                      value={d.command}
                      onChange={(e) => onPatch(d.id, { command: e.target.value })}
                      placeholder="codex.cmd / dsh / …"
                      spellCheck={false}
                    />
                  </div>
                  <p className="hint">
                    ⚠️ Windows 上 npm 全局包装的是 <code>.cmd</code> shim，
                    <strong>裸名通常起不来</strong>（CreateProcess 只自动补 <code>.exe</code>）——
                    找不到时写全路径。
                  </p>
                </div>
                <div className="field">
                  <label>参数模板（每行一个，必须含 {"{prompt}"}）</label>
                  <div className="row">
                    <textarea
                      className="desk-textarea"
                      rows={4}
                      value={d.args.join("\n")}
                      onChange={(e) =>
                        onPatch(d.id, {
                          args: e.target.value.split("\n").filter((x) => x !== ""),
                        })
                      }
                      placeholder={"exec\n--json\n{prompt}"}
                      spellCheck={false}
                    />
                  </div>
                  <p className="hint">
                    <code>{"{prompt}"}</code> 会被替换成任务文本。没有它 ⇒ 文本传不进去，
                    表现为"CLI 正常退出、什么都没做"（后端会直接拒绝调度）。
                  </p>
                </div>
                <div className="field">
                  <label>工作目录</label>
                  <div className="row">
                    <input
                      value={d.cwd}
                      onChange={(e) => onPatch(d.id, { cwd: e.target.value })}
                      placeholder="部分 CLI（如 dsh）没有 cwd 会拒绝执行"
                      spellCheck={false}
                    />
                  </div>
                </div>
              </>
            )}

            {/* ---------- 通用：能力声明（紧凑一行，且"按能力路由"要靠它） ---------- */}
            <div className="field">
              <label>能力声明</label>
              <div className="desk-prefer">
                {(Object.keys(CAPABILITY_LABELS) as Capability[]).map((c) => (
                  <label key={c} className="desk-prefer-item">
                    <input
                      type="checkbox"
                      checked={d.capabilities.includes(c)}
                      onChange={(e) => {
                        const capabilities = e.target.checked
                          ? [...d.capabilities, c]
                          : d.capabilities.filter((x) => x !== c);
                        onPatch(d.id, { capabilities });
                      }}
                    />
                    {CAPABILITY_LABELS[c]}
                  </label>
                ))}
              </div>
              <p className="hint">
                槽位绑成「按能力」或「自动判定」时靠它挑人。一个都不勾 ⇒ 那两种绑定挑不到它。
              </p>
            </div>

            {/* ---------- 高级：默认收起 ---------- */}
            <div className="field">
              <button className="btn-ghost" onClick={() => setAdvanced(isAdvanced ? null : d.id)}>
                {isAdvanced ? "▾ 收起高级选项" : "▸ 高级选项（超时 / 完成判定 / 门禁 …）"}
              </button>
            </div>

            {isAdvanced && (
              <>
                <div className="field">
                  <label>单次执行超时（秒）</label>
                  <div className="row">
                    <input
                      type="number"
                      min={0}
                      value={d.timeout_ms === 0 ? "" : Math.round(d.timeout_ms / 1000)}
                      onChange={(e) =>
                        onPatch(d.id, {
                          timeout_ms: Math.max(0, Number(e.target.value) || 0) * 1000,
                        })
                      }
                      placeholder={`留空 = 用默认 ${DEFAULT_TIMEOUT_MS / 1000} 秒`}
                    />
                  </div>
                  <p className="hint">
                    留空或填 0 = 用默认的 <strong>{DEFAULT_TIMEOUT_MS / 1000} 秒</strong>
                    （那个默认值是照"一次 HTTP 调用"定的）。CLI Agent 跑真实任务动辄几分钟到几十分钟
                    —— 拿默认值卡它，结果是每个任务都在 3 分钟处被判超时。
                    <br />⚠️ <strong>没有"不超时"这个选项</strong>：没有回收点的执行器一旦卡死，
                    槽位会被永久占住。
                  </p>
                </div>

                {d.kind === "api" ? (
                  <div className="field">
                    <label>完成判定</label>
                    <p className="hint">
                      {d.completion.kind === "http_stream_end" ? (
                        <>
                          <strong>HTTP 流结束</strong> —— Api 形态只有这一种取值，所以不给你选。
                        </>
                      ) : (
                        <>
                          ⚠️ 当前是「{COMPLETION_LABELS[d.completion.kind] ?? d.completion.kind}」，
                          与 Api 形态<strong>矛盾</strong>（它没有子进程，永远判不出完成，后端会拒绝调度）。
                        </>
                      )}
                    </p>
                    {d.completion.kind !== "http_stream_end" && (
                      <div className="row">
                        <button
                          className="btn-ghost"
                          onClick={() =>
                            onPatch(d.id, {
                              completion: { ...d.completion, kind: "http_stream_end" },
                            })
                          }
                        >
                          修正为「HTTP 流结束」
                        </button>
                      </div>
                    )}
                  </div>
                ) : (
                  <div className="field">
                    <label>完成判定</label>
                    <div className="row">
                      <select
                        className="select"
                        value={d.completion.kind}
                        onChange={(e) =>
                          onPatch(d.id, {
                            completion: {
                              ...d.completion,
                              kind: e.target.value as DriverConfig["completion"]["kind"],
                            },
                          })
                        }
                      >
                        {Object.entries(COMPLETION_LABELS).map(([k, v]) => (
                          <option key={k} value={k}>
                            {v}
                          </option>
                        ))}
                      </select>
                      {d.completion.kind === "log_pattern" && (
                        <input
                          value={d.completion.pattern}
                          onChange={(e) =>
                            onPatch(d.id, {
                              completion: { ...d.completion, pattern: e.target.value },
                            })
                          }
                          // 支持多个候选串（`;` 分隔），任一命中即完成。placeholder 里就给例子。
                          placeholder="turn.completed;task_complete"
                          spellCheck={false}
                        />
                      )}
                    </div>
                    <p className="hint">
                      <strong>进程退出码只证明「进程结束了」，不证明「事情做成了」</strong> ——
                      所以它单独出现时可信度只有「未校验」，界面与播报都会这样标。
                      选<strong>日志权威串</strong>并填对（如 codex 的{" "}
                      <code>turn.completed</code>）才是高置信度。
                    </p>
                  </div>
                )}

                <div className="field">
                  <label>世界门禁命令（可空）</label>
                  <div className="row">
                    <input
                      value={d.completion.evidence_cmd}
                      onChange={(e) =>
                        onPatch(d.id, {
                          completion: { ...d.completion, evidence_cmd: e.target.value },
                        })
                      }
                      placeholder="如 test -f out.png"
                      spellCheck={false}
                    />
                  </div>
                  <p className="hint">
                    填了就升级为<strong>已校验</strong>：跑测试 / 查文件 / 看产物，
                    由外部证据决定完成，而不是上游自己的说法。
                  </p>
                </div>

                {d.kind === "cli" && (
                  <>
                    <div className="field">
                      <label>环境变量（每行 KEY=VALUE）</label>
                      <div className="row">
                        <textarea
                          className="desk-textarea"
                          rows={2}
                          value={d.env.join("\n")}
                          onChange={(e) =>
                            onPatch(d.id, {
                              env: e.target.value.split("\n").filter((x) => x !== ""),
                            })
                          }
                          placeholder={"HOME=C:\\Users\\<你>\nDSH_HOME=C:\\Users\\<你>\\.dsh"}
                          spellCheck={false}
                        />
                      </div>
                      {!declaresHome(d.env) && (
                        <p className="hint bad">
                          ⚠️ 没显式给 <code>HOME</code> / <code>USERPROFILE</code>：
                          本机 <code>~</code> 可能被其他软件（如 Cadence）改写，
                          CLI 会去错误的位置找自己的配置，表现为<strong>鉴权莫名失败</strong>。
                        </p>
                      )}
                    </div>

                    <div className="field">
                      <label>产物过滤（可选 glob）</label>
                      <div className="row">
                        <input
                          value={d.artifact_glob}
                          onChange={(e) => onPatch(d.id, { artifact_glob: e.target.value })}
                          placeholder="如 out/*.png、**/*.md（留空 = 不按名字过滤）"
                          spellCheck={false}
                        />
                      </div>
                      <p className="hint">
                        任务跑完后，系统在<strong>工作目录</strong>里按
                        「文件改动时间 ≥ 任务开始时刻」找出产物；这一项是在那批文件里
                        再按名字筛一层。<strong>相对工作目录</strong>匹配：
                        <code>*.md</code> 只命中根下，<code>**/*.md</code> 才跨目录。
                        没填工作目录就<strong>不会扫盘</strong>。
                      </p>
                    </div>

                    <div className="field">
                      <label>成功后自动打开产物</label>
                      <div className="row">
                        <input
                          type="checkbox"
                          checked={d.artifact_open}
                          onChange={(e) => onPatch(d.id, { artifact_open: e.target.checked })}
                        />
                        <span className="desk-hint-inline">
                          {d.artifact_open
                            ? "开：任务成功且扫到产物时，1 个就直接打开它，多个就打开所在文件夹并选中最新的。"
                            : "关：只在任务历史里列出产物路径，不弹任何窗口。"}
                        </span>
                      </div>
                      <p className="hint">
                        它会<strong>抢走你的焦点</strong>，所以只在
                        <strong>任务成功</strong>且<strong>确实扫到文件</strong>时才动作；
                        失败 / 超时 / 取消的任务一律不打开（那时目录里留下的是半成品）。
                      </p>
                    </div>

                    <div className="field">
                      <label>会话续接（多轮对话）</label>
                      <div className="row">
                        <input
                          type="checkbox"
                          checked={d.continuation !== null}
                          onChange={(e) =>
                            onPatch(d.id, {
                              continuation: e.target.checked
                                ? { id_event_type: "", id_field: "", args: [] }
                                : null,
                            })
                          }
                        />
                        <span className="toggle-desc">
                          在同一段会话里继续追问（而不是每个任务都重开）
                        </span>
                      </div>
                      {d.continuation !== null && (
                        <>
                          <div className="row">
                            <input
                              value={d.continuation.id_event_type}
                              onChange={(e) =>
                                onPatch(d.id, {
                                  continuation: {
                                    ...d.continuation!,
                                    id_event_type: e.target.value,
                                  },
                                })
                              }
                              placeholder="会话 id 所在事件：thread.started"
                              spellCheck={false}
                            />
                            <input
                              value={d.continuation.id_field}
                              onChange={(e) =>
                                onPatch(d.id, {
                                  continuation: { ...d.continuation!, id_field: e.target.value },
                                })
                              }
                              placeholder="id 的键名：thread_id"
                              spellCheck={false}
                            />
                          </div>
                          <div className="row">
                            <input
                              value={d.continuation.args.join(" ")}
                              onChange={(e) =>
                                onPatch(d.id, {
                                  continuation: {
                                    ...d.continuation!,
                                    args: e.target.value.split(/\s+/).filter((a) => a.length > 0),
                                  },
                                })
                              }
                              placeholder="续接参数模板：resume {id}"
                              spellCheck={false}
                            />
                          </div>
                        </>
                      )}
                      <p className="hint">
                        三个格子回答一件事：<strong>会话 id 在哪一行输出里</strong>、
                        <strong>那一行的哪个键</strong>、<strong>怎么把它带回去</strong>。
                      </p>
                    </div>
                  </>
                )}
              </>
            )}

            {d.note && <p className="hint">{d.note}</p>}
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="driver-groups">
      <div className="driver-group">
        <div className="driver-group-head">
          <h4>直连模型（API）</h4>
          <span className="hint">
            一次 HTTP 往返：<strong>端点 + 模型 + Key</strong> 填齐就能用，不占本机资源。
          </span>
          <button className="btn-ghost" onClick={() => onAdd("api")}>
            + 添加模型
          </button>
        </div>
        <div className="desk-driver-editor">
          {apiDrivers.length === 0 ? (
            <p className="hint">还没有直连模型。点右上「+ 添加模型」加一个。</p>
          ) : (
            apiDrivers.map(renderRow)
          )}
        </div>
      </div>

      <div className="driver-group">
        <div className="driver-group-head">
          <h4>CLI 执行器</h4>
          <span className="hint">
            本地 Agent 进程：<strong>能改你的磁盘</strong>，所以还要配「完成判定」——
            否则"退出码 0"会被当成"做完了"。
          </span>
          <button className="btn-ghost" onClick={() => onAdd("cli")}>
            + 添加 CLI 执行器
          </button>
        </div>
        <div className="desk-driver-editor">
          {cliDrivers.length === 0 ? (
            <p className="hint">还没有 CLI 执行器。</p>
          ) : (
            cliDrivers.map(renderRow)
          )}
        </div>
      </div>
    </div>
  );
}
