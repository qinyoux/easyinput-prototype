/**
 * StyleCam 风格调色盘 —— 纯逻辑层
 *
 * 硬边界（DESIGN §5 / §10.3）：
 *   - 本文件**禁止**引用宿主全局（DOM 宿主对象 / 宿主 API 门面 / `await`）
 *   - 全部导出必须是纯函数：同样输入 → 同样输出，不读时钟、不读随机、不发网络
 *   - 只依赖 constants.js
 */

import {
  STYLES,
  CREATIVE_MODES,
  STAGES,
  STRENGTH_BANDS,
  LIMITS,
  KEYS,
  DEFAULTS,
  CONFIG_VERSION,
  CODE,
  SCRIPT_ERR_MAP,
  STAGE_HINTS,
} from './constants.js';

// ───────────────────────────────────────────────────────────────────────────
// §0 小工具
// ───────────────────────────────────────────────────────────────────────────

/** 把 `<code>` / `<message原文>` / `<exitCode>` / `<n>` 占位符替换成实际值。 */
export function fillTemplate(tpl, vars) {
  const s = String(tpl == null ? '' : tpl);
  if (!vars) return s;
  return s
    .replace(/<code>/g, String(vars.code == null ? '' : vars.code))
    .replace(/<message原文>/g, String(vars.message == null ? '' : vars.message))
    .replace(/<exitCode>/g, String(vars.exitCode == null ? '' : vars.exitCode))
    .replace(/<n>/g, String(vars.n == null ? '' : vars.n))
    .replace(/\{provider\}/g, String(vars.provider == null ? '' : vars.provider));
}

const B64_ALPHA = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';

/** 只解 base64 前 12 个字符 → 字节数组（用于嗅探真实格式，避免依赖 atob/DOM）。 */
function b64HeadBytes(b64) {
  const s = String(b64 == null ? '' : b64).replace(/[^A-Za-z0-9+/]/g, '');
  const out = [];
  const limit = Math.min(12, s.length - (s.length % 4));
  for (let i = 0; i + 3 <= limit; i += 4) {
    const a = B64_ALPHA.indexOf(s[i]);
    const b = B64_ALPHA.indexOf(s[i + 1]);
    const c = B64_ALPHA.indexOf(s[i + 2]);
    const d = B64_ALPHA.indexOf(s[i + 3]);
    if (a < 0 || b < 0 || c < 0 || d < 0) break;
    out.push((a << 2) | (b >> 4), ((b & 15) << 4) | (c >> 2), ((c & 3) << 6) | d);
  }
  return out;
}

// ───────────────────────────────────────────────────────────────────────────
// §1 集中配置 v2
// ───────────────────────────────────────────────────────────────────────────

const CONFIG_FIELDS = Object.keys(DEFAULTS);
const PROVIDERS = ['easysu', 'ark', 'openai_compat', 'zhipu'];
const RESOLUTIONS = ['1K', '2K', '4K'];
const FORMATS = ['auto', 'jpeg', 'png'];
const RATIOS = [
  ['1:1', 1],
  ['16:9', 16 / 9],
  ['9:16', 9 / 16],
  ['4:3', 4 / 3],
  ['3:4', 3 / 4],
];

export function migrateConfig(raw) {
  const src = raw && typeof raw === 'object' ? raw : {};
  const out = Object.assign({}, DEFAULTS);
  for (const k of CONFIG_FIELDS) if (src[k] !== undefined) out[k] = src[k];
  if (src.v !== CONFIG_VERSION) {
    const size = String(src.size || '').trim();
    if (RESOLUTIONS.includes(size)) out.resolution = size;
    else {
      const m = /^(\d+)x(\d+)$/i.exec(size);
      if (m) {
        out.width = Number(m[1]);
        out.height = Number(m[2]);
      }
    }
  }
  delete out.size;
  delete out.apiKey;
  delete out.IMAGE_API_KEY;
  delete out.ARK_API_KEY;
  out.v = CONFIG_VERSION;
  return out;
}

