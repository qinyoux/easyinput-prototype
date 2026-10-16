/**
 * StyleCam 风格调色盘 —— UI 层
 *
 * 硬边界（DESIGN §5）：
 *   - 不发网络请求、不直接调 `eo.*`（一律经 `cmds` / `host`）
 *   - 不改 `state` 的形状（只调 state 暴露的方法）
 *   - **不写 fixed 定位**（页签契约 P2 明令禁止）—— 浮层一律 `absolute` 挂在插件根
 *   - 6 种创意显示全部纯前端 Canvas + CSS，零网络请求（DESIGN §9.1）
 *
 * 依赖：constants / logic / state / cmds / keys / styles（单向，无环）
 */

import {
  STYLES,
  CREATIVE_MODES,
  STAGES,
  STAGE_LABELS,
  STRENGTH_PRESETS,
  LIMITS,
  CODE,
  STAGE_HINTS,
  SERVICE_BADGE,
  TOASTS,
  PRIVACY_PROVIDER_FALLBACK,
} from './constants.js';

import { STYLE_SAMPLES } from './samples.js';

import {
  planImageFit,
  clampStrength,
  strengthBand,
  styleById,
  quantizePalette,
  formatBytes,
  formatElapsed,
  formatStamp,
  fillTemplate,
} from './logic.js';

import { getState, setGalleryOpen, setStage, setMainIndex, saveConfig, setStrength } from './state.js';

import {
  cmdImageAdd,
  cmdImageRemove,
  cmdImageClear,
  cmdStylePick,
  cmdStrengthSet,
  cmdGenerate,
  cmdResultSave,
  cmdResultExport,
  cmdCreativeRender,
  cmdGalleryRemove,
  cmdConfigSet,
  cmdConfigValidate,
  cmdConfigReset,
  cmdKeyStatus,
  cmdKeySet,
  cmdProviderList,
  cmdDoctor,
  cmdHardware,
  cmdResultDiscard,
  PRIVACY_REQUIRED,
} from './cmds.js';

import { attachHwKeys } from './keys.js';
import { injectStyle, removeStyle } from './styles.js';
import { runRaw, safeLog } from './host.js';

// ───────────────────────────────────────────────────────────────────────────
// §0 DOM 小工具
// ───────────────────────────────────────────────────────────────────────────

function d() {
  return typeof document !== 'undefined' ? document : null;
}

/** 建元素。`cls` 走 sc- 前缀；`attrs` 里的键写进 dataset（驼峰转短横线）。 */
function h(tag, cls, text) {
  const doc = d();
  const el = doc.createElement(tag);
  if (cls) el.className = cls;
  if (text != null) el.textContent = String(text);
  return el;
}

function btn(label, cls, onClick, variant, action) {
  const b = h('button', cls || 'sc-btn', label);
  b.type = 'button';
  if (variant) b.setAttribute('data-variant', variant);
  if (action) b.setAttribute('data-action', action);
  if (onClick) b.addEventListener('click', onClick);
  return b;
}

function clear(el) {
  if (!el) return;
  while (el.firstChild) el.removeChild(el.firstChild);
}

function show(el, on) {
  if (!el) return;
  el.style.display = on ? '' : 'none';
}

// ───────────────────────────────────────────────────────────────────────────
// §1 视图：buildView / destroyView
// ───────────────────────────────────────────────────────────────────────────

/**
 * 建面板。只建骨架容器，内容由 renderAll 填。
 * @param {HTMLElement} container 宿主容器（**不移动、不改定位**，契约 P3）
 */
export function buildView(container) {
  const doc = d();
  const view = {
    container,
    root: null,
    style: null,
    els: {},
    detachKeys: null,
    detachPaste: null,
    unsub: null,
    compare: 50, // 原图对比滑杆（纯视图态，不进 state）
    errZone: '', // 错误显示在「用户刚操作的那个区域」
    overlay: null,
    toastTimer: 0,
    disposed: false,
    thumbCache: new Map(),
    gallerySelected: null,
  };

  const root = h('div', 'stylecam-panel');
  root.setAttribute('data-sc', 'panel');
  view.root = root;

  // ── 顶部工具栏：状态语义居左，设置退居右上 ──
  const topbar = h('div', 'sc-topbar');
  topbar.setAttribute('data-sc', 'topbar');
  const brand = h('div', 'sc-brand');
  brand.appendChild(h('strong', '', 'StyleCam 工作台'));
  const stages = h('div', 'sc-stages');
  stages.setAttribute('data-sc', 'stages');
  brand.appendChild(stages);
  const tools = h('div', 'sc-top-tools');
  const summary = h('div', 'sc-summary');
  summary.setAttribute('data-sc', 'summary');
  tools.appendChild(summary);
  tools.appendChild(btn('设置', 'sc-tool-btn', () => showOverlay(view, 'settings'), 'ghost', 'open-settings'));
  topbar.appendChild(brand);
  topbar.appendChild(tools);
  root.appendChild(topbar);

  const banner = h('div', 'sc-error');
  banner.setAttribute('data-sc', 'banner');
  banner.setAttribute('data-level', 'warn');
  show(banner, false);
  root.appendChild(banner);

  // ── 四区创作工作台：素材架 / 创作机器 / 强度 / 作品带 ──
  const material = h('section', 'sc-material-shelf sc-workbench-card');
  material.setAttribute('data-sc', 'assets');
  material.setAttribute('data-role', 'material-shelf');
  root.appendChild(material);
  view.els.assets = material;

  const machine = h('div', 'sc-machine-grid');
  const stage = h('main', 'sc-creation-stage sc-workbench-card');
  stage.setAttribute('data-sc', 'creation-stage');
  stage.setAttribute('data-role', 'creation-stage');
  const stageZones = ['styles', 'generate', 'result', 'creative'];
  for (let i = 0; i < stageZones.length; i += 1) {
    const z = stageZones[i];
    const sec = h('div', `sc-stage-zone sc-stage-zone-${z}`);
    sec.setAttribute('data-sc', z);
    stage.appendChild(sec);
    view.els[z] = sec;
  }
  const strength = h('aside', 'sc-strength-dial sc-workbench-card');
  strength.setAttribute('data-sc', 'strength');
  strength.setAttribute('data-role', 'strength-dial');
  view.els.strength = strength;
  machine.appendChild(stage);
  machine.appendChild(strength);
  root.appendChild(machine);

  const gallery = h('section', 'sc-gallery-strip sc-workbench-card');
  gallery.setAttribute('data-sc', 'gallery');
  gallery.setAttribute('data-role', 'gallery-strip');
  root.appendChild(gallery);
  view.els.gallery = gallery;

  const hintbar = h('div', 'sc-hintbar');
  hintbar.setAttribute('data-sc', 'hintbar');
  hintbar.setAttribute('data-role', 'next-action');
  root.appendChild(hintbar);
  view.els.hintbar = hintbar;

  container.appendChild(root);
  view.style = injectStyle(doc);
  bindEvents(view);
  return view;
}

/**
 * 摘干净：DOM + <style> + window keydown + window paste + 订阅。
 * 宿主只调 `dispose`（PluginPageContainer.tsx:26），但这里必须一次摘全（DESIGN §10.4.5）。
 */
export function destroyView(view) {
  if (!view) return;
  view.disposed = true;
  if (typeof view.detachKeys === 'function') view.detachKeys();
  if (typeof view.detachPaste === 'function') view.detachPaste();
  if (typeof view.unsub === 'function') view.unsub();
  view.detachKeys = null;
  view.detachPaste = null;
  view.unsub = null;
  if (view.overlay && view.overlay.parentNode) view.overlay.parentNode.removeChild(view.overlay);
  view.overlay = null;
  if (view.root && view.root.parentNode) view.root.parentNode.removeChild(view.root);
  view.root = null;
  removeStyle(d());
  if (view.toastTimer) {
    clearTimeout(view.toastTimer);
    view.toastTimer = 0;
  }
}

// ───────────────────────────────────────────────────────────────────────────
// §2 Toast / 浮层
// ───────────────────────────────────────────────────────────────────────────

