/**
 * StyleCam 风格调色盘 —— 命令层（15 条）
 *
 * 硬边界（DESIGN §5 / §10.3）：
 *   - 本文件**禁止**出现宿主全局 DOM 宿主对象 / 宿主窗口对象标识符（脱离 DOM 也必须返回合法信封）
 *   - **绝不 import `ui.js`**（⌘SS 触发时页签可能从未挂载）
 *   - 需要 UI 能力的地方一律走「可安装钩子」（缩略图生成器），由 ui.js 注入
 *
 * 信封（DESIGN §6.4）：{ ok, code, data, message }
 *   ok:true  → code 恒为 'OK'
 *   ok:false → code 取自 CODE 表，message 逐字取自 PRD 5.2
 *
 * 三条 tools 命令（state / doctor / gallery.list）**额外**调 eo.log.info：
 *   ⌘SS 里 `await fn()` 丢弃返回值（pluginHost.ts:230），不打日志用户看不到任何反馈。
 */

import {
  STYLES,
  CREATIVE_MODES,
  LIMITS,
  KEYS,
  DEFAULTS,
  CODE,
  STAGE_HINTS,
  TOASTS,
  PROVIDER_HINTS,
  PRIVACY_PROVIDER_FALLBACK,
} from './constants.js';

import {
  planImageFit,
  checkImageBudget,
  clampStrength,
  strengthBand,
  styleById,
  buildPrompt,
  buildGenerateArgs,
  computeRunTimeout,
  validateConfig,
  migrateConfig,
  mapDimensionsForProvider,
  assertArgsSafe,
  parseScriptOutput,
  mapHardwareKey,
  summarizeState,
  summarizeGallery,
  formatBytes,
  formatElapsed,
  fillTemplate,
  creativeIndexFromKey,
  cycleCreativeIndex,
  isGalleryFull,
} from './logic.js';

import { runScript, logInfo, safeLog } from './host.js';

import {
  getState,
  addImage,
  removeImage,
  clearImages,
  setMainIndex,
  setStyle,
  setStrength,
  setStage,
  setResult,
  markResultSaved,
  setCreative,
  setGalleryOpen,
  setBusy,
  setDoctor,
  setLastError,
  saveConfig,
  writeSecret,
  clearSecret,
  pushGalleryItem,
  removeGalleryItem,
  writeJob,
  clearJob,
} from './state.js';

// ───────────────────────────────────────────────────────────────────────────
// §0 工程侧补码（PRD 5.2 未列，与 constants 的 IO_FAILED 同理，已存档 README）
// ───────────────────────────────────────────────────────────────────────────

/** 首次生图的隐私告知未确认 —— UI 收到后弹页内确认浮层，确认后带 ackPrivacy 重试。 */
export const PRIVACY_REQUIRED = 'PRIVACY_REQUIRED';

/** 需要一个「当前结果」但内存里没有（保存 / 导出 / 创意显示的前置）。 */
export const NEED_RESULT = 'NEED_RESULT';

/** 阶段回退表（PRD 5.1：browse 的上一阶段是它自己）。 */
export const PREV_STAGE = {
  browse: 'browse',
  adjusting: 'browse',
  generating: 'adjusting',
  result: 'adjusting',
  creative: 'result',
};

// ───────────────────────────────────────────────────────────────────────────
// §1 信封
// ───────────────────────────────────────────────────────────────────────────

/** 统一信封。ok:true 时 code 恒为 'OK'。 */
export function envelope(ok, code, data, message) {
  return {
    ok: !!ok,
    code: ok ? 'OK' : code || null,
    data: data === undefined ? null : data,
    message: message || '',
  };
}

function ok(data, message) {
  return envelope(true, 'OK', data, message);
}

/** 失败信封：message 逐字取自 CODE 表（占位符运行时替换）。 */
function fail(code, vars, data) {
  return envelope(false, code, data === undefined ? null : data, fillTemplate(CODE[code] || '', vars || {}));
}

/** 自定义文案的失败信封（用于 PRD 未列文案的工程侧补码）。 */
function failText(code, text, data) {
  return envelope(false, code, data === undefined ? null : data, text || '');
}

/** 记一次错误到 state（错误显示在用户刚操作的区域，这里只留痕）。 */
function markError(code, message) {
  setLastError(code, message);
}

/** 清掉上一次错误。 */
function clearError() {
  setLastError(null, '');
}

// ───────────────────────────────────────────────────────────────────────────
// §2 缩略图生成钩子（canvas 属 UI 能力，由 ui.js 注入，cmds 保持无 DOM）
// ───────────────────────────────────────────────────────────────────────────

let _thumbMaker = null;

/**
 * ui.js 在 buildView 时注入：`async (dataUrl) => thumbB64`。
 * 未注入时作品集仍会保存，只是 thumbB64 为空（页签里显示占位）。
 */
export function setThumbMaker(fn) {
  _thumbMaker = typeof fn === 'function' ? fn : null;
}

export function getThumbMaker() {
  return _thumbMaker;
}

async function makeThumb(dataUrl) {
  if (!_thumbMaker || !dataUrl) return '';
  try {
    const b64 = await _thumbMaker(dataUrl);
    return typeof b64 === 'string' ? b64 : '';
  } catch (e) {
    safeLog(`缩略图生成失败：${e && e.message ? e.message : e}`, 'warn');
    return '';
  }
}

// ───────────────────────────────────────────────────────────────────────────
// §3 小工具
// ───────────────────────────────────────────────────────────────────────────

let _seq = 0;

function nextJobId() {
  _seq += 1;
  return `sc-${Date.now().toString(36)}-${_seq}`;
}

function imgB64(img) {
  if (!img) return '';
  if (img.b64) return img.b64;
  const u = String(img.dataUrl || '');
  const i = u.indexOf(',');
  return i >= 0 ? u.slice(i + 1) : '';
}

function totalImageBytes(st) {
  let n = 0;
  for (let i = 0; i < st.images.length; i += 1) n += Number(st.images[i].bytes) || 0;
  return n;
}