export function validateConfig(raw) {
  const cfg = migrateConfig(raw);
  const errors = [];
  const integerRange = (name, min, max) => {
    const v = cfg[name];
    if (!Number.isInteger(v) || v < min || v > max) errors.push(`${name} 必须是 ${min}..${max} 的整数`);
  };
  if (!PROVIDERS.includes(cfg.provider)) errors.push(`provider 不支持：${cfg.provider}`);
  if (!RESOLUTIONS.includes(cfg.resolution)) errors.push(`resolution 不支持：${cfg.resolution}`);
  for (const name of ['width', 'height']) {
    integerRange(name, LIMITS.DIMENSION_MIN, LIMITS.DIMENSION_MAX);
    if (Number.isInteger(cfg[name]) && cfg[name] % LIMITS.DIMENSION_STEP !== 0) errors.push(`${name} 必须是 64 的倍数`);
  }
  if (Number(cfg.width) * Number(cfg.height) > LIMITS.MAX_PIXELS) errors.push('width * height 超过 16777216');
  integerRange('httpTimeoutSecs', LIMITS.HTTP_TIMEOUT_MIN_SECS, LIMITS.HTTP_TIMEOUT_MAX_SECS);
  integerRange('retryCount', LIMITS.RETRY_MIN, LIMITS.RETRY_MAX);
  integerRange('n', LIMITS.N_MIN, LIMITS.N_MAX);
  if (!FORMATS.includes(cfg.format)) errors.push(`format 不支持：${cfg.format}`);
  if (cfg.quality !== null && (!Number.isInteger(cfg.quality) || cfg.quality < 1 || cfg.quality > 100)) errors.push('quality 必须是 null 或 1..100 的整数');
  if (cfg.quality !== null && cfg.format !== 'jpeg') errors.push('quality 仅在 format=jpeg 时可用');
  if (typeof cfg.baseUrl !== 'string' || cfg.baseUrl.length > 2048) {
    errors.push('baseUrl 必须是最长 2048 字符的字符串');
  } else if (cfg.baseUrl) {
    try {
      const parsed = new URL(cfg.baseUrl);
      const local = ['localhost', '127.0.0.1', '::1'].includes(parsed.hostname);
      if (parsed.protocol !== 'https:' && !(parsed.protocol === 'http:' && local)) errors.push('baseUrl 必须是 HTTPS；仅 localhost 可使用 HTTP');
    } catch (e) {
      errors.push('baseUrl 必须是合法 URL');
    }
  }
  const stringLimits = { model: 256, outDir: 2048, pyExe: 260 };
  for (const name of Object.keys(stringLimits)) {
    if (typeof cfg[name] !== 'string' || cfg[name].length > stringLimits[name]) errors.push(`${name} 必须是最长 ${stringLimits[name]} 字符的字符串`);
  }
  return { ok: errors.length === 0, config: cfg, errors };
}

export function mapDimensionsForProvider(provider, width, height, resolution) {
  const p = String(provider || '').toLowerCase();
  const w = Number(width);
  const h = Number(height);
  const res = String(resolution || DEFAULTS.resolution);
  if (!Number.isInteger(w) || !Number.isInteger(h) || w <= 0 || h <= 0) {
    return { ok: false, code: 'CONFIG_INVALID', message: 'width/height 必须是正整数' };
  }
  if (p === 'easysu') {
    const actual = w / h;
    let best = RATIOS[0];
    let error = Infinity;
    for (const candidate of RATIOS) {
      const relative = Math.abs(actual - candidate[1]) / candidate[1];
      if (relative < error) { best = candidate; error = relative; }
    }
    if (error > 0.03) return { ok: false, code: 'DIMENSION_UNREPRESENTABLE', message: `${w}x${h} 与最近比例 ${best[0]} 相差 ${(error * 100).toFixed(2)}%` };
    return { ok: true, resolution: res, ratio: best[0] };
  }
  if (p === 'ark') {
    if (w !== h) return { ok: false, code: 'DIMENSION_UNREPRESENTABLE', message: `Ark 只接受方形，收到 ${w}x${h}` };
    return { ok: true, size: res };
  }
  if (p === 'openai_compat' || p === 'zhipu') return { ok: true, size: `${w}x${h}` };
  return { ok: false, code: 'CAPABILITY_UNSUPPORTED', message: `未知 provider=${p}` };
}

export function computeRunTimeout(httpTimeoutSecs, retryCount, downloads) {
  const http = Math.min(LIMITS.HTTP_TIMEOUT_MAX_SECS, Math.max(LIMITS.HTTP_TIMEOUT_MIN_SECS, Number(httpTimeoutSecs) || DEFAULTS.httpTimeoutSecs));
  const retry = Math.min(LIMITS.RETRY_MAX, Math.max(LIMITS.RETRY_MIN, Math.trunc(Number(retryCount) || 0)));
  const downloadCount = Math.max(0, Math.trunc(Number(downloads) || 0));
  const backoff = [1, 2, 4].slice(0, retry).reduce((a, b) => a + b, 0);
  const raw = (retry + 1) * http + downloadCount * http + backoff + 30;
  return Math.min(LIMITS.RUN_TIMEOUT_MAX_SECS, Math.max(LIMITS.RUN_TIMEOUT_MIN_SECS, Math.ceil(raw)));
}

