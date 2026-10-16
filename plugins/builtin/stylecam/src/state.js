/**
 * StyleCam 风格调色盘 —— 状态层
 *
 * 职责（DESIGN §5）：内存 `state` 单例 + config / gallery / job 三个 key 的持久化 + 变更订阅。
 * 硬边界：
 *   - 不碰 DOM、不渲染（渲染归 ui.js，经 `subscribe` 感知变化）
 *   - 不直接调 `eo.run`（一律经 `host.js`）
 *   - 不做「能不能生图」这类业务判断（归 cmds）
 * 依赖：constants → host → state（单向，无环）
 */

import { KEYS, DEFAULTS, LIMITS, CONFIG_VERSION, GALLERY_VERSION, STAGES } from './constants.js';
import { storageGet, storageSet, safeLog } from './host.js';
import { migrateConfig, validateConfig } from './logic.js';

// ───────────────────────────────────────────────────────────────────────────
// §1 内存 state 单例
// ───────────────────────────────────────────────────────────────────────────

function freshState() {
  return {
    mounted: false, // 页签是否挂载（keydown 门控）
    stage: 'browse', // browse|adjusting|generating|result|creative（gallery 为叠加态）
    images: [], // ≤4，planImageFit 后的结果
    mainIndex: 0,
    styleId: null,
    strength: DEFAULTS.strength,
    size: DEFAULTS.resolution,
    result: null,
    creative: 0, // 0..5，索引进 CREATIVE_MODES
    galleryOpen: false,
    busy: null, // null | { jobId, startedAt }
    doctor: { at: 0, ok: null, keyOk: null, model: null, pyExe: '', outDir: '', msg: '' },
    lastError: null, // { code, message, at }
    config: Object.assign({}, DEFAULTS),
    gallery: [],
  };
}

let _state = freshState();
const _subs = [];

export function getState() {
  return _state;
}

/** 订阅变更；返回取消订阅函数。回调抛错不影响其它订阅者。 */
export function subscribe(fn) {
  if (typeof fn !== 'function') return () => {};
  _subs.push(fn);
  return () => {
    const i = _subs.indexOf(fn);
    if (i >= 0) _subs.splice(i, 1);
  };
}

/** 广播变更（ui.js 收到后 renderAll）。 */
export function notify() {
  for (let i = 0; i < _subs.length; i += 1) {
    try {
      _subs[i](_state);
    } catch (e) {
      safeLog(`订阅者回调失败：${e && e.message ? e.message : e}`, 'warn');
    }
  }
}

/** 改完 state 顺手广播。 */
function touch(fn) {
  if (typeof fn === 'function') fn(_state);
  notify();
}

// ───────────────────────────────────────────────────────────────────────────
// §2 通用读写
// ───────────────────────────────────────────────────────────────────────────

async function readJSON(key) {
  const v = await storageGet(key);
  return v == null || v === '' ? null : v;
}

async function writeJSON(key, val) {
  return storageSet(key, val);
}

// ───────────────────────────────────────────────────────────────────────────
// §3 config（长期）
// ───────────────────────────────────────────────────────────────────────────

/** 读 `stylecam.config`，迁移 v1 并尽力写回；非法配置回落 DEFAULTS。 */
export async function loadConfig() {
  const raw = await readJSON(KEYS.CONFIG);
  const migrated = migrateConfig(raw);
  const checked = validateConfig(migrated);
  const cfg = checked.ok ? checked.config : Object.assign({}, DEFAULTS);
  touch((s) => { s.config = cfg; });
  if (raw && (raw.v !== CONFIG_VERSION || Object.prototype.hasOwnProperty.call(raw, 'size'))) {
    const ok = await writeJSON(KEYS.CONFIG, cfg);
    if (!ok) safeLog('stylecam.config 已在内存迁移为 v2，但写回失败；本次继续使用迁移结果', 'warn');
  }
  if (!checked.ok) safeLog(`stylecam.config 无效，已回落默认值：${checked.errors.join('；')}`, 'warn');
  return cfg;
}

/** 打补丁式保存；只接受合法 v2，且拒绝任何 Key 字段。 */
export async function saveConfig(patch) {
  const p = patch || {};
  const forbidden = Object.keys(p).some((k) => /(^|_)(api_?key|ark_api_key|image_api_key)$/i.test(k));
  if (forbidden) {
    safeLog('拒绝把 API Key 写入 stylecam.config', 'warn');
    return false;
  }
  const checked = validateConfig(Object.assign({}, _state.config, p, { v: CONFIG_VERSION }));
  if (!checked.ok) {
    safeLog(`拒绝保存无效 stylecam.config：${checked.errors.join('；')}`, 'warn');
    return false;
  }
  const ok = await writeJSON(KEYS.CONFIG, checked.config);
  if (ok) touch((s) => { s.config = checked.config; });
  else safeLog('stylecam.config 写入失败（STORAGE_FAILED 候选）', 'warn');
  return ok;
}

export async function writeSecret(value, nonce) {
  const secret = String(value || '');
  const token = String(nonce || '');
  if (!secret || secret.length >= LIMITS.MAX_ARG_CHARS || !token) return false;
  return writeJSON(KEYS.SECRET, { v: 1, nonce: token, apiKey: secret, createdAt: Date.now() });
}

export async function clearSecret() {
  return writeJSON(KEYS.SECRET, null);
}

// ───────────────────────────────────────────────────────────────────────────
// §4 gallery（长期，≤30 条）
// ───────────────────────────────────────────────────────────────────────────

export async function loadGallery() {
  const raw = await readJSON(KEYS.GALLERY);
  const items = raw && Array.isArray(raw.items) ? raw.items.filter((x) => x && typeof x === 'object') : [];
  touch((s) => { s.gallery = items.slice(0, LIMITS.MAX_GALLERY); });
  return _state.gallery;
}