/**
 * 未保存结果拦截（PRD 5.1 result 行 / T38）。
 * 已保存过 → 放行。
 */
function guardUnsaved() {
  const st = getState();
  if (st.result && !st.result.saved) return fail('RESULT_UNSAVED');
  return null;
}

function goPrevStage() {
  const st = getState();
  setStage(PREV_STAGE[st.stage] || 'browse');
}

// ───────────────────────────────────────────────────────────────────────────
// §4 脚本输出的行协议解析（doctor / dir / save 用；generate 走 logic.parseScriptOutput）
// ───────────────────────────────────────────────────────────────────────────

/**
 * 把 `K=V` 行解析成对象（纯函数，可脱离 DOM 单测）。
 * BEGIN_IMAGE/END_IMAGE 段会被跳过（doctor 不发图，但 generate 复用时不至于炸）。
 */
export function parseKeyValue(stdout) {
  const kv = {};
  const lines = String(stdout == null ? '' : stdout).split(/\r?\n/);
  let inImage = false;
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i].trim();
    if (!line) continue;
    if (line === 'BEGIN_IMAGE') { inImage = true; continue; }
    if (line === 'END_IMAGE') { inImage = false; continue; }
    if (inImage) continue;
    const eq = line.indexOf('=');
    if (eq > 0) kv[line.slice(0, eq)] = line.slice(eq + 1);
  }
  return kv;
}

/** 解析 `doctor` 的输出 → 结构化结果（纯函数）。 */
export function parseDoctorOutput(stdout, exitCode, timedOut) {
  const kv = parseKeyValue(stdout);
  const flag = (k) => kv[k] === '1' || kv[k] === 'true';
  return {
    ok: kv.OK === '1',
    err: kv.ERR || '',
    msg: kv.MSG || '',
    py: flag('PY'),
    pyExe: kv.PY_EXE || '',
    pyVer: kv.PY_VER || '',
    key: flag('KEY'),
    keySrc: kv.KEY_SRC || 'none',
    keyMasked: kv.KEY_MASKED || '',
    model: kv.MODEL || '',
    outDir: kv.OUTDIR || '',
    outDirWritable: flag('OUTDIR_WRITABLE'),
    // provider 注册表：换 API 只改 .env，脚本把当前生效的 provider 回传
    provider: kv.PROVIDER || '',
    providerName: kv.PROVIDER_NAME || '',
    endpoint: kv.ENDPOINT || '',
    i2i: kv.I2I === '' ? null : flag('I2I'),   // 1 = 支持图生图；0 = 退化为纯文生图
    exitCode: exitCode == null ? -1 : exitCode,
    timedOut: !!timedOut,
  };
}

/** 解析 `dir` / `save` 的输出 → { ok, path, bytes, msg }（纯函数）。 */
export function parseSimpleOutput(stdout) {
  const kv = parseKeyValue(stdout);
  return {
    ok: kv.OK === '1',
    path: kv.PATH || '',
    bytes: Number(kv.BYTES) || 0,
    msg: kv.MSG || '',
    err: kv.ERR || '',
  };
}

// ───────────────────────────────────────────────────────────────────────────
// §5 命令 1：state（tools）
// ───────────────────────────────────────────────────────────────────────────

export async function cmdState() {
  const st = getState();
  const message = summarizeState(st);
  logInfo(message); // ⌘SS 不显示返回值（pluginHost.ts:230）
  return ok(
    {
      stage: st.stage,
      images: st.images.length,
      styleId: st.styleId,
      strength: st.strength,
      result: !!st.result,
      gallery: st.gallery.length,
      doctor: st.doctor,
    },
    message,
  );
}

// ───────────────────────────────────────────────────────────────────────────
// §6 命令 2–4：素材
// ───────────────────────────────────────────────────────────────────────────

/**
 * 添加素材。图片已经由 UI 按 planImageFit 缩好图，这里只做**校验 + 落 state**。
 * @param {{dataUrl:string, bytes:number, w:number, h:number, decodeFailed?:boolean, role?:string}} p
 */
export async function cmdImageAdd(p) {
  const st = getState();
  if (!st.mounted) return fail('NOT_READY');

  const o = p || {};
  const w = Number(o.w) || 0;
  const h = Number(o.h) || 0;
  const bytes = Number(o.bytes) || 0;

  if (o.decodeFailed || Math.max(w, h) >= LIMITS.MAX_DECODE_EDGE) {
    markError('IMG_DECODE_FAILED', CODE.IMG_DECODE_FAILED);
    return fail('IMG_DECODE_FAILED');
  }
  if (!o.dataUrl) {
    markError('BAD_ARGS', CODE.BAD_ARGS);
    return fail('BAD_ARGS');
  }
  if (st.images.length >= LIMITS.MAX_IMAGES) {
    markError('MAX_IMAGES', CODE.MAX_IMAGES);
    return fail('MAX_IMAGES');
  }
  if (st.result && !st.result.saved) {
    markError('RESULT_UNSAVED', CODE.RESULT_UNSAVED);
    return fail('RESULT_UNSAVED');
  }

  const plan = planImageFit(w, h, bytes, 'main');
  if (!plan.ok) {
    markError(plan.code, fillTemplate(CODE[plan.code] || '', {}));
    return fail(plan.code);
  }
  const budget = checkImageBudget(totalImageBytes(st) + bytes);
  if (!budget.ok) {
    markError('IMG_TOO_LARGE', CODE.IMG_TOO_LARGE);
    return fail('IMG_TOO_LARGE');
  }

  clearError();
  const img = {
    id: `img-${Date.now().toString(36)}-${st.images.length + 1}`,
    role: st.images.length === 0 ? 'main' : 'ref',
    b64: imgB64({ dataUrl: o.dataUrl, b64: o.b64 }),
    dataUrl: String(o.dataUrl),
    w,
    h,
    bytes,
  };
  const added = addImage(img);
  if (!added) {
    markError('MAX_IMAGES', CODE.MAX_IMAGES);
    return fail('MAX_IMAGES');
  }
  const cur = getState();
  if (cur.stage === 'browse' && cur.images.length === 1) setStage('browse');
  return ok(
    { id: img.id, count: cur.images.length, max: LIMITS.MAX_IMAGES },
    `已加入素材（素材 ${cur.images.length} / ${LIMITS.MAX_IMAGES}）`,
  );
}

