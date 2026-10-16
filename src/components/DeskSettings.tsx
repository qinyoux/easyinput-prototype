// 「设置 → Agent 任务台」：任务台的**行为与调度**配置。
//
// 设计原则与本项目既有的设置页一致：**页面是纯表单，保存是显式动作**。
// 改动先落到本地 state，用户点「保存」才提交给后端 —— 避免每个按键都写盘，
// 也避免改到一半被轮询刷新覆盖。
//
// ⚠️ 两次拆分（都不用在这里找执行器了）：
// - 2026-09-22 第一轮：**API 凭据迁到「模型配置」页**（`ApiSettings.tsx`）——
//   语音合成（TTS）整段、以及各 API 型执行器的端点/模型/密钥。
// - 2026-09-22 第二轮：**执行器整段迁到「模型配置 → ④ 执行器」**（`DriverEditor.tsx`）——
//   含 CLI 型的命令/参数/环境变量、以及全部执行器的启用/超时/完成判定/能力声明。
//
// 为什么这么切：**一个字段只有一个家**。同一个 `api_key`（或同一个 `enabled`）
// 有两个编辑入口，先后保存就会互相覆盖，而界面上完全看不出谁覆盖了谁。
// 现在本页只剩"任务台层面"的东西：总开关、产物目录、总结与播报、外部会话监工。
import { useEffect, useState } from "react";
import * as api from "../api";
import type { DeskConfig } from "../types";

/**
 * 进度档位**上限** —— 必须与 Rust 侧 `NoticeKind::PROGRESS_MARKS` 的长度一致。
 *
 * 前端拿不到 Rust 常量，所以这是一个**跨语言契约**，只能靠注释对齐。
 * 为什么上限存在：每一档都要有一句固定语料并**预先合成成音频**（离线也要能出声），
 * 所以档数是"闭集"的一部分，不是能随便配的自由列表。
 * ⚠️ 改 Rust 侧 `PROGRESS_MARKS` 时必须同步这里，否则提示会骗人。
 */
const NOTICE_MARK_CAPACITY = 3;

interface Props {
  notify: (kind: "ok" | "err", text: string) => void;
}

