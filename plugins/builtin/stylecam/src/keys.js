/**
 * StyleCam 风格调色盘 —— 硬件键层（window keydown）
 *
 * 硬边界（DESIGN §4 无环约束 2 / §5）：
 *   - **绝不 import `ui.js` / `cmds.js`**：键盘只产出「动作意图」，由 ui.js 注入的 onAction 去调命令
 *   - 不碰 DOM 结构（只挂/摘 window 监听）
 *
 * 四道规避（DESIGN 8 节 T41）：
 *   ① state.mounted（页签未挂载不响应）
 *   ② config.hwKeys（⌨ 硬件键开关，默认开）
 *   ③ 非 Ctrl / Meta / Alt（不和 easyop 全局快捷键抢）
 *   ④ 非 INPUT / TEXTAREA / SELECT / contentEditable
 *
 * 自愈（DESIGN 2.1 偏离 #8）：`PluginPageContainer.tsx:14-28` 用 async IIFE 拿 lifecycle，
 * 若激活未完成时卸载，`lifecycle` 仍为 null → dispose 永不执行，keydown 会留在 window 上。
 * 因此 handler **首行**判断容器是否还在文档里，不在就自己摘掉自己。
 */

import { LIMITS } from './constants.js';
import { mapHardwareKey, isTypingTarget } from './logic.js';
import { getState } from './state.js';

// ───────────────────────────────────────────────────────────────────────────
// 挂摘计数（QA 断言「挂/摘次数相等」用；DESIGN T21 组⑭）
// ───────────────────────────────────────────────────────────────────────────

let _attachCount = 0;
let _detachCount = 0;

export function getAttachCount() {
  return _attachCount;
}

export function getDetachCount() {
  return _detachCount;
}

export function resetKeyCounters() {
  _attachCount = 0;
  _detachCount = 0;
}

/** 允许 repeat 的动作（旋钮旋转类：方向键、对比滑杆）。 */
function isRepeatable(action) {
  return (
    action === 'adjustStrength' ||
    action === 'selectImage' ||
    action === 'cycleCreative' ||
    action === 'compareSlide'
  );
}

// ───────────────────────────────────────────────────────────────────────────
// 挂载 / 摘除
// ───────────────────────────────────────────────────────────────────────────

/**
 * 挂 window keydown。返回摘除函数（幂等）。
 *
 * @param {{root?:HTMLElement, container?:HTMLElement}} view 视图对象（只取 root/container 做自愈判断）
 * @param {(intent:{cls:string, action:string, arg:number|null}) => void} onAction 动作意图回调
 * @returns {() => void} detachHwKeys
 */
export function attachHwKeys(view, onAction) {
  const v = view || {};
  const g = typeof globalThis !== 'undefined' ? globalThis : null;
  if (!g || typeof g.addEventListener !== 'function' || typeof onAction !== 'function') {
    return () => {};
  }

  let lastRotate = 0;
  let detached = false;

  /** 自愈：容器已经不在文档里（宿主 dispose 竞态）→ 自己摘掉自己。 */
  function stillAlive() {
    const root = v.root;
    const box = v.container;
    if (root && typeof root.isConnected === 'boolean' && !root.isConnected) return false;
    if (box && typeof box.isConnected === 'boolean' && !box.isConnected) return false;
    return true;
  }

  function detach() {
    if (detached) return;
    detached = true;
    try {
      g.removeEventListener('keydown', handler);
    } catch (e) {
      /* ignore */
    }
    _detachCount += 1;
  }

  function handler(e) {
    // ── 自愈：不依赖宿主一定调 dispose（偏离 #8） ──
    if (!stillAlive()) {
      detach();
      return;
    }

    // ── 门控 ①：页签未挂载 ──
    const st = getState();
    if (!st.mounted) return;

    // ── 门控 ②：硬件键开关 ──
    if (!st.config || !st.config.hwKeys) return;

    // ── 门控 ③：Ctrl / Meta / Alt 一律放行给宿主 ──
    if (e.ctrlKey || e.metaKey || e.altKey) return;

    // ── 门控 ④：输入焦点内 ──
    if (isTypingTarget(e.target)) return;

    const intent = mapHardwareKey(st.stage, e);
    if (!intent || !intent.action || intent.action === 'none') return;

    // 命令类：按住不放只算一次
    if (!isRepeatable(intent.action) && e.repeat) return;

    // 旋转类：允许 repeat，但 60ms 节流（旋钮手感）
    if (isRepeatable(intent.action)) {
      const now = Date.now();
      if (now - lastRotate < LIMITS.ROTATE_THROTTLE_MS) return;
      lastRotate = now;
    }

    // ── 已消费才 preventDefault（不吞宿主快捷键、不拦方向键滚屏之外的行为） ──
    if (typeof e.preventDefault === 'function') e.preventDefault();
    onAction(intent);
  }

  g.addEventListener('keydown', handler);
  _attachCount += 1;
  return detach;
}

/** 摘除（attachHwKeys 的返回值即此函数，这里再给一个具名导出便于 QA）。 */
export function detachHwKeys(detach) {
  if (typeof detach === 'function') detach();
}

/** 转出 logic.isTypingTarget —— DESIGN §5 要求 keys.js 也导出它（QA 传桩对象即可测）。 */
export { isTypingTarget };

// ───────────────────────────────────────────────────────────────────────────
// 测试缝
// ───────────────────────────────────────────────────────────────────────────

export const __test = {
  attachHwKeys,
  detachHwKeys,
  isTypingTarget,
  isRepeatable,
  getAttachCount,
  getDetachCount,
  resetKeyCounters,
};