/** Toast：强度边界这类**提示**走这里，不进错误码（PRD 5.2 尾）。 */
export function toast(view, text, level) {
  if (!view || !view.root || !text) return;
  const old = view.root.querySelector('.sc-toast');
  if (old && old.parentNode) old.parentNode.removeChild(old);
  const t = h('div', 'sc-toast', text);
  t.setAttribute('data-sc', 'toast');
  if (level) t.setAttribute('data-level', level);
  view.root.appendChild(t);
  if (view.toastTimer) clearTimeout(view.toastTimer);
  view.toastTimer = setTimeout(() => {
    if (t.parentNode) t.parentNode.removeChild(t);
    view.toastTimer = 0;
  }, 2600);
}

/**
 * 页内浮层。一律 `position:absolute`（契约 P2）——宿主容器没有 relative，
 * 定位基准是插件根 `.stylecam-panel{position:relative}`。
 * @param {'privacy'|'hwIntro'|'zoom'|'settings'|null} kind
 */
export function showOverlay(view, kind) {
  if (!view || !view.root) return;
  if (view.overlay && view.overlay.parentNode) view.overlay.parentNode.removeChild(view.overlay);
  view.overlay = null;
  if (!kind) return;

  const st = getState();
  const ov = h('div', 'sc-overlay');
  ov.setAttribute('data-sc', 'overlay');
  ov.setAttribute('data-kind', String(kind));

  const head = h('div', 'sc-overlay-head');
  const body = h('div', 'sc-overlay-body');
  const foot = h('div', 'sc-row');

  if (kind === 'privacy') {
    head.appendChild(h('div', 'sc-overlay-title', '隐私告知'));
    const p = h('div', '', fillTemplate(STAGE_HINTS.privacy, {
      provider: (st.doctor && st.doctor.providerName) || PRIVACY_PROVIDER_FALLBACK,
    }));
    p.className = 'sc-hint-line';
    body.appendChild(p);
    const label = h('label', 'sc-row');
    const cb = h('input');
    cb.type = 'checkbox';
    label.appendChild(cb);
    label.appendChild(h('span', '', '不再提示'));
    body.appendChild(label);
    foot.appendChild(btn('取消', 'sc-btn', () => showOverlay(view, null), 'ghost'));
    foot.appendChild(btn('继续', 'sc-btn', async () => {
      const remember = !!cb.checked;
      showOverlay(view, null);
      await run(view, 'strength', () => cmdGenerate({ ackPrivacy: true, remember }));
    }, 'primary'));
  } else if (kind === 'hwIntro') {
    head.appendChild(h('div', 'sc-overlay-title', '硬件键说明'));
    const p = h('div', 'sc-hint-line', STAGE_HINTS.hw_intro);
    body.appendChild(p);
    foot.appendChild(btn('知道了', 'sc-btn', async () => {
      showOverlay(view, null);
      await saveConfig({ hwIntroSeen: true });
      renderAll(view);
    }, 'primary'));
  } else if (kind === 'zoom') {
    const r = st.result;
    head.appendChild(h('div', 'sc-overlay-title', r && r.styleName ? r.styleName : '结果'));
    if (r && r.dataUrl) {
      const img = h('img');
      img.src = r.dataUrl;
      img.alt = '生成结果';
      body.appendChild(img);
    } else {
      body.appendChild(h('div', 'sc-empty', CODE.TOO_LARGE_PREVIEW));
    }
    foot.appendChild(btn('关闭', 'sc-btn', () => showOverlay(view, null), 'ghost'));
    if (r && r.path) {
      foot.appendChild(btn('打开目录', 'sc-btn', () => openDir(dirOf(r.path))));
    }
  } else if (kind === 'settings') {
    head.appendChild(h('div', 'sc-overlay-title', '设置'));
    const form = h('div', 'sc-settings-form');
    const note = h('div', 'sc-settings-note', '');
    const fields = {};
    const addField = (key, label, type, values) => {
      const row = h('label', 'sc-setting-field');
      row.appendChild(h('span', '', label));
      let input;
      if (type === 'select') {
        input = h('select');
        (values || []).forEach((v) => { const opt = h('option', '', v); opt.value = v; input.appendChild(opt); });
      } else { input = h('input'); input.type = type || 'text'; }
      input.value = st.config[key] == null ? '' : String(st.config[key]);
      input.setAttribute('data-config-key', key);
      fields[key] = input;
      row.appendChild(input);
      form.appendChild(row);
      return input;
    };
    form.appendChild(h('div', 'sc-settings-section-title', '图像参数'));
    addField('resolution', '分辨率', 'select', ['1K', '2K', '4K']);
    addField('width', '宽度', 'number');
    addField('height', '高度', 'number');
    addField('format', '输出格式', 'select', ['auto', 'jpeg', 'png']);
    addField('quality', 'JPEG 质量', 'number');
    addField('n', '生成数量', 'number');
    addField('outDir', '输出目录', 'text');
    form.appendChild(h('div', 'sc-settings-section-title', 'API 配置'));
    addField('provider', 'Provider', 'select', ['easysu', 'ark', 'openai_compat', 'zhipu']);
    addField('baseUrl', '接口地址', 'text');
    addField('model', '模型', 'text');
    addField('httpTimeoutSecs', 'HTTP 超时', 'number');
    addField('retryCount', '重试次数', 'number');
    const keyRow = h('div', 'sc-setting-field sc-key-row');
    keyRow.appendChild(h('span', '', 'API Key'));
    const keyInput = h('input'); keyInput.type = 'password'; keyInput.placeholder = '不会预填，留空不修改';
    const keyStatus = h('span', 'sc-key-status', '读取中…');
    const keyInputRow = h('div', 'sc-key-input-row');
    keyInputRow.appendChild(keyInput);
    const keySaveBtn = btn('保存 Key', 'sc-btn', async () => {
      const value = keyInput.value;
      if (!value) { note.textContent = '请输入新的 API Key。'; return; }
      const r = await cmdKeySet({ apiKey: value });
      keyInput.value = '';
      note.textContent = r.message;
      if (r.ok) keyStatus.textContent = `来源 ${r.data.source} · ${r.data.masked} · ${r.data.length} 位`;
    }, 'primary', 'key-set');
    keyInputRow.appendChild(keySaveBtn);
    keyRow.appendChild(keyInputRow);
    keyRow.appendChild(keyStatus);
    form.appendChild(keyRow);
    body.appendChild(form); body.appendChild(note);

    const refreshCapabilities = async () => {
      const res = await cmdProviderList();
      if (!res.ok) { note.textContent = res.message; return; }
      const current = fields.provider.value;
      const row = res.data.providers.find((x) => x.id === current);
      const cap = row && row.capabilities;
      const unsupported = cap && cap.quality === false;
      fields.quality.disabled = !!unsupported;
      fields.quality.title = unsupported ? '当前 Provider 不支持质量参数' : '';
      if (unsupported) note.textContent = '当前 Provider 不支持质量参数，请先修正后验证。';
    };
    fields.provider.addEventListener('change', refreshCapabilities);
    cmdKeyStatus().then((res) => { keyStatus.textContent = res.ok ? `来源 ${res.data.source} · ${res.data.masked} · ${res.data.length} 位` : res.message; });
    refreshCapabilities();

    const readPatch = () => {
      const patch = {};
      Object.keys(fields).forEach((k) => { if (fields[k].value !== '') patch[k] = fields[k].type === 'number' ? Number(fields[k].value) : fields[k].value; });
      return patch;
    };
    foot.appendChild(btn('验证配置', 'sc-btn', async () => { const r = await cmdConfigValidate({ patch: readPatch() }); note.textContent = r.message; }, 'ghost'));
    foot.appendChild(btn('保存配置', 'sc-btn', async () => { const r = await cmdConfigSet({ patch: readPatch() }); note.textContent = r.message; if (r.ok) renderAll(view); }, 'primary'));
    foot.appendChild(btn('自检连接', 'sc-btn', async () => { const r = await cmdDoctor({ probe: false }); note.textContent = r.message; }, 'ghost'));
    foot.appendChild(btn('重置非敏感配置', 'sc-btn', async () => { const r = await cmdConfigReset(); note.textContent = r.message; if (r.ok) showOverlay(view, 'settings'); }, 'danger'));
    foot.appendChild(btn('关闭', 'sc-btn', () => showOverlay(view, null), 'ghost'));
  }

  ov.appendChild(head);
  ov.appendChild(body);
  ov.appendChild(foot);
  view.root.appendChild(ov);
  view.overlay = ov;
}