// ───────────────────────────────────────────────────────────────────────────
// §2 风格索引换算（PRD §2：1–9、0 对应第 1–10 个；←→ 循环）
// ───────────────────────────────────────────────────────────────────────────

/** 按 id 找风格对象；找不到返回 null。 */
export function styleById(id) {
  if (!id) return null;
  for (let i = 0; i < STYLES.length; i += 1) if (STYLES[i].id === id) return STYLES[i];
  return null;
}

/** 按 id 找风格下标；找不到返回 -1。 */
export function styleIndexById(id) {
  if (!id) return -1;
  for (let i = 0; i < STYLES.length; i += 1) if (STYLES[i].id === id) return i;
  return -1;
}

/**
 * 数字键 → 风格下标：`1`–`9` → 0–8，`0` → 9（第 10 个）。非数字键返回 -1。
 */
export function styleIndexFromKey(key) {
  const k = String(key == null ? '' : key);
  if (!/^[0-9]$/.test(k)) return -1;
  return k === '0' ? 9 : Number(k) - 1;
}

/** 在 10 个风格间循环移动（delta 可为负）。cur 非法时归 0。 */
export function cycleStyleIndex(cur, delta, len) {
  const n = len == null ? STYLES.length : len;
  if (!(n > 0)) return 0;
  const base = Number.isFinite(cur) && cur >= 0 ? cur % n : 0;
  const d = Number.isFinite(delta) ? delta : 0;
  return ((base + d) % n + n) % n;
}

/** 在 6 种创意显示间循环移动。 */
export function cycleCreativeIndex(cur, delta) {
  return cycleStyleIndex(cur, delta, CREATIVE_MODES.length);
}

/** 创意显示的 1 -based 序号 → 0-based 下标；越界返回 -1。 */
export function creativeIndexFromKey(key) {
  const n = Number(String(key == null ? '' : key));
  if (!Number.isFinite(n) || n < 1 || n > CREATIVE_MODES.length) return -1;
  return n - 1;
}

// ───────────────────────────────────────────────────────────────────────────
// §2 图片档位规划（DESIGN 计划 §三.A）
// ───────────────────────────────────────────────────────────────────────────

/** 5 档递降：先给最高画质，装不下就降一档，5 档都不行 → IMG_TOO_LARGE（不静默截断）。 */
export const FIT_TIERS = {
  main: [
    [1600, 0.92],
    [1280, 0.88],
    [1024, 0.85],
    [800, 0.8],
    [640, 0.72],
  ],
  ref: [
    [1024, 0.9],
    [900, 0.86],
    [768, 0.82],
    [640, 0.78],
    [512, 0.72],
  ],
};

/**
 * 规划这张图该压到多大。纯估算，不做实际编码。
 * @returns {{ok:boolean, code:string|null, longEdge:number, q:number, estBytes:number, tier:number}}
 */
export function planImageFit(w, h, bytes, kind) {
  const tiers = kind === 'ref' ? FIT_TIERS.ref : FIT_TIERS.main;
  const long = Math.max(Number(w) || 0, Number(h) || 0);
  const size = Number(bytes) || 0;
  if (!(long > 0) || !(size > 0)) {
    return { ok: false, code: 'IMG_TOO_LARGE', longEdge: 0, q: 0, estBytes: 0, tier: -1 };
  }
  if (long >= LIMITS.MAX_DECODE_EDGE) {
    return { ok: false, code: 'IMG_DECODE_FAILED', longEdge: long, q: 0, estBytes: size, tier: -1 };
  }
  for (let i = 0; i < tiers.length; i += 1) {
    const target = Math.min(tiers[i][0], long);
    const q = tiers[i][1];
    const est = Math.round(size * Math.pow(target / long, 2) * (q / 0.92));
    if (est <= LIMITS.MAX_SINGLE_BYTES) {
      return { ok: true, code: null, longEdge: target, q, estBytes: est, tier: i };
    }
  }
  const last = tiers[tiers.length - 1];
  const target = Math.min(last[0], long);
  const est = Math.round(size * Math.pow(target / long, 2) * (last[1] / 0.92));
  return { ok: false, code: 'IMG_TOO_LARGE', longEdge: target, q: last[1], estBytes: est, tier: tiers.length - 1 };
}

/** 合计预算校验（4 张合计上限 2.4MB）。 */
export function checkImageBudget(totalBytes) {
  const n = Number(totalBytes) || 0;
  return n <= LIMITS.MAX_TOTAL_BYTES
    ? { ok: true, code: null, totalBytes: n }
    : { ok: false, code: 'IMG_TOO_LARGE', totalBytes: n };
}

