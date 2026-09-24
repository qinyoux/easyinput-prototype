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
// 极简 DOM 桩 —— 只为把「页签渲染」和「⌘SS 浮层挂载」两条路径真跑起来。
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
      "studio.toggle",
      "studio.state",
      "studio.topics.list",
      "studio.topics.save",
      "studio.topics.remove",
      "studio.stage.advance",
      "studio.gen.title",
      "studio.gen.body",
      "studio.gen.humanize",
      "studio.export",
      "studio.doctor",
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

  // ── 3. 阶段流转与门禁 ───────────────────────────────────────────────
  // 门禁语义 = 「该阶段的输入是否就绪」：进 intel 不需前置（灵感本身即输入），
  // 进 body 需要情报卡够 3 条，进 humanized/figures/exported 需要正文。
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

  // ── 5. 导出（eo.run）─────────────────────────────────────────────────
  console.log("\n⑤ 导出：组稿 → pub.py check → pack xhs（eo.run）");
  {
    const store = {
      topics: [{ id: "t1", title: "T", chosenTitle: "标题A", stage: "humanized", intel: intel(), titles: ["标题A"], body: "正文", humanized: "正文（去过 AI 味）" }],
      meta: { currentId: "t1" },
    };
    const { commands, calls } = await bootstrap({
      store,
      runReplies: [
        { match: "studio_io.py", exitCode: 0, stdout: "POST=D:\\drafts\\标题A.md\n字数（去空白）：12\n" },
        { match: "pub.py check", exitCode: 0, stdout: "check ok" },
        { match: "pub.py pack", exitCode: 0, stdout: "pack ok" },
      ],
    });
    const r = await commands["studio.export"]({ packXhs: true });
    check("导出成功", r.ok === true, JSON.stringify(r));
    check("解析出稿件路径", r.data.postPath === "D:\\drafts\\标题A.md", r.data.postPath);
    check("check 通过", r.data.checkOk === true);
    check("pack 通过", r.data.packOk === true);
    check("提示「导出≠已发布」", /手动上传/.test(r.message || ""), r.message);
    check("稿件路径已落库", store.topics[0].postPath === "D:\\drafts\\标题A.md");

    const runCalls = calls.filter((c) => c[0] === "run");
    check("三次 eo.run", runCalls.length === 3, `实际 ${runCalls.length}`);
    const argv = (i) => runCalls[i][2].join(" ");
    check("第一条是自带脚本 compose", argv(0).includes("studio_io.py") && argv(0).includes("compose"), argv(0));
    check("compose 传了 topic-id", argv(0).includes("--topic-id") && argv(0).includes("t1"), argv(0));
    check("第二条是 pub.py check", argv(1).includes("pub.py") && /(^|\s)check(\s|$)/.test(argv(1)), argv(1));
    check("第三条是 pub.py pack xhs", argv(2).includes("pub.py") && argv(2).includes("pack xhs"), argv(2));
    check("run 都带 cwd", runCalls.every((c) => !!c[3] && !!c[3].cwd));
    check("run 都带超时", runCalls.every((c) => !!c[3] && typeof c[3].timeoutSecs === "number"));
  }
  {
    // 小红书缺配图：pub.py 真机退出码 3 —— 必须如实上报，不能报"已导出"了事
    const store = {
      topics: [{ id: "t1", title: "T", chosenTitle: "标题A", stage: "humanized", intel: intel(), titles: [], body: "正文", humanized: "正文" }],
      meta: { currentId: "t1" },
    };
    const { commands } = await bootstrap({
      store,
      runReplies: [
        { match: "studio_io.py", exitCode: 0, stdout: "POST=D:\\drafts\\A.md\n" },
        { match: "pub.py check", exitCode: 0, stdout: "ok" },
        { match: "pub.py pack", exitCode: 3, stdout: "!! 没有可用配图。小红书必须至少 1 张图" },
      ],
    });
    const r = await commands["studio.export"]({ packXhs: true });
    check("组稿成功时仍算导出成功", r.ok === true, JSON.stringify(r));
    check("投放包失败被标出 packOk=false", r.data.packOk === false, JSON.stringify(r.data));
    check("消息里带上退出码 3", /exit=3/.test(r.message || ""), r.message);
    check("消息里说明未生成", /未生成/.test(r.message || ""), r.message);
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

  // ── 5b. 公众号导出：登录态有效走 publish，失效降级 HTML 投放包 ──────
  console.log("\n⑤b 公众号导出（publishWechat 分支）");
  {
    // 登录态有效 → pub.py publish wechat --save-draft
    const store = {
      topics: [{ id: "t1", title: "T", chosenTitle: "标题A", stage: "humanized", intel: intel(), titles: ["标题A"], body: "正文", humanized: "正文（去过 AI 味）" }],
      meta: { currentId: "t1" },
    };
    const { commands, calls } = await bootstrap({
      store,
      chatReply: "<p>排版片段</p>",
      runReplies: [
        { match: "studio_io.py", exitCode: 0, stdout: "POST=D:\\drafts\\标题A.md\n" },
        { match: "pub.py check", exitCode: 0, stdout: "ok" },
        { match: "pub.py status", exitCode: 0, stdout: "  微信公众号    已登录 ✓\n  小红书      已登录 ✓\n" },
        { match: "publish wechat", exitCode: 0, stdout: "草稿已保存" },
      ],
    });
    const r = await commands["studio.export"]({ publishWechat: true });
    check("公众号有效→导出成功", r.ok === true, JSON.stringify(r));
    check("公众号走 auto 模式", r.data.wechatMode === "auto", JSON.stringify(r.data));
    check("公众号自动填草稿成功", r.data.wechatOk === true, JSON.stringify(r.data));
    const runCalls = calls.filter((c) => c[0] === "run");
    check("调了 publish wechat --save-draft", runCalls.some((c) => c[2].join(" ").includes("publish") && c[2].join(" ").includes("wechat") && c[2].join(" ").includes("--save-draft")));
  }
  {
    // 登录态失效 → 降级生成本地 HTML 投放包（eo.chat 排版 + write-html）
    const store = {
      topics: [{ id: "t1", title: "T", chosenTitle: "标题A", stage: "humanized", intel: intel(), titles: ["标题A"], body: "正文", humanized: "正文（去过 AI 味）" }],
      meta: { currentId: "t1" },
    };
    const { commands, calls } = await bootstrap({
      store,
      chatReply: '<p style="font-size:15px">排版后 HTML 片段</p>',
      runReplies: [
        { match: "studio_io.py", exitCode: 0, stdout: "POST=D:\\drafts\\标题A.md\n" },
        { match: "pub.py check", exitCode: 0, stdout: "ok" },
        { match: "pub.py status", exitCode: 1, stdout: "  微信公众号    登录态已失效 ✗\n  小红书      已登录 ✓\n" },
        { match: "write-html", exitCode: 0, stdout: "HTML=D:\\drafts\\标题A-wechat投放包\\正文.html\n" },
      ],
    });
    const r = await commands["studio.export"]({ publishWechat: true });
    check("公众号失效→导出仍成功", r.ok === true, JSON.stringify(r));
    check("公众号走 html 降级模式", r.data.wechatMode === "html", JSON.stringify(r.data));
    check("HTML 投放包生成成功", r.data.wechatOk === true, JSON.stringify(r.data));
    const runCalls = calls.filter((c) => c[0] === "run");
    check("调了 status 预检", runCalls.some((c) => c[2].join(" ").includes("status")));
    check("调了 write-html", runCalls.some((c) => c[2].join(" ").includes("write-html")));
    check("没调 publish wechat", !runCalls.some((c) => c[2].join(" ").includes("publish wechat")));
    const chatCalls = calls.filter((c) => c[0] === "chat");
    check("调了 eo.chat 排版", chatCalls.some((c) => c[2] && c[2].system && /排版/.test(c[2].system)));
    check("消息提示手动粘贴", /手动粘贴/.test(r.message || ""), r.message);
  }

  // ── 6. 无 DOM 时 toggle 必须报错 ────────────────────────────────────
  console.log("\n⑥ 面板挂载边界（Node 无 DOM）");
  {
    const { commands } = await bootstrap();
    const r = await commands["studio.toggle"]();
    check("无 DOM 时 toggle 返回 NOT_READY（不谎报已打开）", r.ok === false && r.code === "NOT_READY", JSON.stringify(r));
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
    check("页签模式标记 data-studio-mode=page", !!panelRoot && panelRoot.getAttribute("data-studio-mode") === "page");
    check("页签模式带 is-page class", !!panelRoot && /is-page/.test(panelRoot.className), panelRoot && panelRoot.className);
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

    // —— ⌘SS 浮层回归：页签改造不能把老路径搞坏 ——
    const r1 = await commands["studio.toggle"]();
    check("⌘SS 浮层仍能打开", r1.ok && r1.data.open === true, JSON.stringify(r1));
    const overlay = dom.body.children[0];
    check("浮层用 .studio-overlay 包装承担定位", !!overlay && /studio-overlay/.test(overlay.className), overlay && overlay.className);
    check(
      "浮层内层面板不带 is-page（两种模式共用同一份 DOM）",
      !!overlay && overlay.children[0] && !/is-page/.test(overlay.children[0].className),
      overlay && overlay.children[0] && overlay.children[0].className,
    );
    const r2 = await commands["studio.toggle"]();
    check("再 toggle 一次收起浮层", r2.ok && r2.data.open === false, JSON.stringify(r2));
    check("收起后 body 里不留浮层残骸", dom.body.children.length === 0, `body=${dom.body.children.length}`);

    // —— P2：静态扫描，position:fixed 只允许挂在浮层包装类上 ——
    const src = readFileSync(join(PLUGIN_DIR, "index.js"), "utf8");
    const fixedOwners = [...src.matchAll(/\.([a-z][a-z0-9-]*)\s*\{[^}]*position:\s*fixed/gi)].map((m) => m[1]);
    check(
      "position:fixed 只挂在 .studio-overlay 上",
      fixedOwners.every((c) => c === "studio-overlay"),
      fixedOwners.join(",") || "(无)",
    );
    check("页签面板类 .studio-panel 自身无 fixed", !/\.studio-panel\s*\{[^}]*position:\s*fixed/.test(src));

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
