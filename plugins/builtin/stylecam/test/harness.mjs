#!/usr/bin/env node
/**
 * StyleCam 风格调色盘 —— 验证台（Node，零依赖）
 *
 * 为什么这么写：`index.js` 是 esbuild 打包出的自包含 ESM，宿主用 `blob:` 动态 import 加载，
 * 它**不能被 Node 直接 import**（无相对路径、且依赖宿主 `eo.*` 与 DOM）。因此分两层断言：
 *   ① 行为层：直接 import `src/` 下 8 个模块（Node 可直接跑），配桩 `eo.*` + 极简 DOM 桩，
 *      真跑 activate / renderPage / 15 条命令 / 生图全链路；
 *   ② 契约层：对 `index.js` 做文本级断言（自包含、无 fixed、无 file://、无 TAURI 后门）。
 *
 * 分组沿用计划第 8 节：① 命令注册 / ⑧ 清单↔实现 / ⑨ 页签契约 P1–P4 /
 * ⑩ 命名空间 / ⑪ 图片管线 / ⑫ 输出解析 / ⑬ 纯函数 / ⑭ 页签补强 / ⑮ 文案一致性 / ⑯ 契约交叉。
 *
 * 用法：node plugins/builtin/stylecam/test/harness.mjs
 */

import { readFileSync, existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";

const HERE = resolve(fileURLToPath(import.meta.url), "..");
const PLUGIN_DIR = resolve(HERE, "..");
const SRC_DIR = join(PLUGIN_DIR, "src");
const REPO = resolve(PLUGIN_DIR, "..", "..", "..");
const BUILTIN_DIR = resolve(PLUGIN_DIR, "..");

const BUNDLE = join(PLUGIN_DIR, "index.js");
const MANIFEST = join(PLUGIN_DIR, "plugin.json");
const PRD = join(PLUGIN_DIR, "PRD.md");
const DESIGN = join(PLUGIN_DIR, "DESIGN.md");

// ───────────────────────────────────────────────────────────────────────────
// 断言器
// ───────────────────────────────────────────────────────────────────────────

let pass = 0;
let fail = 0;
const failures = [];

/**
 * @param {string} group 组号，如 '①'
 * @param {string} name 断言名
 * @param {boolean} cond
 * @param {string} [detail] 期望 vs 实际
 */
function check(group, name, cond, detail) {
  if (cond) {
    pass += 1;
    console.log(`  ✔ ${name}`);
  } else {
    fail += 1;
    const line = `[${group}] ${name}${detail ? ` —— ${detail}` : ""}`;
    failures.push(line);
    console.log(`  ✘ ${line}`);
  }
}

/** 期望值 vs 实际值的中文说明。 */
function vs(label, expected, actual) {
  return `${label} 期望 ${JSON.stringify(expected)}，实际 ${JSON.stringify(actual)}`;
}

// ───────────────────────────────────────────────────────────────────────────
// 桩 eo.*（严格镜像 src/pluginHost.ts 提供的能力）
// ───────────────────────────────────────────────────────────────────────────

function makeEo(opts) {
  const store = opts.store;
  const calls = opts.calls;
  return {
    storage: {
      get: async (key) => {
        calls.push(["storage.get", key]);
        return key in store ? store[key] : null;
      },
      set: async (key, value) => {
        store[key] = value;
        calls.push(["storage.set", key, value]);
      },
    },
    log: {
      info: async (msg) => calls.push(["log.info", msg]),
      warn: async (msg) => calls.push(["log.warn", msg]),
      error: async (msg) => calls.push(["log.error", msg]),
    },
    run: async (program, args, o) => {
      const list = Array.isArray(args) ? args.slice() : [];
      calls.push(["run", program, list, o ? { ...o } : undefined]);
      const key = list.join(" ");
      const canned = (opts.runReplies || []).find((r) => key.includes(r.match));
      return {
        exit_code: canned && canned.exitCode != null ? canned.exitCode : 0,
        stdout: canned ? canned.stdout || "" : "",
        stderr: canned ? canned.stderr || "" : "",
        timed_out: !!(canned && canned.timedOut),
        cancelled: false,
      };
    },
  };
}

// ───────────────────────────────────────────────────────────────────────────
// 极简 DOM 桩 —— 只为把「页签渲染」与「keydown 挂摘」真跑起来
// ───────────────────────────────────────────────────────────────────────────

function makeEl(tag) {
  const el = {
    tagName: String(tag).toUpperCase(),
    children: [],
    parentNode: null,
    attributes: {},
    textContent: "",
    className: "",
    value: "",
    style: {},
    classList: { add() {}, remove() {}, contains: () => false },
    setAttribute(k, v) {
      el.attributes[k] = String(v);
    },
    getAttribute(k) {
      return k in el.attributes ? el.attributes[k] : null;
    },
    appendChild(c) {
      c.parentNode = el;
      el.children.push(c);
      return c;
    },
    removeChild(c) {
      const i = el.children.indexOf(c);
      if (i >= 0) el.children.splice(i, 1);
      c.parentNode = null;
      return c;
    },
    addEventListener() {},
    removeEventListener() {},
    click() {},
    getContext() {
      return { drawImage() {}, getImageData: () => ({ data: [] }) };
    },
    toDataURL() {
      return "data:image/jpeg;base64,AAAB";
    },
    /** 只支持插件实际用到的 `[data-sc="x"]`；其余选择器一律返回 null（插件已做 null 兜底）。 */
    querySelector(sel) {
      const m = /\[data-sc="([^"]+)"\]/.exec(String(sel));
      if (!m) return null;
      const want = m[1];
      const stack = [el];
      while (stack.length) {
        const cur = stack.pop();
        if (cur.attributes && cur.attributes["data-sc"] === want) return cur;
        for (let i = 0; i < cur.children.length; i += 1) stack.push(cur.children[i]);
      }
      return null;
    },
    querySelectorAll(sel) {
      const m = /^style\[([a-zA-Z-]+)="([^"]+)"\]$/.exec(String(sel));
      if (!m) return [];
      const out = [];
      const stack = [el];
      while (stack.length) {
        const cur = stack.pop();
        if (cur.attributes && cur.attributes[m[1]] === m[2]) out.push(cur);
        for (let i = 0; i < cur.children.length; i += 1) stack.push(cur.children[i]);
      }
      return out;
    },
  };
  Object.defineProperty(el, "firstChild", { get: () => el.children[0] || null });
  return el;
}

/** 装上 document + globalThis 事件桩。返回 { head, listeners }。 */
function installDomStub() {
  const head = makeEl("head");
  const body = makeEl("body");
  globalThis.document = { head, body, createElement: (t) => makeEl(t) };

  const listeners = {};
  globalThis.addEventListener = (type, fn) => {
    if (!listeners[type]) listeners[type] = [];
    listeners[type].push(fn);
  };
  globalThis.removeEventListener = (type, fn) => {
    const arr = listeners[type] || [];
    const i = arr.indexOf(fn);
    if (i >= 0) arr.splice(i, 1);
  };
  return { head, body, listeners };
}

// ───────────────────────────────────────────────────────────────────────────
// 装载 src/ 模块（ESM 单例，全程共用）
// ───────────────────────────────────────────────────────────────────────────

const urlOf = (p) => pathToFileURL(p).href;

let MOD = null;
async function loadSrc() {
  if (MOD) return MOD;
  const [constants, logic, cmds, keys, styles, ui, main, state] = await Promise.all([
    import(urlOf(join(SRC_DIR, "constants.js"))),
    import(urlOf(join(SRC_DIR, "logic.js"))),
    import(urlOf(join(SRC_DIR, "cmds.js"))),
    import(urlOf(join(SRC_DIR, "keys.js"))),
    import(urlOf(join(SRC_DIR, "styles.js"))),
    import(urlOf(join(SRC_DIR, "ui.js"))),
    import(urlOf(join(SRC_DIR, "main.js"))),
    import(urlOf(join(SRC_DIR, "state.js"))),
  ]);
  MOD = { constants, logic, cmds, keys, styles, ui, main, state };
  return MOD;
}

/**
 * 启动一次插件：`deactivate` 清场 → `activate(context)`。
 * @returns {Promise<{commands:object, pages:object, calls:Array, store:object, mod:object}>}
 */
async function bootstrap(opts = {}) {
  const { main } = await loadSrc();
  main.deactivate();
  main.__test.resetAll();
  main.__test.resetKeyCounters();

  const store = opts.store || {};
  const calls = [];
  const commands = {};
  const pages = {};
  const context = {
    api: makeEo({ store, calls, ...opts }),
    pluginId: "stylecam",
    pluginDir: PLUGIN_DIR,
    registerCommand: (id, fn) => {
      commands[id] = fn;
    },
    registerPage: (id, fn) => {
      pages[id] = fn;
    },
  };
  await main.activate(context);
  // activate 里的 loadConfig/loadGallery 是游离 Promise，等一个宏任务让它落地，避免断言读到半截状态
  await new Promise((r) => setTimeout(r, 0));
  return { commands, pages, calls, store, context, mod: main };
}