/** 删除单张素材（默认删当前选中的那张）。 */
export async function cmdImageRemove(p) {
  const st = getState();
  if (!st.mounted) return fail('NOT_READY');
  const o = p || {};
  if (!st.images.length) return ok({ count: 0 }, '素材已经空了。');

  let id = o.id;
  if (!id) {
    const idx = Number.isFinite(Number(o.index)) ? Number(o.index) : st.mainIndex;
    const at = idx >= 0 && idx < st.images.length ? idx : 0;
    id = st.images[at].id;
  }
  const removed = removeImage(id);
  if (!removed) return failText('BAD_ARGS', CODE.BAD_ARGS);
  const cur = getState();
  if (!cur.images.length) setStage('browse');
  clearError();
  return ok({ id, count: cur.images.length }, `已删除 1 张素材（素材 ${cur.images.length} / ${LIMITS.MAX_IMAGES}）`);
}

/** 清空素材（Esc）。 */
export async function cmdImageClear() {
  const st = getState();
  if (!st.mounted) return fail('NOT_READY');
  clearImages();
  setStage('browse');
  setMainIndex(0);
  clearError();
  return ok({ count: 0 }, '素材已清空。');
}

// ───────────────────────────────────────────────────────────────────────────
// §7 命令 5–6：风格 / 强度
// ───────────────────────────────────────────────────────────────────────────

/** 选风格。**不校验素材**（PRD 2.2：未选风格就 Enter 才报 NO_STYLE）。 */
export async function cmdStylePick(p) {
  const st = getState();
  if (!st.mounted) return fail('NOT_READY');
  const o = p || {};
  const id = o.styleId || (Number.isFinite(Number(o.index)) ? (STYLES[Number(o.index)] || {}).id : null);
  const style = styleById(id);
  if (!style) {
    markError('BAD_ARGS', CODE.BAD_ARGS);
    return fail('BAD_ARGS');
  }
  if (st.result && !st.result.saved && st.styleId !== style.id) {
    markError('RESULT_UNSAVED', CODE.RESULT_UNSAVED);
    return fail('RESULT_UNSAVED');
  }
  clearError();
  setStyle(style.id);
  if (getState().stage === 'browse') setStage('adjusting');
  const cur = getState();
  return ok(
    { styleId: style.id, styleName: style.name, strength: cur.strength },
    `已选风格 ${style.name} · 强度 ${clampStrength(cur.strength)}% · ${strengthBand(cur.strength).name}`,
  );
}

/**
 * 设强度。
 * @param {{value?:number, delta?:number}} p
 * 边界提示**不是错误**（PRD 5.2 尾）：塞进 data.toast 由 UI 弹 toast，code 仍是 OK。
 */
export async function cmdStrengthSet(p) {
  const st = getState();
  if (!st.mounted) return fail('NOT_READY');
  const o = p || {};
  if (!st.images.length) {
    setStage('browse');
    markError('NO_IMAGES', CODE.NO_IMAGES);
    return fail('NO_IMAGES'); // PRD 5.1：自动退回 browse
  }
  if (!st.styleId) {
    markError('NO_STYLE', CODE.NO_STYLE);
    return fail('NO_STYLE');
  }

  const cur = clampStrength(st.strength);
  let next;
  if (o.delta != null && Number.isFinite(Number(o.delta))) next = cur + Number(o.delta);
  else next = clampStrength(o.value);

  const clamped = clampStrength(next);
  const hitMin = next <= 0 && clamped === 0;
  const hitMax = next >= 100 && clamped === 100;
  clearError();
  setStrength(clamped);
  if (getState().stage === 'browse') setStage('adjusting');

  let toast = '';
  if (hitMin) toast = TOASTS.STRENGTH_MIN;
  else if (hitMax) toast = TOASTS.STRENGTH_MAX;
  else if (clamped > LIMITS.STRENGTH_WARN) toast = TOASTS.STRENGTH_WARN;

  const band = strengthBand(clamped);
  return ok(
    { strength: clamped, band: band.name, toast, hitMin, hitMax },
    `强度 ${clamped}% · ${band.name}`,
  );
}

// ───────────────────────────────────────────────────────────────────────────
// §8 命令 7：generate（全链路）
// ───────────────────────────────────────────────────────────────────────────

/**
 * 生图：writeJob → runScript → parse → clearJob → setResult。
 * @param {{ackPrivacy?:boolean, remember?:boolean}} p
 */