// ───────────────────────────────────────────────────────────────────────────
// §3 强度
// ───────────────────────────────────────────────────────────────────────────

/**
 * 夹到 0–100 的整数；**非法值回落默认 50**。
 *
 * ⚠ `Number(null) === 0`、`Number('') === 0`，早期版本因此把 `null` 当成合法 0，
 * 强度会静默跳到 0（用户什么都没按，图却变成"原图"）。2026-09-25 修：
 * `null` / `undefined` / `NaN` / 布尔 / 空串 / 非数字字符串一律回落 50。
 */
const NUMERIC_STRING = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/;

export function clampStrength(n) {
  if (n === null || n === undefined) return DEFAULTS.strength;
  if (typeof n === 'boolean') return DEFAULTS.strength;
  const s = typeof n === 'string' ? n.trim() : n;
  if (s === '' || typeof s === 'object') return DEFAULTS.strength;
  // 字符串只认十进制写法：`Number('0x10')===16`、`Number('Infinity')` 都不算合法强度
  if (typeof s === 'string' && !NUMERIC_STRING.test(s)) return DEFAULTS.strength;
  const v = Number(s);
  if (!Number.isFinite(v)) return DEFAULTS.strength;
  return Math.min(100, Math.max(0, Math.round(v)));
}

/** 强度 → 档位（0–30 轻度点缀 / 31–69 平衡融合 / 70–100 大改风格）。 */
export function strengthBand(n) {
  const v = clampStrength(n);
  for (let i = 0; i < STRENGTH_BANDS.length; i += 1) {
    if (v >= STRENGTH_BANDS[i].min && v <= STRENGTH_BANDS[i].max) return STRENGTH_BANDS[i];
  }
  return STRENGTH_BANDS[STRENGTH_BANDS.length - 1];
}

// ───────────────────────────────────────────────────────────────────────────
// §4 Prompt 与脚本参数
// ───────────────────────────────────────────────────────────────────────────

/**
 * 组装英文 prompt。风格 prompt 片段是主体，强度档位用中文档位名做语义锚，
 * 参考图数量决定是否追加「保持主体构图」约束。
 */
export function buildPrompt(style, strength, imgCount) {
  const s = typeof style === 'string' ? styleById(style) : style;
  if (!s) return '';
  const v = clampStrength(strength);
  const band = strengthBand(v).name;
  // 档位名用英文进 prompt：中文档位名会被生图模型原样画进图里（首批样品实测 5/10 张中招）
  const bandEn = { '轻度点缀': 'light accent', '平衡融合': 'balanced blend', '大改风格': 'maximum restyle' }[band] || band;
  const n = Math.max(1, Number(imgCount) || 1);
  const parts = [s.prompt];
  parts.push(`style intensity: ${bandEn} (${v}/100)`);
  if (n > 1) parts.push('keep the composition and subject of the main reference image');
  parts.push('high quality, no text overlay, no watermark');
  return parts.join(', ');
}

/**
 * 拼 `generate` 子命令的参数数组。
 * ⚠ 绝不把 base64 塞进来（Windows CreateProcess 命令行上限）——图片走 storage.json 中转。
 */
export function buildGenerateArgs(job, opts) {
  const o = opts || {};
  const j = job || {};
  const pick = (name, fallback) => o[name] !== undefined ? o[name] : (j[name] !== undefined ? j[name] : fallback);
  const args = ['generate', '--job-key', String(o.jobKey || KEYS.JOB)];
  const add = (flag, value) => { if (value !== undefined && value !== null && value !== '') args.push(flag, String(value)); };
  add('--storage', o.storagePath);
  add('--style', j.styleId);
  if (j.strength != null) add('--strength', clampStrength(j.strength));
  add('--provider', pick('provider'));
  add('--base-url', pick('baseUrl'));
  add('--resolution', pick('resolution'));
  add('--width', pick('width'));
  add('--height', pick('height'));
  add('--format', pick('format'));
  add('--quality', pick('quality'));
  add('--n', pick('n'));
  add('--http-timeout', pick('httpTimeoutSecs'));
  add('--retry', pick('retryCount'));
  if (j.size) add('--size', j.size);
  add('--model', o.model);
  add('--out-dir', o.outDir);
  args.push('--chunk', String(o.chunk || LIMITS.CHUNK_BYTES));
  return args;
}