/** 默认配置（避免首次进页签弹硬件键说明浮层 / 隐私浮层带来的噪声）。 */
function seededConfig() {
  return { v: 1, outDir: "", hwKeys: true, privacyAcked: true, hwIntroSeen: true, pyExe: "", model: "", size: "2K" };
}

// ───────────────────────────────────────────────────────────────────────────
// 组 ① 命令注册
// ───────────────────────────────────────────────────────────────────────────

const EXPECTED_COMMANDS = [
  "stylecam.state",
  "stylecam.image.add",
  "stylecam.image.remove",
  "stylecam.image.clear",
  "stylecam.style.pick",
  "stylecam.strength.set",
  "stylecam.generate",
  "stylecam.result.save",
  "stylecam.result.export",
  "stylecam.creative.render",
  "stylecam.gallery.list",
  "stylecam.gallery.remove",
  "stylecam.config.get",
  "stylecam.config.set",
  "stylecam.config.validate",
  "stylecam.config.reset",
  "stylecam.provider.list",
  "stylecam.key.status",
  "stylecam.key.set",
  "stylecam.doctor",
  "stylecam.hardware",
];

async function group1() {
  console.log("\n① 命令注册（21 条 / id 前缀）");
  const G = "①";
  const { commands, calls } = await bootstrap({ store: { "stylecam.config": seededConfig() } });

  for (const id of EXPECTED_COMMANDS) {
    check(G, `注册了 ${id}`, typeof commands[id] === "function", `commands[${id}] 类型为 ${typeof commands[id]}`);
  }
  const registered = Object.keys(commands);
  check(G, "注册条数恰好 21", registered.length === 21, vs("注册条数", 21, registered.length));

  const badPrefix = registered.filter((id) => !id.startsWith("stylecam."));
  check(G, "全部 id 以 stylecam. 开头", badPrefix.length === 0, `不合前缀：${badPrefix.join(", ")}`);

  const ids = Object.keys((await loadSrc()).cmds.COMMANDS);
  check(G, "cmds.COMMANDS 恰有 21 条", ids.length === 21, vs("COMMANDS 条数", 21, ids.length));
  check(
    G,
    "COMMANDS 键与宿主注册的完全一致",
    ids.slice().sort().join(",") === registered.slice().sort().join(","),
    vs("COMMANDS 键", ids.slice().sort().join(","), registered.slice().sort().join(",")),
  );

  // 三条 tools 命令必须额外打日志（DESIGN §10.4.7：⌘SS 丢弃返回值，不打日志用户看不到反馈）
  const tools = (await loadSrc()).cmds.TOOLS_COMMANDS;
  check(G, "tools 恰有 6 条", tools.length === 6, vs("tools 条数", 6, tools.length));
  for (const id of tools) {
    const before = calls.filter((c) => c[0] === "log.info").length;
    await commands[id]();
    const after = calls.filter((c) => c[0] === "log.info").length;
    check(G, `tools 命令 ${id} 调了 eo.log.info`, after > before, `log.info 次数 ${before} → ${after}`);
  }

  check(G, "激活时写了日志", calls.some((c) => c[0] === "log.info"));
}

// ───────────────────────────────────────────────────────────────────────────
// 组 ⑧ 清单 ↔ 实现（双向差集为空）
// ───────────────────────────────────────────────────────────────────────────

/** 从打包产物里抽出 `var COMMANDS = { "stylecam.x": fn, ... }` 的键。 */
function bundleCommandIds(src) {
  const m = /var COMMANDS = \{([\s\S]*?)\n\};/.exec(src);
  if (!m) return null;
  return [...m[1].matchAll(/"(stylecam\.[A-Za-z0-9._-]+)"\s*:/g)].map((x) => x[1]);
}

async function group8() {
  console.log("\n⑧ 清单 ↔ 实现（plugin.json 声明 vs index.js 注册，双向差集为空）");
  const G = "⑧";
  const manifest = JSON.parse(readFileSync(MANIFEST, "utf8"));
  const src = readFileSync(BUNDLE, "utf8");

  const declared = (manifest.contributes.commands || []).map((c) => c.id);
  const implemented = bundleCommandIds(src);

  check(G, "index.js 里能定位到 COMMANDS 表", Array.isArray(implemented), "未匹配到 `var COMMANDS = { ... };`，断言无法进行");
  if (!implemented) return;

  const missing = declared.filter((id) => !implemented.includes(id));
  const extra = implemented.filter((id) => !declared.includes(id));

  check(G, "清单声明的每一条都实现了（差集为空）", missing.length === 0, `清单有、实现没有：${missing.join(", ") || "无"}`);
  check(G, "实现里没有清单未声明的命令（差集为空）", extra.length === 0, `实现有、清单没有：${extra.join(", ") || "无"}`);
  check(G, "声明条数 === 实现条数", declared.length === implemented.length, vs("条数", declared.length, implemented.length));

  const noTitle = (manifest.contributes.commands || []).filter((c) => !c.title);
  check(G, "每条命令都有 title", noTitle.length === 0, `缺 title：${noTitle.map((c) => c.id).join(", ") || "无"}`);
  const noKw = (manifest.contributes.commands || []).filter((c) => !Array.isArray(c.keywords) || !c.keywords.length);
  check(G, "每条命令都有 keywords", noKw.length === 0, `缺 keywords：${noKw.map((c) => c.id).join(", ") || "无"}`);

  const toolsCmds = (manifest.contributes.tools || []).map((t) => t.command);
  const dangling = toolsCmds.filter((id) => !declared.includes(id));
  check(G, "tools 只引用已声明的命令", dangling.length === 0, `悬空引用：${dangling.join(", ") || "无"}`);
  const noSubtitle = (manifest.contributes.tools || []).filter((t) => !t.subtitle);
  check(G, "每条 tools 都有 subtitle", noSubtitle.length === 0, `缺 subtitle：${noSubtitle.map((t) => t.command).join(", ") || "无"}`);

  check(G, "清单声明 1 个页签", (manifest.contributes.pages || []).length === 1, vs("页签数", 1, (manifest.contributes.pages || []).length));
  check(G, "index.js 注册了页签 id stylecam.board", src.includes('"stylecam.board"'), "未在打包产物里找到 stylecam.board");
  check(
    G,
    "registerPage 用的就是清单里的页签 id",
    src.includes('registerPage("stylecam.board"') || src.includes("registerPage('stylecam.board'"),
    "打包产物里 registerPage 的入参与清单 pages[].id 不一致",
  );
}

// ───────────────────────────────────────────────────────────────────────────
// 组 ⑨ 页签契约 P1–P4
// ───────────────────────────────────────────────────────────────────────────

async function group9(dom) {
  console.log("\n⑨ 页签契约 P1–P4（同步返回 / dispose 摘干净 / 不改容器定位）");
  const G = "⑨";
  const { pages } = await bootstrap({ store: { "stylecam.config": seededConfig() } });
  const render = pages["stylecam.board"];
  check(G, "页签 stylecam.board 已注册", typeof render === "function", `类型为 ${typeof render}`);
  if (typeof render !== "function") return;

  const container = makeEl("div");
  const life = render(container);

  check(G, "P1 renderPage 返回的是对象（非 Promise）", !!life && typeof life === "object" && !(life instanceof Promise), `实际 ${String(life)}`);
  check(G, "P1 返回 refresh 函数", typeof (life && life.refresh) === "function");
  check(G, "P1 返回 dispose 函数", typeof (life && life.dispose) === "function");

  const root = container.children[0];
  check(G, "容器里挂上了面板根节点", !!root, `children=${container.children.length}`);
  check(G, "面板根 class 为 stylecam-panel", !!root && root.className === "stylecam-panel", root && root.className);
  check(G, "面板根带 data-sc=panel", !!root && root.getAttribute("data-sc") === "panel");
  check(G, "P3 容器仍是面板根的父节点（未被移走）", !!root && root.parentNode === container);
  check(G, "P4 面板根未设置内联定位（不改宿主布局）", !!root && root.style.position === undefined, root && root.style.position);

  check(G, "渲染后 <style> 注入了 document.head", dom.head.children.length > 0, `head.children=${dom.head.children.length}`);

  // P1 refresh 可用
  const before = container.children.length;
  life.refresh();
  check(G, "P1 refresh 可调用且不叠加面板", container.children.length === before, `children ${before} → ${container.children.length}`);

  life.dispose();
  check(G, "dispose 后容器被清空", container.children.length === 0, `children=${container.children.length}`);
  check(G, "dispose 后 <style> 归零", dom.head.children.length === 0, `head.children=${dom.head.children.length}`);
}

