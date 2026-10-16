/**
 * StyleCam 风格调色盘 —— 入口
 *
 * 职责（DESIGN §5）：只做生命周期，**不放业务逻辑**。
 *   activate  → initHost + 载入持久化 + 注册命令 + registerPage
 *   renderPage→ 同步返回 { refresh, dispose }（页签契约 P1）
 *   deactivate→ 摘干净页签视图
 *
 * 宿主事实（已核实，别再查）：
 *   - 宿主**只调** `lifecycle.dispose()`，`refresh()` 从不调（PluginPageContainer.tsx:21-27）
 *     —— 契约要求有，所以照样导出，但别指望它被调用
 *   - 若激活未完成时卸载，`lifecycle` 仍为 null → dispose 永不执行（偏离 #8）
 *     —— 因此 keys.js 的 handler 首行自带 `isConnected` 自愈
 *   - `src/`（宿主前端）与 `src-tauri/` 一行都不许改
 */

import {
  STYLES,
  CREATIVE_MODES,
  CODE,
  STAGE_HINTS,
} from './constants.js';

import {
  planImageFit,
  clampStrength,
  strengthBand,
  buildPrompt,
  buildGenerateArgs,
  parseScriptOutput,
  mapHardwareKey,
  isTypingTarget,
  summarizeState,
  summarizeGallery,
  quantizePalette,
  sniffExt,
  applyGalleryLimit,
} from './logic.js';

import { initHost, getCtx, hasHost, safeLog, resetHost } from './host.js';

import {
  getState,
  subscribe,
  loadConfig,
  loadGallery,
  saveConfig,
  setMounted,
  setStage,
  resetSession,
  resetAll,
} from './state.js';

import { COMMANDS, TOOLS_COMMANDS, setThumbMaker } from './cmds.js';

import { buildView, destroyView, renderAll, showOverlay, toast, makeThumbB64 } from './ui.js';

import { resetKeyCounters, getAttachCount, getDetachCount } from './keys.js';

// ───────────────────────────────────────────────────────────────────────────
// 运行时（每次 activate 重建）
// ───────────────────────────────────────────────────────────────────────────

let _ctx = null;
let _view = null; // 当前挂载的页签视图（同一时刻最多一个）
let _unsub = null;
let _pendingDiscardNotice = false; // 上次有未保存结果被丢弃 → 切回时提示一次

// ───────────────────────────────────────────────────────────────────────────
// 生命周期
// ───────────────────────────────────────────────────────────────────────────

/**
 * 宿主调用一次。注册 15 条命令 + 1 个页签。
 * @param {{api:object, pluginId:string, pluginDir:string, registerCommand:Function, registerPage:Function}} context
 */
export async function activate(context) {
  _ctx = context || null;
  initHost(context);
  safeLog('风格调色盘开始激活');

  // 持久化数据先异步载入，页签挂载时可能还没回来 —— 回来后靠订阅自动重渲染。
  Promise.resolve()
    .then(() => loadConfig())
    .then(() => loadGallery())
    .then(() => {
      if (_view) renderAll(_view);
    })
    .catch((e) => safeLog(`持久化载入失败：${e && e.message ? e.message : e}`, 'warn'));

  // ── 命令注册 ──
  // 宿主调用时可能带参（pluginHost.ts `invokePluginCommand` 会透传 args，如硬件键绑定 /
  // ⌘SS 面板）；无参调用给 `{}` 兜底，命令内部从 state 自取上下文，缺上下文返回 NOT_READY。
  const ids = Object.keys(COMMANDS);
  for (let i = 0; i < ids.length; i += 1) {
    const id = ids[i];
    const fn = COMMANDS[id];
    context.registerCommand(id, (args) => Promise.resolve(fn(args || {})));
  }
  safeLog(`已注册 ${ids.length} 条命令；其中进 ⌘SS 的 ${TOOLS_COMMANDS.length} 条会额外打日志`);

  // ── 页签贡献点 ──
  // 宿主尚未实现 registerPage 时静默跳过（对齐 studio 的做法，不因清单多了 pages 就崩）。
  if (typeof context.registerPage === 'function') {
    context.registerPage('stylecam.board', renderPage);
  } else {
    safeLog('宿主未提供 registerPage，已跳过页签注册', 'warn');
  }

  safeLog(`风格调色盘已激活（pluginDir=${context && context.pluginDir ? context.pluginDir : '未提供'}）`);
}

