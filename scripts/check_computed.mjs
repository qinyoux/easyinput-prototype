/**
 * 运行时配色自检：读浏览器 **getComputedStyle 的实际结果**，而不是从 CSS 猜。
 *
 * # 为什么必须是运行时
 *
 * `scripts/audit_contrast.py` 按"规则声明的背景色 + 文字色"算对比度，
 * 这在**规则赢了**的前提下是准的。但存在第二类缺陷：
 * **规则声明得完全正确，却根本没赢**。
 *
 *   v0.41.12 的真实事故：
 *     .desk-prefer-item { color: var(--on-hi) }      ← 声明正确
 *     .field label      { color: var(--text-dim) }   ← 特异度 0,1,1，实际赢
 *   那个元素是 <label class="desk-prefer-item"> 且外层套着 .field，
 *   于是文字实际是 --text-dim 压在橙底上，几乎看不见。
 *
 * 静态分析在这里连续失败过三次（详见 audit_contrast.py 里的记录），
 * 根因是**祖先关系属于 DOM，不属于 CSS** —— 选择器文本里没有祖先信息。
 * 所以唯一可靠的依据是问浏览器：`getComputedStyle(el).color` 返回的就是赢家。
 *
 * # 用法
 *
 *   1) 带调试端口起应用：
 *        WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222" \
 *          src-tauri/target/debug/easyop.exe
 *   2) 跑本脚本：
 *        node scripts/check_computed.mjs
 *
 * 退出码：0 = 全部达标；1 = 有不达标项。
 */

import { readFileSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const CDP = path.join(ROOT, ".workbuddy", "cdp_eval.mjs");

if (!existsSync(CDP)) {
  console.error("⛔ 找不到 .workbuddy/cdp_eval.mjs —— 运行时检查依赖它。");
  process.exit(1);
}

/** 在页面里执行的探针：遍历所有元素，取"非透明背景 + 有文字"的组合，算实际对比度。 */
const PROBE = `
(() => {
  const lum = (rgb) => {
    const [r, g, b] = rgb;
    const f = (c) => { c /= 255; return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
  };
  const parse = (s) => {
    const m = s.match(/rgba?\\(([^)]+)\\)/);
    if (!m) return null;
    const p = m[1].split(",").map((x) => parseFloat(x));
    if (p.length < 3 || p[3] === 0) return null;   // 全透明 ⇒ 看不到，跳过
    return [p[0], p[1], p[2]];
  };
  const cr = (a, b) => {
    const x = lum(a), y = lum(b);
    const hi = Math.max(x, y), lo = Math.min(x, y);
    return (hi + 0.05) / (lo + 0.05);
  };
  // 逐层往上找第一个不透明背景（background-color 为 transparent 时看父级）
  const bgOf = (el) => {
    let n = el;
    while (n && n !== document.documentElement) {
      const c = parse(getComputedStyle(n).backgroundColor);
      if (c) return c;
      n = n.parentElement;
    }
    return [255, 255, 255];
  };
  const out = [];
  for (const el of document.querySelectorAll("*")) {
    // 只看直接持有可见文字的元素
    const txt = Array.from(el.childNodes)
      .filter((n) => n.nodeType === 3)
      .map((n) => n.textContent.trim())
      .join("");
    if (!txt) continue;
    const st = getComputedStyle(el);
    if (st.visibility === "hidden" || st.display === "none" || parseFloat(st.opacity) < 0.15) continue;
    const fg = parse(st.color);
    if (!fg) continue;
    const bg = bgOf(el);
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) continue;
    out.push({
      sel: el.tagName.toLowerCase() + (el.className && typeof el.className === "string"
            ? "." + el.className.trim().split(/\\s+/).join(".") : ""),
      text: txt.slice(0, 18),
      ratio: Math.round(cr(fg, bg) * 100) / 100,
    });
  }
  return out;
})()
`;

let raw;
try {
  raw = execFileSync("node", [CDP, "eval", PROBE, "--url", "localhost:1420"], {
    encoding: "utf8",
    timeout: 30000,
  });
} catch (e) {
  console.error("⛔ CDP 求值失败 —— 应用是否带 --remote-debugging-port=9222 启动？");
  console.error(String(e.stderr || e.message).slice(0, 500));
  process.exit(1);
}

let items;
try {
  items = JSON.parse(raw.trim());
} catch {
  console.error("⛔ 无法解析 CDP 返回：", raw.slice(0, 300));
  process.exit(1);
}

if (!Array.isArray(items) || items.length === 0) {
  console.log("（页面上没有匹配到带文字的元素 —— 可能当前不在设置页）");
  process.exit(0);
}

const THEME = process.argv.includes("--light") ? "亮色" : "深色";
const bad = items.filter((i) => i.ratio < 4.5);
const warn = items.filter((i) => i.ratio >= 4.5 && i.ratio < 7);

console.log(`\n运行时配色自检 · ${THEME}主题 · 采样 ${items.length} 个文字元素`);
console.log(`  ✅ ≥7:1  ${items.length - bad.length - warn.length} 个`);
console.log(`  △ 4.5–7:1 ${warn.length} 个（达标但偏弱）`);
console.log(`  ✗ <4.5:1 ${bad.length} 个${bad.length ? "  ← 需修" : ""}`);

if (bad.length) {
  console.log("\n不达标明细（按严重度排序）：");
  for (const b of bad.sort((a, x) => a.ratio - x.ratio).slice(0, 30)) {
    console.log(`  ✗ ${b.ratio.toFixed(2).padStart(6)}:1  ${b.sel.slice(0, 46).padEnd(48)} "${b.text}"`);
  }
  console.log("\n  ⇒ 这些是**浏览器实际算出的颜色**，不是 CSS 声明值 —— 说明可能存在特异度覆盖。");
  process.exit(1);
}
console.log("\n✅ 全部达标\n");
process.exit(0);