/** 参数安全校验：每个 arg 必须 < 4096 字符（harness 组⑪ 钉死）。 */
export function assertArgsSafe(args) {
  const list = Array.isArray(args) ? args : [];
  for (let i = 0; i < list.length; i += 1) {
    if (String(list[i]).length >= LIMITS.MAX_ARG_CHARS) {
      return { ok: false, index: i, length: String(list[i]).length };
    }
  }
  return { ok: true, index: -1, length: 0 };
}

// ───────────────────────────────────────────────────────────────────────────
// §5 脚本输出解析（DESIGN §6.5 契约）
// ───────────────────────────────────────────────────────────────────────────

const EXIT_FALLBACK = { 2: 'IO_FAILED', 3: 'IO_FAILED', 4: 'WRITE_FAILED' };

/** 扩展名 → mime（落盘按实际字节嗅探，不硬编码 png）。 */
export function extToMime(ext) {
  const e = String(ext || '').toLowerCase();
  if (e === 'png') return 'image/png';
  if (e === 'gif') return 'image/gif';
  if (e === 'webp') return 'image/webp';
  if (e === 'bin') return 'application/octet-stream';
  return 'image/jpeg';
}

/** 按响应实际字节嗅探扩展名（DESIGN 计划 风险 7）。 */
export function sniffExt(bytes) {
  const b = bytes && bytes.length ? bytes : [];
  if (b[0] === 0x89 && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47) return 'png';
  if (b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff) return 'jpg';
  if (b[0] === 0x47 && b[1] === 0x49 && b[2] === 0x46) return 'gif';
  if (b[0] === 0x52 && b[1] === 0x49 && b[2] === 0x46 && b[3] === 0x46) return 'webp';
  return 'bin';
}

function fail(code, vars, extra) {
  return Object.assign(
    { ok: false, code, message: fillTemplate(CODE[code] || '', vars || {}), dataUrl: null, path: '', meta: null },
    extra || {},
  );
}

function apiCodeFromStatus(status, msg) {
  const st = Number(status);
  if (st === 401 || st === 403) return 'AUTH_FAILED';
  if (st === 429) return 'RATE_LIMITED';
  if (String(msg || '').indexOf('ModelNotOpen') >= 0) return 'MODEL_NOT_OPEN';
  if (String(msg || '').indexOf('AuthenticationError') >= 0) return 'AUTH_FAILED';
  return 'API_ERROR';
}

/**
 * 解析 `generate` 的 stdout。
 * CHUNK 按 `i` 落位 → 天然容忍乱序。
 * @returns {{ok:boolean, code:string|null, dataUrl:string|null, path:string, meta:object|null, message:string, tooLarge:boolean}}
 */
export function parseScriptOutput(stdout, exitCode, timedOut) {
  const raw = String(stdout == null ? '' : stdout);
  if (timedOut) return fail('IO_TIMEOUT', {});

  const kv = {};
  const slots = [];
  let begin = false;
  let end = false;

  const lines = raw.split(/\r?\n/);
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i].trim();
    if (!line) continue;
    if (line === 'BEGIN_IMAGE') { begin = true; continue; }
    if (line === 'END_IMAGE') { end = true; continue; }
    if (line.indexOf('CHUNK=') === 0) {
      const body = line.slice(6);
      const colon = body.indexOf(':');
      if (colon > 0) {
        const head = body.slice(0, colon).split('/');
        const idx = parseInt(head[0], 10);
        if (Number.isFinite(idx) && idx >= 1) slots[idx - 1] = body.slice(colon + 1);
      }
      continue;
    }
    const eq = line.indexOf('=');
    if (eq > 0) kv[line.slice(0, eq)] = line.slice(eq + 1);
  }

  let meta = null;
  if (kv.META) {
    try { meta = JSON.parse(kv.META); } catch (e) { meta = null; }
  }
  const path = kv.PATH || '';

  // 超 12MB：脚本改发 TOO_LARGE=1，不发 BEGIN_IMAGE
  if (kv.TOO_LARGE === '1') {
    return {
      ok: true,
      code: 'TOO_LARGE_PREVIEW',
      dataUrl: null,
      path,
      meta,
      message: CODE.TOO_LARGE_PREVIEW,
      tooLarge: true,
    };
  }

  if (kv.OK !== '1') {
    const msg = kv.MSG || '';
    const err = kv.ERR || '';
    let code = null;
    let vars = { message: msg, exitCode: exitCode == null ? '' : exitCode, code: kv.STATUS || '' };
    if (err === 'E_API') {
      code = apiCodeFromStatus(kv.STATUS, msg);
      vars = { code: kv.STATUS || '', message: msg, exitCode };
    } else if (SCRIPT_ERR_MAP[err]) {
      code = SCRIPT_ERR_MAP[err];
    } else if (kv.KEY === '0') {
      code = 'NO_KEY';
    } else if (EXIT_FALLBACK[exitCode]) {
      code = EXIT_FALLBACK[exitCode];
    } else {
      code = 'SCRIPT_EXIT';
    }
    return fail(code, vars);
  }

  // OK=1 但没有图 → 输出不完整
  const hasChunk = slots.some((s) => typeof s === 'string' && s.length > 0);
  if ((begin && !end) || !hasChunk) return fail('NO_RESULT', {});

  const b64 = slots.join('');
  if (!b64) return fail('NO_RESULT', {});
  if (b64.length > LIMITS.MAX_B64_BYTES) {
    return {
      ok: true,
      code: 'TOO_LARGE_PREVIEW',
      dataUrl: null,
      path,
      meta,
      message: CODE.TOO_LARGE_PREVIEW,
      tooLarge: true,
    };
  }

  const ext = (meta && meta.ext) || sniffExt(b64HeadBytes(b64));
  const dataUrl = `data:${extToMime(ext)};base64,${b64}`;
  const style = styleById((meta && meta.styleId) || '');
  const name = style ? style.name : '自定义';
  const elapsed = meta && meta.elapsedMs != null ? formatElapsed(meta.elapsedMs) : '';
  const size = meta && meta.bytes != null ? formatBytes(meta.bytes) : '';
  const bits = [name];
  if (meta && meta.strength != null) bits.push(`强度 ${clampStrength(meta.strength)}%`);
  if (meta && meta.size) bits.push(String(meta.size));
  if (elapsed) bits.push(`用时 ${elapsed}`);
  if (size) bits.push(size);

  return {
    ok: true,
    code: 'OK',
    dataUrl,
    path,
    meta,
    message: `生成完成：${bits.join(' · ')}`,
    tooLarge: false,
  };
}

