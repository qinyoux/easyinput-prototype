/**
 * 实时翻译插件（live-translate）—— 页签 UI。
 *
 * 宿主契约（plugin_host 在激活时提供）：
 *   context.api            —— eo.* 宿主能力（storage / log / listen / invoke / ...）
 *   context.registerPage   —— 注册页签渲染函数（页签激活时由宿主调 renderFn(container)）
 *   context.registerCommand—— 声明命令实现（id 须与 plugin.json 一致）
 *
 * 设计要点：
 *   - 原生 JS + DOM，不引入框架（与 demo-quick-note 一致的插件风格）。
 *   - ⛔ 不用 innerHTML 拼接内容：字幕文本来自 ASR，直接拼 HTML 等于把
 *     识别结果当代码执行。所有文本一律走 textContent。
 *   - 事件订阅在 dispose 时解除，否则页签切走后旧监听仍在更新已卸载的 DOM。
 *
 * 后端契约（Rust 侧 live_translate.rs + asr.rs）：
 *   命令：lt_start {mode,sourceLang,targetLang} / lt_stop / lt_status / lt_export {format}
 *   事件：lt-state / lt-partial / lt-final / lt-correction /
 *        lt-translate-chunk / lt-translate-final
 */

const PAGE_ID = "live-translate.board";
const CFG_KEY = "lt_config";

const DEFAULT_CFG = {
  mode: "conversation", // conversation | meeting
  source_lang: "en-US",
  target_lang: "zh-CN",
  auto_scroll: true,
  export_format: "md", // md | txt | json
};

/** 最近一次导出路径（localStorage 键）：供「打开目录」定位，避免用户自己去翻。 */
const LAST_EXPORT_KEY = "lt_last_export";

const SOURCE_LANGS = [
  { v: "en-US", label: "英语" },
  { v: "zh-CN", label: "中文" },
  { v: "auto", label: "自动" },
];
const TARGET_LANGS = [
  { v: "zh-CN", label: "中文" },
  { v: "en-US", label: "英语" },
];

/** 说话人配色：spk0 / spk1 / ... 轮转取色，保证同一人全程同色。 */
const SPEAKER_COLORS = ["#4f9cf9", "#f9a04f", "#7b5cff", "#2bb673", "#e05c8a", "#38b2ac"];

function speakerIndex(speakerId) {
  const m = /(\d+)/.exec(speakerId || "");
  return m ? parseInt(m[1], 10) : 0;
}

function speakerColor(speakerId) {
  return SPEAKER_COLORS[speakerIndex(speakerId) % SPEAKER_COLORS.length];
}

function speakerLabel(speakerId, mode) {
  if (mode === "conversation") {
    return speakerIndex(speakerId) === 0 ? "我方" : "对方";
  }
  return `Speaker ${speakerIndex(speakerId) + 1}`;
}

async function loadConfig(eo) {
  const stored = (await eo.storage.get(CFG_KEY)) || {};
  return { ...DEFAULT_CFG, ...stored };
}

async function saveConfig(eo, cfg) {
  await eo.storage.set(CFG_KEY, cfg);
}

// ---------------------------------------------------------------------------
// 生命周期
// ---------------------------------------------------------------------------

export function activate(context) {
  const eo = context.api;

  context.registerPage(PAGE_ID, (container) => createPage(container, eo));

  const startCmd = async () => {
    const cfg = await loadConfig(eo);
    await eo.invoke("lt_start", {
      mode: cfg.mode,
      sourceLang: "auto",
      targetLang: cfg.target_lang,
    });
  };
  const stopCmd = async () => {
    await eo.invoke("lt_stop");
  };
  const exportCmd = async () => {
    const path = await eo.invoke("lt_export", { format: "md" });
    await eo.log.info(`实时翻译记录已导出：${path}`);
    return path;
  };

  context.registerCommand("live-translate.start", startCmd);
  context.registerCommand("live-translate.stop", stopCmd);
  context.registerCommand("live-translate.export", exportCmd);
}

