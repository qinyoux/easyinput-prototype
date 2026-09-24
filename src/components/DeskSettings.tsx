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
          把键盘变成任务台：<strong>按一下 S1–S4 开始说话</strong>（固件按键只有一条
          报文，没有按下/松开 ⇒ 只能"按一下开、再按一下关"，松开不会停）→ 说话时和语音
          输入一样出字幕 → 看完觉得没问题，<strong>再按一下同一个键</strong>：easyop 会把
          该槽执行器的<strong>完整命令行</strong>投进你正在用的终端并回车 ——
          <strong>过程在你眼前，输出你自己看</strong>。
          <br />
          键位在「<strong>按键重映射</strong>」里配：把某个键设为「任务台」并选 S1–S4。
          该槽要绑「命令行（CLI）」形态的执行器；是否自动回车由「粘贴后回车」设置决定。
          <br />
          <strong>槽位键与语音键（KEY1）等入口互斥</strong>：一段录音只能属于一个入口，
          录音期间别的键按下会被明确拒绝并告诉你是谁在录 —— 不会替你收尾。否则按一下语音键
          就会把正在录的任务内容当成命令派出去执行。
          <br />
          ⚠️ 尚未落地：播报只从 <strong>电脑喇叭</strong> 出声（键盘喇叭需要固件新增
          PCM 通道，未做）；灯语 <code>0x16</code> 两侧已通但<strong>未烧板</strong>。
          <br />
          🔑 <strong>API 类的配置不在这里</strong>：语音合成（TTS）与各执行器的
          端点 / 模型 / 密钥，统一到「<strong>模型配置</strong>」页改。
        </p>
        <div className="field toggle-field">
          <label>启用任务台</label>
          <div className="row">
            <span className="toggle-desc">关闭时配置保留，但不派发任何任务</span>
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
          <p className="hint">生成的图片 / 视频 / 音频落在这里，作为「产物已交付」的判定依据。</p>
        </div>
      </div>

      <div className="settings-section">
        <h3 className="settings-section-title">槽位绑定（已移到按键重映射）</h3>
        <p className="hint">
          槽位不再在这里配。到「<strong>按键重映射</strong>」里把某个键的类型设为
          「<strong>任务台</strong>」、选好 S1–S4，槽位的执行器绑定（指定执行器 / 按能力 /
          自动判定）就在那个键的面板里配。
        </p>
        <p className="hint">
          这样做的原因：按键和槽位原本是两处真相——设置页按 S1–S4 列绑定，按键那边
          S1–S4 却只是前四个键的隐含约定，一旦某个键被改成别的动作，界面上就会出现
          「设置里说 S1 绑了执行器，但按下去什么都没发生」这种对不上号的状况。
          现在<strong>一键一槽</strong>，不存在配了却没有键指向的槽位。
        </p>
      </div>

      <div className="settings-section">
        <h3 className="settings-section-title">执行器（Driver）</h3>
        <p className="hint">
          执行器（直连模型 / CLI Agent）已<strong>整体迁到「模型配置 → ④ 执行器」</strong>——
          命令、参数、工作目录、超时、完成判定、会话续接、能力声明全在那里。
          <strong>本页不再是它的编辑入口</strong>：同一个字段有两个地方能改，先后保存会互相覆盖，
          而界面上完全看不出是谁覆盖了谁。
        </p>
        <p className="hint">
          本页只留<strong>任务台层面</strong>的东西：总开关、产物目录、总结与播报、外部会话监工。
          <br />
          执行器<strong>本身怎么配</strong> → 「模型配置」页；
          哪个槽位<strong>派给谁</strong> → 「按键重映射」页。
        </p>
      </div>

      <div className="settings-section">
        <h3 className="settings-section-title">总结与播报</h3>
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
              <option value="">复用「模型配置 → ② 语音编辑」的 LLM 通道</option>
              {cfg.drivers.map((d) => (
                <option key={d.id} value={d.id}>
                  {d.label}
                </option>
              ))}
            </select>
          </div>
        </div>
        <div className="field">
          <label>摘要字数上限</label>
          <div className="row">
            <input
              type="number"
              value={cfg.summary.max_chars}
              onChange={(e) =>
                patch({
                  summary: { ...cfg.summary, max_chars: Number(e.target.value) || 0 },
                })
              }
            />
          </div>
          <p className="hint">
            TTS 硬上限 512 Token。摘要超长会直接失败，所以这里要留余量。
          </p>
        </div>
        <div className="field toggle-field">
          <label>总结失败时退回截断原文</label>
          <div className="row">
            <span className="toggle-desc">
              宁可播一句粗糙的原文截断，也不要「灯亮了按下去没有声音」
            </span>
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
              任务开始 / 进度 / 完成 / 异常时播一句固定语料。**不产生未读** —— 状态播报与
              任务结果严格分离；<strong>关掉它不影响灯</strong>（灯是另一条信道，只管"有几件事等你听"）
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
              「完成」这一类还包含"任务完成，但产物交付失败"（措辞纪律：产物没落地时
              <strong>绝不</strong>说"已完成"）。「进度」是最吵的一类，可以单独关掉 ——
              不必为了安静一点，连"任务已完成"一起关掉。
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
            只约束<strong>进度</strong>类。终态（完成 / 失败 / 失联）与"任务已开始"
            <strong>永不受它约束</strong>——因为"任务已完成"被"离上一条太近"吞掉，
            是这套系统最不能犯的错。填 0 = 不限。
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
            真正会说的是 <strong>4 句话</strong>，其中第一句不在这张表里：
            <br />
            <strong>① 正在推进</strong> —— 任务一跑起来就说；跨越下面第一档后按时刻升档，
            没有任务在跑时一句都不播。
            <br />
            <strong>② / ③ / ④</strong> —— 就是下面第 1 / 2 / 3 档，到点各播一次
            （同一档不会重复播，因为只有<strong>档位跃迁</strong>才触发）。
            <br />
            <strong>留空或填 0 = 这一档不用</strong>，它会<strong>留在原来的位置</strong>：
            第 N 个框永远对应第 N 句话，不会被后面的档顶上来。
            <br />
            <strong>只有 {NOTICE_MARK_CAPACITY} 档</strong>：每档都要有一句固定语料并
            <strong>预先合成成音频资产</strong>（拔网线后也要能出声）—— 语料是有限闭集，
            所以配置只能决定"这几档分别落在第几分钟"，造不出第 {NOTICE_MARK_CAPACITY + 1} 档。
          </p>
        </div>
      </div>

      <div className="settings-section">
        <h3 className="settings-section-title">外部会话监工（C 方案）</h3>
        <div className="field toggle-field">
          <label>启用</label>
          <div className="row">
            <span className="toggle-desc">
              盯 codex 的 rollout 日志，见到「任务完成」就播一句。适合<strong>
              你自己开终端跑 codex</strong> 的场景 —— 过程你看得见，它只在
              <strong>你走开时</strong>帮你喊一声
            </span>
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
              留空时用 codex 的默认落点。只有改过 <code>CODEX_HOME</code> 才需要填。
              没装 codex 时目录不存在 ⇒ 什么都不会发生，开着也无害。
            </p>
          </div>
        )}
        <p className="hint">
          ⚠️ <strong>它不产生未读</strong>：结果就在你正开着的终端里、永久驻留，
          不需要再来一颗"有结果没听"的灯 —— 那属于重复记账，也会稀释
          "灯亮 = 真有东西等你听"这条契约。
          <br />
          另外它<strong>只播固定语料</strong>："任务已完成"。摘要只显示在屏幕上，不进语音 ——
          动态文本要现调 TTS，那就毁掉了"预合成一次、断网也能播"这个前提。
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