// ───────────────────────────────────────────────────────────────────────────
// §6 硬件键映射（DESIGN 计划 §六：键盘与鼠标按钮共用同一个 cmdHardware）
// ───────────────────────────────────────────────────────────────────────────

const NONE = { cls: 'none', action: 'none', arg: null };

/** 按键归类：left/right/enter/escape/delete/digit/other。 */
export function classifyKey(key) {
  const k = String(key == null ? '' : key);
  if (k === 'ArrowLeft' || k === 'Left') return 'left';
  if (k === 'ArrowRight' || k === 'Right') return 'right';
  if (k === 'Enter') return 'enter';
  if (k === 'Escape' || k === 'Esc') return 'escape';
  if (k === 'Delete' || k === 'Backspace' || k === 'Del') return 'delete';
  if (/^[0-9]$/.test(k)) return 'digit';
  return 'other';
}

/**
 * 纯函数：阶段 × 按键 → 动作意图。不含任何副作用，键盘与鼠标共用。
 * @returns {{cls:string, action:string, arg:number|null}}
 */
export function mapHardwareKey(stage, ev) {
  if (!ev || typeof ev !== 'object') return NONE;
  const key = String(ev.key == null ? '' : ev.key);
  const cls0 = classifyKey(key);
  const s = STAGES.indexOf(stage) >= 0 ? stage : 'browse';
  const shift = !!ev.shiftKey;

  if (s === 'generating') return { cls: 'blocked', action: 'busy', arg: null };

  if (s === 'browse') {
    if (cls0 === 'left') return { cls: 'nav', action: 'selectImage', arg: -1 };
    if (cls0 === 'right') return { cls: 'nav', action: 'selectImage', arg: 1 };
    if (cls0 === 'enter') return { cls: 'confirm', action: 'confirm', arg: null };
    if (cls0 === 'escape') return { cls: 'command', action: 'clearImages', arg: null };
    if (cls0 === 'delete') return { cls: 'command', action: 'deleteImage', arg: null };
    if (cls0 === 'digit') {
      const n = Number(key);
      if (n >= 1 && n <= LIMITS.MAX_IMAGES) return { cls: 'pick', action: 'selectImage', arg: n - 1 };
      return { cls: 'wrong', action: 'wrongStage', arg: null };
    }
    return NONE;
  }

  if (s === 'adjusting') {
    if (cls0 === 'left') return { cls: 'nav', action: 'adjustStrength', arg: shift ? -1 : -5 };
    if (cls0 === 'right') return { cls: 'nav', action: 'adjustStrength', arg: shift ? 1 : 5 };
    if (cls0 === 'digit') return { cls: 'pick', action: 'pickStyle', arg: styleIndexFromKey(key) };
    if (cls0 === 'enter') return { cls: 'confirm', action: 'generate', arg: null };
    if (cls0 === 'escape') return { cls: 'back', action: 'back', arg: null };
    return NONE;
  }

  if (s === 'result') {
    if (cls0 === 'enter') return { cls: 'confirm', action: 'saveResult', arg: null };
    if (cls0 === 'digit') {
      if (key === '1') return { cls: 'confirm', action: 'saveResult', arg: null };
      if (key === '2') return { cls: 'pick', action: 'enterCreative', arg: null };
      if (key === '3') return { cls: 'command', action: 'discardResult', arg: null };
      return { cls: 'wrong', action: 'wrongStage', arg: null };
    }
    if (cls0 === 'escape') return { cls: 'back', action: 'back', arg: null };
    if (cls0 === 'left') return { cls: 'nav', action: 'compareSlide', arg: -10 };
    if (cls0 === 'right') return { cls: 'nav', action: 'compareSlide', arg: 10 };
    return NONE;
  }

  // creative
  if (cls0 === 'digit') {
    const idx = creativeIndexFromKey(key);
    if (idx >= 0) return { cls: 'pick', action: 'setCreative', arg: idx };
    return { cls: 'range', action: 'creativeRange', arg: Number(key) };
  }
  if (cls0 === 'left') return { cls: 'nav', action: 'cycleCreative', arg: -1 };
  if (cls0 === 'right') return { cls: 'nav', action: 'cycleCreative', arg: 1 };
  if (cls0 === 'enter') return { cls: 'confirm', action: 'saveResult', arg: null };
  if (cls0 === 'escape') return { cls: 'back', action: 'back', arg: null };
  return NONE;
}