export function deactivate() {
  // 页签 dispose 会解除事件订阅，这里无需额外清理。
}

// ---------------------------------------------------------------------------
// 页签 UI
// ---------------------------------------------------------------------------

function createPage(container, eo) {
  container.replaceChildren();
  injectStyles(container);

  const root = el("div", "lt-root");
  const toolbar = buildToolbar();
  const stream = el("div", "lt-stream");
  root.append(toolbar.el, stream);
  container.append(root);

  // 每句一行：utterance_id → { row, originalEl, translationEl, speakerId, mode }
  const rows = new Map();
  let cfg = { ...DEFAULT_CFG };
  let running = false;
  let lastExportPath = "";

  // ── 事件处理 ──

  function setStatus(text, kind) {
    toolbar.status.textContent = text;
    toolbar.status.dataset.kind = kind || "";
  }

  function ensureRow(uid, speakerId) {
    let r = rows.get(uid);
    if (r) {
      // speaker_id 可能在 partial→final 之间漂移（设计文档 §一7：partial 阶段 speaker_id 不稳定）。
      // 以最新值为准，更新标签 / 颜色 / 分栏 / data-speaker。
      if (!r.finalized && String(speakerId) !== r.speakerId) {
        r.speakerId = String(speakerId);
        r.row.dataset.speaker = String(speakerIndex(speakerId));
        const tag = r.row.querySelector(".lt-tag");
        if (tag) {
          tag.textContent = speakerLabel(speakerId, cfg.mode);
          tag.style.background = speakerColor(speakerId);
        }
        r.row.classList.remove("lt-left", "lt-right");
        if (cfg.mode === "conversation") {
          r.row.classList.add(speakerIndex(speakerId) % 2 === 0 ? "lt-left" : "lt-right");
        }
      }
      return r;
    }
    const row = el("div", "lt-row");
    row.dataset.speaker = String(speakerIndex(speakerId));
    const head = el("div", "lt-head");
    const tag = el("span", "lt-tag");
    tag.textContent = speakerLabel(speakerId, cfg.mode);
    tag.style.background = speakerColor(speakerId);
    head.append(tag);
    const original = el("div", "lt-original");
    const translation = el("div", "lt-translation");
    row.append(head, original, translation);
    // 对话模式：spk0 靠左、其余靠右（像聊天分栏）。
    if (cfg.mode === "conversation") {
      row.classList.add(speakerIndex(speakerId) % 2 === 0 ? "lt-left" : "lt-right");
    }
    stream.append(row);
    r = { row, originalEl: original, translationEl: translation, speakerId, finalized: false };
    rows.set(uid, r);
    maybeScroll();
    return r;
  }

  function onState(p) {
    running = !!(p && p.running);
    if (p && p.error) {
      setStatus(`错误：${p.error}`, "error");
    } else if (running) {
      setStatus(p && p.stopping ? "正在停止…" : "翻译中…", "on");
    } else {
      setStatus("未运行", "");
    }
    toolbar.startBtn.textContent = running ? "停止" : "开始";
    toolbar.startBtn.classList.toggle("lt-on", running);
  }

  function onPartial(p) {
    // 中间结果：原文实时刷新（译文区留空，避免半句被误当定稿）。
    const r = ensureRow(p.utterance_id, p.speaker_id);
    // **定稿保护**：已定稿的行不再接受中间结果覆盖。
    // 旧缺陷：换人/迟到帧把已定稿句当成"当前句"继续覆盖式写入 → 末尾文本消失。
    if (r.finalized) return;
    r.originalEl.textContent = p.text;
    r.row.classList.add("lt-partial");
  }

  function onFinal(p) {
    const r = ensureRow(p.utterance_id, p.speaker_id);
    r.originalEl.textContent = p.text;
    r.finalized = true;
    r.row.classList.remove("lt-partial");
    r.row.classList.add("lt-final");
    if (!r.translationEl.textContent) {
      r.translationEl.textContent = "…";
    }
  }

  // **迟到修订**：服务端二遍重识别回补了已定稿句的原文。
  // 改写原文即可，译文沿用旧版（不重翻译，避免重复调用与闪烁）。
  function onCorrection(p) {
    const r = rows.get(p.utterance_id);
    if (!r) return;
    r.originalEl.textContent = p.text;
    r.finalized = true;
    r.row.classList.add("lt-final");
  }

  function onChunk(p) {
    const r = rows.get(p.utterance_id);
    if (!r) return;
    if (r.translationEl.textContent === "…") r.translationEl.textContent = "";
    r.translationEl.textContent += p.delta;
    maybeScroll();
  }

  function onTranslationFinal(p) {
    const r = rows.get(p.utterance_id);
    if (!r) return;
    r.translationEl.textContent = p.translation;
    r.translationEl.classList.remove("lt-pending");
    maybeScroll();
  }

  function maybeScroll() {
    if (cfg.auto_scroll) stream.scrollTop = stream.scrollHeight;
  }

  // ── 事件订阅（dispose 时解除）──
  const unlisteners = [];
  const setup = (async () => {
    unlisteners.push(await eo.listen("lt-state", onState));
    unlisteners.push(await eo.listen("lt-partial", onPartial));
    unlisteners.push(await eo.listen("lt-final", onFinal));
    unlisteners.push(await eo.listen("lt-correction", onCorrection));
    unlisteners.push(await eo.listen("lt-translate-chunk", onChunk));
    unlisteners.push(await eo.listen("lt-translate-final", onTranslationFinal));
  })();

  // ── 交互绑定 ──
  toolbar.startBtn.addEventListener("click", async () => {
    try {
      if (running) {
        await eo.invoke("lt_stop");
      } else {
        await eo.invoke("lt_start", {
          mode: cfg.mode,
          sourceLang: "auto",
          targetLang: cfg.target_lang,
        });
      }
    } catch (e) {
      setStatus(`操作失败：${e}`, "error");
    }
  });

  toolbar.modeSel.addEventListener("change", async () => {
    cfg.mode = toolbar.modeSel.value;
    await saveConfig(eo, cfg);
    // 运行中不允许切换模式（需停止后切），提示而不是静默忽略。
    if (running) {
      setStatus("模式已保存，停止后生效", "warn");
    }
    applyModeClass();
  });

  toolbar.srcSel.addEventListener("change", async () => {
    cfg.source_lang = toolbar.srcSel.value;
    await saveConfig(eo, cfg);
  });

  toolbar.tgtSel.addEventListener("change", async () => {
    cfg.target_lang = toolbar.tgtSel.value;
    await saveConfig(eo, cfg);
  });

  toolbar.exportBtn.addEventListener("click", async () => {
    try {
      const fmt = toolbar.fmtSel.value;
      const path = await eo.invoke("lt_export", { format: fmt });
      cfg.export_format = fmt;
      await saveConfig(eo, cfg);
      lastExportPath = path;
      await eo.storage.set(LAST_EXPORT_KEY, path);
      setStatus(`已导出：${path}`, "ok");
    } catch (e) {
      setStatus(`导出失败：${e}`, "error");
    }
  });

  // 清空 = 只清屏幕显示；后端 sentences 不动，导出仍含本次会话全部记录。
  toolbar.clearBtn.addEventListener("click", () => {
    rows.clear();
    stream.replaceChildren();
    setStatus("屏幕已清空（记录仍保留，导出含全部内容）", "ok");
  });

  // 打开目录：定位最近一次导出的文件所在目录（explorer 只认反斜杠路径）。
  toolbar.openDirBtn.addEventListener("click", async () => {
    const p = lastExportPath || (await eo.storage.get(LAST_EXPORT_KEY)) || "";
    if (!p) {
      setStatus("还没有导出过，先点「导出」", "warn");
      return;
    }
    const win = String(p).replace(/\//g, "\\");
    const dir = win.replace(/\\[^\\]*$/, "");
    try {
      await eo.run("explorer.exe", [dir], { timeoutSecs: 15 });
      setStatus(`已打开目录：${dir}`, "ok");
    } catch (e) {
      setStatus(`打开目录失败：${e}`, "error");
    }
  });

  toolbar.autoChk.addEventListener("change", async () => {
    cfg.auto_scroll = toolbar.autoChk.checked;
    await saveConfig(eo, cfg);
  });

  function applyModeClass() {
    root.dataset.mode = cfg.mode;
    // 已有行按新模式重排左右分栏。
    for (const [uid, r] of rows) {
      r.row.classList.remove("lt-left", "lt-right");
      if (cfg.mode === "conversation") {
        r.row.classList.add(speakerIndex(r.speakerId) % 2 === 0 ? "lt-left" : "lt-right");
      }
      const tag = r.row.querySelector(".lt-tag");
      if (tag) tag.textContent = speakerLabel(r.speakerId, cfg.mode);
      void uid;
    }
  }

  // ── 初始化：加载配置并回填控件 ──
  (async () => {
    cfg = await loadConfig(eo);
    toolbar.modeSel.value = cfg.mode;
    toolbar.srcSel.value = cfg.source_lang;
    toolbar.tgtSel.value = cfg.target_lang;
    toolbar.autoChk.checked = !!cfg.auto_scroll;
    toolbar.fmtSel.value = cfg.export_format || "md";
    lastExportPath = (await eo.storage.get(LAST_EXPORT_KEY)) || "";
    applyModeClass();
    try {
      const st = await eo.invoke("lt_status");
      onState(st);
    } catch (_) {
      setStatus("未运行", "");
    }
  })();

  return {
    refresh: () => {},
    dispose: () => {
      setup
        .then(() => unlisteners.forEach((fn) => fn()))
        .catch(() => {});
    },
  };
}

function buildToolbar() {
  const el_ = el("div", "lt-toolbar");

  const modeSel = el("select", "lt-sel");
  for (const [v, label] of [
    ["conversation", "对话模式"],
    ["meeting", "会议模式"],
  ]) {
    const o = document.createElement("option");
    o.value = v;
    o.textContent = label;
    modeSel.append(o);
  }

  const srcSel = el("select", "lt-sel");
  for (const { v, label } of SOURCE_LANGS) {
    const o = document.createElement("option");
    o.value = v;
    o.textContent = label;
    srcSel.append(o);
  }

  const srcHint = el("span", "lt-srchint");
  srcHint.textContent = "自动识别";
  srcHint.title = "对话/会议模式下源语种由系统自动识别（说话人分离要求），无需手动选择";

  const arrow = el("span", "lt-arrow");
  arrow.textContent = "→";

  const tgtSel = el("select", "lt-sel");
  for (const { v, label } of TARGET_LANGS) {
    const o = document.createElement("option");
    o.value = v;
    o.textContent = label;
    tgtSel.append(o);
  }

  const autoWrap = el("label", "lt-autowrap");
  const autoChk = document.createElement("input");
  autoChk.type = "checkbox";
  autoChk.checked = true;
  const autoTxt = el("span", "lt-autotxt");
  autoTxt.textContent = "自动滚动";
  autoWrap.append(autoChk, autoTxt);

  const startBtn = el("button", "lt-btn lt-start");
  startBtn.textContent = "开始";

  const exportBtn = el("button", "lt-btn lt-export");
  exportBtn.textContent = "导出";

  const fmtSel = el("select", "lt-sel lt-fmt");
  for (const [v, label] of [
    ["md", "Markdown"],
    ["txt", "纯文本"],
    ["json", "JSON"],
  ]) {
    const o = document.createElement("option");
    o.value = v;
    o.textContent = label;
    fmtSel.append(o);
  }

  const clearBtn = el("button", "lt-btn lt-clear");
  clearBtn.textContent = "清空";
  clearBtn.title = "只清屏幕显示，不清数据——导出仍包含本次会话全部记录";

  const openDirBtn = el("button", "lt-btn lt-opendir");
  openDirBtn.textContent = "打开目录";

  // 源语种恒由系统自动识别（说话人分离要求），下拉禁用避免误导。
  srcSel.disabled = true;
  srcSel.title = "对话/会议模式下源语种由系统自动识别，无需手动选择";

  const status = el("span", "lt-status");
  status.textContent = "未运行";

  el_.append(
    modeSel,
    srcSel,
    srcHint,
    arrow,
    tgtSel,
    autoWrap,
    startBtn,
    fmtSel,
    exportBtn,
    clearBtn,
    openDirBtn,
    status,
  );
  return {
    el: el_,
    modeSel,
    srcSel,
    srcHint,
    tgtSel,
    autoChk,
    startBtn,
    fmtSel,
    exportBtn,
    clearBtn,
    openDirBtn,
    status,
  };
}

/** 创建带 class 的元素（不写 innerHTML）。 */
function el(tag, cls) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  return e;
}