export async function cmdGenerate(p) {
  const st = getState();
  if (!st.mounted) return fail('NOT_READY');
  const o = p || {};

  if (!st.images.length) {
    setStage('browse');
    markError('NO_IMAGES', CODE.NO_IMAGES);
    return fail('NO_IMAGES');
  }
  if (!st.styleId) {
    markError('NO_STYLE', CODE.NO_STYLE);
    return fail('NO_STYLE');
  }
  if (st.doctor && st.doctor.keyOk === false) {
    markError('NO_KEY', CODE.NO_KEY);
    return fail('NO_KEY'); // PRD 5.1 generating 行：前置阻断，不进 generating
  }

  // 首次生图隐私告知（PRD 决策 3）：未确认 → 交回 UI 弹浮层
  if (!st.config.privacyAcked && !o.ackPrivacy) {
    // {provider} 用 doctor 实测回传的 PROVIDER_NAME 填；没自检过就回落成不含厂牌的说法
    const privacyText = fillTemplate(STAGE_HINTS.privacy, {
      provider: (st.doctor && st.doctor.providerName) || PRIVACY_PROVIDER_FALLBACK,
    });
    return failText(PRIVACY_REQUIRED, privacyText, { needPrivacy: true });
  }
  if (o.remember && !st.config.privacyAcked) await saveConfig({ privacyAcked: true });

  const style = styleById(st.styleId);
  const strength = clampStrength(st.strength);
  const jobId = nextJobId();
  const job = {
    jobId,
    styleId: style.id,
    strength,
    size: st.config.resolution || st.size || '1K',
    provider: st.config.provider,
    baseUrl: st.config.baseUrl,
    model: st.config.model,
    resolution: st.config.resolution,
    width: st.config.width,
    height: st.config.height,
    format: st.config.format,
    quality: st.config.quality,
    n: st.config.n,
    httpTimeoutSecs: st.config.httpTimeoutSecs,
    retryCount: st.config.retryCount,
    prompt: buildPrompt(style, strength, st.images.length),
    outDir: st.config.outDir || '',
    images: st.images.map((img, i) => ({ role: i === st.mainIndex ? 'main' : 'ref', b64: imgB64(img) })),
  };

  const written = await writeJob(job);
  if (!written) {
    markError('STORAGE_FAILED', CODE.STORAGE_FAILED);
    return fail('STORAGE_FAILED');
  }

  const args = buildGenerateArgs(job, {
    jobKey: KEYS.JOB,
    model: st.config.model || '',
    outDir: st.config.outDir || '',
    chunk: LIMITS.CHUNK_BYTES,
  });
  const safe = assertArgsSafe(args);
  if (!safe.ok) {
    await clearJob();
    markError('BAD_ARGS', CODE.BAD_ARGS);
    return fail('BAD_ARGS');
  }

  clearError();
  setStage('generating');
  setBusy({ jobId, startedAt: Date.now() });
  logInfo(`开始生成 jobId=${jobId} 风格=${style.name} 强度=${strength} 素材=${st.images.length} 张`);

  let r = null;
  try {
    r = await runScript(args, computeRunTimeout(st.config.httpTimeoutSecs, st.config.retryCount, 1));
  } finally {
    await clearJob();
    setBusy(null);
  }

  if (!r) {
    markError('IO_FAILED', fillTemplate(CODE.IO_FAILED, { message: '没找到可用的 Python 解释器（试过 python / py -3 / python3）' }));
    setStage('adjusting');
    return fail('IO_FAILED', { message: '没找到可用的 Python 解释器（试过 python / py -3 / python3）' });
  }
  if (r.stderr) safeLog(`脚本 stderr：${String(r.stderr).slice(0, 400)}`, 'warn');

  const parsed = parseScriptOutput(r.stdout, r.exit_code, r.timed_out);
  if (!parsed.ok) {
    markError(parsed.code, parsed.message);
    setStage('adjusting');
    return envelope(false, parsed.code, { tooLarge: false }, parsed.message);
  }

  const meta = parsed.meta || {};
  const thumbB64 = parsed.dataUrl ? await makeThumb(parsed.dataUrl) : '';
  const result = {
    jobId,
    styleId: (meta.styleId || style.id),
    styleName: style.name,
    strength: clampStrength(meta.strength != null ? meta.strength : strength),
    size: meta.size || job.size,
    dataUrl: parsed.dataUrl,
    thumbB64,
    path: parsed.path,
    model: meta.model || '',
    provider: meta.provider || 'ark',
    elapsedMs: Number(meta.elapsedMs) || 0,
    bytes: Number(meta.bytes) || 0,
    images: Array.isArray(meta.images) ? meta.images : [],
    saved: false,
    creativeFail: null,
  };
  setResult(result);
  setCreative(0);

  if (parsed.tooLarge) {
    setStage('result');
    markError('TOO_LARGE_PREVIEW', CODE.TOO_LARGE_PREVIEW);
    return envelope(false, 'TOO_LARGE_PREVIEW', { path: parsed.path, tooLarge: true }, CODE.TOO_LARGE_PREVIEW);
  }

  setStage('result');
  const bits = [style.name, `强度 ${result.strength}%`, result.size];
  if (result.elapsedMs) bits.push(`用时 ${formatElapsed(result.elapsedMs)}`);
  if (result.bytes) bits.push(formatBytes(result.bytes));
  const message = `生成完成：${bits.join(' · ')}`;
  logInfo(message);
  return ok({ path: result.path, bytes: result.bytes, elapsedMs: result.elapsedMs, images: result.images }, message);
}

// ───────────────────────────────────────────────────────────────────────────
// §9 命令 8–10：结果（保存 / 导出 / 创意显示）
// ───────────────────────────────────────────────────────────────────────────

/** 存进作品集。缩略图由 UI 注入的 canvas 钩子产出（脚本无 PIL，不产 THUMB）。 */
export async function cmdResultSave() {
  const st = getState();
  if (!st.mounted) return fail('NOT_READY');
  const r = st.result;
  if (!r) {
    markError(NEED_RESULT, STAGE_HINTS.creative_no_result);
    return failText(NEED_RESULT, STAGE_HINTS.creative_no_result);
  }
  if (r.saved) {
    return ok({ id: null, count: st.gallery.length, already: true }, `这张已经在作品集里了（${st.gallery.length} / ${LIMITS.MAX_GALLERY} 条）`);
  }
  if (isGalleryFull(st.gallery)) {
    markError('GALLERY_FULL', CODE.GALLERY_FULL);
    return fail('GALLERY_FULL');
  }

  let thumbB64 = r.thumbB64 || '';
  if (!thumbB64 && r.dataUrl) thumbB64 = await makeThumb(r.dataUrl);

  const item = {
    id: `g-${Date.now().toString(36)}-${st.gallery.length + 1}`,
    at: Date.now(),
    styleId: r.styleId || '',
    styleName: r.styleName || '',
    strength: r.strength,
    size: r.size || '2K',
    thumbB64,
    path: r.path || '',
    bytes: r.bytes || 0,
  };
  const okWrite = await pushGalleryItem(item);
  if (!okWrite) {
    markError('STORAGE_FAILED', CODE.STORAGE_FAILED);
    return fail('STORAGE_FAILED');
  }
  markResultSaved();
  clearError();
  const count = getState().gallery.length;
  const message = `已存进作品集（${count} 条）`;
  logInfo(message);
  return ok({ id: item.id, count }, message);
}

