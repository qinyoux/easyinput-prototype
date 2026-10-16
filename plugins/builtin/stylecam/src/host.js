/**
 * StyleCam 风格调色盘 —— `eo.*` 门面
 *
 * 硬边界（DESIGN §4 无环约束 3）：
 *   - **只封装 `eo.*` 的调用与错误兜底**，不做业务判断、不读 state、不拼业务参数
 *   - 绝不 import `state.js` / `cmds.js`（否则依赖图成环）
 *   - 业务参数由 cmds 用 `logic.buildGenerateArgs` 拼好后传进来
 *
 * 宿主事实（已核实，别再查）：
 *   - `eo.*` 只有 8 个能力，本插件用 storage / log / run 三个
 *   - `eo.run(program, args, opts)`，opts 只认 `timeout_secs` 与 `cwd`
 *     —— `timeoutSecs` 被 serde 静默忽略（plugin_host.rs:196-201），两个都写
 *   - `cwd` 必须传 `context.pluginDir`（pluginHost.ts:159 提供）
 */

import { LIMITS, LOG_PREFIX } from './constants.js';

// ───────────────────────────────────────────────────────────────────────────
// §1 上下文
// ───────────────────────────────────────────────────────────────────────────

let _ctx = null;

/** activate 时调用一次，之后所有门面函数都从这里取 `eo`。 */
export function initHost(ctx) {
  _ctx = ctx && ctx.api ? ctx : null;
  return !!_ctx;
}

export function getCtx() {
  return _ctx;
}

/** 无宿主上下文（harness / 无 DOM 场景）时的判定。 */
export function hasHost() {
  return !!(_ctx && _ctx.api);
}

// ───────────────────────────────────────────────────────────────────────────
// §2 日志（永不抛、永不阻断）
// ───────────────────────────────────────────────────────────────────────────

/**
 * 安全日志：宿主未就绪 / 抛错都静默吞掉。
 * 日志前缀一律 `[plugin] stylecam: …`（宿主已自动加 `[plugin]`）。
 */
export function safeLog(msg, lvl) {
  const text = `${LOG_PREFIX}${String(msg == null ? '' : msg)}`;
  const level = lvl === 'warn' || lvl === 'error' ? lvl : 'info';
  try {
    if (_ctx && _ctx.api && _ctx.api.log && typeof _ctx.api.log[level] === 'function') {
      _ctx.api.log[level](text);
    }
  } catch (e) {
    // 日志失败绝不能影响主流程
  }
  return text;
}

export const logInfo = (m) => safeLog(m, 'info');
export const logWarn = (m) => safeLog(m, 'warn');
export const logError = (m) => safeLog(m, 'error');

// ───────────────────────────────────────────────────────────────────────────
// §3 路径（脚本按 __file__ 自行推导同一份 storage.json，这里只给前端用）
// ───────────────────────────────────────────────────────────────────────────

/** 插件目录内的固定路径。pluginDir 缺失时一律返回空串，不做猜测。 */
export function pluginPaths() {
  const dir = (_ctx && _ctx.pluginDir) || '';
  if (!dir) return { pluginDir: '', scriptPath: '', envPath: '', envExamplePath: '', storagePath: '' };
  const base = dir.replace(/[\\/]+$/, '');
  return {
    pluginDir: base,
    scriptPath: `${base}/tools/stylecam_io.py`,
    envPath: `${base}/tools/stylecam_io.env`,
    envExamplePath: `${base}/tools/stylecam_io.env.example`,
    storagePath: `${base}/storage.json`,
  };
}

// ───────────────────────────────────────────────────────────────────────────
// §4 storage（失败返回 null / false，绝不抛给调用方）
// ───────────────────────────────────────────────────────────────────────────

export async function storageGet(key) {
  if (!hasHost()) return null;
  try {
    return await _ctx.api.storage.get(key);
  } catch (e) {
    safeLog(`storage.get 失败 key=${key}：${e && e.message ? e.message : e}`, 'warn');
    return null;
  }
}

/**
 * 写入。返回是否成功。
 * ⚠ `storage.set(key, null)` **不删 key**，文件里留下 `null`（plugin_host.rs:150 是 map.insert）。
 *    瘦身目的达到，但断言要写「值为 null」而不是「key 不存在」。
 */