function dirOf(p) {
  const s = String(p || '');
  const i = Math.max(s.lastIndexOf('\\'), s.lastIndexOf('/'));
  return i > 0 ? s.slice(0, i) : s;
}

/** 「打开目录」：调 explorer（纯便捷，不回传值）。 */
function openDir(dir) {
  if (!dir) return;
  runRaw('explorer', [String(dir)], LIMITS.SHORT_RUN_TIMEOUT_SECS);
}

// ───────────────────────────────────────────────────────────────────────────
// §3 命令执行与错误归位
// ───────────────────────────────────────────────────────────────────────────

/**
 * 跑一条命令，并把结果落到 UI：
 *   - 错误显示在**用户刚操作的那个区域**（PRD 5.2 尾规则 1）
 *   - data.toast 走 toast（强度边界这类提示）
 *   - PRIVACY_REQUIRED → 弹隐私告知浮层
 */
async function run(view, zone, fn) {
  view.errZone = zone;
  let res = null;
  try {
    res = await fn();
  } catch (e) {
    safeLog(`命令执行异常：${e && e.message ? e.message : e}`, 'error');
    res = { ok: false, code: 'BAD_ARGS', data: null, message: CODE.BAD_ARGS };
  }
  if (!res) return res;

  if (res.code === PRIVACY_REQUIRED) {
    showOverlay(view, 'privacy');
    return res;
  }
  if (res.data && typeof res.data === 'object' && res.data.toast) {
    toast(view, res.data.toast, res.data.blocked ? 'warn' : 'info');
  }
  if (!res.ok && res.code === 'RESULT_UNSAVED') toast(view, res.message, 'error');
  renderAll(view);
  return res;
}

/** 错误条：两行（发生了什么 / 你现在该做什么）+ 旁边必有「自检」按钮（PRD A8）。 */
function errorBox(view, zone) {
  const st = getState();
  if (!st.lastError || view.errZone !== zone) return null;
  const box = h('div', 'sc-error');
  box.setAttribute('data-sc', 'error');
  const text = h('div', '', st.lastError.message || '');
  box.appendChild(text);
  const actions = h('div', 'sc-error-actions');
  actions.appendChild(btn('自检', 'sc-btn', async () => {
    const res = await cmdDoctor({});
    toast(view, res.message, res.ok ? 'info' : 'error');
    renderAll(view);
  }));
  box.appendChild(actions);
  return box;
}

// ───────────────────────────────────────────────────────────────────────────
// §4 事件绑定
// ───────────────────────────────────────────────────────────────────────────

export function bindEvents(view) {
  const g = typeof globalThis !== 'undefined' ? globalThis : null;

  // ── 硬件键：只产意图，动作由 cmdHardware 派发 ──
  view.detachKeys = attachHwKeys(view, (intent) => {
    // 对比滑杆是纯视图态，就地处理，不惊动命令层
    if (intent.action === 'compareSlide') {
      view.compare = Math.min(100, Math.max(0, view.compare + (Number(intent.arg) || 0)));
      renderCreative(view);
      return;
    }
    cmdHardware({ intent }).then((res) => {
      if (res && !res.ok && res.message) toast(view, res.message, 'error');
      else if (res && res.data && res.data.toast) toast(view, res.data.toast, res.data.blocked ? 'warn' : 'info');
      renderAll(view);
    });
  });

  // ── 剪贴板图片：`eo.clipboard_read` 只返回文本（偏离 #9），只能走 paste 事件 ──
  if (g && typeof g.addEventListener === 'function') {
    const onPaste = (e) => {
      const st = getState();
      if (!st.mounted) return;
      const items = e.clipboardData && e.clipboardData.items ? e.clipboardData.items : [];
      const files = [];
      for (let i = 0; i < items.length; i += 1) {
        const it = items[i];
        if (it && it.kind === 'file' && String(it.type || '').indexOf('image/') === 0) {
          const f = typeof it.getAsFile === 'function' ? it.getAsFile() : null;
          if (f) files.push(f);
        }
      }
      if (!files.length) {
        view.errZone = 'assets';
        toast(view, CODE.CLIPBOARD_NO_IMAGE, 'error');
        return;
      }
      if (typeof e.preventDefault === 'function') e.preventDefault();
      ingestFiles(view, files);
    };
    g.addEventListener('paste', onPaste);
    view.detachPaste = () => {
      try {
        g.removeEventListener('paste', onPaste);
      } catch (e) {
        /* ignore */
      }
    };
  }
}

// ───────────────────────────────────────────────────────────────────────────
// §5 素材摄取（拖拽 / 粘贴 / 选择文件）
// ───────────────────────────────────────────────────────────────────────────

async function ingestFiles(view, files) {
  const list = Array.prototype.slice.call(files || []);
  for (let i = 0; i < list.length; i += 1) {
    // eslint-disable-next-line no-await-in-loop
    await ingestOne(view, list[i]);
  }
}

async function ingestOne(view, file) {
  const g = typeof globalThis !== 'undefined' ? globalThis : null;
  const size = Number(file && file.size) || 0;
  if (!g || typeof g.createImageBitmap !== 'function') {
    view.errZone = 'assets';
    toast(view, CODE.BAD_ARGS, 'error');
    return;
  }

  let bmp = null;
  try {
    bmp = await g.createImageBitmap(file);
  } catch (e) {
    // ≥8192px 等超大图 → WebView2 解码失败（PRD 5.1）
    view.errZone = 'assets';
    await cmdImageAdd({ dataUrl: '', bytes: size, w: 0, h: 0, decodeFailed: true });
    renderAll(view);
    return;
  }

  const plan = planImageFit(bmp.width, bmp.height, size || 1, 'main');
  if (!plan.ok) {
    if (typeof bmp.close === 'function') bmp.close();
    view.errZone = 'assets';
    await cmdImageAdd({ dataUrl: '', bytes: size, w: bmp.width, h: bmp.height, decodeFailed: plan.code === 'IMG_DECODE_FAILED' });
    renderAll(view);
    return;
  }

  const doc = d();
  const long = Math.max(bmp.width, bmp.height);
  const scale = Math.min(1, plan.longEdge / long);
  const w = Math.max(1, Math.round(bmp.width * scale));
  const hh = Math.max(1, Math.round(bmp.height * scale));
  const canvas = doc.createElement('canvas');
  canvas.width = w;
  canvas.height = hh;
  const ctx = canvas.getContext('2d');
  ctx.drawImage(bmp, 0, 0, w, hh);
  if (typeof bmp.close === 'function') bmp.close();

  const dataUrl = canvas.toDataURL('image/jpeg', plan.q);
  const comma = dataUrl.indexOf(',');
  const bytes = comma >= 0 ? Math.round((dataUrl.length - comma - 1) * 0.75) : size;

  await run(view, 'assets', () => cmdImageAdd({ dataUrl, bytes, w, h: hh }));
}

// ───────────────────────────────────────────────────────────────────────────
// §6 缩略图（脚本无 PIL → 前端 canvas 产出，DESIGN 2.1 偏离 #1）
// ───────────────────────────────────────────────────────────────────────────

function b64ToBlob(b64, mime) {
  const g = typeof globalThis !== 'undefined' ? globalThis : null;
  const bin = g && typeof g.atob === 'function' ? g.atob(b64) : '';
  const arr = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i += 1) arr[i] = bin.charCodeAt(i);
  return new (g.Blob)([arr], { type: mime || 'image/jpeg' });
}

