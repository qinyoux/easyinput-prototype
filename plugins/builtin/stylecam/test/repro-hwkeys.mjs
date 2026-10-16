#!/usr/bin/env node
/**
 * 复现「硬件键开关切不动 / Enter 不进风格选择」—— 真实子进程 eo.run + 真配置。
 * 用法：node plugins/builtin/stylecam/test/repro-hwkeys.mjs
 */
import { join, resolve } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { execFile } from "node:child_process";

const HERE = resolve(fileURLToPath(import.meta.url), "..");
const PLUGIN_DIR = resolve(HERE, "..");
const SRC_DIR = join(PLUGIN_DIR, "src");

// 与运行目录（%APPDATA%\com.easyop.app\plugins\stylecam\storage.json）同构的配置，无 Key
const runtimeConfig = {
  v: 2, outDir: "", hwKeys: true, privacyAcked: true, hwIntroSeen: true,
  pyExe: "C:\\Espressif\\tools\\python\\python.exe", model: "gpt-image-2.5-flare",
  provider: "easysu", resolution: "2K", width: 1024, height: 1024,
  format: "auto", quality: null, n: 1, httpTimeoutSecs: 120, retryCount: 1,
};

function makeEo(opts) {
  const store = opts.store;
  const calls = opts.calls;
  return {
    storage: {
      get: async (key) => (key in store ? store[key] : null),
      set: async (key, value) => { store[key] = value; calls.push(["storage.set", key]); },
    },
    log: {
      info: async (msg) => calls.push(["log.info", String(msg)]),
      warn: async (msg) => calls.push(["log.warn", String(msg)]),
      error: async (msg) => calls.push(["log.error", String(msg)]),
    },
    run: (program, args, o) =>
      new Promise((res) => {
        const opts2 = { timeout: ((o && o.timeout_secs) || 60) * 1000, maxBuffer: 64 * 1024 * 1024 };
        execFile(program, args || [], opts2, (err, stdout, stderr) => {
          calls.push(["run", program, (args || []).join(" ")]);
          if (err && !stdout) {
            res({ exit_code: -1, stdout: "", stderr: String(err && err.message), timed_out: false, cancelled: false });
            return;
          }
          res({ exit_code: err && typeof err.code === "number" ? err.code : 0, stdout: String(stdout), stderr: String(stderr), timed_out: false, cancelled: false });
        });
      }),
    get_selection: async () => "",
    clipboard_read: async () => "",
    replace_selection: async () => {},
    inject: async () => {},
    chat: async () => "",
  };
}

async function loadSrc() {
  const urlOf = (p) => pathToFileURL(p).href;
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
  return { constants, logic, cmds, keys, styles, ui, main, state };
}

const { main, cmds, logic, state } = await loadSrc();
main.deactivate();
main.__test.resetAll();
main.__test.resetKeyCounters();

const store = { "stylecam.config": runtimeConfig };
const calls = [];
const commands = {};
const pages = {};
await main.activate({
  api: makeEo({ store, calls }),
  pluginId: "stylecam",
  pluginDir: PLUGIN_DIR,
  registerCommand: (id, fn) => { commands[id] = fn; },
  registerPage: (id, fn) => { pages[id] = fn; },
});
await new Promise((r) => setTimeout(r, 50));

// 模拟页签挂载 + 已装载一张素材（browse 阶段）
state.setMounted(true);
state.addImage({ id: "img-1", name: "cat.png", dataUrl: "data:image/png;base64,AAAA", b64: "AAAA", bytes: 4, w: 64, h: 64, role: "main" });
state.setStage("browse");

console.log("== ① UI 按钮真实路径：cmds.cmdConfigSet { hwKeys: false } ==");
const res = await cmds.cmdConfigSet({ patch: { hwKeys: false } });
console.log("结果:", JSON.stringify({ ok: res.ok, code: res.code, message: res.message }));
console.log("storage hwKeys:", store["stylecam.config"].hwKeys, "（期望 false）");

console.log("\n== ② 经注册包装器调 stylecam.config.set（⌘SS / 硬件键路径，带参） ==");
const res2 = await commands["stylecam.config.set"]({ patch: { hwKeys: true } });
console.log("结果:", JSON.stringify({ ok: res2.ok, code: res2.code, message: res2.message }));
console.log("storage hwKeys:", store["stylecam.config"].hwKeys, "（期望 true）");

console.log("\n== ③ browse + Enter（confirm）应进入 adjusting ==");
const hw = await cmds.cmdHardware({ intent: { cls: "confirm", action: "confirm", arg: null } });
console.log("结果:", JSON.stringify({ ok: hw.ok, code: hw.code, message: hw.message }));
console.log("当前 stage:", state.getState().stage, "（期望 adjusting）");

console.log("\n== ④ browse + Enter（无素材时应提示，不进 adjusting） ==");
state.clearImages();
state.setStage("browse");
const hw2 = await cmds.cmdHardware({ intent: { cls: "confirm", action: "confirm", arg: null } });
console.log("结果:", JSON.stringify({ ok: hw2.ok, code: hw2.code, message: hw2.message, toast: hw2.data && hw2.data.toast }));
console.log("当前 stage:", state.getState().stage, "（期望 browse）");

console.log("\n== ⑤ config.validate 同路径 ==");
state.addImage({ id: "img-2", name: "cat2.png", dataUrl: "data:image/png;base64,AAAA", b64: "AAAA", bytes: 4, w: 64, h: 64, role: "main" });
const cv = await cmds.cmdConfigValidate({ patch: { hwKeys: false } });
console.log("结果:", JSON.stringify({ ok: cv.ok, code: cv.code, message: cv.message }));

console.log("\n== run 调用记录 ==");
for (const c of calls.filter((x) => x[0] === "run")) console.log("  run:", c[1], c[2].slice(0, 80));