// ───────────────────────────────────────────────────────────────────────────
// 组 ⑩ 命名空间
// ───────────────────────────────────────────────────────────────────────────

function readManifest(dir) {
  const p = join(dir, "plugin.json");
  if (!existsSync(p)) return null;
  try {
    return JSON.parse(readFileSync(p, "utf8"));
  } catch (e) {
    return null;
  }
}

async function group10() {
  console.log("\n⑩ 命名空间（stylecam.* 与 studio.*/demo.* 无交集；页签 id 不与命令重名）");
  const G = "⑩";
  const manifest = JSON.parse(readFileSync(MANIFEST, "utf8"));
  const mine = (manifest.contributes.commands || []).map((c) => c.id);

  check(G, "本插件全部命令以 stylecam. 开头", mine.every((id) => id.startsWith("stylecam.")), mine.filter((id) => !id.startsWith("stylecam.")).join(", "));

  // 与其它内置插件比对
  const others = ["studio", "demo-quick-note"];
  const foreign = [];
  for (const name of others) {
    const m = readManifest(join(BUILTIN_DIR, name));
    if (!m) {
      check(G, `读到 ${name}/plugin.json`, false, `文件缺失或不是合法 JSON：${join(BUILTIN_DIR, name, "plugin.json")}`);
      continue;
    }
    for (const c of m.contributes && m.contributes.commands ? m.contributes.commands : []) foreign.push({ plugin: name, id: c.id });
  }
  const foreignIds = foreign.map((x) => x.id);
  const dup = mine.filter((id) => foreignIds.includes(id));
  check(G, "与其它内置插件的命令 id 无交集", dup.length === 0, `重名：${dup.join(", ") || "无"}`);

  const prefixes = new Set(foreignIds.map((id) => id.split(".")[0]));
  check(G, "未占用 studio / demo 前缀", !prefixes.has("stylecam"), `其它插件已用前缀：${[...prefixes].join(", ")}`);
  check(G, "本插件前缀唯一为 stylecam", new Set(mine.map((id) => id.split(".")[0])).size === 1);

  // 页签 id 不与任何 commandId 重名
  const allIds = mine.concat(foreignIds);
  const pageIds = (manifest.contributes.pages || []).map((p) => p.id);
  const clash = pageIds.filter((id) => allIds.includes(id));
  check(G, "pages[].id 不与任何命令 id 重名", clash.length === 0, `重名：${clash.join(", ") || "无"}`);

  // storage key 同样带前缀（DESIGN §10.1）
  const { constants } = await loadSrc();
  const keys = Object.values(constants.KEYS);
  check(G, "storage key 全部以 stylecam. 开头", keys.every((k) => String(k).startsWith("stylecam.")), keys.join(", "));
  const clash2 = keys.filter((k) => allIds.includes(k));
  check(G, "storage key 不与命令 id 撞车", clash2.length === 0, `撞车：${clash2.join(", ") || "无"}`);
}

// ───────────────────────────────────────────────────────────────────────────
// 组 ⑪ 图片管线
// ───────────────────────────────────────────────────────────────────────────

const GEN_STDOUT = [
  "OK=1",
  "PATH=C:\\Users\\86136\\Pictures\\easyop-stylecam\\20260925-120000_cyber-neon.jpg",
  'META={"styleId":"cyber-neon","strength":55,"size":"2K","bytes":278528,"elapsedMs":9100,"ext":"jpg","model":"doubao-seedream-5-0-250128"}',
  "BEGIN_IMAGE",
  "CHUNK=1/2:/9j/4AAQ",
  "CHUNK=2/2:SkZJRgA",
  "END_IMAGE",
].join("\n");

function makeJob() {
  return {
    match: "generate",
    exitCode: 0,
    stdout: GEN_STDOUT,
  };
}

const PY_VER = { match: "--version", exitCode: 0, stdout: "Python 3.11.9\n" };

function imgPayload(n) {
  return { dataUrl: `data:image/jpeg;base64,AAAA${n}`, w: 1600, h: 1200, bytes: 200 * 1024 };
}

/** 组 ⑪：素材上限 + 生图全链路（顺序 / arg 长度 / opts 双写 / job 置 null）。 */
async function group11Pipeline() {
  const G = "⑪";
  console.log("\n⑪ 图片管线（先写 job 再 eo.run / arg < 4096 / opts 双写 / job 置 null / 第 5 张 → MAX_IMAGES）");
  const { cmds, constants } = await loadSrc();
  const store = { "stylecam.config": seededConfig() };
  const { pages, calls } = await bootstrap({ store, runReplies: [PY_VER, makeJob()] });

  const container = makeEl("div");
  const life = pages["stylecam.board"](container);
  check(G, "页签挂载成功（命令才有上下文）", container.children.length === 1, `children=${container.children.length}`);

  // ── 素材上限：前 4 张进得去，第 5 张 → MAX_IMAGES ──
  const added = [];
  for (let i = 0; i < 4; i += 1) {
    // eslint-disable-next-line no-await-in-loop
    added.push(await cmds.COMMANDS["stylecam.image.add"](imgPayload(i + 1)));
  }
  check(G, "前 4 张素材都能加进去", added.every((r) => r.ok === true), added.map((r) => r.code).join(","));
  const fifth = await cmds.COMMANDS["stylecam.image.add"](imgPayload(5));
  check(G, "第 5 张素材 → MAX_IMAGES", fifth.ok === false && fifth.code === "MAX_IMAGES", vs("第 5 张 code", "MAX_IMAGES", fifth.code));
  check(G, "MAX_IMAGES 文案逐字来自 CODE 表", fifth.message === constants.CODE.MAX_IMAGES, vs("message", constants.CODE.MAX_IMAGES, fifth.message));
  check(G, "素材上限常量是 4", constants.LIMITS.MAX_IMAGES === 4, vs("MAX_IMAGES", 4, constants.LIMITS.MAX_IMAGES));

  await cmds.COMMANDS["stylecam.style.pick"]({ styleId: "cyber-neon" });
  await cmds.COMMANDS["stylecam.strength.set"]({ value: 55 });

  const mark = calls.length;
  const gen = await cmds.COMMANDS["stylecam.generate"]({ ackPrivacy: true });
  const after = calls.slice(mark);

  check(G, "生图成功返回 ok:true", gen.ok === true, vs("生图结果", "ok:true", JSON.stringify(gen)));

  const setJobIdx = after.findIndex((c) => c[0] === "storage.set" && c[1] === "stylecam.job");
  const runIdx = after.findIndex((c) => c[0] === "run" && Array.isArray(c[2]) && c[2].includes("generate"));
  check(G, "生图前先写了 stylecam.job", setJobIdx >= 0, "调用序列里没有 storage.set('stylecam.job')");
  check(G, "确实调了 eo.run 跑 generate", runIdx >= 0, "调用序列里没有含 generate 的 eo.run");
  check(
    G,
    "顺序：storage.set(stylecam.job) 在 eo.run 之前",
    setJobIdx >= 0 && runIdx >= 0 && setJobIdx < runIdx,
    `setJob 位置 ${setJobIdx}，run 位置 ${runIdx}`,
  );

  const runCall = runIdx >= 0 ? after[runIdx] : null;
  if (runCall) {
    const args = runCall[2];
    const tooLong = args.filter((a) => String(a).length >= 4096);
    check(G, "每个 arg 长度 < 4096（钉死 base64 不进 args）", tooLong.length === 0, `超长参数 ${tooLong.length} 个，最长 ${Math.max(...args.map((a) => String(a).length))} 字符`);
    check(G, "args 里不含 base64 图片（无超长 data: 串）", !args.some((a) => String(a).indexOf("base64,") >= 0), args.filter((a) => String(a).indexOf("base64,") >= 0).length + " 个含 base64");
    const opts = runCall[3] || {};
    check(G, "opts 含 timeout_secs（Rust 侧真正生效的字段）", "timeout_secs" in opts, `opts 键：${Object.keys(opts).join(",")}`);
    check(G, "opts 同时含 timeoutSecs（对齐 studio 写法）", "timeoutSecs" in opts, `opts 键：${Object.keys(opts).join(",")}`);
    check(G, "opts 的 cwd 传的是 pluginDir", opts.cwd === PLUGIN_DIR, vs("cwd", PLUGIN_DIR, opts.cwd));
  }

  check(G, "成功后 stylecam.job 被置 null", store["stylecam.job"] === null, vs("store['stylecam.job']", null, store["stylecam.job"]));
  const clearedAfterRun = after.findIndex((c) => c[0] === "storage.set" && c[1] === "stylecam.job" && c[2] === null);
  check(G, "job 置 null 发生在 eo.run 之后", clearedAfterRun > runIdx, `clear 位置 ${clearedAfterRun}，run 位置 ${runIdx}`);

  const st = (await loadSrc()).main.__test.getState();
  check(G, "出图后阶段进入 result", st.stage === "result", vs("stage", "result", st.stage));
  check(G, "结果里有 data: URL", !!st.result && String(st.result.dataUrl || "").indexOf("data:") === 0);
  check(G, "结果里的风格中文名取自 STYLES", !!st.result && st.result.styleName === "赛博霓虹", st.result && st.result.styleName);

  life.dispose();
}