/** 320px / q0.82 缩略图（前端 canvas 产出；脚本无 PIL，DESIGN 2.1 偏离 #1）。 */
export async function makeThumbB64(dataUrl) {
  const g = typeof globalThis !== 'undefined' ? globalThis : null;
  if (!g || typeof g.createImageBitmap !== 'function') return '';
  const comma = String(dataUrl).indexOf(',');
  if (comma < 0) return '';
  const head = String(dataUrl).slice(0, comma);
  const mime = head.indexOf('image/png') >= 0 ? 'image/png' : 'image/jpeg';
  const b64 = String(dataUrl).slice(comma + 1);

  const bmp = await g.createImageBitmap(b64ToBlob(b64, mime));
  const long = Math.max(bmp.width, bmp.height);
  const scale = Math.min(1, LIMITS.THUMB_EDGE / long);
  const w = Math.max(1, Math.round(bmp.width * scale));
  const hh = Math.max(1, Math.round(bmp.height * scale));
  const canvas = d().createElement('canvas');
  canvas.width = w;
  canvas.height = hh;
  canvas.getContext('2d').drawImage(bmp, 0, 0, w, hh);
  if (typeof bmp.close === 'function') bmp.close();
  const out = canvas.toDataURL('image/jpeg', LIMITS.THUMB_QUALITY);
  const c2 = out.indexOf(',');
  return c2 >= 0 ? out.slice(c2 + 1) : '';
}

// ───────────────────────────────────────────────────────────────────────────
// §7 总渲染
// ───────────────────────────────────────────────────────────────────────────

/**
 * 找真正在滚动的容器：宿主容器 / 面板根 / 文档滚动元素，按序取第一个「内容超高」的。
 * 防御式：harness 桩 DOM 没有 scrollTop/scrollHeight 字段时安全跳过。
 */
function findScrollView(view) {
  const cands = [view.container, view.root];
  const doc = typeof document !== 'undefined' ? document : null;
  if (doc) cands.push(doc.scrollingElement || null, doc.documentElement || null);
  for (let i = 0; i < cands.length; i += 1) {
    const el = cands[i];
    if (el && typeof el.scrollTop === 'number' && typeof el.scrollHeight === 'number'
      && typeof el.clientHeight === 'number' && el.scrollHeight > el.clientHeight + 1) return el;
  }
  return null;
}

/** 焦点在插件根内则返回描述符（能唯一指回该控件的 data-* 属性），否则 null。 */
function focusDescriptor(view, el) {
  if (!el || typeof el.getAttribute !== 'function') return null;
  // 只认插件根内的元素（用 parentNode 链判断，桩 DOM 没有 contains）
  let n = el;
  while (n && n !== view.root) n = n.parentNode;
  if (!n) return null;
  const attrs = ['data-style-id', 'data-action', 'data-sc'];
  for (let i = 0; i < attrs.length; i += 1) {
    const val = el.getAttribute(attrs[i]);
    if (val) return { attr: attrs[i], val };
  }
  return null;
}

/** 树遍历找第一个 attributes[attr]===val 的元素（桩 DOM 的 querySelectorAll 不支持通用选择器）。 */
function findByAttr(root, attr, val) {
  if (!root) return null;
  const stack = [root];
  while (stack.length) {
    const cur = stack.pop();
    if (cur.attributes && cur.attributes[attr] === val) return cur;
    const kids = cur.children || [];
    for (let i = kids.length - 1; i >= 0; i -= 1) stack.push(kids[i]);
  }
  return null;
}

export function renderAll(view) {
  if (!view || view.disposed || !view.root) return;
  // 防跳顶：全量重建瞬间内容塌缩 + 聚焦控件被销毁，浏览器会把滚动钳到 0。
  // 重建前记下滚动位置与焦点，重建后原样放回（同步完成，塌缩中间态不会触发重排）。
  const sc = findScrollView(view);
  const scrollTop = sc ? sc.scrollTop : 0;
  const doc = typeof document !== 'undefined' ? document : null;
  const fd = focusDescriptor(view, doc ? doc.activeElement : null);
  renderTop(view);
  renderBanner(view);
  renderAssets(view);
  renderStyles(view);
  renderStrength(view);
  renderGenerating(view);
  renderResult(view);
  renderCreative(view);
  renderGallery(view);
  renderHintBar(view);
  if (sc) {
    try { sc.scrollTop = scrollTop; } catch (e) { /* 桩 DOM 无滚动，忽略 */ }
  }
  if (fd) {
    const el = findByAttr(view.root, fd.attr, fd.val);
    if (el && typeof el.focus === 'function') {
      try { el.focus({ preventScroll: true }); } catch (e) { try { el.focus(); } catch (e2) { /* 忽略 */ } }
    }
  }
}

/** 顶部：阶段指示条 + 中文参数摘要（不显示风格 id，PRD A3）。 */
function renderTop(view) {
  const st = getState();
  const stagesEl = view.root.querySelector('[data-sc="stages"]');
  const summaryEl = view.root.querySelector('[data-sc="summary"]');
  if (!stagesEl || !summaryEl) return;

  clear(stagesEl);
  const cur = STAGES.indexOf(st.stage);
  for (let i = 0; i < STAGES.length; i += 1) {
    if (i > 0) stagesEl.appendChild(h('span', 'sc-stage-sep', '→'));
    const chip = h('span', 'sc-stage', (i < cur ? '✓ ' : '') + STAGE_LABELS[STAGES[i]]);
    chip.setAttribute('data-state', i < cur ? 'done' : i === cur ? 'current' : 'todo');
    stagesEl.appendChild(chip);
  }

  clear(summaryEl);
  const style = styleById(st.styleId);
  const bits = [];
  bits.push(h('strong', '', style ? style.name : '未选风格'));
  bits.push(h('span', '', ` · 强度 ${clampStrength(st.strength)}%`));
  bits.push(h('span', '', ` · ${st.size || '2K'}`));
  for (let i = 0; i < bits.length; i += 1) summaryEl.appendChild(bits[i]);
}

/** 黄条：Key 不可用（PRD 5.1 adjusting 行的常驻黄条）。 */
function renderBanner(view) {
  const st = getState();
  const el = view.root.querySelector('[data-sc="banner"]');
  if (!el) return;
  const bad = st.doctor && st.doctor.keyOk === false;
  show(el, !!bad);
  if (!bad) return;
  clear(el);
  el.appendChild(h('div', '', STAGE_HINTS.key_bad));
  const actions = h('div', 'sc-error-actions');
  actions.appendChild(btn('自检', 'sc-btn', async () => {
    const res = await cmdDoctor({});
    toast(view, res.message, res.ok ? 'info' : 'error');
    renderAll(view);
  }));
  el.appendChild(actions);
}

// ── 素材区 ────────────────────────────────────────────────────────────────

