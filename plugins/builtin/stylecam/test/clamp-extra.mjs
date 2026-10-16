#!/usr/bin/env node
/**
 * clampStrength 非法值语义 —— 补断言（零依赖，Node 直跑）
 *
 * 为什么单独一个文件：`test/harness.mjs` 是 QA 那批的活，本批不改它。
 * 2026-09-25 修 `clampStrength(null) === 0`（`Number(null)===0` 让强度静默跳到 0），
 * 这里把「null / undefined / NaN / 布尔 / 空串 / 非数字字符串 → 默认 50」钉死，
 * 同时守住「合法数字仍照旧夹取」不被这次修改误伤。
 *
 * 用法：node plugins/builtin/stylecam/test/clamp-extra.mjs
 */

import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = resolve(fileURLToPath(import.meta.url), "..");
const SRC_DIR = join(HERE, "..", "src");
const urlOf = (p) => pathToFileURL(p).href;

const logic = await import(urlOf(join(SRC_DIR, "logic.js")));
const constants = await import(urlOf(join(SRC_DIR, "constants.js")));
const DEFAULTS = constants.DEFAULTS;

let pass = 0;
let fail = 0;
const failures = [];

function check(name, cond, detail) {
  if (cond) {
    pass += 1;
    console.log(`  ✔ ${name}`);
  } else {
    fail += 1;
    const line = `${name}${detail ? ` —— ${detail}` : ""}`;
    failures.push(line);
    console.log(`  ✘ ${line}`);
  }
}

const vs = (label, expected, actual) => `${label} 期望 ${JSON.stringify(expected)}，实际 ${JSON.stringify(actual)}`;

console.log("clampStrength 非法值回落（补断言）");
check("默认强度是 50", DEFAULTS.strength === 50, vs("DEFAULTS.strength", 50, DEFAULTS.strength));

// ① 非法值一律回落默认 50（`null` 是本次修掉的那条）
const BAD = [null, undefined, NaN, "", "   ", "abc", "强度", "0x10", "Infinity", "-Infinity", true, false, {}, [70]];
for (const input of BAD) {
  const got = logic.clampStrength(input);
  check(`clampStrength(${JSON.stringify(input)}) === 50（非法值回落）`, got === 50, vs("结果", 50, got));
}

// ② 合法数字与数字字符串仍照旧夹取，不能被这次修改误伤
const GOOD = [
  [-5, 0],
  [0, 0],
  [49.6, 50],
  [50, 50],
  [70, 70],
  [100, 100],
  [150, 100],
  ["0", 0],
  ["70", 70],
  ["  70  ", 70],
  ["1e2", 100],
];
for (const [input, want] of GOOD) {
  const got = logic.clampStrength(input);
  check(`clampStrength(${JSON.stringify(input)}) === ${want}（合法值照旧）`, got === want, vs("结果", want, got));
}

// ③ 档位跟随：非法值 → 50 → 平衡融合，不能再落进「轻度点缀」
check("clampStrength(null) 不是 0（强度不会静默跳到 0）", logic.clampStrength(null) !== 0, vs("结果", "非 0", logic.clampStrength(null)));
check("strengthBand(null) 是平衡融合", logic.strengthBand(null).name === "平衡融合", logic.strengthBand(null).name);
check("buildPrompt 用 null 强度时写 50/100", logic.buildPrompt({ id: "x", prompt: "p" }, null, 1).includes("50/100"), logic.buildPrompt({ id: "x", prompt: "p" }, null, 1));

console.log(`\n总计 ${pass + fail} 条断言，通过 ${pass}，失败 ${fail}`);
if (fail) {
  console.log("\n失败清单：");
  for (const line of failures) console.log(`  - ${line}`);
  process.exit(1);
}