export default function DeskSettings({ notify }: Props) {
  const [cfg, setCfg] = useState<DeskConfig | null>(null);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    (async () => {
      try {
        const env = await api.deskGetConfig();
        if (env.ok && env.data) {
          setCfg(env.data);
          setDirty(false);
        } else {
          notify("err", env.message ?? env.code);
        }
      } catch (e) {
        notify("err", String(e));
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (!cfg) return <p className="hint">读取任务台配置中…</p>;

  function patch(p: Partial<DeskConfig>) {
    setCfg((c) => (c ? { ...c, ...p } : c));
    setDirty(true);
  }

  async function save() {
    if (!cfg) return;
    setBusy(true);
    try {
      const env = await api.deskSetConfig(cfg);
      if (env.ok && env.data) {
        setCfg(env.data);
        setDirty(false);
        notify("ok", "Agent 任务台配置已保存");
      } else {
        notify("err", env.message ?? env.code);
      }
    } catch (e) {
      notify("err", String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="settings-page">
      <div className="settings-section">
        <h3 className="settings-section-title">Agent 任务台</h3>
        <p className="hint">
          按 S1–S4：<strong>按一下说话 → 再按一下结束并派发</strong>，任务后台执行，完成后播报、有产物自动打开。
          <br />
          键位在「<strong>按键重映射</strong>」配（类型选「任务台」）；一段录音只属于一个入口，别的键按下会被明确拒绝。
          <br />
          ⚠️ 播报只走<strong>电脑喇叭</strong>（键盘喇叭未接 PCM 通道）；灯语 <code>0x16</code> 未烧板。
          <br />
          🔑 TTS 与执行器凭据在「<strong>模型配置</strong>」页，不在本页。
        </p>
        <div className="field toggle-field">
          <label>启用任务台</label>
          <div className="row">
            <span className="toggle-desc">关闭时配置保留，但按 S1–S4 不会派发任何任务</span>
            <button
              className={`switch${cfg.enabled ? " on" : ""}`}
              onClick={() => patch({ enabled: !cfg.enabled })}
              aria-pressed={cfg.enabled}
              aria-label="启用 Agent 任务台"
            >
              <span className="switch-text">{cfg.enabled ? "开" : "关"}</span>
              <span className="knob" />
            </button>
          </div>
        </div>
        <div className="field">
          <label>产物目录</label>
          <div className="row">
            <input
              value={cfg.output_dir}
              onChange={(e) => patch({ output_dir: e.target.value })}
              placeholder="留空 = 配置目录旁的 agent_desk_out\\"
              spellCheck={false}
            />
          </div>
          <p className="hint">
            产物落点 + <code>tasks/</code>（任务历史）。留空 = 配置目录旁的 <code>agent_desk_out\</code>。
          </p>
        </div>
      </div>

      <div className="settings-section">
        <h3 className="settings-section-title">总结与播报</h3>
        <p className="hint">
          <strong>总结</strong> = 任务跑完后要念的那句话怎么来；<strong>播报</strong> = 什么时候出声。
          两者互不依赖：播报关掉不影响总结，总结留空也不影响固定语料。
        </p>
        <div className="field">
          <label>总结用执行器</label>
          <div className="row">
            <select
              className="select"
              value={cfg.summary.driver_id}
              onChange={(e) =>
                patch({ summary: { ...cfg.summary, driver_id: e.target.value } })
              }
            >
              <option value="">不复述，直接念 Agent 的结论（推荐）</option>
              {cfg.drivers.map((d) => (
                <option key={d.id} value={d.id}>
                  {d.label}
                  {d.enabled ? "" : "（未启用 · 选了也不会总结）"}
                </option>
              ))}
            </select>
          </div>
          <p className="hint">
            留空 = <strong>不做二次总结</strong>，直接念 Agent 自己最后一句（按上面的字数上限截断）。
            选了执行器才会再跑一次把它压短 —— 多一次模型调用，<strong>总结不占槽位</strong>。
          </p>
        </div>
        <div className="field">
          <label>摘要字数上限</label>
          <div className="row">
            <input
              type="number"
              min={20}
              max={1500}
              value={cfg.summary.max_chars}
              onChange={(e) =>
                patch({
                  summary: {
                    ...cfg.summary,
                    max_chars: Math.min(1500, Math.max(20, Number(e.target.value) || 20)),
                  },
                })
              }
            />
            <span className="desk-opt-reason">字（20–1500）</span>
          </div>
          <p className="hint">
            实际生效时会再取 <code>max(20, 本值)</code>。TTS 服务端实测<strong>接受 1500 字</strong>，
            真正卡时长的是固件"单条音频 ≤ 150 秒"，不是这个数。
          </p>
        </div>
        {cfg.summary.driver_id.trim() !== "" && (
          <div className="field">
            <label>总结提示词模板</label>
            <div className="row">
              <textarea
                className="desk-textarea"
                rows={5}
                value={cfg.summary.prompt_template}
                onChange={(e) =>
                  patch({
                    summary: { ...cfg.summary, prompt_template: e.target.value },
                  })
                }
                spellCheck={false}
              />
            </div>
            <p className="hint">
              占位符：<code>{"{task}"}</code> 原始指令 · <code>{"{output}"}</code> Agent 输出 ·{" "}
              <code>{"{max_chars}"}</code> 上面的字数上限。留空 ⇒ 不会二次总结。
            </p>
            <p className="hint">
              ⚠️ 这一项<strong>只影响二次总结</strong>，且执行器已被停用时本项不生效。
            </p>
          </div>
        )}
        <div className="field toggle-field">
          <label>总结失败时退回截断原文</label>
          <div className="row">
            <span className="toggle-desc">宁可播一句粗糙的原文，也不要「灯亮了按下去没声音」</span>
            <button
              className={`switch${cfg.summary.fallback_truncate ? " on" : ""}`}
              onClick={() =>
                patch({
                  summary: { ...cfg.summary, fallback_truncate: !cfg.summary.fallback_truncate },
                })
              }
              aria-pressed={cfg.summary.fallback_truncate}
              aria-label="总结失败时退回截断原文"
            >
              <span className="switch-text">{cfg.summary.fallback_truncate ? "开" : "关"}</span>
              <span className="knob" />
            </button>
          </div>
        </div>

        <div className="field toggle-field">
          <label>状态播报</label>
          <div className="row">
            <span className="toggle-desc">
              开始 / 进度 / 完成 / 异常各播一句固定语料。<strong>不产生未读</strong>，关掉它不影响灯。
            </span>
            <button
              className={`switch${cfg.notice.enabled ? " on" : ""}`}
              onClick={() => patch({ notice: { ...cfg.notice, enabled: !cfg.notice.enabled } })}
              aria-pressed={cfg.notice.enabled}
              aria-label="状态播报"
            >
              <span className="switch-text">{cfg.notice.enabled ? "开" : "关"}</span>
              <span className="knob" />
            </button>
          </div>
        </div>

        {cfg.notice.enabled && (
          <div className="field">
            <label>播报哪几类</label>
            <div className="desk-prefer">
              <label className="desk-prefer-item">
                <input
                  type="checkbox"
                  checked={cfg.notice.on_start}
                  onChange={(e) =>
                    patch({ notice: { ...cfg.notice, on_start: e.target.checked } })
                  }
                />
                <span>任务开始</span>
              </label>
              <label className="desk-prefer-item">
                <input
                  type="checkbox"
                  checked={cfg.notice.on_progress}
                  onChange={(e) =>
                    patch({ notice: { ...cfg.notice, on_progress: e.target.checked } })
                  }
                />
                <span>进度</span>
              </label>
              <label className="desk-prefer-item">
                <input
                  type="checkbox"
                  checked={cfg.notice.on_complete}
                  onChange={(e) =>
                    patch({ notice: { ...cfg.notice, on_complete: e.target.checked } })
                  }
                />
                <span>完成</span>
              </label>
              <label className="desk-prefer-item">
                <input
                  type="checkbox"
                  checked={cfg.notice.on_fail}
                  onChange={(e) => patch({ notice: { ...cfg.notice, on_fail: e.target.checked } })}
                />
                <span>异常</span>
              </label>
            </div>
            <p className="hint">
              「完成」含"任务完成但产物交付失败"（产物没落地绝不说「已完成」）。「进度」最吵，可单独关。
            </p>
          </div>
        )}

        <div className="field">
          <label>去抖间隔（秒）</label>
          <div className="row">
            <input
              type="number"
              min={0}
              value={cfg.notice.min_interval_secs}
              onChange={(e) =>
                patch({
                  notice: {
                    ...cfg.notice,
                    min_interval_secs: Math.max(0, Number(e.target.value) || 0),
                  },
                })
              }
            />
          </div>
          <p className="hint">
            只约束<strong>进度</strong>类。终态与"任务已开始"<strong>永不受约束</strong>。填 0 = 不限。
          </p>
        </div>

        <div className="field">
          <label>进度档位（分钟）</label>
          <div className="row">
            {Array.from({ length: NOTICE_MARK_CAPACITY }, (_, i) => {
              const secs = cfg.notice.progress_marks_secs[i];
              return (
                <label key={i} className="desk-mark">
                  <span>第 {i + 1} 档</span>
                  <input
                    type="number"
                    min={0}
                    value={secs === undefined || secs === 0 ? "" : Math.round(secs / 60)}
                    onChange={(e) => {
                      const next = [...cfg.notice.progress_marks_secs];
                      while (next.length < NOTICE_MARK_CAPACITY) next.push(0);
                      next[i] = Math.max(0, Number(e.target.value) || 0) * 60;
                      patch({
                        notice: {
                          ...cfg.notice,
                          progress_marks_secs: next.slice(0, NOTICE_MARK_CAPACITY),
                        },
                      });
                    }}
                    placeholder="—"
                  />
                  <span>分钟</span>
                </label>
              );
            })}
          </div>
          <p className="hint">
            「正在推进」不在表内（任务一跑就说）；第 1/2/3 档到点各播一次。
            留空或 0 = 这一档不用，<strong>留在原位</strong>（第 N 个框永远对应第 N 句话）。
            超出 {NOTICE_MARK_CAPACITY} 档的部分会被忽略并在日志里记一条。
          </p>
        </div>
      </div>

      <div className="settings-section">
        <h3 className="settings-section-title">外部会话监工</h3>
        <p className="hint">
          你<strong>自己开终端</strong>跑 codex 时，它盯着 rollout 日志，见完成就播一句。挂在 5 秒一次的灯语心跳上，所以不占额外线程。
        </p>
        <div className="field toggle-field">
          <label>启用</label>
          <div className="row">
            <span className="toggle-desc">默认开：没装 codex 时目录不存在 ⇒ 什么都不发生</span>
            <button
              className={`switch${cfg.probe.enabled ? " on" : ""}`}
              onClick={() => patch({ probe: { ...cfg.probe, enabled: !cfg.probe.enabled } })}
              aria-pressed={cfg.probe.enabled}
              aria-label="外部会话监工"
            >
              <span className="switch-text">{cfg.probe.enabled ? "开" : "关"}</span>
              <span className="knob" />
            </button>
          </div>
        </div>
        {cfg.probe.enabled && (
          <div className="field">
            <label>rollout 根目录</label>
            <div className="row">
              <input
                value={cfg.probe.root}
                onChange={(e) => patch({ probe: { ...cfg.probe, root: e.target.value } })}
                placeholder="留空 = %USERPROFILE%\.codex\sessions"
                spellCheck={false}
              />
            </div>
            <p className="hint">
              留空 = <code>%USERPROFILE%\.codex\sessions</code>。改过 <code>CODEX_HOME</code> 才需手填；
              填错不会报错，只是<strong>一直扫不到东西</strong>。
            </p>
          </div>
        )}
        <p className="hint">
          ⚠️ <strong>不产生未读</strong>（结果就在你开着的终端里），且<strong>只播固定语料</strong>。
        </p>
      </div>

      <div className="settings-save-bar">
        <span className={`hint ${dirty ? "bad" : "ok"}`}>{dirty ? "有未保存的修改" : "已保存"}</span>
        <button className="btn-primary" onClick={save} disabled={busy || !dirty}>
          {busy ? "保存中…" : "保存任务台配置"}
        </button>
      </div>
    </div>
  );
}
