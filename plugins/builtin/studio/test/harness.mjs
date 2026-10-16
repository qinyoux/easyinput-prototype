#!/usr/bin/env node
/**
 * 内容台插件验证台（Node，零依赖）
 *
 * 为什么要有它：插件的实现文件是 JS，但它跑在 easyop 的 webview 里、依赖 `eo.*`。
 * 没有这个台子，就只能"装进去、开 easyop、手点"——既慢又不可回归。
 * 这里用**桩 `eo.*`** 把插件真跑一遍：activate → 逐条命令 → 断言行为与信封。
 *
 * 三层校验：
 *   ① 行为：台账 CRUD / 阶段门 / 生成 / 导出的信封与落库结果
 *   ② 契约交叉校验：插件用到的每个 `eo.X`，必须同时出现在「本台桩」与「宿主 pluginHost.ts」
 *      —— 两处独立来源对齐，才能证明插件的调用不会在真机上落空
 *   ③ 失败不静默：前置不足时命令必须 ok:false 且带可读原因
 *
 * 用法：node plugins/builtin/studio/test/harness.mjs
 */

import { readFileSync, existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";

const HERE = resolve(fileURLToPath(import.meta.url), "..");
const PLUGIN_DIR = resolve(HERE, "..");
const REPO = resolve(PLUGIN_DIR, "..", "..", "..");

let pass = 0;
let fail = 0;
function check(name, cond, detail) {
  if (cond) {
    pass++;
    console.log(`  ✔ ${name}`);
  } else {
    fail++;
    console.log(`  ✘ ${name}${detail ? `  —— ${detail}` : ""}`);
  }
}

// ---------------------------------------------------------------------------
// 桩 eo.*（严格镜像 src/pluginHost.ts 的构造）
// ---------------------------------------------------------------------------

function makeEo(opts) {
  const store = opts.store;
  const calls = opts.calls;
  const io = {};
  const eo = {
    get_selection: async () => {
      calls.push(["get_selection"]);
      return opts.selection || "";
    },
    clipboard_read: async () => {
      calls.push(["clipboard_read"]);
      if (opts.clipboardThrows) throw new Error(opts.clipboardThrows);
      return opts.clipboard || "";
    },
    replace_selection: async (text) => {
      calls.push(["replace_selection", text]);
    },
    inject: async (text) => {
      calls.push(["inject", text]);
    },
    storage: {
      get: async (key) => (key in store ? store[key] : null),
      set: async (key, value) => {
        store[key] = value;
        calls.push(["storage.set", key]);
      },
    },
    log: {
      info: async (msg) => calls.push(["log.info", msg]),
      warn: async (msg) => calls.push(["log.warn", msg]),
      error: async (msg) => calls.push(["log.error", msg]),
    },
    chat: async (messages, o) => {
      calls.push(["chat", messages, o]);
      if (opts.chatThrows) throw new Error(opts.chatThrows);
      if (opts.chatReplies) return opts.chatReplies.shift() ?? "（桩回复）";
      return opts.chatReply ?? "（桩回复）";
    },
    run: async (program, args, o) => {
      calls.push(["run", program, args, o]);
      const key = args.join(" ");
      const canned = (opts.runReplies || []).find((r) => key.includes(r.match));
      return {
        exit_code: canned ? canned.exitCode : 0,
        stdout: canned ? canned.stdout : "",
        stderr: canned ? canned.stderr : "",
        timed_out: false,
        cancelled: false,
      };
    },
  };
  return eo;
}

async function loadPlugin() {
  const url = pathToFileURL(join(PLUGIN_DIR, "index.js")).href;
  return import(url);
}

/** 装载插件：返回 {commands, calls, store}。 */
async function bootstrap(opts = {}) {
  const mod = await loadPlugin();
  const store = opts.store || {};
  const calls = [];
  const commands = {};
  const pages = opts.pagesSink || {};
  const context = {
    api: makeEo({ store, calls, ...opts }),
    pluginId: "studio",
    pluginDir: PLUGIN_DIR,
    registerCommand: (id, fn) => {
      commands[id] = fn;
    },
  };
  // 桩默认**不**提供 registerPage —— 这是刻意的：它镜像今天的真实宿主
  // （宿主侧 H4 尚未实现）。传 opts.withPages: true 才模拟「宿主已支持页签」。
  // 两条路径都要测，否则「旧宿主静默跳过」这条向后兼容承诺就没有证据。
  if (opts.withPages) {
    context.registerPage = (id, fn) => {
      pages[id] = fn;
    };
  }
  await mod.activate(context);
  return { mod, commands, calls, store, context, pages };
}

// ---------------------------------------------------------------------------
// 极简 DOM 桩 —— 只为把「页签渲染」路径真跑起来。
//
// 不追求通用：只实现插件实际用到的方法。querySelector 不做真解析，
// 而是按 `[data-sp="x"]` 的约定返回一个占位节点——插件只按这个约定取节点，
// 所以够用，且能真实覆盖 buildView 的整条装配流程。
// ---------------------------------------------------------------------------

function makeEl(tag) {
  const el = {
    tagName: String(tag).toUpperCase(),
    children: [],
    parentNode: null,
    attributes: {},
    _html: "",
    _spNodes: {},
    _class: "",
    textContent: "",
    value: "",
    disabled: false,
    title: "",
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
    querySelector(sel) {
      const m = /\[data-sp="([^"]+)"\]/.exec(String(sel));
      if (!m) return makeEl("div");
      if (!el._spNodes[m[1]]) el._spNodes[m[1]] = makeEl("div");
      return el._spNodes[m[1]];
    },
    set innerHTML(v) {
      el._html = String(v);
      el._spNodes = {};
    },
    get innerHTML() {
      return el._html;
    },
    set className(v) {
      el._class = String(v);
    },
    get className() {
      return el._class;
    },
  };
  Object.defineProperty(el, "firstChild", { get: () => el.children[0] || null });
  return el;
}