export function renderAssets(view) {
  const st = getState();
  const sec = view.els.assets;
  if (!sec) return;
  show(sec, true);
  clear(sec);

  const title = h('div', 'sc-section-title');
  title.appendChild(h('span', '', '素材'));
  title.appendChild(h('span', 'sc-count', `${st.images.length} / ${LIMITS.MAX_IMAGES} · 拖入或粘贴`));
  sec.appendChild(title);

  const shelf = h('div', 'sc-material-track');
  shelf.setAttribute('data-role', 'material-track');
  const zone = h('div', 'sc-dropzone sc-add-material');
  zone.setAttribute('data-sc', 'dropzone');
  zone.setAttribute('data-action', 'add-material');
  zone.appendChild(h('strong', '', '＋ 添加素材'));
  zone.appendChild(h('span', '', '拖入 / Ctrl+V / 选择文件'));
  zone.addEventListener('click', () => {
    const input = view.root.querySelector('input[type="file"][data-sc="file"]');
    if (input) input.click();
  });

  const input = h('input');
  input.type = 'file';
  input.accept = 'image/*';
  input.multiple = true;
  input.setAttribute('data-sc', 'file');
  input.style.display = 'none';
  input.addEventListener('change', () => {
    ingestFiles(view, input.files);
    input.value = '';
  });
  zone.appendChild(input);
  shelf.appendChild(zone);

  const onDrag = (on) => (e) => {
    if (typeof e.preventDefault === 'function') e.preventDefault();
    zone.setAttribute('data-drag', on ? '1' : '0');
  };
  zone.addEventListener('dragover', onDrag(true));
  zone.addEventListener('dragenter', onDrag(true));
  zone.addEventListener('dragleave', onDrag(false));
  zone.addEventListener('drop', (e) => {
    if (typeof e.preventDefault === 'function') e.preventDefault();
    zone.setAttribute('data-drag', '0');
    const dt = e.dataTransfer;
    const files = dt && dt.files ? dt.files : [];
    if (files && files.length) ingestFiles(view, files);
  });
  for (let i = 0; i < st.images.length; i += 1) {
    const img = st.images[i];
    const cell = h('div', 'sc-thumb sc-material-card');
    cell.setAttribute('data-selected', i === st.mainIndex ? '1' : '0');
    cell.setAttribute('data-role', 'material-card');
    const im = h('img');
    im.src = img.dataUrl;
    im.alt = `素材 ${i + 1}`;
    cell.appendChild(im);
    cell.appendChild(h('span', 'sc-thumb-tag', i === st.mainIndex ? '已装载' : '参考素材'));
    const del = btn('×', 'sc-thumb-del', () => run(view, 'assets', () => cmdImageRemove({ id: img.id })), null, 'remove-material');
    del.title = '删除这张';
    cell.appendChild(del);
    const mainBtn = btn(i === st.mainIndex ? '当前主体' : '装载为主体', 'sc-thumb-main', () => {
      setMainIndex(i);
      renderAll(view);
    }, null, 'load-material');
    cell.appendChild(mainBtn);
    im.addEventListener('click', () => {
      setMainIndex(i);
      renderAll(view);
    });
    shelf.appendChild(cell);
  }
  sec.appendChild(shelf);
  if (st.images.length) sec.appendChild(btn('清空素材', 'sc-btn sc-clear-materials', () => run(view, 'assets', cmdImageClear), 'ghost', 'clear-materials'));

  const err = errorBox(view, 'assets');
  if (err) sec.appendChild(err);
}

// ── 风格宫格 ──────────────────────────────────────────────────────────────

export function renderStyles(view) {
  const st = getState();
  const sec = view.els.styles;
  if (!sec) return;
  show(sec, true);
  clear(sec);

  const cur = styleById(st.styleId);
  const hero = h('div', 'sc-machine-head');
  const copy = h('div');
  copy.appendChild(h('div', 'sc-kicker', st.images.length ? '当前创作风格' : '先装载素材'));
  copy.appendChild(h('h2', 'sc-machine-title', cur ? cur.name : '选择一种风格'));
  copy.appendChild(h('div', 'sc-machine-subtitle', cur ? cur.hint : '装载素材后，从下方 10 种风格中选择创作方向。'));
  hero.appendChild(copy);
  const statePill = h('div', 'sc-state-pill', stageStatusText(st));
  statePill.setAttribute('data-stage', st.stage);
  hero.appendChild(statePill);
  sec.appendChild(hero);

  const preview = h('div', 'sc-machine-preview');
  preview.setAttribute('data-busy', st.busy ? '1' : '0');
  if (cur) {
    preview.style['--sc-style-glow'] = cur.palette[1];
    preview.style['--sc-style-accent'] = cur.palette[2];
  }
  const main = st.images.length ? st.images[Math.min(st.mainIndex, st.images.length - 1)] : null;
  if (main && main.dataUrl) {
    const img = h('img');
    img.src = main.dataUrl;
    img.alt = '当前装载素材';
    preview.appendChild(img);
    preview.appendChild(h('span', 'sc-loaded-flag', '已装载'));
  } else {
    preview.appendChild(h('div', 'sc-machine-empty', '拖入、粘贴或选择图片，素材会装载到这里'));
  }
  sec.appendChild(preview);

  const railHead = h('div', 'sc-rail-head');
  railHead.appendChild(h('span', '', '风格轨道'));
  railHead.appendChild(h('span', 'sc-count', '数字键 1–9、0 选择'));
  sec.appendChild(railHead);
  const rail = h('div', 'sc-style-rail');
  rail.setAttribute('data-role', 'style-track');
  for (let i = 0; i < STYLES.length; i += 1) {
    const s = STYLES[i];
    const card = h('button', 'sc-style-card');
    card.type = 'button';
    card.setAttribute('data-style-id', s.id);
    card.setAttribute('data-selected', st.styleId === s.id ? '1' : '0');
    card.setAttribute('data-sc', 'style-card');
    card.setAttribute('data-action', 'pick-style');
    card.disabled = !st.images.length;
    const sw = h('span', 'sc-style-swatch');
    const sample = STYLE_SAMPLES[s.id];
    if (sample) {
      const im = h('img', 'sc-style-sample');
      im.src = sample;
      im.alt = `${s.name} 风格样品`;
      im.decoding = 'async';
      im.setAttribute('data-sc', 'style-sample');
      sw.appendChild(im);
    } else {
      sw.style.background = s.grad;
    }
    card.appendChild(sw);
    card.appendChild(h('span', 'sc-style-name', `${i + 1 === 10 ? 0 : i + 1}. ${s.name}`));
    card.appendChild(h('span', 'sc-style-hint', s.hint));
    card.addEventListener('click', () => run(view, 'styles', () => cmdStylePick({ styleId: s.id })));
    rail.appendChild(card);
  }
  sec.appendChild(rail);
  if (cur) sec.appendChild(h('div', 'sc-style-context', `适合：${cur.fit} · 不建议：${cur.avoid}`));

  const err = errorBox(view, 'styles');
  if (err) sec.appendChild(err);
}

// ── 强度 ──────────────────────────────────────────────────────────────────

function stageStatusText(st) {
  if (st.busy || st.stage === 'generating') return '生成中 · 请等待出片';
  if (st.stage === 'creative') return `创意显示 · ${CREATIVE_MODES[st.creative].name}`;
  if (st.stage === 'result') return '结果已出片 · 选择下一步';
  if (st.styleId) return '调整强度 · 下一步生成';
  if (st.images.length) return '选择风格 · 下一步调强度';
  return '装载素材 · 下一步选风格';
}

export function renderStrength(view) {
  const st = getState();
  const sec = view.els.strength;
  if (!sec) return;
  show(sec, true);
  clear(sec);

  const title = h('div', 'sc-section-title');
  title.appendChild(h('span', '', '强度'));
  const badge = btn(serviceBadgeText(st), 'sc-badge', async () => {
    const res = await cmdDoctor({});
    toast(view, res.message, res.ok ? 'info' : 'error');
    renderAll(view);
  }, null, 'doctor');
  badge.setAttribute('data-level', serviceLevel(st));
  title.appendChild(badge);
  sec.appendChild(title);

  const v = clampStrength(st.strength);
  const band = strengthBand(v);
  const dial = h('div', 'sc-dial-face');
  dial.style['--sc-strength'] = String(v);
  dial.appendChild(h('strong', '', `${v}%`));
  dial.appendChild(h('span', '', band.name));
  sec.appendChild(dial);
  sec.appendChild(h('div', 'sc-mode-copy', stageStatusText(st)));

  const control = h('div', 'sc-vertical-control');
  const slider = h('input', 'sc-slider sc-strength-input');
  slider.type = 'range';
  slider.min = '0';
  slider.max = '100';
  slider.step = '1';
  slider.value = String(v);
  slider.setAttribute('aria-label', '风格强度');
  slider.setAttribute('data-role', 'strength-input');
  slider.disabled = !st.images.length || !st.styleId || !!st.busy;
  slider.addEventListener('input', () => {
    setStrength(clampStrength(slider.value));
    renderStrength(view);
    renderHintBar(view);
    renderTop(view);
  });
  slider.addEventListener('change', () => run(view, 'strength', () => cmdStrengthSet({ value: slider.value })));
  const ticks = h('div', 'sc-strength-ticks');
  [100, 75, 50, 25, 0].forEach((n) => ticks.appendChild(h('span', '', String(n))));
  control.appendChild(slider);
  control.appendChild(ticks);
  sec.appendChild(control);

  const presets = h('div', 'sc-presets sc-strength-presets');
  for (let i = 0; i < STRENGTH_PRESETS.length; i += 1) {
    const p = STRENGTH_PRESETS[i];
    presets.appendChild(btn(`${p.name} ${p.value}%`, 'sc-btn', () => run(view, 'strength', () => cmdStrengthSet({ value: p.value })), p.value === v ? 'primary' : 'ghost', 'set-strength'));
  }
  sec.appendChild(presets);
  if (v > LIMITS.STRENGTH_WARN) sec.appendChild(h('div', 'sc-hint-line', TOASTS.STRENGTH_WARN));

  const genBtn = btn(st.busy ? '正在生成…' : '生成并出片', 'sc-btn sc-generate-btn', () => doGenerate(view), 'primary', 'generate');
  genBtn.disabled = !st.images.length || !st.styleId || !!st.busy;
  sec.appendChild(genBtn);
  sec.appendChild(h('div', 'sc-next-copy', st.busy ? '下一步：等待结果从机器下方出片' : '下一步：按 Enter 或点击生成'));

  const err = errorBox(view, 'strength');
  if (err) sec.appendChild(err);
}