export async function storageSet(key, val) {
  if (!hasHost()) return false;
  try {
    await _ctx.api.storage.set(key, val);
    return true;
  } catch (e) {
    safeLog(`storage.set 失败 key=${key}：${e && e.message ? e.message : e}`, 'warn');
    return false;
  }
}

// ───────────────────────────────────────────────────────────────────────────
// §5 Python 解释器探测（绝不硬编码绝对路径 —— studio 硬编码是反例）
// ───────────────────────────────────────────────────────────────────────────

const PY_CANDIDATES = [
  { program: 'python', prefix: [] },
  { program: 'py', prefix: ['-3'] },
  { program: 'python3', prefix: [] },
];

let _py = null; // { program, prefix } | null

/** 通用子进程调用：返回 `{exit_code, stdout, stderr, timed_out, cancelled}` 或 null。 */
export async function runRaw(program, args, secs) {
  if (!hasHost()) return null;
  const paths = pluginPaths();
  const t = Number(secs) > 0 ? Number(secs) : LIMITS.SHORT_RUN_TIMEOUT_SECS;
  const opts = {
    cwd: paths.pluginDir,
    // ↓ 双写：timeout_secs 生效（Rust 侧字段名），timeoutSecs 只为可读性对齐 studio
    timeout_secs: t,
    timeoutSecs: t,
  };
  try {
    return await _ctx.api.run(program, Array.isArray(args) ? args : [], opts);
  } catch (e) {
    safeLog(`eo.run 失败 ${program}：${e && e.message ? e.message : e}`, 'warn');
    return null;
  }
}

/**
 * 探测并缓存可用的 Python 启动器。成功者由 cmds 写回 `stylecam.config.pyExe`。
 * @returns {Promise<{program:string, prefix:string[]}|null>}
 */
export async function resolvePythonLauncher(force) {
  if (!force && _py) return _py;
  if (!hasHost()) return null;
  for (let i = 0; i < PY_CANDIDATES.length; i += 1) {
    const c = PY_CANDIDATES[i];
    const r = await runRaw(c.program, c.prefix.concat(['--version']), LIMITS.SHORT_RUN_TIMEOUT_SECS);
    if (r && r.exit_code === 0) {
      _py = { program: c.program, prefix: c.prefix.slice() };
      safeLog(`Python 解释器：${c.program} ${c.prefix.join(' ')} → ${String(r.stdout || '').trim()}`);
      return _py;
    }
  }
  safeLog('没找到可用的 Python 解释器（试过 python / py -3 / python3）', 'error');
  _py = null;
  return null;
}

/** 只要解释器名（'python' / 'py' / 'python3'）；没找到返回空串。 */
export async function resolvePython(force) {
  const l = await resolvePythonLauncher(force);
  return l ? l.program : '';
}

/** 清掉缓存（deactivate 或 harness 复位用）。 */
export function resetPythonCache() {
  _py = null;
}

// ───────────────────────────────────────────────────────────────────────────
// §6 脚本调用
// ───────────────────────────────────────────────────────────────────────────

/**
 * 调 `tools/stylecam_io.py`。
 * @param {string[]} args 子命令 + 参数（**每个 < 4096 字符，绝不塞 base64**）
 * @param {number} timeoutSec 超时秒数
 * @returns {Promise<{exit_code:number, stdout:string, stderr:string, timed_out:boolean, cancelled:boolean}|null>}
 */
export async function runScript(args, timeoutSec) {
  const launcher = await resolvePythonLauncher(false);
  if (!launcher) return null;
  const paths = pluginPaths();
  const secs = Number(timeoutSec) > 0 ? Number(timeoutSec) : LIMITS.RUN_TIMEOUT_SECS;
  const full = launcher.prefix.concat(['-u', paths.scriptPath], Array.isArray(args) ? args : []);
  return runRaw(launcher.program, full, secs);
}

/** harness 复位用：清掉上下文与解释器缓存。 */
export function resetHost() {
  _ctx = null;
  _py = null;
}

export const __test = {
  initHost,
  getCtx,
  hasHost,
  safeLog,
  logInfo,
  logWarn,
  logError,
  pluginPaths,
  storageGet,
  storageSet,
  runRaw,
  runScript,
  resolvePython,
  resolvePythonLauncher,
  resetPythonCache,
  resetHost,
  PY_CANDIDATES,
};