/** 输入焦点判定（keys.js 的四道规避之一）。传桩对象即可测。 */
export function isTypingTarget(el) {
  if (!el || typeof el !== 'object') return false;
  const tag = String(el.tagName || '').toUpperCase();
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true;
  if (el.isContentEditable === true) return true;
  return false;
}

// ───────────────────────────────────────────────────────────────────────────
// §7 摘要与格式化（⌘SS 三条 tools 命令的可朗读结论）
// ───────────────────────────────────────────────────────────────────────────

/** 状态一句话摘要（PRD 3.2 示例句式：`素材 0 张 / 当前风格 <中文风格名> / 作品集 12 条`）。 */
export function summarizeState(s) {
  const st = s || {};
  const imgs = Array.isArray(st.images) ? st.images.length : 0;
  const gallery = Array.isArray(st.gallery) ? st.gallery.length : 0;
  const style = styleById(st.styleId);
  const parts = [`素材 ${imgs} / ${LIMITS.MAX_IMAGES} 张`];
  parts.push(style ? `当前风格 ${style.name}` : '当前风格 未选');
  if (style) parts.push(`强度 ${clampStrength(st.strength)}%`);
  parts.push(`作品集 ${gallery} / ${LIMITS.MAX_GALLERY} 条`);
  return parts.join(' · ');
}

/** 作品集一句话摘要（PRD 3.3 A5：不返回裸数组）。 */
export function summarizeGallery(items, n) {
  const list = Array.isArray(items) ? items : [];
  const count = Number(n) > 0 ? Number(n) : 3;
  if (!list.length) return STAGE_HINTS.gallery_empty;
  const recent = list.slice(0, count).map((it) => {
    const style = styleById(it && it.styleId);
    const name = style ? style.name : (it && it.styleName) || '自定义';
    const strength = it && it.strength != null ? `强度 ${clampStrength(it.strength)}` : '';
    const at = formatStamp(it && it.at);
    return [name, strength].filter(Boolean).join('·') + (at ? `（${at}）` : '');
  });
  return `作品集 ${list.length} / ${LIMITS.MAX_GALLERY} 条，最近 ${recent.length} 条：${recent.join('、')}`;
}