function serviceLevel(st) {
  if (!st.doctor || !st.doctor.at) return 'unknown';
  if (st.doctor.keyOk === false) return 'bad';
  return st.doctor.ok ? 'ok' : 'unknown';
}

function serviceBadgeText(st) {
  const lv = serviceLevel(st);
  const map = { ok: SERVICE_BADGE.ok, unknown: SERVICE_BADGE.unknown, bad: SERVICE_BADGE.bad };
  return `生图服务：${map[lv]}${lv === 'unknown' ? ' · 点这里自检' : ''}`;
}

// ── 生成中 ────────────────────────────────────────────────────────────────

export function renderGenerating(view) {
  const st = getState();
  const sec = view.els.generate;
  if (!sec) return;
  const on = st.stage === 'generating' || (!!st.busy);
  show(sec, on && !hasErrorHere(view, 'generate'));
  if (!on) {
    const err = errorBox(view, 'generate');
    if (err) {
      show(sec, true);
      clear(sec);
      const title = h('div', 'sc-section-title');
      title.appendChild(h('span', '', '生图'));
      sec.appendChild(title);
      const prog = h('div', 'sc-progress');
      prog.setAttribute('data-error', '1');
      prog.appendChild(h('i'));
      sec.appendChild(prog);
      sec.appendChild(err);
      const acts = h('div', 'sc-presets');
      acts.appendChild(btn('重试', 'sc-btn', () => doGenerate(view), 'primary'));
      sec.appendChild(acts);
    }
    return;
  }
  clear(sec);

  const title = h('div', 'sc-section-title');
  title.appendChild(h('span', '', '生图'));
  sec.appendChild(title);

  const prog = h('div', 'sc-progress');
  prog.appendChild(h('i'));
  sec.appendChild(prog);

  const waited = st.busy && st.busy.startedAt ? Math.round((Date.now() - st.busy.startedAt) / 1000) : 0;
  sec.appendChild(h('div', 'sc-generating-note', `已等待 ${waited} 秒 / 上限 ${LIMITS.RUN_TIMEOUT_SECS} 秒`));
  sec.appendChild(h('div', 'sc-generating-note', STAGE_HINTS.generating_busy));

  // 倒计时靠订阅刷新；这里补一个定时器（dispose 会被 renderAll 覆盖，无泄漏风险）
  setTimeout(() => {
    if (!view.disposed && getState().busy) renderGenerating(view);
  }, 1000);
}

function hasErrorHere(view, zone) {
  const st = getState();
  return !!(st.lastError && view.errZone === zone);
}

// ── 结果 ──────────────────────────────────────────────────────────────────

export function renderResult(view) {
  const st = getState();
  const sec = view.els.result;
  if (!sec) return;
  const on = !!st.result && (st.stage === 'result' || st.stage === 'creative');
  show(sec, on);
  if (!on) return;
  clear(sec);

  const r = st.result;
  const title = h('div', 'sc-section-title');
  title.appendChild(h('span', '', '结果'));
  title.appendChild(h('span', 'sc-count', r.saved ? '已存进作品集' : '未保存'));
  sec.appendChild(title);

  const stage = h('div', 'sc-result-stage sc-photo-output');
  stage.setAttribute('data-role', 'photo-output');
  if (r.dataUrl) {
    const img = h('img');
    img.src = r.dataUrl;
    img.alt = '生成结果';
    img.addEventListener('click', () => showOverlay(view, 'zoom'));
    stage.appendChild(img);
  } else {
    stage.appendChild(h('div', 'sc-empty', CODE.TOO_LARGE_PREVIEW));
  }
  sec.appendChild(stage);

  // 元信息行：`<中文风格名> · 强度 <n>% · <size> · 用时 <x> 秒 · <KB>`（风格名一律取自 STYLES[].name）
  const bits = [r.styleName || '自定义', `强度 ${clampStrength(r.strength)}%`, r.size || '2K'];
  if (r.elapsedMs) bits.push(`用时 ${formatElapsed(r.elapsedMs)}`);
  if (r.bytes) bits.push(formatBytes(r.bytes));
  sec.appendChild(h('div', 'sc-result-meta', bits.join(' · ')));
  if (r.path) sec.appendChild(h('div', 'sc-mono', r.path));

  const acts = h('div', 'sc-presets');
  acts.appendChild(btn('存进作品集（S1）', 'sc-btn', () => run(view, 'result', cmdResultSave), 'primary'));
  acts.appendChild(btn('创意显示（S2）', 'sc-btn', () => run(view, 'result', () => cmdCreativeRender({ index: st.creative + 1 }))));
  acts.appendChild(btn('导出到磁盘', 'sc-btn', () => run(view, 'result', cmdResultExport)));
  acts.appendChild(btn('放弃并重来（S3）', 'sc-btn', () => run(view, 'result', cmdResultDiscard), 'danger'));
  if (r.path) acts.appendChild(btn('打开目录', 'sc-btn', () => openDir(dirOf(r.path))));
  sec.appendChild(acts);

  const err = errorBox(view, 'result');
  if (err) sec.appendChild(err);
}

// ── 创意显示（6 种，纯前端，零网络） ──────────────────────────────────────