/** 导出到磁盘：走 `save --from <已落盘 path>`。 */
export async function cmdResultExport() {
  const st = getState();
  if (!st.mounted) return fail('NOT_READY');
  const r = st.result;
  if (!r) {
    markError(NEED_RESULT, STAGE_HINTS.creative_no_result);
    return failText(NEED_RESULT, STAGE_HINTS.creative_no_result);
  }
  if (!r.path) {
    markError('WRITE_FAILED', CODE.WRITE_FAILED);
    return fail('WRITE_FAILED');
  }
  const args = ['save', '--from', r.path];
  if (st.config.outDir) args.push('--out-dir', st.config.outDir);
  const safe = assertArgsSafe(args);
  if (!safe.ok) {
    markError('BAD_ARGS', CODE.BAD_ARGS);
    return fail('BAD_ARGS');
  }
  const res = await runScript(args, LIMITS.SHORT_RUN_TIMEOUT_SECS);
  if (!res) return fail('IO_FAILED', { message: '没找到可用的 Python 解释器（试过 python / py -3 / python3）' });
  const p = parseSimpleOutput(res.stdout);
  if (!p.ok) {
    // 落盘失败统一走 WRITE_FAILED（PRD 5.2 E_IO）
    const vars = { message: p.msg || CODE.WRITE_FAILED };
    markError('WRITE_FAILED', fillTemplate(CODE.WRITE_FAILED, vars));
    return fail('WRITE_FAILED', vars);
  }
  clearError();
  return ok({ path: p.path, bytes: p.bytes }, `已导出到 \`${p.path}\``);
}

/**
 * 创意显示：切到某个形态。
 * @param {{index?:number, delta?:number}} p index 为 1-based（PRD：按 1–6）
 */
export async function cmdCreativeRender(p) {
  const st = getState();
  if (!st.mounted) return fail('NOT_READY');
  const o = p || {};
  if (!st.result) {
    markError(NEED_RESULT, STAGE_HINTS.creative_no_result);
    return failText(NEED_RESULT, STAGE_HINTS.creative_no_result);
  }

  let idx;
  if (o.delta != null && Number.isFinite(Number(o.delta))) idx = cycleCreativeIndex(st.creative, Number(o.delta));
  else if (o.index != null) idx = creativeIndexFromKey(o.index);
  else idx = st.creative;

  if (idx < 0 || idx >= CREATIVE_MODES.length) {
    const n = st.creative + 1;
    const message = fillTemplate(CODE.CREATIVE_RANGE, { n });
    markError('CREATIVE_RANGE', message);
    return envelope(false, 'CREATIVE_RANGE', { n, max: CREATIVE_MODES.length }, message);
  }

  clearError();
  setCreative(idx);
  setStage('creative');
  const mode = CREATIVE_MODES[idx];
  return ok({ index: idx, id: mode.id, name: mode.name }, `创意显示：${idx + 1}. ${mode.name}`);
}

/** 放弃当前结果并重来（PRD 5.1 的 S3）。 */
export async function cmdResultDiscard() {
  const st = getState();
  if (!st.mounted) return fail('NOT_READY');
  setResult(null);
  setCreative(0);
  setStage('browse');
  clearError();
  return ok({ stage: 'browse' }, '已放弃当前结果，回到素材区。');
}

// ───────────────────────────────────────────────────────────────────────────
// §10 命令 11–12：作品集
// ───────────────────────────────────────────────────────────────────────────

/** 作品集列表（tools）：**必须返回一句话摘要**，不吐裸数组（PRD A5）。 */
export async function cmdGalleryList() {
  const st = getState();
  const message = summarizeGallery(st.gallery, 3);
  logInfo(message); // ⌘SS 不显示返回值
  return ok({ count: st.gallery.length, max: LIMITS.MAX_GALLERY, items: st.gallery }, message);
}

/** 删除作品集条目。 */
export async function cmdGalleryRemove(p) {
  const st = getState();
  if (!st.mounted) return fail('NOT_READY');
  const o = p || {};
  const id = o.id;
  if (!id) return fail('NOT_READY');
  const okWrite = await removeGalleryItem(id);
  if (!okWrite) {
    markError('STORAGE_FAILED', CODE.STORAGE_FAILED);
    return fail('STORAGE_FAILED');
  }
  clearError();
  const count = getState().gallery.length;
  return ok({ id, count }, `已删除 1 条作品（作品集 ${count} / ${LIMITS.MAX_GALLERY} 条）`);
}

// ───────────────────────────────────────────────────────────────────────────
// §11 命令 13：config.set
// ───────────────────────────────────────────────────────────────────────────

const CONFIG_KEYS = [
  'outDir', 'hwKeys', 'privacyAcked', 'hwIntroSeen', 'pyExe', 'provider',
  'baseUrl', 'model', 'resolution', 'width', 'height', 'format', 'quality',
  'n', 'httpTimeoutSecs', 'retryCount', 'size',
];

async function configFailure(candidate) {
  const checked = validateConfig(candidate);
  if (!checked.ok) return { code: 'CONFIG_INVALID', message: checked.errors.join('；'), config: checked.config };
  const mapped = mapDimensionsForProvider(checked.config.provider, checked.config.width, checked.config.height, checked.config.resolution);
  if (!mapped.ok) return { code: mapped.code, message: mapped.message, config: checked.config };

  // ⚠️ 不传 --model：stylecam_io.py 的 `provider list` 子命令没有这个参数，传了 argparse 直接报错
  const args = ['provider', 'list'];
  const res = await runScript(args, LIMITS.SHORT_RUN_TIMEOUT_SECS);
  const kv = res ? parseKeyValue(res.stdout) : {};
  const providers = parseJsonField(kv, 'PROVIDERS', []);
  const row = providers.find((item) => item && item.id === checked.config.provider);
  const cap = row && row.capabilities;
  if (!cap) return { code: 'CONFIG_INVALID', message: kv.MSG || '读取 provider 能力失败', config: checked.config };
  if (!Array.isArray(cap.formats) || !cap.formats.includes(checked.config.format)) {
    return { code: 'CAPABILITY_UNSUPPORTED', message: `format=${checked.config.format}`, config: checked.config };
  }
  if (checked.config.quality !== null && cap.quality !== true) {
    return { code: 'CAPABILITY_UNSUPPORTED', message: 'quality', config: checked.config };
  }
  if (checked.config.n > Number(cap.maxN || 1)) {
    return { code: 'CAPABILITY_UNSUPPORTED', message: `n 最大为 ${Number(cap.maxN || 1)}`, config: checked.config };
  }
  return null;
}