/** 注入插件样式（只注入一次；容器卸载时随 DOM 一起消失）。 */
function injectStyles(container) {
  const style = document.createElement("style");
  style.textContent = `
.lt-root{display:flex;flex-direction:column;height:100%;min-height:0;font-size:13px;color:var(--text,#e8e8ff)}
.lt-toolbar{display:flex;align-items:center;gap:8px;flex-wrap:wrap;padding:10px 12px;border-bottom:1px solid var(--border,rgba(255,255,255,.12))}
.lt-sel{background:var(--panel,#1b2233);color:inherit;border:1px solid var(--border,rgba(255,255,255,.16));border-radius:6px;padding:4px 6px}
.lt-sel:disabled{opacity:.5;cursor:not-allowed}
.lt-srchint{font-size:11px;opacity:.65;white-space:nowrap}
.lt-arrow{opacity:.6}
.lt-autowrap{display:flex;align-items:center;gap:4px;opacity:.85;cursor:pointer}
.lt-btn{border:1px solid var(--border,rgba(255,255,255,.16));background:var(--panel,#1b2233);color:inherit;border-radius:6px;padding:5px 14px;cursor:pointer}
.lt-btn:hover{filter:brightness(1.15)}
.lt-fmt{max-width:104px}
.lt-start.lt-on{background:#e05c5c;border-color:#e05c5c;color:#fff}
.lt-status{margin-left:auto;opacity:.8}
.lt-status[data-kind="on"]{color:#4f9cf9}
.lt-status[data-kind="ok"]{color:#2bb673}
.lt-status[data-kind="warn"]{color:#f9a04f}
.lt-status[data-kind="error"]{color:#e05c5c}
.lt-stream{flex:1;overflow-y:auto;padding:12px;display:flex;flex-direction:column;gap:10px;min-height:0}
.lt-row{max-width:78%;border-radius:10px;padding:8px 10px;background:var(--panel,rgba(255,255,255,.05));border-left:3px solid transparent}
.lt-row.lt-left{align-self:flex-start}
.lt-row.lt-right{align-self:flex-end}
.lt-row[data-speaker="0"]{border-left-color:#4f9cf9}
.lt-row[data-speaker="1"]{border-left-color:#f9a04f}
.lt-row[data-speaker="2"]{border-left-color:#7b5cff}
.lt-row.lt-partial{opacity:.7}
.lt-head{display:flex;align-items:center;gap:6px;margin-bottom:4px}
.lt-tag{font-size:11px;padding:1px 8px;border-radius:10px;color:#fff}
.lt-original{line-height:1.5;word-break:break-word}
.lt-translation{margin-top:6px;padding-top:6px;border-top:1px dashed var(--border,rgba(255,255,255,.12));color:var(--text-dim,#b9c0d4);line-height:1.5;word-break:break-word}
.lt-root[data-mode="meeting"] .lt-row{max-width:100%;align-self:stretch}
`;
  container.append(style);
}