async function persistGallery(next) {
  const ok = await writeJSON(KEYS.GALLERY, { v: GALLERY_VERSION, items: next });
  if (ok) touch((s) => { s.gallery = next; });
  else safeLog('作品集写入失败（STORAGE_FAILED 候选）', 'warn');
  return ok;
}

/** 新作品插到最前（items[0] 最新，上限 30）。返回是否成功。 */
export async function pushGalleryItem(item) {
  if (!item) return false;
  return persistGallery([item].concat(_state.gallery).slice(0, LIMITS.MAX_GALLERY));
}

export async function removeGalleryItem(id) {
  return persistGallery(_state.gallery.filter((it) => it && it.id !== id));
}

// ───────────────────────────────────────────────────────────────────────────
// §5 job（单次请求，用完即瘦 → 置 null）
// ───────────────────────────────────────────────────────────────────────────

/**
 * 把本次生图的 job 写进 `stylecam.job`（图片 base64 走这里中转，不进 eo.run 的 args）。
 * @returns {Promise<boolean>}
 */
export async function writeJob(job) {
  const ok = await writeJSON(KEYS.JOB, job || null);
  if (!ok) safeLog('stylecam.job 写入失败', 'warn');
  return ok;
}

/**
 * 生图结束**立刻**置 null（DESIGN §6.3）。
 * ⚠ 宿主 `storage.set(key, null)` 不删 key，只是把值写成 null —— 体积回到 5 字节。
 */
export async function clearJob() {
  return writeJSON(KEYS.JOB, null);
}

// ───────────────────────────────────────────────────────────────────────────
// §6 会话内状态变更（不落盘，除非注明）
// ───────────────────────────────────────────────────────────────────────────

/** 加一张素材。超上限返回 false（由 cmds 转成 MAX_IMAGES 信封，不做业务判断）。 */
export function addImage(img) {
  if (!img) return false;
  if (_state.images.length >= LIMITS.MAX_IMAGES) return false;
  touch((s) => {
    s.images.push(img);
    if (s.mainIndex < 0) s.mainIndex = 0;
  });
  return true;
}

/** 删一张素材；返回被删对象或 null。 */
export function removeImage(id) {
  let removed = null;
  touch((s) => {
    const i = s.images.findIndex((x) => x && x.id === id);
    if (i < 0) return;
    removed = s.images[i];
    s.images.splice(i, 1);
    if (s.mainIndex >= s.images.length) s.mainIndex = Math.max(0, s.images.length - 1);
  });
  return removed;
}

/** 清空素材（Esc）。 */
export function clearImages() {
  touch((s) => {
    s.images = [];
    s.mainIndex = 0;
  });
}

/** 设主体图下标。 */
export function setMainIndex(i) {
  touch((s) => {
    const n = Number(i);
    s.mainIndex = n >= 0 && n < s.images.length ? n : 0;
  });
}

/** 选风格（只改内存；拦截与校验归 cmds）。 */
export function setStyle(id) {
  touch((s) => { s.styleId = id || null; });
}

/** 设强度（夹取由 logic.clampStrength 负责，这里只落值）。 */
export function setStrength(n) {
  touch((s) => { s.strength = n; });
}

/** 切阶段；非法值回落 browse。 */
export function setStage(stage) {
  touch((s) => { s.stage = STAGES.indexOf(stage) >= 0 ? stage : 'browse'; });
}

/** 设结果（DESIGN §6.2）。 */
export function setResult(r) {
  touch((s) => { s.result = r || null; });
}

/** 结果已存进作品集。 */
export function markResultSaved() {
  touch((s) => {
    if (s.result) s.result.saved = true;
  });
}

/** 设创意显示下标（0..5）。 */
export function setCreative(i) {
  touch((s) => {
    const n = Number(i);
    s.creative = n >= 0 ? n : 0;
  });
}

/** 作品集展开/收起（叠加态）。 */
export function setGalleryOpen(open) {
  touch((s) => { s.galleryOpen = !!open; });
}

/** 生成忙态；null 表示空闲。 */
export function setBusy(busy) {
  touch((s) => { s.busy = busy || null; });
}

/** 记一次 doctor 结果。 */
export function setDoctor(patch) {
  touch((s) => { s.doctor = Object.assign({}, s.doctor, patch || {}); });
}

/** 记最后一次错误（错误显示在用户刚操作的区域，这里只留痕）。 */
export function setLastError(code, message) {
  touch((s) => {
    s.lastError = code ? { code, message: message || '', at: Date.now() } : null;
  });
}

/** 页签挂载标记（keydown 门控用）。 */
export function setMounted(v) {
  touch((s) => { s.mounted = !!v; });
}

/**
 * 会话复位：页签切走/重开时调用。
 * 未保存的结果丢弃（PRD 5.1：给一次温和提示，由 cmds 决定文案）。
 * @returns {boolean} 丢弃前是否有未保存结果
 */
export function resetSession() {
  let hadUnsaved = false;
  touch((s) => {
    hadUnsaved = !!(s.result && !s.result.saved);
    s.stage = 'browse';
    s.images = [];
    s.mainIndex = 0;
    s.result = null;
    s.creative = 0;
    s.busy = null;
    s.lastError = null;
    s.galleryOpen = false;
  });
  return hadUnsaved;
}

/** harness 用：整体复位到出厂状态。 */
export function resetAll() {
  _state = freshState();
  notify();
}

export const __test = {
  getState,
  subscribe,
  notify,
  loadConfig,
  saveConfig,
  loadGallery,
  pushGalleryItem,
  removeGalleryItem,
  writeJob,
  clearJob,
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
  setMounted,
  resetSession,
  resetAll,
  freshState,
};