function parseJsonField(kv, key, fallback) {
  try { return kv[key] ? JSON.parse(kv[key]) : fallback; } catch (e) { return fallback; }
}

function scriptFailure(parsed, fallback) {
  const text = parsed.msg || parsed.err || fallback || '';
  let code = 'CONFIG_INVALID';
  if (/不支持|quality|format|\bn\b/i.test(text)) code = 'CAPABILITY_UNSUPPORTED';
  if (/比例|方形|宽高|DIMENSION/i.test(text)) code = 'DIMENSION_UNREPRESENTABLE';
  return fail(code, { message: text }, { errors: text ? [text] : [] });
}

export async function cmdConfigValidate(p) {
  const patch = (p && p.patch) || p || {};
  const candidate = migrateConfig(Object.assign({}, getState().config, patch));
  const invalid = await configFailure(candidate);
  if (invalid) return fail(invalid.code, { message: invalid.message }, { config: invalid.config, errors: [invalid.message] });
  return ok({ config: candidate, errors: [] }, '配置有效。');
}

/** 改配置。改输出目录时先跑脚本验证可写，**仅当 OK=1 才落盘**（DESIGN §9.2）。 */
export async function cmdConfigSet(p) {
  const patch = (p && p.patch) || p || {};
  const next = {};
  for (let i = 0; i < CONFIG_KEYS.length; i += 1) {
    const k = CONFIG_KEYS[i];
    if (patch[k] !== undefined) next[k] = patch[k];
  }
  if (!Object.keys(next).length) return fail('BAD_ARGS');
  const candidate = migrateConfig(Object.assign({}, getState().config, next));
  const invalid = await configFailure(candidate);
  if (invalid) return fail(invalid.code, { message: invalid.message }, { errors: [invalid.message] });

  if (candidate.outDir && candidate.outDir !== getState().config.outDir) {
    const args = ['dir', '--out-dir', String(candidate.outDir)];
    if (!assertArgsSafe(args).ok) return fail('BAD_ARGS');
    const res = await runScript(args, LIMITS.SHORT_RUN_TIMEOUT_SECS);
    const parsed = res ? parseSimpleOutput(res.stdout) : { ok: false, msg: '没找到可用的 Python 解释器（试过 python / py -3 / python3）' };
    if (!parsed.ok) return fail('WRITE_FAILED', { message: parsed.msg || '' });
  }
  const okWrite = await saveConfig(candidate);
  if (!okWrite) return fail('STORAGE_FAILED');
  clearError();
  return ok({ config: getState().config }, '配置已更新。');
}

export async function cmdConfigGet() {
  const res = await runScript(['config', 'get'], LIMITS.SHORT_RUN_TIMEOUT_SECS);
  const kv = res ? parseKeyValue(res.stdout) : {};
  if (!res || kv.OK !== '1') {
    logInfo(`配置读取失败：${kv.MSG || kv.ERR || '脚本不可用'}`);
    return scriptFailure({ msg: kv.MSG, err: kv.ERR }, '读取配置失败');
  }
  const data = { config: parseJsonField(kv, 'CONFIG', getState().config), sources: parseJsonField(kv, 'SOURCES', {}) };
  logInfo(`配置：provider=${data.config.provider} model=${data.config.model || '默认'} ${data.config.width}x${data.config.height}`);
  return ok(data, '已读取生效配置。');
}

export async function cmdConfigReset() {
  const okWrite = await saveConfig(Object.assign({}, DEFAULTS));
  if (!okWrite) return fail('STORAGE_FAILED');
  return ok({ config: getState().config, keyPreserved: true }, '非敏感配置已重置，API Key 保持不变。');
}

export async function cmdProviderList() {
  const res = await runScript(['provider', 'list'], LIMITS.SHORT_RUN_TIMEOUT_SECS);
  const kv = res ? parseKeyValue(res.stdout) : {};
  if (!res || kv.OK !== '1') { logInfo(`生图服务读取失败：${kv.MSG || '脚本不可用'}`); return fail('IO_FAILED', { message: kv.MSG || '读取 provider 能力失败' }); }
  const providers = parseJsonField(kv, 'PROVIDERS', []);
  logInfo(`生图服务：${providers.map((x) => x.name || x.id).join('、')}`);
  return ok({ providers }, `共 ${providers.length} 个生图服务。`);
}

export async function cmdKeyStatus() {
  const res = await runScript(['key', 'status'], LIMITS.SHORT_RUN_TIMEOUT_SECS);
  const kv = res ? parseKeyValue(res.stdout) : {};
  const data = { provider: kv.PROVIDER || '', available: kv.KEY === '1', source: kv.KEY_SRC || 'none', masked: kv.KEY_MASKED || '', length: Number(kv.KEY_LEN) || 0 };
  logInfo(`Key 状态：来源=${data.source} 长度=${data.length} 可用=${data.available ? '是' : '否'}`);
  return ok(data, data.available ? `Key 可用（${data.masked}）` : '未找到可用 Key。');
}