export function renderCreative(view) {
  const st = getState();
  const sec = view.els.creative;
  if (!sec) return;
  const on = !!st.result && st.stage === 'creative';
  show(sec, on);
  if (!on) return;
  clear(sec);

  const title = h('div', 'sc-section-title');
  title.appendChild(h('span', '', '创意显示'));
  title.appendChild(h('span', 'sc-count', `第 ${st.creative + 1} / ${CREATIVE_MODES.length} 种`));
  sec.appendChild(title);

  const sw = h('div', 'sc-creative-switch');
  for (let i = 0; i < CREATIVE_MODES.length; i += 1) {
    const m = CREATIVE_MODES[i];
    const chip = btn(`${i + 1}. ${m.name}`, 'sc-creative-chip', () => run(view, 'creative', () => cmdCreativeRender({ index: i + 1 })));
    chip.setAttribute('data-active', i === st.creative ? '1' : '0');
    sw.appendChild(chip);
  }
  sec.appendChild(sw);

  const box = h('div', 'sc-result-stage');
  box.setAttribute('data-sc', 'creative-stage');
  const r = st.result;
  const orig = st.images.length ? st.images[Math.min(st.mainIndex, st.images.length - 1)].dataUrl : '';
  const mode = CREATIVE_MODES[st.creative];

  if (mode && mode.id === 'palette') {
    // ④ 是唯一需要 canvas 的一种：异步取样，填充完就地替换，不阻塞其它形态
    const node = h('div', 'sc-creative');
    node.setAttribute('data-mode', 'palette');
    box.appendChild(node);
    crPalette(node, r).catch((e) => {
      clear(node);
      node.appendChild(h('div', 'sc-empty', CODE.CREATIVE_FAILED));
      safeLog(`创意显示渲染失败：${e && e.message ? e.message : e}`, 'warn');
    });
  } else {
    let node = null;
    try {
      if (mode.id === 'compare') node = crCompare(view, r, orig);
      else if (mode.id === 'polaroid') node = crPolaroid(r);
      else if (mode.id === 'magazine') node = crMagazine(r);
      else if (mode.id === 'loupe') node = crLoupe(r);
      else node = crCarousel(r, orig);
    } catch (e) {
      // 单个形态渲染失败只影响当前形态（T40）
      node = h('div', 'sc-empty', CODE.CREATIVE_FAILED);
      safeLog(`创意显示渲染失败：${e && e.message ? e.message : e}`, 'warn');
    }
    box.appendChild(node);
  }
  sec.appendChild(box);

  sec.appendChild(h('div', 'sc-hint-line', `按 1–6 切换显示形态 · 按 S1 保存当前显示 · ${CREATIVE_MODES[st.creative].hint}`));

  const acts = h('div', 'sc-presets');
  acts.appendChild(btn('保存当前显示（S1）', 'sc-btn', () => run(view, 'creative', cmdResultSave), 'primary'));
  acts.appendChild(btn('回结果区（Esc）', 'sc-btn', () => {
    setStage('result');
    renderAll(view);
  }, 'ghost'));
  sec.appendChild(acts);

  const err = errorBox(view, 'creative');
  if (err) sec.appendChild(err);
}

/** ① 原图对比：两层叠放 + clip-path 滑动（纯 CSS）。 */
function crCompare(view, r, orig) {
  const box = h('div', 'sc-creative');
  box.setAttribute('data-mode', 'compare');
  if (r && r.dataUrl) {
    const under = h('img');
    under.src = r.dataUrl;
    under.setAttribute('data-layer', 'bottom');
    box.appendChild(under);
  }
  if (orig) {
    const over = h('img');
    over.src = orig;
    over.setAttribute('data-layer', 'top');
    over.style.clipPath = `inset(0 ${100 - view.compare}% 0 0)`;
    box.appendChild(over);
  }
  const slider = h('input', 'sc-slider');
  slider.type = 'range';
  slider.min = '0';
  slider.max = '100';
  slider.value = String(view.compare);
  slider.style.position = 'absolute';
  slider.style.bottom = '6px';
  slider.style.left = '8%';
  slider.style.width = '84%';
  slider.addEventListener('input', () => {
    view.compare = Number(slider.value);
    renderCreative(view);
  });
  box.appendChild(slider);
  return box;
}

/** ② 拍立得相框：不均等白边 + 投影 + 轻微旋转（纯 CSS）。 */
function crPolaroid(r) {
  const box = h('div', 'sc-creative');
  box.setAttribute('data-mode', 'polaroid');
  if (r && r.dataUrl) {
    const img = h('img');
    img.src = r.dataUrl;
    box.appendChild(img);
  }
  const bits = [r && r.styleName ? r.styleName : '', r ? `强度 ${clampStrength(r.strength)}%` : '', formatStamp(Date.now())];
  box.appendChild(h('div', 'sc-polaroid-cap', bits.filter(Boolean).join(' · ')));
  return box;
}

/** ③ 杂志跨页：grid 两列 + background-position 左右各半（纯 CSS）。 */
function crMagazine(r) {
  const box = h('div', 'sc-creative');
  box.setAttribute('data-mode', 'magazine');
  const url = r && r.dataUrl ? r.dataUrl : '';
  const left = h('div', 'sc-mag-page');
  left.setAttribute('data-side', 'left');
  left.style.backgroundImage = `url("${url}")`;
  const gut = h('div', 'sc-mag-gutter');
  const right = h('div', 'sc-mag-page');
  right.setAttribute('data-side', 'right');
  right.style.backgroundImage = `url("${url}")`;
  box.appendChild(left);
  box.appendChild(gut);
  box.appendChild(right);
  return box;
}

/**
 * ④ 色板拆解：唯一需要 canvas 的一种（64×64 离屏取样 + 4bit/通道分桶计数）。
 * `data:` URL 不污染 canvas，无 CORS 问题，零网络请求。
 * @param {HTMLElement} box 由 renderCreative 建好的容器（异步填充，便于不阻塞其它形态）
 */
async function crPalette(box, r) {
  if (!r || !r.dataUrl) return box;

  const g = typeof globalThis !== 'undefined' ? globalThis : null;
  const comma = String(r.dataUrl).indexOf(',');
  const head = String(r.dataUrl).slice(0, comma);
  const mime = head.indexOf('image/png') >= 0 ? 'image/png' : 'image/jpeg';
  const bmp = await g.createImageBitmap(b64ToBlob(String(r.dataUrl).slice(comma + 1), mime));

  const doc = d();
  const canvas = doc.createElement('canvas');
  canvas.width = 64;
  canvas.height = 64;
  const ctx = canvas.getContext('2d');
  ctx.drawImage(bmp, 0, 0, 64, 64);
  if (typeof bmp.close === 'function') bmp.close();
  const data = ctx.getImageData(0, 0, 64, 64).data;
  const top = quantizePalette(data, 64, 64, 6);
  clear(box);

  const strip = h('div', 'sc-palette-strip');
  for (let i = 0; i < top.length; i += 1) {
    const seg = h('span');
    seg.style.background = top[i].hex;
    seg.style.flex = String(Math.max(0.04, top[i].ratio));
    strip.appendChild(seg);
  }
  box.appendChild(strip);

  for (let i = 0; i < top.length; i += 1) {
    const row = h('div', 'sc-palette-row');
    const dot = h('span', 'sc-palette-dot');
    dot.style.background = top[i].hex;
    row.appendChild(dot);
    row.appendChild(h('span', '', top[i].hex));
    row.appendChild(h('span', '', `rgb(${top[i].rgb.join(', ')})`));
    row.appendChild(h('span', '', `${(top[i].ratio * 100).toFixed(1)}%`));
    box.appendChild(row);
  }
  return box;
}

/** ⑤ 细节放大镜（替换胶片连拍）：4 个圆形取四象限 + 1 个中心，background-size 300%。 */
function crLoupe(r) {
  const box = h('div', 'sc-creative');
  box.setAttribute('data-mode', 'loupe');
  const url = r && r.dataUrl ? r.dataUrl : '';
  const spots = [
    ['left top', '左上'],
    ['right top', '右上'],
    ['left bottom', '左下'],
    ['right bottom', '右下'],
  ];
  for (let i = 0; i < spots.length; i += 1) {
    const c = h('div', 'sc-loupe');
    c.style.backgroundImage = `url("${url}")`;
    c.style.backgroundPosition = spots[i][0];
    c.title = spots[i][1];
    box.appendChild(c);
  }
  const center = h('div', 'sc-loupe');
  center.style.backgroundImage = `url("${url}")`;
  center.style.backgroundPosition = 'center';
  center.style.gridColumn = '1 / -1';
  center.style.width = '56%';
  center.style.justifySelf = 'center';
  center.title = '中心';
  box.appendChild(center);
  return box;
}

/** ⑥ 前后轮播：两层叠放 + @keyframes 交叉淡入淡出（reduced-motion 由 CSS 降级）。 */
function crCarousel(r, orig) {
  const box = h('div', 'sc-creative');
  box.setAttribute('data-mode', 'carousel');
  if (orig) {
    const a = h('img');
    a.src = orig;
    a.setAttribute('data-layer', 'bottom');
    box.appendChild(a);
  }
  if (r && r.dataUrl) {
    const b = h('img');
    b.src = r.dataUrl;
    b.setAttribute('data-layer', 'top');
    box.appendChild(b);
  }
  return box;
}

// ── 作品集（叠加态） ──────────────────────────────────────────────────────