/** 毫秒 → `MM-DD HH:MM`（作品集条目用）。 */
export function formatStamp(ms) {
  const t = Number(ms);
  if (!Number.isFinite(t) || t <= 0) return '';
  const d = new Date(t);
  const p = (x) => String(x).padStart(2, '0');
  return `${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** 字节 → 中文可读（PRD 元信息行示例「278KB」）。 */
export function formatBytes(n) {
  const v = Number(n);
  if (!Number.isFinite(v) || v < 0) return '0 B';
  if (v < 1024) return `${Math.round(v)} B`;
  if (v < 1024 * 1024) return `${Math.round(v / 1024)}KB`;
  return `${(v / (1024 * 1024)).toFixed(1)}MB`;
}

/** 毫秒 → 中文可读（PRD 元信息行示例「用时 9.1 秒」）。 */
export function formatElapsed(ms) {
  const v = Number(ms);
  if (!Number.isFinite(v) || v < 0) return '0 秒';
  if (v < 1000) return `${Math.round(v)} 毫秒`;
  if (v < 60000) return `${(v / 1000).toFixed(1)} 秒`;
  const m = Math.floor(v / 60000);
  const s = Math.round((v % 60000) / 1000);
  return `${m} 分 ${s} 秒`;
}

// ───────────────────────────────────────────────────────────────────────────
// §8 色板量化（创意显示 ④ 色板拆解：唯一需要 canvas 的一种，取样算法在此）
// ───────────────────────────────────────────────────────────────────────────

/**
 * 4bit/通道分桶计数 → 取 Top k 色。
 * @param {Array<number>|Uint8ClampedArray} rgba 连续 RGBA
 * @returns {Array<{hex:string, rgb:number[], count:number, ratio:number}>}
 */
export function quantizePalette(rgba, w, h, k) {
  const data = rgba || [];
  const total = (Number(w) || 0) * (Number(h) || 0);
  const top = Number(k) > 0 ? Number(k) : 6;
  if (!data.length || !(total > 0)) return [];
  const buckets = new Map();
  const limit = Math.min(data.length, total * 4);
  for (let i = 0; i + 3 < limit; i += 4) {
    const a = data[i + 3];
    if (a != null && a < 128) continue;
    const r = data[i] >> 4;
    const g = data[i + 1] >> 4;
    const b = data[i + 2] >> 4;
    const key = (r << 8) | (g << 4) | b;
    const cur = buckets.get(key);
    if (cur) {
      cur.n += 1;
      cur.r += data[i];
      cur.g += data[i + 1];
      cur.b += data[i + 2];
    } else {
      buckets.set(key, { n: 1, r: data[i], g: data[i + 1], b: data[i + 2] });
    }
  }
  let counted = 0;
  buckets.forEach((v) => { counted += v.n; });
  if (!counted) return [];
  const out = [];
  buckets.forEach((v) => {
    out.push({
      hex: toHex(Math.round(v.r / v.n), Math.round(v.g / v.n), Math.round(v.b / v.n)),
      rgb: [Math.round(v.r / v.n), Math.round(v.g / v.n), Math.round(v.b / v.n)],
      count: v.n,
      ratio: v.n / counted,
    });
  });
  out.sort((a, b) => b.count - a.count);
  return out.slice(0, top);
}

function toHex(r, g, b) {
  const h = (x) => Math.max(0, Math.min(255, x)).toString(16).padStart(2, '0');
  return `#${h(r)}${h(g)}${h(b)}`;
}

// ───────────────────────────────────────────────────────────────────────────
// §9 作品集容量
// ───────────────────────────────────────────────────────────────────────────

/**
 * 截断到上限（items[0] 视为最新）。**不做自动淘汰**——满额由 cmds 拦截并提示（PRD 决策 2）。
 */
export function applyGalleryLimit(items, max) {
  const list = Array.isArray(items) ? items : [];
  const cap = Number(max) > 0 ? Number(max) : LIMITS.MAX_GALLERY;
  return list.slice(0, cap);
}

/** 是否已满（PRD：满 30 条拦截，不自动淘汰）。 */
export function isGalleryFull(items, max) {
  const list = Array.isArray(items) ? items : [];
  const cap = Number(max) > 0 ? Number(max) : LIMITS.MAX_GALLERY;
  return list.length >= cap;
}

// ───────────────────────────────────────────────────────────────────────────
// §10 测试缝
// ───────────────────────────────────────────────────────────────────────────

export const __test = {
  fillTemplate,
  migrateConfig,
  validateConfig,
  mapDimensionsForProvider,
  computeRunTimeout,
  styleById,
  styleIndexById,
  styleIndexFromKey,
  cycleStyleIndex,
  cycleCreativeIndex,
  creativeIndexFromKey,
  planImageFit,
  checkImageBudget,
  clampStrength,
  strengthBand,
  buildPrompt,
  buildGenerateArgs,
  assertArgsSafe,
  parseScriptOutput,
  classifyKey,
  mapHardwareKey,
  isTypingTarget,
  summarizeState,
  summarizeGallery,
  formatStamp,
  formatBytes,
  formatElapsed,
  quantizePalette,
  sniffExt,
  extToMime,
  applyGalleryLimit,
  isGalleryFull,
  FIT_TIERS,
};