// ───────────────────────────────────────────────────────────────────────────
// 组 ⑫ 输出解析
// ───────────────────────────────────────────────────────────────────────────

async function group12() {
  console.log("\n⑫ 输出解析（乱序 CHUNK / OK=0 / exit=3 / timed_out / 有 BEGIN 无 END）");
  const G = "⑫";
  const { logic } = await loadSrc();
  const p = logic.parseScriptOutput;

  // ── 乱序 CHUNK 能拼对 ──
  const shuffled = ["OK=1", "BEGIN_IMAGE", "CHUNK=2/3:AAQSk", "CHUNK=1/3:/9j/4", "CHUNK=3/3:ZJRgA", "END_IMAGE"].join("\n");
  const r1 = p(shuffled, 0, false);
  check(G, "乱序 CHUNK 解析成功", r1.ok === true, JSON.stringify(r1));
  check(G, "乱序 CHUNK 按 i 落位拼对", r1.dataUrl === "data:image/jpeg;base64,/9j/4AAQSkZJRgA", vs("dataUrl", "data:image/jpeg;base64,/9j/4AAQSkZJRgA", r1.dataUrl));

  // ── 顺序 CHUNK 与乱序结果一致 ──
  const ordered = ["OK=1", "BEGIN_IMAGE", "CHUNK=1/3:/9j/4", "CHUNK=2/3:AAQSk", "CHUNK=3/3:ZJRgA", "END_IMAGE"].join("\n");
  check(G, "顺序 CHUNK 与乱序结果一致（幂等落位）", p(ordered, 0, false).dataUrl === r1.dataUrl);

  // ── OK=0 → IO_FAILED，且 message 含原文 ──
  const r2 = p("OK=0\nMSG=方舟 500：上游炸了", 3, false);
  check(G, "OK=0 且 exit=3 → IO_FAILED", r2.ok === false && r2.code === "IO_FAILED", vs("code", "IO_FAILED", r2.code));
  check(G, "IO_FAILED 的 message 含脚本原文", String(r2.message).indexOf("方舟 500：上游炸了") >= 0, vs("message 含原文", true, String(r2.message)));

  // ── exit=3 走退出码兜底表 ──
  const r3 = p("OK=0\nMSG=参数不对", 3, false);
  check(G, "exit=3 命中 EXIT_FALLBACK（IO_FAILED）", r3.code === "IO_FAILED", vs("code", "IO_FAILED", r3.code));
  const r3b = p("OK=0\nMSG=写盘失败", 4, false);
  check(G, "exit=4 → WRITE_FAILED", r3b.code === "WRITE_FAILED", vs("code", "WRITE_FAILED", r3b.code));

  // ── timed_out → IO_TIMEOUT ──
  const r4 = p("OK=1", 0, true);
  check(G, "timed_out → IO_TIMEOUT", r4.ok === false && r4.code === "IO_TIMEOUT", vs("code", "IO_TIMEOUT", r4.code));
  check(G, "IO_TIMEOUT 文案含 300 秒", String(r4.message).indexOf("300 秒") >= 0, r4.message);

  // ── 有 BEGIN 无 END → NO_RESULT ──
  const r5 = p("OK=1\nBEGIN_IMAGE\nCHUNK=1/2:AAAA\nCHUNK=2/2:BBBB", 0, false);
  check(G, "有 BEGIN 无 END → NO_RESULT", r5.ok === false && r5.code === "NO_RESULT", vs("code", "NO_RESULT", r5.code));

  // ── OK=1 但一个 CHUNK 都没有 → NO_RESULT ──
  const r6 = p("OK=1\nPATH=D:\\x.jpg", 0, false);
  check(G, "OK=1 但无图 → NO_RESULT", r6.ok === false && r6.code === "NO_RESULT", vs("code", "NO_RESULT", r6.code));

  // ── 脚本错误码映射 ──
  const r7 = p("OK=0\nERR=E_NO_KEY\nMSG=KEY=0", 2, false);
  check(G, "E_NO_KEY → NO_KEY", r7.code === "NO_KEY", vs("code", "NO_KEY", r7.code));
  const r8 = p("OK=0\nERR=E_API\nSTATUS=401\nMSG=Invalid API key", 2, false);
  check(G, "E_API + 401 → AUTH_FAILED", r8.code === "AUTH_FAILED", vs("code", "AUTH_FAILED", r8.code));
  const r9 = p("OK=0\nERR=E_API\nSTATUS=429\nMSG=rate limited", 2, false);
  check(G, "E_API + 429 → RATE_LIMITED", r9.code === "RATE_LIMITED", vs("code", "RATE_LIMITED", r9.code));
  const r10 = p("OK=0\nERR=E_NET\nMSG=timeout", 2, false);
  check(G, "E_NET → NET_TIMEOUT", r10.code === "NET_TIMEOUT", vs("code", "NET_TIMEOUT", r10.code));
  const r11 = p("OK=0\nERR=E_ARGS\nMSG=bad", 2, false);
  check(G, "E_ARGS → BAD_ARGS", r11.code === "BAD_ARGS", vs("code", "BAD_ARGS", r11.code));
  const r12 = p("OK=0\nERR=E_IMG\nMSG=too big", 2, false);
  check(G, "E_IMG → IMG_TOO_LARGE", r12.code === "IMG_TOO_LARGE", vs("code", "IMG_TOO_LARGE", r12.code));

  // ── TOO_LARGE 降级 ──
  const r13 = p("OK=1\nTOO_LARGE=1\nPATH=D:\\big.jpg", 0, false);
  check(G, "TOO_LARGE=1 → TOO_LARGE_PREVIEW", r13.code === "TOO_LARGE_PREVIEW", vs("code", "TOO_LARGE_PREVIEW", r13.code));
  check(G, "TOO_LARGE 仍给出落盘路径", r13.path === "D:\\big.jpg", vs("path", "D:\\big.jpg", r13.path));

  // ── 空输出不崩 ──
  const r14 = p("", 0, false);
  check(G, "空 stdout 不崩且给出失败码", r14.ok === false && typeof r14.code === "string", JSON.stringify(r14));
}

// ───────────────────────────────────────────────────────────────────────────
// 组 ⑬ 纯函数
// ───────────────────────────────────────────────────────────────────────────