export function renderGallery(view) {
  const st = getState();
  const sec = view.els.gallery;
  if (!sec) return;
  show(sec, true);
  clear(sec);

  const bar = h('div', 'sc-gallery-bar');
  const left = h('div', 'sc-row');
  left.appendChild(h('span', '', `作品 ${st.gallery.length} / ${LIMITS.MAX_GALLERY}`));
  const strip = h('div', 'sc-gallery-strip');
  strip.setAttribute('data-role', 'gallery-track');
  const recent = st.gallery.slice(0, 12);
  if (!view.gallerySelected && recent.length) view.gallerySelected = recent[0].id;
  for (let i = 0; i < recent.length; i += 1) {
    const it = recent[i];
    if (!it.thumbB64) continue;
    const im = h('img');
    im.src = `data:image/jpeg;base64,${it.thumbB64}`;
    im.alt = it.styleName || '作品';
    im.setAttribute('data-selected', view.gallerySelected === it.id ? '1' : '0');
    im.addEventListener('click', () => { view.gallerySelected = it.id; renderGallery(view); });
    im.addEventListener('dblclick', () => showZoomItem(view, it));
    strip.appendChild(im);
  }
  left.appendChild(strip);
  bar.appendChild(left);
  bar.appendChild(btn(st.galleryOpen ? '收起' : '全部作品', 'sc-btn', () => {
    setGalleryOpen(!getState().galleryOpen);
    renderGallery(view);
  }, 'ghost', 'toggle-gallery'));
  sec.appendChild(bar);

  const selected = st.gallery.find((it) => it && it.id === view.gallerySelected) || st.gallery[0];
  if (selected) {
    const actions = h('div', 'sc-gallery-actions');
    actions.appendChild(h('span', 'sc-gallery-selection', `${selected.styleName || '自定义'} · 强度 ${clampStrength(selected.strength)}%`));
    actions.appendChild(btn('查看', 'sc-btn', () => showZoomItem(view, selected), 'ghost', 'view-work'));
    if (st.result) {
      actions.appendChild(btn('原图对比', 'sc-btn', () => run(view, 'gallery', () => cmdCreativeRender({ index: 1 })), 'ghost', 'compare-work'));
      actions.appendChild(btn('创意显示', 'sc-btn', () => run(view, 'gallery', () => cmdCreativeRender({ index: st.creative + 1 })), 'ghost', 'creative-work'));
      actions.appendChild(btn('保存当前结果', 'sc-btn', () => run(view, 'gallery', cmdResultSave), 'primary', 'save-work'));
    }
    actions.appendChild(btn('删除', 'sc-btn', () => run(view, 'gallery', () => cmdGalleryRemove({ id: selected.id })), 'danger', 'delete-work'));
    sec.appendChild(actions);
  }

  if (st.galleryOpen) {
    if (!st.gallery.length) {
      sec.appendChild(h('div', 'sc-empty', STAGE_HINTS.gallery_empty));
      return;
    }
    const grid = h('div', 'sc-gallery-grid');
    for (let i = 0; i < st.gallery.length; i += 1) {
      const it = st.gallery[i];
      const cell = h('div', 'sc-gallery-item');
      const im = h('img');
      im.src = it.thumbB64 ? `data:image/jpeg;base64,${it.thumbB64}` : '';
      im.alt = it.styleName || '作品';
      im.addEventListener('click', () => showZoomItem(view, it));
      cell.appendChild(im);
      cell.appendChild(h('div', 'sc-gallery-cap', `${it.styleName || '自定义'} · 强度 ${clampStrength(it.strength)}%`));
      cell.appendChild(h('div', 'sc-gallery-cap', formatStamp(it.at)));
      const del = btn('×', 'sc-gallery-del', () => run(view, 'gallery', () => cmdGalleryRemove({ id: it.id })));
      cell.appendChild(del);
      grid.appendChild(cell);
    }
    sec.appendChild(grid);
  }
}

/** 作品集条目放大：**在当前页签内放大查看（不重载、不重生图）**（PRD 2.7）。 */
function showZoomItem(view, it) {
  showOverlay(view, null);
  const ov = h('div', 'sc-overlay');
  ov.setAttribute('data-sc', 'overlay');
  ov.setAttribute('data-kind', 'galleryItem');
  const head = h('div', 'sc-overlay-head');
  head.appendChild(h('div', '', `${it.styleName || '自定义'} · 强度 ${clampStrength(it.strength)}%`));
  const bodyEl = h('div', 'sc-overlay-body');
  if (it.thumbB64) {
    const im = h('img');
    im.src = `data:image/jpeg;base64,${it.thumbB64}`;
    bodyEl.appendChild(im);
  }
  const foot = h('div', 'sc-row');
  if (it.path) foot.appendChild(btn('打开所在目录', 'sc-btn', () => openDir(dirOf(it.path))));
  foot.appendChild(btn('关闭', 'sc-btn', () => {
    if (ov.parentNode) ov.parentNode.removeChild(ov);
    if (view.overlay === ov) view.overlay = null;
  }, 'ghost'));
  ov.appendChild(head);
  ov.appendChild(bodyEl);
  ov.appendChild(foot);
  view.root.appendChild(ov);
  view.overlay = ov;
}

// ── 底部常驻动作提示条 ────────────────────────────────────────────────────

export function renderHintBar(view) {
  const st = getState();
  const bar = view.els.hintbar;
  if (!bar) return;
  clear(bar);

  const style = styleById(st.styleId);
  let main;
  if (st.stage === 'generating') main = STAGE_HINTS.generating_busy;
  else if (st.stage === 'result') main = '按 S1 存进作品集 · 按 S2 换一种创意显示 · 按 S3 放弃并重来';
  else if (st.stage === 'creative') main = '按 1–6 切换显示形态 · 按 S1 保存当前显示';
  else if (st.stage === 'adjusting') {
    main = style
      ? `按 Enter 生成：${style.name} / 强度 ${clampStrength(st.strength)}% / ${st.images.length} 张主体图`
      : STAGE_HINTS.browse_wrong;
  } else main = st.images.length ? '按 Enter 进入风格选择' : STAGE_HINTS.browse_empty;

  bar.appendChild(h('span', '', main));

  const right = h('span', 'sc-hint-actions');
  const keyBtn = btn(st.config.hwKeys ? '⌨ 硬件键：开' : '⌨ 硬件键：关', 'sc-btn', async () => {
    const r = await cmdConfigSet({ patch: { hwKeys: !getState().config.hwKeys } });
    if (!r.ok) toast(view, r.message || '硬件键开关切换失败', 'error');
    renderAll(view);
  }, 'ghost');
  right.appendChild(keyBtn);
  right.appendChild(btn('⚙ 设置', 'sc-btn', () => showOverlay(view, 'settings'), 'ghost'));
  right.appendChild(btn(`🗂 ${st.gallery.length}/${LIMITS.MAX_GALLERY}`, 'sc-btn', () => {
    setGalleryOpen(!getState().galleryOpen);
    renderGallery(view);
  }, 'ghost'));
  bar.appendChild(right);
}

// ───────────────────────────────────────────────────────────────────────────
// §8 生图入口（含隐私告知）
// ───────────────────────────────────────────────────────────────────────────

async function doGenerate(view) {
  const res = await run(view, 'generate', cmdGenerate);
  if (res && res.code === PRIVACY_REQUIRED) showOverlay(view, 'privacy');
}

// ───────────────────────────────────────────────────────────────────────────
// §9 测试缝
// ───────────────────────────────────────────────────────────────────────────

export const __test = {
  buildView,
  renderAll,
  renderAssets,
  renderStyles,
  renderStrength,
  renderGenerating,
  renderResult,
  renderCreative,
  renderGallery,
  renderHintBar,
  showOverlay,
  toast,
  destroyView,
  bindEvents,
  ingestFiles,
  ingestOne,
  makeThumbB64,
  crCompare,
  crPolaroid,
  crMagazine,
  crPalette,
  crLoupe,
  crCarousel,
  serviceLevel,
  serviceBadgeText,
  dirOf,
};