export async function cmdKeySet(p) {
  const value = String((p && (p.apiKey || p.value)) || '').trim();
  if (!value || value.length >= LIMITS.MAX_ARG_CHARS) return fail('BAD_ARGS');
  const nonce = `sc-key-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  if (!(await writeSecret(value, nonce))) return fail('SECRET_WRITE_FAILED');
  let res = null;
  let cleaned = false;
  try {
    const args = ['key', 'set-env', '--from-storage-key', KEYS.SECRET, '--nonce', nonce];
    res = await runScript(args, LIMITS.SHORT_RUN_TIMEOUT_SECS);
  } finally {
    cleaned = await clearSecret();
  }
  if (!cleaned) return fail('SECRET_CLEANUP_FAILED');
  const kv = res ? parseKeyValue(res.stdout) : {};
  if (!res || kv.OK !== '1') return fail('IO_FAILED', { message: kv.MSG || '保存 Key 失败' });
  return ok({ source: kv.KEY_SRC || 'env', masked: kv.KEY_MASKED || '', length: Number(kv.KEY_LEN) || 0 }, 'API Key 已安全保存。');
}

// ───────────────────────────────────────────────────────────────────────────
// §12 命令 14：doctor（tools）
// ───────────────────────────────────────────────────────────────────────────

/** 环境自检。`--probe` 会真的去方舟问一次（DESIGN §9.2：doctor 固定输出 OUTDIR / OUTDIR_WRITABLE）。 */
export async function cmdDoctor(p) {
  const o = p || {};
  const st = getState();
  const args = ['doctor'];
  if (o.probe === true) args.push('--probe');
  args.push('--provider', String(o.provider || st.config.provider || 'easysu'));
  if (o.baseUrl || st.config.baseUrl) args.push('--base-url', String(o.baseUrl || st.config.baseUrl));
  if (o.model || st.config.model) args.push('--model', String(o.model || st.config.model));
  const outDir = o.outDir || st.config.outDir || '';
  if (outDir) args.push('--out-dir', outDir);

  const res = await runScript(args, computeRunTimeout(st.config.httpTimeoutSecs, st.config.retryCount, o.probe === true ? 1 : 0));
  if (!res) {
    const vars = { message: '没找到可用的 Python 解释器（试过 python / py -3 / python3）' };
    setDoctor({ at: Date.now(), ok: false, keyOk: false, msg: vars.message });
    markError('IO_FAILED', fillTemplate(CODE.IO_FAILED, vars));
    logInfo(`自检失败：${vars.message}`);
    return fail('IO_FAILED', vars);
  }

  const d = parseDoctorOutput(res.stdout, res.exit_code, res.timed_out);
  setDoctor({
    at: Date.now(),
    ok: !!d.ok,
    keyOk: d.key,
    model: d.model,
    pyExe: d.pyExe,
    outDir: d.outDir,
    msg: d.msg,
    provider: d.provider,
    providerName: d.providerName,
    i2i: d.i2i,
  });

  // 探测出的模型 / 输出目录回写配置（不覆盖用户已填的 model）
  const patch = {};
  if (d.pyExe) patch.pyExe = d.pyExe;
  if (d.model && !st.config.model) patch.model = d.model;
  if (d.outDir && !st.config.outDir) patch.outDir = d.outDir;
  if (Object.keys(patch).length) await saveConfig(patch);

  const parts = [];
  parts.push(d.pyVer ? `Python ${d.pyVer}` : 'Python 未知');
  const srcName = { cli: '命令行', env: 'tools/stylecam_io.env', easyop: 'easyop 配置', none: '没找到' }[d.keySrc] || d.keySrc;
  parts.push(d.key ? `Key 可用（来源 ${srcName}）` : `Key 不可用（来源 ${srcName}）`);
  parts.push(d.model ? `模型 ${d.model}` : '模型 未探测到');
  parts.push(d.outDir ? `输出目录 ${d.outDir}${d.outDirWritable ? '（可写）' : '（不可写）'}` : '输出目录 未知');
  // I2I=0：该 provider 不支持图生图，生成时会按文字描述出图 —— 提前说清，别让用户以为素材图被用上了
  if (d.i2i === false) parts.push(PROVIDER_HINTS.i2i_degrade);
  const message = `自检：${parts.join(' · ')}${d.msg ? `｜${d.msg}` : ''}`;
  logInfo(message); // ⌘SS 不显示返回值

  if (!d.key) {
    markError('NO_KEY', CODE.NO_KEY);
    return envelope(false, 'NO_KEY', d, message);
  }
  if (!d.ok) {
    const vars = { message: d.msg || CODE.IO_FAILED };
    markError('IO_FAILED', fillTemplate(CODE.IO_FAILED, vars));
    return fail('IO_FAILED', vars, d);
  }
  clearError();
  return ok(d, message);
}

// ───────────────────────────────────────────────────────────────────────────
// §13 命令 15：hardware（键盘 / 鼠标共用）
// ───────────────────────────────────────────────────────────────────────────

/**
 * 硬件输入入口。
 * @param {{ev?:object, intent?:object, action?:string, arg?:number}} input
 *   给 `ev` → 经 logic.mapHardwareKey 映射（纯函数，键盘与鼠标共用）
 *   给 `intent` / `action` → 直接派发（鼠标按钮走这条）
 */
export async function cmdHardware(input) {
  const st = getState();
  if (!st.mounted) return fail('NOT_READY');

  const raw = input || {};
  let intent = raw.intent;
  if (!intent && raw.ev) intent = mapHardwareKey(st.stage, raw.ev);
  if (!intent && raw.action) intent = { cls: raw.cls || 'command', action: raw.action, arg: raw.arg == null ? null : raw.arg };
  if (!intent || !intent.action || intent.action === 'none') return ok({ consumed: false }, '');

  return dispatchAction(intent);
}

/** 动作意图 → 具体命令。**生成中一律吞掉**（PRD 5.1 generating 行）。 */
export async function dispatchAction(intent) {
  const st = getState();
  const action = intent.action;
  const arg = intent.arg;

  if (action === 'busy') {
    return ok({ consumed: true, blocked: true, toast: STAGE_HINTS.generating_busy }, STAGE_HINTS.generating_busy);
  }
  if (action === 'wrongStage') {
    const text = st.stage === 'browse' ? STAGE_HINTS.browse_wrong : STAGE_HINTS.browse_wrong;
    return ok({ consumed: true, toast: text }, text);
  }
  if (action === 'creativeRange') {
    const n = st.creative + 1;
    const message = fillTemplate(CODE.CREATIVE_RANGE, { n });
    markError('CREATIVE_RANGE', message);
    return envelope(false, 'CREATIVE_RANGE', { n, max: CREATIVE_MODES.length }, message);
  }
  if (action === 'confirm') {
    // 仅 browse 阶段的 Enter 会产出 confirm：有素材 → 进风格选择（与点风格卡的阶段推进一致）
    if (st.stage === 'browse') {
      if (!st.images.length) {
        const text = STAGE_HINTS.browse_empty;
        return ok({ consumed: true, toast: text }, text);
      }
      setStage('adjusting');
      clearError();
      return ok({ stage: 'adjusting' }, '已进入风格选择：选一种风格，调强度，然后生成');
    }
    return ok({ consumed: false }, '');
  }
  if (action === 'back') {
    const guarded = guardUnsaved();
    if (guarded && st.stage !== 'browse') return guarded;
    goPrevStage();
    clearError();
    return ok({ stage: getState().stage }, `已退回：${STAGE_LABEL_OF(getState().stage)}`);
  }

  switch (action) {
    case 'selectImage': {
      const n = Number(arg);
      if (!Number.isFinite(n) || n < 0 || n >= st.images.length) return ok({ consumed: false }, '');
      setMainIndex(n);
      return ok({ mainIndex: n }, `已选中第 ${n + 1} 张素材`);
    }
    case 'adjustStrength':
      return cmdStrengthSet({ delta: arg });
    case 'pickStyle': {
      const idx = Number(arg);
      if (!(idx >= 0) || idx >= STYLES.length) return fail('BAD_ARGS');
      return cmdStylePick({ styleId: STYLES[idx].id });
    }
    case 'generate':
      return cmdGenerate();
    case 'saveResult':
      return cmdResultSave();
    case 'discardResult':
      return cmdResultDiscard();
    case 'enterCreative':
      return cmdCreativeRender({ index: st.creative + 1 });
    case 'setCreative':
      return cmdCreativeRender({ index: Number(arg) + 1 });
    case 'cycleCreative':
      return cmdCreativeRender({ delta: arg });
    case 'clearImages':
      return cmdImageClear();
    case 'deleteImage':
      return cmdImageRemove({ index: st.mainIndex });
    case 'compareSlide':
      // 对比滑杆是纯视图态：交给 UI 按 data.slide 处理，命令层只给意图
      return ok({ consumed: true, slide: Number(arg) || 0 }, '');
    case 'openGallery':
      setGalleryOpen(!st.galleryOpen);
      return ok({ galleryOpen: getState().galleryOpen }, '');
    default:
      return ok({ consumed: false }, '');
  }
}

function STAGE_LABEL_OF(stage) {
  const map = { browse: '素材', adjusting: '风格/强度', generating: '生成', result: '结果', creative: '创意' };
  return map[stage] || '素材';
}

// ───────────────────────────────────────────────────────────────────────────
// §14 命令注册表（main.js 按此注册）
// ───────────────────────────────────────────────────────────────────────────

export const COMMANDS = {
  'stylecam.state': cmdState,
  'stylecam.image.add': cmdImageAdd,
  'stylecam.image.remove': cmdImageRemove,
  'stylecam.image.clear': cmdImageClear,
  'stylecam.style.pick': cmdStylePick,
  'stylecam.strength.set': cmdStrengthSet,
  'stylecam.generate': cmdGenerate,
  'stylecam.result.save': cmdResultSave,
  'stylecam.result.export': cmdResultExport,
  'stylecam.creative.render': cmdCreativeRender,
  'stylecam.gallery.list': cmdGalleryList,
  'stylecam.gallery.remove': cmdGalleryRemove,
  'stylecam.config.get': cmdConfigGet,
  'stylecam.config.set': cmdConfigSet,
  'stylecam.config.validate': cmdConfigValidate,
  'stylecam.config.reset': cmdConfigReset,
  'stylecam.provider.list': cmdProviderList,
  'stylecam.key.status': cmdKeyStatus,
  'stylecam.key.set': cmdKeySet,
  'stylecam.doctor': cmdDoctor,
  'stylecam.hardware': cmdHardware,
};

/** 三条 tools 命令：⌘SS 里看不到返回值，必须额外打日志（DESIGN §10.4.7）。 */
export const TOOLS_COMMANDS = [
  'stylecam.state', 'stylecam.doctor', 'stylecam.gallery.list',
  'stylecam.config.get', 'stylecam.provider.list', 'stylecam.key.status',
];

// ───────────────────────────────────────────────────────────────────────────
// §15 测试缝
// ───────────────────────────────────────────────────────────────────────────

export const __test = {
  envelope,
  ok,
  fail,
  failText,
  setThumbMaker,
  getThumbMaker,
  makeThumb,
  parseKeyValue,
  parseDoctorOutput,
  parseSimpleOutput,
  dispatchAction,
  COMMANDS,
  TOOLS_COMMANDS,
  PREV_STAGE,
  PRIVACY_REQUIRED,
  NEED_RESULT,
  CONFIG_KEYS,
  // 15 条命令本体
  cmdState,
  cmdImageAdd,
  cmdImageRemove,
  cmdImageClear,
  cmdStylePick,
  cmdStrengthSet,
  cmdGenerate,
  cmdResultSave,
  cmdResultExport,
  cmdCreativeRender,
  cmdGalleryList,
  cmdGalleryRemove,
  cmdConfigGet,
  cmdConfigSet,
  cmdConfigValidate,
  cmdConfigReset,
  cmdProviderList,
  cmdKeyStatus,
  cmdKeySet,
  cmdDoctor,
  cmdHardware,
  cmdResultDiscard,
};