/** 装上 document 桩；返回 head/body 供断言检查。 */
function installDomStub() {
  const head = makeEl("head");
  const body = makeEl("body");
  globalThis.document = { head, body, createElement: (t) => makeEl(t) };
  return { head, body };
}

// ---------------------------------------------------------------------------
// 用例
// ---------------------------------------------------------------------------

const TITLE_REPLY = ["① 直给结论的标题", "② 提问式的标题？", "③ 场景切入的标题"].join("\n");
const BODY_REPLY = "第一段正文内容。\n\n## 小节一\n\n第二节的内容写在这里，凑一些字数以便统计。\n";

function intel() {
  return {
    angle: ["角度一", "角度二"],
    fact: ["事实：某设备温升 50℃"],
    quote: ["金句一"],
    risk: [],
  };
}

async function main() {
  console.log("内容台插件验证台\n");

  // ── 1. 激活：命令注册齐 ──────────────────────────────────────────────
  console.log("① activate / 命令注册");
  {
    const { commands, calls } = await bootstrap();
    const EXPECTED = [

      "studio.state",
      "studio.topics.list",
      "studio.topics.save",
      "studio.topics.remove",
      "studio.stage.advance",
      "studio.gen.title",
      "studio.gen.body",
      "studio.gen.humanize",
      "studio.gen.intel",
      "studio.gen.all",
      "studio.export",
      "studio.doctor",
      "studio.quick_action",
    ];
    for (const id of EXPECTED) check(`注册了 ${id}`, typeof commands[id] === "function");
    check("激活时写了日志", calls.some((c) => c[0] === "log.info"));
  }

  // ── 2. 台账 CRUD ────────────────────────────────────────────────────
  console.log("\n② 台账 CRUD（eo.storage）");
  let topicId = null;
  {
    const { commands, store } = await bootstrap();
    const empty = await commands["studio.state"]();
    check("空台账 state 正常", empty.ok && empty.data.topicCount === 0);

    const saved = await commands["studio.topics.save"]({ title: "超声发射链路阻抗匹配", intel: intel() });
    check("保存选题成功", saved.ok === true, JSON.stringify(saved));
    topicId = saved.data.id;
    check("id 已生成", typeof topicId === "string" && topicId.length > 8);
    check("落进 store.topics", Array.isArray(store.topics) && store.topics.length === 1);
    check("初始阶段是 inspiration", store.topics[0].stage === "inspiration");

    const again = await commands["studio.topics.save"]({ id: topicId, body: "x" });
    check("同 id 是 upsert 不新增", store.topics.length === 1, `len=${store.topics.length}`);

    const listed = await commands["studio.topics.list"]();
    check("list 返回 1 条", listed.ok && listed.data.count === 1);

    const badArg = await commands["studio.topics.save"]({});
    check("无 title/id 时失败不静默", badArg.ok === false && badArg.code === "BAD_ARGS");

    const rmMissing = await commands["studio.topics.remove"]({ id: "nope" });
    check("删不存在的选题报 NO_TOPIC", rmMissing.ok === false && rmMissing.code === "NO_TOPIC");
  }

  // ── 2b. 剪贴板 fallback：get_selection 失败后读剪贴板 ──────────────
  console.log("\n②b 剪贴板 fallback（get_selection 空 → clipboard_read）");
  {
    const src = readFileSync(join(PLUGIN_DIR, "index.js"), "utf8");
    check("源码含 clipboard_read fallback", /eo\.clipboard_read/.test(src));
    check("源码含剪贴板提示文案", /Ctrl\+C/.test(src));
    check("源码 source=clipboard", /source\s*=\s*['"]clipboard['"]/.test(src));

    const { commands, store, calls } = await bootstrap({
      selection: "",
      clipboard: "从外部应用复制的内容",
    });
    const r = await commands["studio.topics.save"]({ title: "", source: "selection" });
    check("空标题 + 空选区 → BAD_ARGS（命令层不 fallback）", r.ok === false);
    check("命令层不自动调 clipboard_read", !calls.some((c) => c[0] === "clipboard_read"));
  }

  // ── 2c. 快捷动作 studio.quick_action（旋钮确认） ──────────────────
  console.log("\n②c 快捷动作 studio.quick_action（旋钮确认）");
  {
    // studio_new：从剪贴板取标题
    const { commands: c1, store: s1 } = await bootstrap({
      selection: "",
      clipboard: "旋钮复制的素材",
    });
    const r1 = await c1["studio.quick_action"]({ action: "studio_new" });
    check("studio_new 从剪贴板建选题", r1.ok === true, JSON.stringify(r1));
    check("studio_new 标题正确", s1.topics?.[0]?.title === "旋钮复制的素材");

    // studio_new：选中文本优先于剪贴板
    const { commands: c2, store: s2 } = await bootstrap({
      selection: "选中的文本",
      clipboard: "剪贴板内容",
    });
    const r2 = await c2["studio.quick_action"]({ action: "studio_new" });
    check("studio_new 优先取选中文本", s2.topics?.[0]?.title === "选中的文本");

    // studio_new：都没有则失败
    const { commands: c3 } = await bootstrap({ selection: "", clipboard: "" });
    const r3 = await c3["studio.quick_action"]({ action: "studio_new" });
    check("studio_new 无文本 → BAD_ARGS", r3.ok === false && r3.code === "BAD_ARGS");

    // studio_pipeline：无当前选题 → NOT_READY
    const { commands: c4 } = await bootstrap({});
    const r4 = await c4["studio.quick_action"]({ action: "studio_pipeline" });
    check("studio_pipeline 无选题 → NOT_READY", r4.ok === false && r4.code === "NOT_READY");

    // studio_advance：有选题 → 调 stage.advance
    const { commands: c5, store: s5 } = await bootstrap({});
    await c5["studio.topics.save"]({ title: "测试选题", intel: intel() });
    const r5 = await c5["studio.quick_action"]({ action: "studio_advance" });
    check("studio_advance 有选题 → 成功", r5.ok === true, JSON.stringify(r5));

    // 未知动作 → BAD_ARGS
    const { commands: c6 } = await bootstrap({});
    const r6 = await c6["studio.quick_action"]({ action: "bogus" });
    check("未知动作 → BAD_ARGS", r6.ok === false && r6.code === "BAD_ARGS");

    // 缺 action 参数 → BAD_ARGS
    const r7 = await c6["studio.quick_action"]();
    check("缺 action → BAD_ARGS", r7.ok === false && r7.code === "BAD_ARGS");
  }

  // ── 3. 阶段流转与门禁 ───────────────────────────────────────────────
  // 门禁语义 = 「该阶段的输入是否就绪」：进 intel 不需前置（灵感本身即输入），
  // 进 body 需要情报卡够 3 条，进 humanized/exported 需要正文。（figures 阶段已移除）
  console.log("\n③ 阶段流转 / 门禁");
  {
    const store = {
      topics: [
        {
          id: "t1",
          title: "T",
          stage: "inspiration",
          intel: { angle: [], fact: [], quote: [], risk: [] },
          titles: [],
          body: "",
          humanized: "",
        },
      ],
      meta: { schemaVersion: 1, currentId: "t1" },
    };
    const { commands } = await bootstrap({ store });

    const toIntel = await commands["studio.stage.advance"]();
    check("进 intel 不需前置", toIntel.ok === true && toIntel.data.stage === "intel", JSON.stringify(toIntel));
    check("阶段已落库", store.topics[0].stage === "intel");

    const blockedBody = await commands["studio.stage.advance"]();
    check("情报卡不足 → 进 body 被挡", blockedBody.ok === false && blockedBody.code === "STAGE_GUARD", blockedBody.message);
    check("挡住时说明了缺什么", /情报卡/.test(blockedBody.message || ""), blockedBody.message);

    await commands["studio.topics.save"]({ id: "t1", intel: intel() });
    const toBody = await commands["studio.stage.advance"]();
    check("情报卡够 → 进 body 成功", toBody.ok === true && toBody.data.stage === "body", JSON.stringify(toBody));

    const blockedHuman = await commands["studio.stage.advance"]();
    check("无正文 → 进去 AI 味被挡", blockedHuman.ok === false && blockedHuman.code === "STAGE_GUARD", blockedHuman.message);
  }

  // ── 4. 生成（eo.chat）────────────────────────────────────────────────
  console.log("\n④ 生成：标题 / 正文 / 去 AI 味（eo.chat）");
  {
    const store = {
      topics: [{ id: "t1", title: "T", stage: "intel", intel: intel(), titles: [], body: "", humanized: "" }],
      meta: { schemaVersion: 1, currentId: "t1" },
    };
    const { commands, calls } = await bootstrap({ store, chatReply: TITLE_REPLY });

    const t = await commands["studio.gen.title"]();
    check("生成标题成功", t.ok === true, JSON.stringify(t));
    check("返回 3 个候选", t.data.titles.length === 3, JSON.stringify(t.data.titles));
    check("剥掉了序号前缀", t.data.titles[0].startsWith("①") === false, t.data.titles[0]);
    check("标题落库", store.topics[0].titles.length === 3);
    check("自动选出第一个", store.topics[0].chosenTitle === t.data.titles[0]);

    const chatCall = calls.find((c) => c[0] === "chat");
    check("调 chat 时带了 system 提示词", !!(chatCall && chatCall[2] && chatCall[2].system));
    check("system 提示词是标题用的", /标题/.test(chatCall[2].system));
    check("messages 是 role/content 二元组", chatCall[1][0].role === "user" && typeof chatCall[1][0].content === "string");
  }
  {
    const store = {
      topics: [{ id: "t1", title: "T", stage: "intel", intel: intel(), titles: ["A"], body: "", humanized: "" }],
      meta: { schemaVersion: 1, currentId: "t1" },
    };
    const { commands } = await bootstrap({ store, chatReply: BODY_REPLY });
    const b = await commands["studio.gen.body"]();
    check("生成正文成功", b.ok === true, JSON.stringify(b));
    check("返回字数", typeof b.data.words === "number" && b.data.words > 10, String(b.data.words));
    check("正文落库", !!store.topics[0].body);
    check("阶段推进到 body", store.topics[0].stage === "body");
  }
  {
    // 情报卡不足 → 必须拒绝
    const store = {
      topics: [{ id: "t1", title: "T", stage: "inspiration", intel: { angle: ["a"] }, titles: [], body: "", humanized: "" }],
      meta: { schemaVersion: 1, currentId: "t1" },
    };
    const { commands } = await bootstrap({ store, chatReply: BODY_REPLY });
    const b = await commands["studio.gen.body"]();
    check("情报卡 <3 条时拒绝生成正文", b.ok === false && b.code === "STAGE_GUARD", JSON.stringify(b));
  }
  {
    // 无正文 → 去 AI 味必须拒绝
    const store = { topics: [{ id: "t1", title: "T", stage: "body", intel: intel(), titles: [], body: "", humanized: "" }], meta: { currentId: "t1" } };
    const { commands } = await bootstrap({ store });
    const h = await commands["studio.gen.humanize"]();
    check("无正文时去 AI 味被挡", h.ok === false && h.code === "STAGE_GUARD", JSON.stringify(h));
  }
  {
    // LLM 失败 → 原样带出，不吞
    const store = { topics: [{ id: "t1", title: "T", stage: "intel", intel: intel(), titles: [], body: "", humanized: "" }], meta: { currentId: "t1" } };
    const { commands } = await bootstrap({ store, chatThrows: "LLM API Key 未配置" });
    const t = await commands["studio.gen.title"]();
    check("LLM 抛错 → LLM_FAILED", t.ok === false && t.code === "LLM_FAILED", JSON.stringify(t));
    check("错误原文被带出（不吞）", /Key 未配置/.test(t.message || ""), t.message);
  }
  {
    // LLM 返回空 → 不能当成功
    const store = { topics: [{ id: "t1", title: "T", stage: "intel", intel: intel(), titles: [], body: "", humanized: "" }], meta: { currentId: "t1" } };
    const { commands } = await bootstrap({ store, chatReply: "   " });
    const t = await commands["studio.gen.title"]();
    check("LLM 返回空 → 失败而非空标题", t.ok === false && t.code === "LLM_FAILED", JSON.stringify(t));
  }
  {
    // 去 AI 味：字数漂移要算出来
    const store = {
      topics: [{ id: "t1", title: "T", stage: "body", intel: intel(), titles: [], body: "一二三四五六七八九十", humanized: "" }],
      meta: { currentId: "t1" },
    };
    const { commands } = await bootstrap({ store, chatReply: "一二三四五六七八九十" });
    const h = await commands["studio.gen.humanize"]();
    check("去 AI 味成功", h.ok === true, JSON.stringify(h));
    check("算了前后字数", h.data.before === 10 && h.data.after === 10, JSON.stringify(h.data));
    check("阶段推进到 humanized", store.topics[0].stage === "humanized");
  }

  // ── 4b. 生成情报卡（studio.gen.intel）──────────────────────────────
  console.log("\n④b 生成情报卡（studio.gen.intel）");
  {
    const INTEL_REPLY = JSON.stringify({
      angle: ["从用户痛点切入", "从技术原理切入"],
      fact: ["相关设备温升 50℃", "市场占比 30%"],
      quote: ["某专家说：这是趋势"],
      risk: ["可能存在争议"],
    });
    const { commands, store, calls } = await bootstrap({
      store: { topics: [{ id: "t1", title: "测试选题", stage: "inspiration", intel: { angle: [], fact: [], quote: [], risk: [] } }] },
      chatReply: INTEL_REPLY,
    });
    const r = await commands["studio.gen.intel"]({ id: "t1" });
    check("情报卡生成成功", r.ok, JSON.stringify(r));
    check("返回条数", r.data.count === 6, `count=${r.data.count}`);
    check("情报落库", store.topics[0].intel.angle.length === 2, JSON.stringify(store.topics[0].intel));
    check("阶段推进到 intel", store.topics[0].stage === "intel", store.topics[0].stage);
    check("调了 eo.chat", calls.some((c) => c[0] === "chat"));
  }

  // ── 4c. 一键执行（studio.gen.all）────────────────────────────────────
  console.log("\n④c 一键执行（studio.gen.all）");
  {
    const INTEL_REPLY = JSON.stringify({
      angle: ["角度一", "角度二"], fact: ["事实一"], quote: ["金句一"], risk: [],
    });
    const { commands, store, calls } = await bootstrap({
      store: { topics: [{ id: "t1", title: "一键执行测试", stage: "inspiration", intel: { angle: [], fact: [], quote: [], risk: [] } }] },
      chatReplies: [
        INTEL_REPLY,                                    // gen.intel
        "标题一\n标题二\n标题三",                         // gen.title
        "这是正文内容，足够长的正文用于测试。".repeat(20),  // gen.body
        "这是内容优化后的正文。".repeat(20),                 // gen.humanize
      ],
    });
    const r = await commands["studio.gen.all"]({ id: "t1" });
    check("一键执行成功", r.ok, JSON.stringify(r));
    check("走了 4 步", r.data.steps.length === 4, JSON.stringify(r.data.steps));
    check("情报卡已生成", store.topics[0].intel.angle.length === 2);
    check("标题已生成", store.topics[0].titles.length === 3);
    check("正文已生成", !!(store.topics[0].body || "").trim());
    check("内容优化已生成", !!(store.topics[0].humanized || "").trim());
    check("调了 4 次 eo.chat", calls.filter((c) => c[0] === "chat").length === 4);
  }

  // ── 5. 导出（eo.run）：组稿 → pub.py check（不再打任何投放包）─────────
  console.log("\n⑤ 导出：组稿 → pub.py check（eo.run）");
  {
    const store = {
      topics: [{ id: "t1", title: "T", chosenTitle: "标题A", stage: "humanized", intel: intel(), titles: ["标题A"], body: "正文", humanized: "正文（内容优化后）" }],
      meta: { currentId: "t1" },
    };
    const { commands, calls } = await bootstrap({
      store,
      runReplies: [
        { match: "studio_io.py", exitCode: 0, stdout: "POST=D:\\drafts\\标题A.md\n字数（去空白）：12\n" },
        { match: "pub.py check", exitCode: 0, stdout: "check ok" },
      ],
    });
    const r = await commands["studio.export"]();
    check("导出成功", r.ok === true, JSON.stringify(r));
    check("解析出稿件路径", r.data.postPath === "D:\\drafts\\标题A.md", r.data.postPath);
    check("check 通过", r.data.checkOk === true);
    check("稿件路径已落库", store.topics[0].postPath === "D:\\drafts\\标题A.md");

    const runCalls = calls.filter((c) => c[0] === "run");
    check("两次 eo.run（组稿+check）", runCalls.length === 2, `实际 ${runCalls.length}`);
    const argv = (i) => runCalls[i][2].join(" ");
    check("第一条是自带脚本 compose", argv(0).includes("studio_io.py") && argv(0).includes("compose"), argv(0));
    check("compose 传了 topic-id", argv(0).includes("--topic-id") && argv(0).includes("t1"), argv(0));
    check("第二条是 pub.py check", argv(1).includes("pub.py") && /(^|\s)check(\s|$)/.test(argv(1)), argv(1));
    check("导出不再打任何投放包", !runCalls.some((c) => c[2].join(" ").includes("pack")), "pack 应已移除");
    check("run 都带 cwd", runCalls.every((c) => !!c[3] && !!c[3].cwd));
    check("run 都带超时", runCalls.every((c) => !!c[3] && typeof c[3].timeoutSecs === "number"));
  }
  {
    // 组稿失败 → IO_FAILED，且不谎报成功
    const store = { topics: [{ id: "t1", title: "T", stage: "body", intel: intel(), titles: [], body: "正文", humanized: "" }], meta: { currentId: "t1" } };
    const { commands } = await bootstrap({
      store,
      runReplies: [{ match: "studio_io.py", exitCode: 2, stderr: "台账不存在" }],
    });
    const r = await commands["studio.export"]();
    check("组稿非零码 → IO_FAILED", r.ok === false && r.code === "IO_FAILED", JSON.stringify(r));
    check("带上退出码", /exit=2/.test(r.message || ""), r.message);
  }
  {
    // 无正文 → 直接挡
    const store = { topics: [{ id: "t1", title: "T", stage: "inspiration", intel: intel(), titles: [], body: "", humanized: "" }], meta: { currentId: "t1" } };
    const { commands } = await bootstrap({ store });
    const r = await commands["studio.export"]();
    check("无正文导出被挡", r.ok === false && r.code === "STAGE_GUARD", JSON.stringify(r));
  }
  {
    // 老 host 不给 pluginDir → 明确报错，不瞎猜路径
    const mod = await loadPlugin();
    const store = { topics: [{ id: "t1", title: "T", stage: "body", intel: intel(), titles: [], body: "正文", humanized: "" }], meta: { currentId: "t1" } };
    const commands = {};
    await mod.activate({
      api: makeEo({ store, calls: [] }),
      pluginId: "studio",
      // 故意不给 pluginDir
      registerCommand: (id, fn) => {
        commands[id] = fn;
      },
    });
    const r = await commands["studio.export"]();
    check("无 pluginDir → NOT_READY 且提示原因", r.ok === false && r.code === "NOT_READY", JSON.stringify(r));
  }

  // ── 5b. 平台发布：排版 skill → 只落一个 HTML 文件 ──────────────────
  console.log("\n⑤b 平台发布（studio.publish.html：只产一个 HTML）");
  {
    const store = {
      topics: [{ id: "t1", title: "T", chosenTitle: "标题A", stage: "humanized", intel: intel(), titles: ["标题A"], body: "正文", humanized: "正文（内容优化后）" }],
      meta: { currentId: "t1" },
    };
    const { commands, calls } = await bootstrap({
      store,
      chatReply: '<p style="font-size:15px">排版后 HTML 片段</p>',
      runReplies: [
        { match: "write-html-file", exitCode: 0, stdout: "HTML=D:\\drafts\\标题A.html\n" },
      ],
    });
    const r = await commands["studio.publish.html"]();
    check("平台发布成功", r.ok === true, JSON.stringify(r));
    check("返回 HTML 路径", r.data.htmlPath === "D:\\drafts\\标题A.html", r.data.htmlPath);
    check("htmlPath 已落库", store.topics[0].htmlPath === "D:\\drafts\\标题A.html");

    const runCalls = calls.filter((c) => c[0] === "run");
    check("只调一次 eo.run（写 HTML）", runCalls.length === 1, `实际 ${runCalls.length}`);
    check("调的是 write-html-file", runCalls[0][2].join(" ").includes("write-html-file"), runCalls[0][2].join(" "));
    check("不再组稿 .md", !runCalls.some((c) => c[2].join(" ").includes("compose")));
    check("不再调 pub.py", !runCalls.some((c) => c[2].join(" ").includes("pub.py")));
    const chatCalls = calls.filter((c) => c[0] === "chat");
    check("调了 eo.chat 排版", chatCalls.some((c) => c[2] && c[2].system && /排版/.test(c[2].system)));
    check("消息提示可复制粘贴", /复制/.test(r.message || ""), r.message);
  }
  {
    // LLM 排版失败 → 本地转换器兜底，仍出一个可用 HTML（degraded）
    const store = {
      topics: [{ id: "t1", title: "T", chosenTitle: "标题A", stage: "humanized", intel: intel(), titles: ["标题A"], body: "正文", humanized: "正文（内容优化后）" }],
      meta: { currentId: "t1" },
    };
    const { commands, calls } = await bootstrap({
      store,
      chatThrows: "LLM 请求失败",
      runReplies: [
        { match: "markdown-to-html", exitCode: 0, stdout: "<!DOCTYPE html><html><body><p>本地兜底排版</p></body></html>\n" },
        { match: "write-html-file", exitCode: 0, stdout: "HTML=D:\\drafts\\标题A.html\n" },
      ],
    });
    const r = await commands["studio.publish.html"]();
    check("LLM 挂掉仍出 HTML（degraded）", r.ok === true && r.data.degraded === true, JSON.stringify(r));
    check("降级说明带原因", /本地转换器兜底/.test(r.data.note || ""), r.data.note);
    const runCalls = calls.filter((c) => c[0] === "run");
    check("降级时走了 markdown-to-html", runCalls.some((c) => c[2].join(" ").includes("markdown-to-html")));
  }
  {
    // 无正文 → 直接挡
    const store = { topics: [{ id: "t1", title: "T", stage: "inspiration", intel: intel(), titles: [], body: "", humanized: "" }], meta: { currentId: "t1" } };
    const { commands } = await bootstrap({ store });
    const r = await commands["studio.publish.html"]();
    check("无正文平台发布被挡", r.ok === false && r.code === "STAGE_GUARD", JSON.stringify(r));
  }

  // ── 5c. 打开目录：explorer 参数形状回归防线 ─────────────────────────
  console.log("\n⑤c 打开目录（explorer /select 参数形状）");
  {
    // 真机 bug 回归：曾写成单个字符串 ['/select,"C:\\path"']，runner 的 argv 转义
    // 把含空格串二次包引号 → explorer 解析失败 → 打开「此电脑」。
    // 正确形状（与 easyop 核心 open_containing_folder 一致）：
    // 两个独立参数 ['/select,', '<反斜杠路径>']，路径本身不带引号。
    const store = {
      topics: [{
        id: "t1", title: "T", stage: "exported", intel: intel(), titles: [], body: "正文", humanized: "正文",
        postPath: "D:\\drafts\\A.md",
        htmlPath: "D:\\drafts\\正文.html",
      }],
      meta: { currentId: "t1" },
    };
    const { commands, calls } = await bootstrap({ store });
    const r = await commands["studio.open.dir"]();
    check("打开目录成功", r.ok === true, JSON.stringify(r));
    const runCalls = calls.filter((c) => c[0] === "run" && String(c[1]).includes("explorer"));
    check("调了一次 explorer", runCalls.length === 1, JSON.stringify(runCalls.map((c) => c[2])));
    const argv = runCalls[0][2];
    check("参数是两个独立元素", argv.length === 2, JSON.stringify(argv));
    check("第一个元素是 /select,", argv[0] === "/select,", argv[0]);
    check("路径不带引号（防 argv 二次转义）", !argv[1].includes('"'), argv[1]);
    check("路径是反斜杠（explorer 只认反斜杠）", argv[1].includes("\\") && !argv[1].includes("/"), argv[1]);
    check("htmlPath 优先于 postPath", argv[1] === "D:\\drafts\\正文.html", argv[1]);
    check("返回的 dir 是文件所在目录", r.data.dir === "D:\\drafts", r.data.dir);

    // 只有 postPath（未平台发布）也能打开
    const store2 = {
      topics: [{ id: "t1", title: "T", stage: "exported", intel: intel(), titles: [], body: "正文", humanized: "正文", postPath: "D:\\drafts\\A.md" }],
      meta: { currentId: "t1" },
    };
    const { commands: cmds2, calls: calls2 } = await bootstrap({ store: store2 });
    const r2 = await cmds2["studio.open.dir"]();
    check("仅 postPath 时也可打开", r2.ok === true, JSON.stringify(r2));
    const argv2 = calls2.filter((c) => c[0] === "run" && String(c[1]).includes("explorer"))[0][2];
    check("仅 postPath 时定位 postPath", argv2[1] === "D:\\drafts\\A.md", argv2[1]);

    // 两个路径都没有 → 明确报错，不擅自打开「此电脑」
    const store3 = {
      topics: [{ id: "t1", title: "T", stage: "body", intel: intel(), titles: [], body: "正文", humanized: "" }],
      meta: { currentId: "t1" },
    };
    const { commands: cmds3, calls: calls3 } = await bootstrap({ store: store3 });
    const r3 = await cmds3["studio.open.dir"]();
    check("无产物时明确报 NOT_READY", r3.ok === false && r3.code === "NOT_READY", JSON.stringify(r3));
    check("无产物时不调 explorer", !calls3.some((c) => c[0] === "run" && String(c[1]).includes("explorer")));
  }

  // ── 6. 页签渲染边界 ───────────────────────────────────────────────
  console.log("\n⑥ 页签渲染边界（无 DOM 时 renderPage 安全跳过）");
  {
    const { pages } = await bootstrap({ withPages: true, pagesSink: {} });
    // 无 document 时 renderPage 应安全跳过，不抛异常
    delete globalThis.document;
    const life = pages["studio.board"](null);
    check("无容器时 renderPage 同步返回 lifecycle", !!life && typeof life === "object");
    check("无容器时 lifecycle 仍提供 refresh/dispose", typeof life.refresh === "function" && typeof life.dispose === "function");
  }

  // ── 7. 契约交叉校验：插件用的 eo.* 必须真实存在 ──────────────────────
  console.log("\n⑦ 契约交叉校验（插件 ↔ 桩 ↔ 宿主 pluginHost.ts）");
  {
    const src = readFileSync(join(PLUGIN_DIR, "index.js"), "utf8");
    const host = readFileSync(join(REPO, "src", "pluginHost.ts"), "utf8");
    // 必须先剥注释——否则注释里提到的 `eo.panel.open` 会被当成真实调用。
    const code = src.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:])\/\/[^\n]*/gm, "$1");
    const used = new Set();
    for (const m of code.matchAll(/\beo\.([A-Za-z_][A-Za-z0-9_]*)/g)) used.add(m[1]);
    check("插件用到的 eo.* 非空", used.size > 0, [...used].join(","));
    check("剥注释后不再误报 eo.panel", !used.has("panel"), [...used].join(","));

    const apiSurface = [
      "get_selection",
      "clipboard_read",
      "replace_selection",
      "inject",
      "storage",
      "log",
      "chat",
      "run",
    ];
    for (const name of used) {
      check(`桩里有 eo.${name}`, apiSurface.includes(name));
    }
    // 宿主侧：pluginHost.ts 里每个 eo 方法对应的 tauri 命令必须存在
    const HOST_CMD = {
      get_selection: "eo_get_selection",
      clipboard_read: "eo_clipboard_read",
      replace_selection: "eo_replace_selection",
      inject: "eo_inject",
      storage: "eo_storage_get",
      log: "eo_log_info",
      chat: "eo_chat",
      run: "eo_run",
    };
    for (const name of used) {
      const cmd = HOST_CMD[name];
      check(`宿主 pluginHost.ts 里 eo.${name} → ${cmd}`, host.includes(`"${cmd}"`), `未在 pluginHost.ts 找到 ${cmd}`);
    }
    // pluginDir 契约：宿主必须把 dir 透传进 context
    check("宿主透传 pluginDir", /pluginDir/.test(host), "pluginHost.ts 里找不到 pluginDir");
    const rust = readFileSync(join(REPO, "src-tauri", "src", "plugin_host.rs"), "utf8");
    check(
      "Rust 侧 payload 带 dir",
      /"dir":\s*plugin\.dir/.test(rust.replace(/\s+/g, " ")),
      "plugin_host.rs 的 payload 未带 dir",
    );
  }

  // ── 8. 清单与实现一致 ───────────────────────────────────────────────
  console.log("\n⑧ plugin.json ↔ index.js 一致性");
  {
    const mf = JSON.parse(readFileSync(join(PLUGIN_DIR, "plugin.json"), "utf8"));
    const { commands } = await bootstrap();
    const declared = mf.contributes.commands.map((c) => c.id);
    for (const id of declared) check(`清单声明的 ${id} 真有实现`, typeof commands[id] === "function");
    const impl = Object.keys(commands);
    for (const id of impl) check(`实现的 ${id} 已在清单声明`, declared.includes(id));
    check("tools 为空（studio 走页签，不进 ⌘SS）", Array.isArray(mf.contributes.tools) && mf.contributes.tools.length === 0);
    check("studio.toggle 不在命令里（已移除）", !declared.includes("studio.toggle"));
    for (const t of mf.contributes.tools) {
      check(`tools 引用的 ${t.command} 已声明`, declared.includes(t.command));
    }
    check("id 与命令前缀一致", declared.every((c) => c.startsWith(`${mf.id}.`)));
  }

  // ── 9. 页签契约（docs/路径A-插件页签接口契约.md） ──────────────────
  console.log("\n⑨ 页签契约（路径 A）");
  {
    const PAGE_ID = "studio.board";
    const mf = JSON.parse(readFileSync(join(PLUGIN_DIR, "plugin.json"), "utf8"));
    const pageDecls = mf.contributes.pages || [];
    const declared = mf.contributes.commands.map((c) => c.id);

    // —— 清单侧 ——
    check("清单声明了 contributes.pages", pageDecls.length > 0);
    check("page id 非空", pageDecls.every((p) => typeof p.id === "string" && p.id.trim()));
    check("page title 非空", pageDecls.every((p) => typeof p.title === "string" && p.title.trim()));
    check(
      "page id 不与任何命令 id 冲突",
      pageDecls.every((p) => !declared.includes(p.id)),
      pageDecls.map((p) => p.id).join(","),
    );
    // 安全铁律：icon 只能是「名字」，绝不能是 SVG / HTML 字符串（本项目 csp:null）
    check(
      "icon 是名字而非 SVG/HTML 字符串",
      pageDecls.every((p) => p.icon === undefined || /^[a-z][a-z0-9_-]*$/i.test(String(p.icon))),
      pageDecls.map((p) => p.icon).join(","),
    );
    check(
      "order 若存在须为数字",
      pageDecls.every((p) => p.order === undefined || typeof p.order === "number"),
      pageDecls.map((p) => p.order).join(","),
    );
    check("order 不与宿主静态页签段（0-49）冲突", pageDecls.every((p) => p.order === undefined || p.order >= 50));

    // —— 向后兼容：旧宿主（**没有** registerPage）必须照常工作 ——
    {
      const { commands } = await bootstrap();
      check("旧宿主无 registerPage 时 activate 不抛异常", true);
      check(
        "旧宿主下全部命令仍被注册（页签不应挤掉命令路径）",
        Object.keys(commands).length === declared.length,
        `注册了 ${Object.keys(commands).length}/${declared.length}`,
      );
    }

    // —— 新宿主：提供 registerPage ——
    const dom = installDomStub();
    const pagesMap = {};
    const { commands } = await bootstrap({ withPages: true, pagesSink: pagesMap });

    check("新宿主下 registerPage 被调用", Object.keys(pagesMap).length === 1, Object.keys(pagesMap).join(","));
    check(`registerPage 的 id 与清单一致（${PAGE_ID}）`, typeof pagesMap[PAGE_ID] === "function");
    check(
      "registerPage 的 id 全部在清单里声明过",
      Object.keys(pagesMap).every((id) => pageDecls.some((p) => p.id === id)),
      Object.keys(pagesMap).join(","),
    );

    // —— 渲染函数契约 P1 / P3 ——
    const container = makeEl("div");
    const life = pagesMap[PAGE_ID](container);
    check("P1 渲染函数同步返回（非 Promise）", !(life instanceof Promise), String(life));
    check("P1 返回 lifecycle 对象", !!life && typeof life === "object");
    check("P1 lifecycle.refresh 是函数", typeof (life && life.refresh) === "function");
    check("P1 lifecycle.dispose 是函数", typeof (life && life.dispose) === "function");

    const panelRoot = container.children[0];
    check("渲染后容器里挂上了面板根节点", !!panelRoot);
    check("面板根节点带 data-studio=panel", !!panelRoot && panelRoot.getAttribute("data-studio") === "panel");
    check("面板根节点 class 为 studio-panel", !!panelRoot && panelRoot.className === "studio-panel", panelRoot && panelRoot.className);
    check("样式注入进了 document.head", dom.head.children.length > 0);
    check("P3 容器仍是面板根节点的父节点（未被移走）", !!panelRoot && panelRoot.parentNode === container);

    // 幂等：重复渲染不叠加
    pagesMap[PAGE_ID](container);
    check("重复渲染后容器里仍只有一份面板", container.children.length === 1, `children=${container.children.length}`);

    // dispose 要摘干净
    const life2 = pagesMap[PAGE_ID](container);
    life2.dispose();
    check("dispose 后容器被清空", container.children.length === 0);
    check("dispose 后样式也从 head 摘除", dom.head.children.length === 0, `head=${dom.head.children.length}`);

    // —— P2：页签面板不得用 position:fixed（会脱离宿主布局） ——
    const src = readFileSync(join(PLUGIN_DIR, "index.js"), "utf8");
    check("面板样式无 position:fixed", !/\.studio-panel\s*\{[^}]*position:\s*fixed/.test(src));
    check("源码无 .studio-overlay 残留", !/studio-overlay/.test(src));

    // —— 契约文档在位 ——
    check("契约文档存在", existsSync(join(REPO, "docs", "路径A-插件页签接口契约.md")));
  }

  console.log(`\n结果：${pass} 通过 / ${fail} 失败`);
  if (fail) process.exit(1);
}

main().catch((e) => {
  console.error("验证台自身崩了：", e);
  process.exit(3);
});