/** 宿主卸载插件时调用：摘干净页签，不留 DOM / keydown / <style>。 */
export function deactivate() {
  destroyPage();
  _ctx = null;
  resetHost();
  safeLog('风格调色盘已停用');
}

// ───────────────────────────────────────────────────────────────────────────
// 页签
// ───────────────────────────────────────────────────────────────────────────

/** 摘掉当前页签视图（幂等）。 */
function destroyPage() {
  if (_unsub) {
    _unsub();
    _unsub = null;
  }
  if (_view) {
    // 未保存的结果在此丢弃（PRD 5.1：切回时给一次温和提示）
    _pendingDiscardNotice = resetSession();
    setMounted(false);
    destroyView(_view);
    _view = null;
  }
}

/**
 * 宿主页签渲染函数（契约 P1：**同步**返回生命周期对象，不可为 Promise）。
 * 幂等：重复渲染先摘旧的（契约 P5 + DESIGN T21）。
 *
 * @param {HTMLElement} container 宿主容器（不移动、不改定位 —— 契约 P3）
 * @returns {{refresh: () => void, dispose: () => void}}
 */
export function renderPage(container) {
  if (!container || typeof document === 'undefined') {
    safeLog('renderPage 未拿到容器，页签无法渲染', 'warn');
    return { refresh() {}, dispose() {} };
  }

  // 幂等：宿主重复挂载 / 热重载时不挂两份面板，也不重复注入 <style>
  destroyPage();
  while (container.firstChild) container.removeChild(container.firstChild);

  const view = buildView(container);
  _view = view;

  // 缩略图由前端 canvas 产出（脚本无 PIL）—— ui.js 注入给 cmds.js 用
  setThumbMaker(makeThumbB64);

  setMounted(true);
  setStage('browse');

  _unsub = subscribe(() => {
    if (_view) renderAll(_view);
  });

  renderAll(view);

  // 首次进页签给一次硬件键说明（PRD 决策 6：默认开，但要说明别和 PTT 抢键）
  const st = getState();
  if (st.config.hwKeys && !st.config.hwIntroSeen) {
    showOverlay(view, 'hwIntro');
  }
  // 跨会话的「上次结果没保存」温和提示
  if (_pendingDiscardNotice) {
    _pendingDiscardNotice = false;
    toast(view, CODE.LAST_RESULT_DISCARDED, 'warn');
  }

  safeLog('风格调色盘页签已挂载');

  return {
    /** 宿主从不调它（PluginPageContainer.tsx:21-27），契约要求有，保留实现。 */
    refresh() {
      if (_view) renderAll(_view);
    },
    dispose() {
      destroyPage();
      safeLog('风格调色盘页签已摘除');
    },
  };
}

// ───────────────────────────────────────────────────────────────────────────
// 测试缝（QA 靠它做脱离 DOM 的断言 —— DESIGN 任务书第 9 条）
// ───────────────────────────────────────────────────────────────────────────

export const __test = {
  // ── 硬要求：这 7 项必须有 ──
  planImageFit,
  mapHardwareKey,
  clampStrength,
  buildPrompt,
  parseScriptOutput,
  STYLES,
  CREATIVE_MODES,

  // ── 其余纯函数（同一批断言常用） ──
  strengthBand,
  buildGenerateArgs,
  isTypingTarget,
  summarizeState,
  summarizeGallery,
  quantizePalette,
  sniffExt,
  applyGalleryLimit,

  // ── 常量与文案 ──
  CODE,
  STAGE_HINTS,

  // ── 生命周期与状态 ──
  activate,
  deactivate,
  renderPage,
  destroyPage,
  getState,
  subscribe,
  loadConfig,
  loadGallery,
  saveConfig,
  resetAll,

  // ── 宿主门面 / 键计数 ──
  initHost,
  getCtx,
  hasHost,
  safeLog,
  getAttachCount,
  getDetachCount,
  resetKeyCounters,

  // ── 命令表 ──
  COMMANDS,
  TOOLS_COMMANDS,
};

export default { activate, deactivate, renderPage, __test };