async function group13() {
  console.log("\n⑬ 纯函数（planImageFit / mapHardwareKey / clampStrength / STYLES / CREATIVE_MODES）");
  const G = "⑬";
  const { logic, constants } = await loadSrc();

  // ── planImageFit 5 档递降 ──
  check(G, "main 档位恰好 5 档", logic.FIT_TIERS.main.length === 5, vs("档位数", 5, logic.FIT_TIERS.main.length));
  check(G, "ref 档位恰好 5 档", logic.FIT_TIERS.ref.length === 5, vs("档位数", 5, logic.FIT_TIERS.ref.length));

  const f0 = logic.planImageFit(1600, 1200, 200 * 1024, "main");
  check(G, "小图落在第 0 档（最高画质）", f0.ok === true && f0.tier === 0, JSON.stringify(f0));
  check(G, "第 0 档长边 1600", f0.longEdge === 1600, vs("longEdge", 1600, f0.longEdge));

  const f1 = logic.planImageFit(4000, 3000, 8 * 1024 * 1024, "main");
  check(G, "大图自动降档且仍装得下", f1.ok === true && f1.tier >= 1, JSON.stringify(f1));
  check(G, "降档后长边更小", f1.longEdge < f0.longEdge, vs("longEdge", `< ${f0.longEdge}`, f1.longEdge));
  check(G, "降档后估算体积不超单张上限", f1.estBytes <= constants.LIMITS.MAX_SINGLE_BYTES, vs("estBytes", `<= ${constants.LIMITS.MAX_SINGLE_BYTES}`, f1.estBytes));

  let prevEdge = Infinity;
  let prevQ = Infinity;
  let monotonic = true;
  for (let mb = 1; mb <= 5; mb += 1) {
    const f = logic.planImageFit(4000, 3000, mb * 1024 * 1024, "main");
    if (f.longEdge > prevEdge || f.q > prevQ) monotonic = false;
    prevEdge = f.longEdge;
    prevQ = f.q;
  }
  check(G, "体积越大档位越低（长边与质量单调不升）", monotonic);

  const huge = logic.planImageFit(1000, 1000, 50 * 1024 * 1024, "main");
  check(G, "5 档都装不下 → IMG_TOO_LARGE（不静默截断）", huge.ok === false && huge.code === "IMG_TOO_LARGE", JSON.stringify(huge));

  const tooBig = logic.planImageFit(9000, 100, 1024, "main");
  check(G, "长边 ≥8192 → IMG_DECODE_FAILED", tooBig.ok === false && tooBig.code === "IMG_DECODE_FAILED", JSON.stringify(tooBig));

  const bad = logic.planImageFit(0, 0, 0, "main");
  check(G, "非法尺寸不崩 → IMG_TOO_LARGE", bad.ok === false && bad.code === "IMG_TOO_LARGE", JSON.stringify(bad));

  const ref = logic.planImageFit(1600, 1200, 200 * 1024, "ref");
  check(G, "ref 档位长边比 main 更小", ref.longEdge < f0.longEdge, vs("ref.longEdge", `< ${f0.longEdge}`, ref.longEdge));

  // ── clampStrength 边界 ──
  const cases = [
    [-5, 0],
    [0, 0],
    [50, 50],
    [100, 100],
    [150, 100],
    [49.6, 50],
    ["70", 70],
    [NaN, 50],
    [undefined, 50],
    ["abc", 50],
  ];
  for (const [input, want] of cases) {
    const got = logic.clampStrength(input);
    check(G, `clampStrength(${JSON.stringify(input)}) === ${want}`, got === want, vs("结果", want, got));
  }
  // null 的 Number() 语义是 0（函数的「非法值」判据是 !Number.isFinite），这里只钉住「落在 0–100 内」
  const nullGot = logic.clampStrength(null);
  check(G, "clampStrength(null) 仍落在 0–100 内", nullGot >= 0 && nullGot <= 100, vs("结果", "0–100 内", nullGot));
  check(G, "强度档位 0–30 是轻度点缀", logic.strengthBand(10).name === "轻度点缀", logic.strengthBand(10).name);
  check(G, "强度档位 31–69 是平衡融合", logic.strengthBand(50).name === "平衡融合", logic.strengthBand(50).name);
  check(G, "强度档位 70–100 是大改风格", logic.strengthBand(90).name === "大改风格", logic.strengthBand(90).name);

  // ── mapHardwareKey 5 stage × 5 类键 ──
  const KEYS5 = [
    ["left", { key: "ArrowLeft" }],
    ["right", { key: "ArrowRight" }],
    ["enter", { key: "Enter" }],
    ["escape", { key: "Escape" }],
    ["digit", { key: "1" }],
  ];
  const WANT = {
    browse: { left: ["selectImage", -1], right: ["selectImage", 1], enter: ["confirm", null], escape: ["clearImages", null], digit: ["selectImage", 0] },
    adjusting: { left: ["adjustStrength", -5], right: ["adjustStrength", 5], enter: ["generate", null], escape: ["back", null], digit: ["pickStyle", 0] },
    generating: { left: ["busy", null], right: ["busy", null], enter: ["busy", null], escape: ["busy", null], digit: ["busy", null] },
    result: { left: ["compareSlide", -10], right: ["compareSlide", 10], enter: ["saveResult", null], escape: ["back", null], digit: ["saveResult", null] },
    creative: { left: ["cycleCreative", -1], right: ["cycleCreative", 1], enter: ["saveResult", null], escape: ["back", null], digit: ["setCreative", 0] },
  };
  for (const stage of constants.STAGES) {
    for (const [cls, ev] of KEYS5) {
      const got = logic.mapHardwareKey(stage, ev);
      const want = WANT[stage][cls];
      check(
        G,
        `mapHardwareKey(${stage}, ${cls}) → ${want[0]}`,
        got.action === want[0] && got.arg === want[1],
        vs("意图", `${want[0]}/${JSON.stringify(want[1])}`, `${got.action}/${JSON.stringify(got.arg)}`),
      );
    }
  }
  check(G, "未识别的键 → none（不吞宿主按键）", logic.mapHardwareKey("browse", { key: "q" }).action === "none", logic.mapHardwareKey("browse", { key: "q" }).action);
  check(G, "非法阶段回落 browse", logic.mapHardwareKey("nope", { key: "ArrowRight" }).action === "selectImage", JSON.stringify(logic.mapHardwareKey("nope", { key: "ArrowRight" })));

  // ── 常量表 ──
  check(G, "STYLES.length === 10", constants.STYLES.length === 10, vs("STYLES", 10, constants.STYLES.length));
  check(G, "CREATIVE_MODES.length === 6", constants.CREATIVE_MODES.length === 6, vs("CREATIVE_MODES", 6, constants.CREATIVE_MODES.length));
  check(G, "STAGES 恰有 5 个阶段", constants.STAGES.length === 5, vs("STAGES", 5, constants.STAGES.length));
  check(G, "10 组风格中文名唯一", new Set(constants.STYLES.map((s) => s.name)).size === 10);
  check(G, "每组风格都有 4 色色板", constants.STYLES.every((s) => Array.isArray(s.palette) && s.palette.length === 4), constants.STYLES.map((s) => (s.palette || []).length).join(","));
  check(G, "每组风格都有英文 prompt", constants.STYLES.every((s) => typeof s.prompt === "string" && s.prompt.length > 10));
  check(G, "数字键 1–9、0 覆盖全部 10 个风格", [1, 2, 3, 4, 5, 6, 7, 8, 9, 0].map((k) => logic.styleIndexFromKey(String(k))).join(",") === "0,1,2,3,4,5,6,7,8,9", [1, 2, 3, 4, 5, 6, 7, 8, 9, 0].map((k) => logic.styleIndexFromKey(String(k))).join(","));

  // ── 配置 v2 / v1 迁移 / provider 映射 ──
  const migratedTier = logic.migrateConfig({ v: 1, size: "2K", model: "legacy-model" });
  check(G, "v1 size=2K 迁移为 resolution=2K", migratedTier.v === 2 && migratedTier.resolution === "2K", JSON.stringify(migratedTier));
  check(G, "v1 model 在迁移后保留", migratedTier.model === "legacy-model", migratedTier.model);
  check(G, "迁移结果不包含 API Key", !("apiKey" in logic.migrateConfig({ apiKey: "secret" })), "apiKey 泄漏进配置");
  const migratedPx = logic.migrateConfig({ v: 1, size: "1280x768" });
  check(G, "v1 像素 size 拆成 width/height", migratedPx.width === 1280 && migratedPx.height === 768, JSON.stringify(migratedPx));
  check(G, "配置 v2 默认 provider=easysu", constants.DEFAULTS.provider === "easysu" && constants.CONFIG_VERSION === 2);
  check(G, "配置 v2 默认 format=auto / quality=null", constants.DEFAULTS.format === "auto" && constants.DEFAULTS.quality === null, JSON.stringify(constants.DEFAULTS));
  check(G, "合法默认配置通过严格校验", logic.validateConfig(constants.DEFAULTS).ok === true, logic.validateConfig(constants.DEFAULTS).errors.join("；"));
  check(G, "EasySU 16:9 映射确定", logic.mapDimensionsForProvider("easysu", 1024, 576, "1K").ratio === "16:9");
  check(G, "EasySU 不可表示比例明确拒绝", logic.mapDimensionsForProvider("easysu", 1024, 704, "1K").code === "DIMENSION_UNREPRESENTABLE");
  check(G, "Ark 非方形明确拒绝", logic.mapDimensionsForProvider("ark", 1024, 768, "2K").code === "DIMENSION_UNREPRESENTABLE");
  check(G, "动态 run timeout 含尝试/下载/退避余量", logic.computeRunTimeout(120, 1, 1) === 391, String(logic.computeRunTimeout(120, 1, 1)));
  const v2args = logic.buildGenerateArgs({ styleId: "cyber-neon" }, { provider: "easysu", resolution: "1K", width: 1024, height: 1024, format: "auto", n: 1 });
  check(G, "v2 generate args 含 provider/width/height", v2args.includes("--provider") && v2args.includes("--width") && v2args.includes("--height"), v2args.join(" "));
  check(G, "generate args 不含 api-key 参数", !v2args.includes("--api-key") && !v2args.some((x) => /secret|api[_-]?key/i.test(String(x))), v2args.join(" "));
  const secretMigrated = logic.migrateConfig({ apiKey: "qa-secret", IMAGE_API_KEY: "qa-secret", ARK_API_KEY: "qa-secret" });
  check(G, "迁移时清除全部已知 Key 字段", !JSON.stringify(secretMigrated).includes("qa-secret"), JSON.stringify(secretMigrated));
  check(G, "动态 timeout 最小夹到 60 秒", logic.computeRunTimeout(1, 0, 0) === 60, String(logic.computeRunTimeout(1, 0, 0)));
  check(G, "动态 timeout 最大夹到 1260 秒", logic.computeRunTimeout(999, 99, 9) === 1260, String(logic.computeRunTimeout(999, 99, 9)));
  check(G, "EasySU 3% 边界内接受", logic.mapDimensionsForProvider("easysu", 1053, 1024, "1K").ok === true, JSON.stringify(logic.mapDimensionsForProvider("easysu", 1053, 1024, "1K")));
  check(G, "EasySU 超 3% 边界拒绝", logic.mapDimensionsForProvider("easysu", 1055, 1024, "1K").ok === false, JSON.stringify(logic.mapDimensionsForProvider("easysu", 1055, 1024, "1K")));

  // ── 静态源码扫描：纯函数层不许碰宿主（DESIGN §10.3 判据） ──
  const logicSrc = readFileSync(join(SRC_DIR, "logic.js"), "utf8");
  const cmdsSrc = readFileSync(join(SRC_DIR, "cmds.js"), "utf8");
  const stateSrc = readFileSync(join(SRC_DIR, "state.js"), "utf8");
  const RE_HOST = /\b(document|window)\s*\./;
  check(G, "logic.js 无 document. / window. / eo.", !RE_HOST.test(logicSrc) && !/\beo\s*\./.test(logicSrc), "logic.js 里出现了宿主标识符");
  check(G, "cmds.js 无 document. / window.（⌘SS 场景也要能跑）", !RE_HOST.test(cmdsSrc), "cmds.js 里出现了 DOM 标识符");
  check(G, "state.js 无 document. / window.", !RE_HOST.test(stateSrc), "state.js 里出现了 DOM 标识符");

  // ── isTypingTarget（传桩对象即可测） ──
  check(G, "isTypingTarget(INPUT) 为真", logic.isTypingTarget({ tagName: "input" }) === true);
  check(G, "isTypingTarget(TEXTAREA) 为真", logic.isTypingTarget({ tagName: "textarea" }) === true);
  check(G, "isTypingTarget(contentEditable) 为真", logic.isTypingTarget({ tagName: "div", isContentEditable: true }) === true);
  check(G, "isTypingTarget(DIV) 为假", logic.isTypingTarget({ tagName: "div" }) === false);
  check(G, "isTypingTarget(null) 为假", logic.isTypingTarget(null) === false);

  // ── sniffExt / applyGalleryLimit ──
  check(G, "sniffExt 认 JPEG", logic.sniffExt([0xff, 0xd8, 0xff]) === "jpg");
  check(G, "sniffExt 认 PNG", logic.sniffExt([0x89, 0x50, 0x4e, 0x47]) === "png");
  check(G, "sniffExt 认 GIF", logic.sniffExt([0x47, 0x49, 0x46]) === "gif");
  check(G, "sniffExt 认 WEBP", logic.sniffExt([0x52, 0x49, 0x46, 0x46]) === "webp");
  check(G, "sniffExt 未知 → bin", logic.sniffExt([1, 2, 3]) === "bin");
  check(G, "作品集不自动淘汰（只截断）", logic.applyGalleryLimit(new Array(40).fill(1), 30).length === 30);
  check(G, "作品集满额判定", logic.isGalleryFull(new Array(30).fill(1), 30) === true);
}

