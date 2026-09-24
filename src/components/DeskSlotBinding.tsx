// 单个任务台槽位的「执行器绑定」编辑器。
//
// # 为什么这块 UI 从「设置页」搬到了「按键重映射」
//
// 原来的位置有两份关于槽位的真相：设置页里按 S1–S4 列了四条绑定，而按键重映射
// 里 S1–S4 只是八个键中的前四个的**隐含约定**。两侧一旦不一致（比如用户把 KEY1
// 改成了别的动作），界面上就会出现"设置里说 S1 绑了百炼，但按下去什么都没发生"
// 这种对不上号的状况。
//
// 现在只有一处真相：**槽位属于哪个键，由按键重映射决定**（按键类型选「任务台」
// 再选 S1–S4）；**槽位交给谁执行，就在这个键的面板里配**。键与槽一一对应，
// 不存在"配了但没有键指向它"的槽位。
//
// # 数据流
//
// 槽位的执行器绑定存在 `agent_desk.json`（`DeskConfig.slots`），而键的类型存在
// profile 里——两者是**两份不同的配置**，所以本组件自己拉一次 `desk_get_config`，
// 改完立刻 `desk_set_config` 落盘，不走"父组件保存"那条路。
// 组件只在"选中的键是任务台键"时才挂载，所以这次额外请求不会常驻。
import { useCallback, useEffect, useState } from "react";
import * as api from "../api";
import type { Capability, DeskConfig, SlotBinding } from "../types";

const CAPABILITY_LABELS: Record<Capability, string> = {
  text: "文本",
  code: "代码",
  image: "图片",
  video: "视频",
  search: "检索",
  vision: "读图",
};

const MODE_HINTS: Record<SlotBinding["mode"], string> = {
  empty: "按下该键不派发任何任务（保留按键，先不接执行器）。",
  exact:
    "钉死一个执行器：路径确定、完全可复现，也最省 token——推荐先从这里开始。",
  capability:
    "只声明这一槽要什么能力，由路由在具备该能力的执行器里挑。候选留空时路由会**直接报错**，不会随便挑一个。",
  auto: "连能力都交给路由判。只建议给预估 30 秒内能做完的短任务用——长任务的「先声明后执行」来不及否决。",
};

function clone<T>(v: T): T {
  return JSON.parse(JSON.stringify(v));
}

interface Props {
  /** 槽位号（1 起）。 */
  slot: number;
}

export default function DeskSlotBinding({ slot }: Props) {
  const [cfg, setCfg] = useState<DeskConfig | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const env = await api.deskGetConfig();
      if (env.ok && env.data) {
        setCfg(env.data);
        setErr(null);
      } else {
        setErr(env.message ?? env.code);
      }
    } catch (e) {
      setErr(String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const index = slot - 1;
  const binding: SlotBinding | undefined = cfg?.slots[index];

  async function commit(next: SlotBinding) {
    if (!cfg) return;
    setBusy(true);
    setStatus(null);
    try {
      // 深拷贝后只改这一条：`desk_set_config` 是整份替换，不能把别人的改动带丢。
      const copy = clone(cfg);
      copy.slots[index] = next;
      const env = await api.deskSetConfig(copy);
      if (env.ok && env.data) {
        setCfg(env.data);
        setErr(null);
        setStatus("已保存");
      } else {
        setErr(env.message ?? env.code);
        setStatus(null);
      }
    } catch (e) {
      setErr(String(e));
      setStatus(null);
    } finally {
      setBusy(false);
    }
  }

  if (err && !cfg) {
    return (
      <p className="hint bad">
        读取任务台配置失败：{err}
        <button className="btn-ghost" onClick={() => void load()}>
          重试
        </button>
      </p>
    );
  }
  if (!cfg || !binding) {
    return <p className="hint">读取槽位 {slot} 的执行器绑定中…</p>;
  }

  return (
    <div className="desk-slot-binding">
      <div className="desk-slot-binding-head">
        <span className="desk-slot-key">S{slot}</span>
        <span className="hint">该槽位的执行器绑定</span>
        {busy && <span className="hint">保存中…</span>}
        {!busy && status && <span className="hint ok">{status}</span>}
        {err && <span className="hint bad">{err}</span>}
      </div>

      <div className="field">
        <label>绑定方式</label>
        <div className="row">
          <select
            className="select"
            value={binding.mode}
            disabled={busy}
            onChange={(e) => {
              const mode = e.target.value as SlotBinding["mode"];
              if (mode === "empty") void commit({ mode: "empty" });
              else if (mode === "auto") void commit({ mode: "auto" });
              else if (mode === "exact")
                void commit({ mode: "exact", driver: cfg.drivers[0]?.id ?? "" });
              else void commit({ mode: "capability", capability: "text", prefer: [] });
            }}
          >
            <option value="empty">未绑定</option>
            <option value="exact">指定执行器</option>
            <option value="capability">按能力</option>
            <option value="auto">自动判定</option>
          </select>

          {binding.mode === "exact" && (
            <select
              className="select"
              value={binding.driver}
              disabled={busy}
              onChange={(e) => void commit({ mode: "exact", driver: e.target.value })}
            >
              <option value="">（请选择）</option>
              {cfg.drivers.map((d) => (
                <option key={d.id} value={d.id}>
                  {d.label}
                  {d.enabled ? "" : "（未启用）"}
                </option>
              ))}
            </select>
          )}

          {binding.mode === "capability" && (
            <select
              className="select"
              value={binding.capability}
              disabled={busy}
              onChange={(e) =>
                void commit({
                  mode: "capability",
                  capability: e.target.value as Capability,
                  prefer: binding.prefer,
                })
              }
            >
              {(Object.keys(CAPABILITY_LABELS) as Capability[]).map((c) => (
                <option key={c} value={c}>
                  {CAPABILITY_LABELS[c]}
                </option>
              ))}
            </select>
          )}
        </div>
        <p className="hint">{MODE_HINTS[binding.mode]}</p>
      </div>

      {binding.mode === "capability" && (
        <div className="field">
          <label>候选执行器（勾选即加入优先顺序）</label>
          <div className="desk-prefer">
            {cfg.drivers.map((d) => (
              <label key={d.id} className="desk-prefer-item">
                <input
                  type="checkbox"
                  disabled={busy}
                  checked={binding.prefer.includes(d.id)}
                  onChange={(e) => {
                    const prefer = e.target.checked
                      ? [...binding.prefer, d.id]
                      : binding.prefer.filter((x) => x !== d.id);
                    void commit({
                      mode: "capability",
                      capability: binding.capability,
                      prefer,
                    });
                  }}
                />
                {d.label}
              </label>
            ))}
          </div>
          {binding.prefer.length === 0 && (
            <p className="hint bad">
              候选为空 —— 路由判到该能力时会<strong>直接报错</strong>，不会随便挑一个。
            </p>
          )}
        </div>
      )}

      <div className="desk-slot-binding-cfgpath">
        <p className="hint">
          执行器本身在别处配：「<strong>设置 → Agent 任务台 → 执行器</strong>」管命令 / 启用 /
          完成判定，<strong>API 型执行器的端点与密钥</strong>在「<strong>模型配置</strong>」页。
          这个面板只管「槽位交给谁」。
        </p>
      </div>
    </div>
  );
}