// ───────────────────────────────────────────────────────────────────────────
// 组 ⑭ 页签补强
// ───────────────────────────────────────────────────────────────────────────

async function group14(dom) {
  console.log("\n⑭ 页签补强（<style> 挂摘 / 无 fixed / keydown 挂摘相等 / 重复 render 只挂一次）");
  const G = "⑭";
  const { main } = await loadSrc();
  const { pages } = await bootstrap({ store: { "stylecam.config": seededConfig() } });
  const render = pages["stylecam.board"];

  const container = makeEl("div");

  // ── 重复渲染只挂一份 ──
  const life1 = render(container);
  check(G, "首次渲染后容器里 1 份面板", container.children.length === 1, `children=${container.children.length}`);
  render(container);
  check(G, "重复 render 后容器里仍只有 1 份面板", container.children.length === 1, `children=${container.children.length}`);
  check(G, "重复 render 后 head 里仍只有 1 个 <style>", dom.head.children.length === 1, `head.children=${dom.head.children.length}`);
  check(
    G,
    "重复 render 后 keydown 只挂 1 个监听",
    (dom.listeners.keydown || []).length === 1,
    `keydown 监听数 ${(dom.listeners.keydown || []).length}`,
  );

  const injected = dom.head.children.filter((el) => el.getAttribute("data-sc") === "style");
  check(G, "注入的 <style> 带 data-sc=style 标记", injected.length === 1, `匹配 ${injected.length} 个`);
  check(G, "注入的 <style> 在 document.head 里", injected.length === 1 && injected[0].parentNode === dom.head);

  // ── keydown 挂摘计数相等 ──
  check(G, "keydown 挂载计数 ≥ 1", main.__test.getAttachCount() >= 1, `attach=${main.__test.getAttachCount()}`);
  life1.dispose();
  check(
    G,
    "dispose 后 keydown 挂载次数 === 摘除次数",
    main.__test.getAttachCount() === main.__test.getDetachCount(),
    vs("挂/摘", `${main.__test.getAttachCount()}`, `${main.__test.getDetachCount()}`),
  );
  check(G, "dispose 后 keydown 监听数为 0", (dom.listeners.keydown || []).length === 0, `keydown 监听数 ${(dom.listeners.keydown || []).length}`);
  check(G, "dispose 后 paste 监听数为 0", (dom.listeners.paste || []).length === 0, `paste 监听数 ${(dom.listeners.paste || []).length}`);
  check(G, "dispose 后 <style> 归零", dom.head.children.length === 0, `head.children=${dom.head.children.length}`);

  // ── 挂 → 摘 → 挂 → 摘 多次仍然相等 ──
  const l2 = render(container);
  l2.dispose();
  const l3 = render(container);
  l3.dispose();
  check(
    G,
    "多轮 render/dispose 后挂摘仍相等",
    main.__test.getAttachCount() === main.__test.getDetachCount(),
    vs("挂/摘", `${main.__test.getAttachCount()}`, `${main.__test.getDetachCount()}`),
  );
  check(G, "多轮后 head 仍归零", dom.head.children.length === 0, `head.children=${dom.head.children.length}`);

  // ── 无 DOM 时 renderPage 不崩（契约：返回一个空 lifecycle） ──
  const savedDoc = globalThis.document;
  globalThis.document = undefined;
  let noDom = null;
  try {
    noDom = render(makeEl("div"));
  } catch (e) {
    noDom = null;
  }
  globalThis.document = savedDoc;
  check(G, "无 document 时 renderPage 不抛异常", !!noDom && typeof noDom.dispose === "function");

  // ── 打包产物全文无 position:fixed（契约 P2） ──
  const src = readFileSync(BUNDLE, "utf8");
  check(G, "index.js 全文无 position:fixed", !/position:\s*fixed/.test(src), "打包产物里出现了 position:fixed");
  const cssSrc = readFileSync(join(SRC_DIR, "styles.js"), "utf8");
  check(G, "styles.js 无 position:fixed", !/position:\s*fixed/.test(cssSrc), "styles.js 里出现了 position:fixed");
  const uiSrc = readFileSync(join(SRC_DIR, "ui.js"), "utf8");
  check(G, "ui.js 无 position:fixed", !/position:\s*fixed/.test(uiSrc), "ui.js 里出现了 position:fixed");
  check(G, "CSS 选择器收口在 .stylecam-panel 下", cssSrc.indexOf(".stylecam-panel") >= 0);
  check(G, "CSS 有 light 主题覆盖", cssSrc.indexOf('[data-theme="light"]') >= 0);

  // ── DeskMate 风格工作台四区与响应式契约 ──
  const workbenchContainer = makeEl("div");
  const workbenchView = (await loadSrc()).ui.buildView(workbenchContainer);
  (await loadSrc()).ui.renderAll(workbenchView);
  const workbenchRoot = workbenchContainer.children[0];
  const wbStack = workbenchRoot ? [workbenchRoot] : [];
  const wbNodes = [];
  while (wbStack.length) {
    const cur = wbStack.pop();
    wbNodes.push(cur);
    for (const child of cur.children || []) wbStack.push(child);
  }
  const classes = wbNodes.map((x) => x.className || "");
  const roles = wbNodes.map((x) => x.getAttribute && x.getAttribute("data-role")).filter(Boolean);
  const actions = wbNodes.map((x) => x.getAttribute && x.getAttribute("data-action")).filter(Boolean);
  check(G, "主布局包含 sc-material-shelf", classes.some((x) => String(x).includes("sc-material-shelf")), classes.join(" | "));
  check(G, "主布局包含 sc-creation-stage", classes.some((x) => String(x).includes("sc-creation-stage")), classes.join(" | "));
  check(G, "主布局包含 sc-strength-dial", classes.some((x) => String(x).includes("sc-strength-dial")), classes.join(" | "));
  check(G, "主布局包含 sc-gallery-strip", classes.some((x) => String(x).includes("sc-gallery-strip")), classes.join(" | "));
  check(G, "四区均有 data-role 语义", ["material-shelf", "creation-stage", "strength-dial", "gallery-strip"].every((x) => roles.includes(x)), roles.join(","));
  check(G, "设置入口位于顶部工具按钮", actions.includes("open-settings"), actions.join(","));
  check(G, "CSS 含 900px 强度区下移断点", /@media\s*\(max-width:\s*900px\)/.test(cssSrc), "缺少 900px 断点");
  check(G, "CSS 含 640px 单列断点", /@media\s*\(max-width:\s*640px\)/.test(cssSrc), "缺少 640px 断点");
  check(G, "CSS 限制主容器横向溢出", /\.stylecam-panel\s*\{[\s\S]*?overflow:\s*hidden;/.test(cssSrc), "主容器未限制 overflow");
  check(G, "素材与作品轨道允许局部横滚", /sc-material-track[\s\S]*?overflow-x:\s*auto/.test(cssSrc) && /sc-gallery-bar[\s\S]*?sc-gallery-strip[\s\S]*?overflow-x:\s*auto/.test(cssSrc), "局部轨道缺少横滚");
  check(G, "CSS 含 prefers-reduced-motion 降级", /prefers-reduced-motion:\s*reduce/.test(cssSrc), "缺少 reduced-motion");
  check(G, "CSS 含生成脉冲与出片动画", /@keyframes\s+sc-pulse/.test(cssSrc) && /@keyframes\s+sc-output/.test(cssSrc), "缺少生成或出片动画");
  check(G, "UI 保留关键 data-action 契约", ["open-settings", "add-material"].every((x) => actions.includes(x)), actions.join(","));
  (await loadSrc()).ui.destroyView(workbenchView);

  // ── 设置面板 DOM 结构（不依赖浏览器布局引擎） ──
  const { ui } = await loadSrc();
  const settingsContainer = makeEl("div");
  const settingsView = ui.buildView(settingsContainer);
  ui.showOverlay(settingsView, "settings");
  const overlay = settingsView.overlay;
  const stack = overlay ? [overlay] : [];
  const nodes = [];
  while (stack.length) {
    const cur = stack.pop();
    nodes.push(cur);
    for (const child of cur.children || []) stack.push(child);
  }
  const sectionTitles = nodes.filter((x) => x.className === "sc-settings-section-title").map((x) => x.textContent);
  const configKeys = nodes.map((x) => x.getAttribute && x.getAttribute("data-config-key")).filter(Boolean);
  const passwordInputs = nodes.filter((x) => x.tagName === "INPUT" && x.type === "password");
  check(G, "设置面板恰有图像参数/API 配置两分区", sectionTitles.join("|") === "API 配置|图像参数" || sectionTitles.join("|") === "图像参数|API 配置", sectionTitles.join("|"));
  check(G, "设置面板覆盖 12 个非敏感配置字段", configKeys.length === 12, `字段=${configKeys.join(",")}`);
  check(G, "设置面板包含 resolution/width/height/format/quality/n", ["resolution", "width", "height", "format", "quality", "n"].every((k) => configKeys.includes(k)), configKeys.join(","));
  check(G, "设置面板包含 provider/baseUrl/model/httpTimeoutSecs/retryCount", ["provider", "baseUrl", "model", "httpTimeoutSecs", "retryCount"].every((k) => configKeys.includes(k)), configKeys.join(","));
  check(G, "API Key 输入框是 password 且永不预填", passwordInputs.length === 1 && passwordInputs[0].value === "", `passwordInputs=${passwordInputs.length}, value=${passwordInputs[0] && passwordInputs[0].value}`);
  check(G, "设置表单 input/select 宽度 100% 且 min-width 0", /\.sc-setting-field input,[\s\S]*?width:\s*100%;[\s\S]*?min-width:\s*0;/.test(cssSrc), "缺少防溢出样式");
  ui.destroyView(settingsView);

  // ── 中文风格名不得硬编码在非 constants 的模块里（PRD A7 / DESIGN §10.4.6） ──
  const others = ["ui.js", "styles.js", "cmds.js", "keys.js", "host.js", "logic.js"];
  let hardCoded = "";
  for (const f of others) {
    const t = readFileSync(join(SRC_DIR, f), "utf8");
    for (const s of (await loadSrc()).constants.STYLES) {
      if (t.indexOf(s.name) >= 0) hardCoded += `${f}:${s.name} `;
    }
  }
  check(G, "非 constants.js 的模块里没有硬编码中文风格名", hardCoded === "", `硬编码：${hardCoded || "无"}`);
}

// ───────────────────────────────────────────────────────────────────────────
// 组 ⑮ 文案一致性（CODE 表 ↔ PRD 5.2，逐字）
// ───────────────────────────────────────────────────────────────────────────

/** 从 PRD.md 的 5.2 节表格里按行序抽出「界面文案」。 */
function prd52Texts(prdSrc) {
  const start = prdSrc.indexOf("### 5.2");
  if (start < 0) return null;
  const end = prdSrc.indexOf("\n## ", start + 6);
  const section = prdSrc.slice(start, end < 0 ? prdSrc.length : end);
  const rows = section.split(/\r?\n/).filter((l) => l.trim().startsWith("|"));
  const out = [];
  for (const row of rows) {
    const m = /「([^」]+)」/.exec(row);
    if (m) out.push(m[1]);
  }
  return out;
}

/** PRD 5.2 表格的行序 → CODE 表的键序（硬验收 A6）。 */
const PRD52_ORDER = [
  "NO_KEY",
  "MODEL_NOT_OPEN",
  "AUTH_FAILED",
  "RATE_LIMITED",
  "API_ERROR",
  "NET_TIMEOUT",
  "IMG_TOO_LARGE",
  "PARSE_FAILED",
  "WRITE_FAILED",
  "STORAGE_FAILED",
  "IO_TIMEOUT",
  "NO_RESULT",
  "MAX_IMAGES",
  "NOT_READY",
  "SCRIPT_EXIT",
  "BAD_ARGS",
];

async function group15() {
  console.log("\n⑮ 文案一致性（constants.js 的 CODE 表 ↔ PRD.md 5.2，逐字比对）");
  const G = "⑮";
  const { constants } = await loadSrc();
  const prdSrc = readFileSync(PRD, "utf8");

  check(G, "PRD.md 里能定位到 5.2 节", prdSrc.indexOf("### 5.2") >= 0);
  const texts = prd52Texts(prdSrc);
  check(G, "PRD 5.2 表格抽到 16 条文案", Array.isArray(texts) && texts.length === 16, `抽到 ${Array.isArray(texts) ? texts.length : 0} 条`);
  if (!texts || texts.length !== 16) return;

  const code = constants.CODE;
  for (let i = 0; i < PRD52_ORDER.length; i += 1) {
    const key = PRD52_ORDER[i];
    const want = texts[i];
    const got = code[key];
    check(G, `${key} 文案与 PRD 5.2 逐字一致`, got === want, `文件 src/constants.js:CODE.${key}\n      期望（PRD）：${want}\n      实际（CODE）：${got}`);
  }

  // 反向：PRD 的每条文案都必须能在 CODE 里找到（防止抄漏 / 抄串）
  const allValues = Object.values(code);
  const orphan = texts.filter((t) => !allValues.includes(t));
  check(G, "PRD 5.2 的每条文案都能在 CODE 表里找到（无遗漏）", orphan.length === 0, `未收录：${orphan.length} 条 → ${orphan[0] || ""}`);

  // 占位符必须保留（运行时替换的前提）
  check(G, "API_ERROR 保留 <code> / <message原文> 占位符", code.API_ERROR.includes("<code>") && code.API_ERROR.includes("<message原文>"));
  check(G, "SCRIPT_EXIT 保留 <exitCode> 占位符", code.SCRIPT_EXIT.includes("<exitCode>"));
  check(G, "CREATIVE_RANGE 保留 <n> 占位符", code.CREATIVE_RANGE.includes("<n>"));

  // 前端自产码齐全（DESIGN §10.2 下表）
  const selfCodes = [
    "IMG_DECODE_FAILED",
    "CLIPBOARD_NO_IMAGE",
    "NO_IMAGES",
    "NO_STYLE",
    "RESULT_UNSAVED",
    "GALLERY_FULL",
    "CREATIVE_RANGE",
    "CREATIVE_FAILED",
    "LAST_RESULT_DISCARDED",
    "TOO_LARGE_PREVIEW",
  ];
  for (const k of selfCodes) {
    check(G, `CODE.${k} 存在且非空`, typeof code[k] === "string" && code[k].length > 0);
  }

  // 禁止无信息量文案（PRD 5.2 规则 4）
  const BANNED = ["操作失败", "请求异常", "发生错误"];
  const bannedHit = Object.keys(code).filter((k) => BANNED.includes(String(code[k]).trim()));
  check(G, "CODE 表无「操作失败 / 请求异常 / 发生错误」这类空文案", bannedHit.length === 0, `命中：${bannedHit.join(", ") || "无"}`);

  // 三条强度边界提示走 TOASTS，不进 CODE
  check(G, "强度边界提示在 TOASTS 里（不进错误码）", constants.TOASTS.STRENGTH_MIN.includes("强度已是 0%") && constants.TOASTS.STRENGTH_MAX.includes("强度已是 100%"));

  check(G, "DESIGN.md 存在（§10.2 是同一份文案的第二来源）", existsSync(DESIGN));
}

// ───────────────────────────────────────────────────────────────────────────
// 组 ⑯ 契约交叉（index.js 自包含 / 无后门）
// ───────────────────────────────────────────────────────────────────────────

async function group16() {
  console.log("\n⑯ 契约交叉（index.js 自包含 / 无 file:// / 无 __TAURI_INTERNALS__）");
  const G = "⑯";
  const src = readFileSync(BUNDLE, "utf8");

  check(G, "index.js 存在", existsSync(BUNDLE));
  check(G, "index.js 无静态 `import ... from`（宿主走 blob:，必须自包含）", !/^\s*import\s[^;]*\bfrom\b/m.test(src), "出现了静态 import ... from");
  check(G, "index.js 无 `export ... from` 转发", !/^\s*export\s[^;]*\bfrom\b/m.test(src));
  check(G, "index.js 无动态 import(", !/\bimport\s*\(/.test(src), "出现了 import(");
  check(G, "index.js 无 require(", !/\brequire\s*\(/.test(src));
  check(G, "index.js 无 file:// 直接读本地文件", src.indexOf("file://") < 0, "出现了 file://");
  check(G, "index.js 无 __TAURI_INTERNALS__ 后门", src.indexOf("__TAURI_INTERNALS__") < 0, "出现了 __TAURI_INTERNALS__");
  check(G, "index.js 无 convertFileSrc", src.indexOf("convertFileSrc") < 0);
  check(G, "index.js 导出了 activate", /\bexport\s*\{[\s\S]*\bactivate\b/.test(src) || /export function activate/.test(src));
  check(G, "index.js 导出了 renderPage", /\bexport\s*\{[\s\S]*\brenderPage\b/.test(src) || /export function renderPage/.test(src));

  // 运行时大小：T11 原约定 60–160KB；0.5.2 起内嵌 10 张风格样品缩略图（约 +130KB），上限放宽到 512KB
  const bytes = Buffer.byteLength(src, "utf8");
  check(G, "index.js 体积在 60–512KB 之间", bytes >= 60 * 1024 && bytes <= 512 * 1024, `实际 ${(bytes / 1024).toFixed(1)}KB`);

  // 页签契约文档在位
  check(G, "宿主契约文档存在", existsSync(join(REPO, "docs", "路径A-插件页签接口契约.md")));

  // 脚本与 env 模板在位
  check(G, "tools/stylecam_io.py 存在", existsSync(join(PLUGIN_DIR, "tools", "stylecam_io.py")));
  const example = join(PLUGIN_DIR, "tools", "stylecam_io.env.example");
  check(G, "tools/stylecam_io.env.example 存在", existsSync(example));
  if (existsSync(example)) {
    const t = readFileSync(example, "utf8");
    check(G, "env.example 不含真 Key（无 ark- 开头的值）", t.indexOf("ark-") < 0, "env.example 里出现了 ark- 开头的真 Key");
  }
  check(G, "README.md 存在", existsSync(join(PLUGIN_DIR, "README.md")));
  check(G, "plugin.json 存在且合法", existsSync(MANIFEST));
}

// ───────────────────────────────────────────────────────────────────────────
// 组 ⑰ 回归：config.set 必败 / Enter 无响应 / 注册命令丢参 / provider list 传 --model
//   2026-10-13 线上事故回归：
//   ① configFailure 是 async 但调用处漏 await → Promise 恒真 → 所有配置写入必败
//      （硬件键开关切不动、设置面板保存静默失效）；
//   ② dispatchAction 缺 confirm 分支 → browse 阶段 Enter 无响应；
//   ③ main.js 注册命令 `() => fn({})` 丢参数 → ⌘SS / 硬件键带参调用全废；
//   ④ configFailure 给 `provider list` 传 --model → Python argparse 直接报错。
// ───────────────────────────────────────────────────────────────────────────

const PROVIDERS_STDOUT = [
  "OK=1",
  'PROVIDERS=[{"id":"easysu","name":"EasySU 中转","capabilities":{"formats":["auto","jpeg","png"],"quality":true,"maxN":4}}]',
  "MSG=provider 能力列表",
].join("\n");

async function group17Regression() {
  const G = "⑰";
  console.log("\n⑰ 回归（config.set 必败 / Enter confirm / 命令带参透传 / provider list 参数）");
  const { cmds, state } = await loadSrc();
  const config = seededConfig();
  config.model = "gpt-image-2.5-flare";
  config.provider = "easysu";
  const store = { "stylecam.config": config };
  const { commands, pages, calls } = await bootstrap({
    store,
    runReplies: [PY_VER, { match: "provider", exitCode: 0, stdout: PROVIDERS_STDOUT }],
  });

  // 挂页签拿 mounted 上下文（与组 ⑪ 同法）
  const container = makeEl("div");
  pages["stylecam.board"](container);

  // ① ②：config.set 必须成功，且 provider list 调用不得带 --model
  const r1 = await cmds.COMMANDS["stylecam.config.set"]({ patch: { hwKeys: false } });
  check(G, "config.set 切 hwKeys 成功（configFailure 必须 await）", r1.ok === true && r1.code === "OK", vs("ok/code", "true/OK", `${r1.ok}/${r1.code}`));
  check(G, "hwKeys=false 已写入 storage", store["stylecam.config"].hwKeys === false, vs("hwKeys", false, store["stylecam.config"].hwKeys));
  const providerRuns = calls.filter((c) => c[0] === "run" && String(c[2]).includes("provider"));
  check(G, "provider list 调用不带 --model（Python 端不收）", providerRuns.length > 0 && providerRuns.every((c) => !String(c[2]).includes("--model")), providerRuns.map((c) => c[2]).join(" | "));

  // ③：注册包装器必须透传参数（⌘SS / 硬件键带参调用）
  const r2 = await commands["stylecam.config.set"]({ patch: { hwKeys: true } });
  check(G, "注册命令透传参数（hwKeys 切回 true）", r2.ok === true && store["stylecam.config"].hwKeys === true, vs("hwKeys", true, store["stylecam.config"].hwKeys));

  // ④：browse + Enter（confirm）→ adjusting；无素材时只提示不跳阶段
  state.setStage("browse");
  const hw0 = await cmds.COMMANDS["stylecam.hardware"]({ intent: { cls: "confirm", action: "confirm", arg: null } });
  check(G, "browse 无素材 + Enter → 停在 browse 并给提示", hw0.ok === true && state.getState().stage === "browse" && !!hw0.data.toast, `stage=${state.getState().stage}`);
  await cmds.COMMANDS["stylecam.image.add"](imgPayload(1));
  state.setStage("browse");
  const hw1 = await cmds.COMMANDS["stylecam.hardware"]({ intent: { cls: "confirm", action: "confirm", arg: null } });
  check(G, "browse 有素材 + Enter → 进入 adjusting", hw1.ok === true && state.getState().stage === "adjusting", `stage=${state.getState().stage}`);
  const back = await cmds.COMMANDS["stylecam.hardware"]({ intent: { cls: "back", action: "back", arg: null } });
  check(G, "adjusting + Esc → 退回 browse", back.ok === true && state.getState().stage === "browse", `stage=${state.getState().stage}`);

  // ⑤：config.validate 同路径也必须成功（曾经同样必败）
  const r3 = await cmds.COMMANDS["stylecam.config.validate"]({ patch: { hwKeys: true } });
  check(G, "config.validate 成功（同 configFailure 路径）", r3.ok === true && r3.code === "OK", vs("ok/code", "true/OK", `${r3.ok}/${r3.code}`));
}

// ───────────────────────────────────────────────────────────────────────────

async function main() {
  console.log("StyleCam 风格调色盘 —— 验证台");
  console.log(`插件目录：${PLUGIN_DIR}`);

  const dom = installDomStub();

  await group1();
  await group8();
  await group9(dom);
  await group10();
  await group11Pipeline();
  await group12();
  await group13();
  await group14(dom);
  await group15();
  await group16();
  await group17Regression();

  console.log(`\n总计 ${pass + fail} 条断言，通过 ${pass}，失败 ${fail}`);
  if (fail) {
    console.log("\n失败清单：");
    for (const line of failures) console.log(`  - ${line}`);
    process.exit(1);
  }
}

main().catch((e) => {
  console.error("验证台自身崩了：", e);
  process.exit(3);
});
