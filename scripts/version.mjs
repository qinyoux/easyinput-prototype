#!/usr/bin/env node
/**
 * easyop 版本一致性工具（零依赖，Node 原生）
 *
 * 设计前提：**版本号只有一个真源 —— package.json 的 version**，
 * 其余四处（tauri.conf.json / Cargo.toml / package-lock.json / Cargo.lock）由本脚本同步。
 *
 * 用法：
 *   node scripts/version.mjs check              校验：五处版本号一致 + CHANGELOG / README 有对应条目 + 署名已填
 *   node scripts/version.mjs sync               以 package.json 为准，同步其余四处版本号
 *   node scripts/version.mjs bump <spec>        升版本，同步版本号并插入 CHANGELOG / README 骨架
 *                                               spec = major | minor | patch | 0.7.0
 *   node scripts/version.mjs bump <spec> --author "<署名>"
 *                                               署名写入骨架，格式：宿主/项目/会话名
 *                                               例：workbuddy/easyinput-prototype/AI小助手
 *
 * 署名规则（见 AGENTS.md §3.1）：**每个版本节必须署名**，否则 check 失败。
 * 目的是多 agent 并行改动同一工作树时，能一眼回答"这一版是谁改的"。
 *
 * 退出码：0 = 通过；1 = 校验失败（CI / 收工自检可直接用退出码判断）
 */

import { readFileSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const F = {
  pkg: path.join(ROOT, "package.json"),
  conf: path.join(ROOT, "src-tauri", "tauri.conf.json"),
  cargo: path.join(ROOT, "src-tauri", "Cargo.toml"),
  cargoLock: path.join(ROOT, "src-tauri", "Cargo.lock"),
  lock: path.join(ROOT, "package-lock.json"),
  changelog: path.join(ROOT, "CHANGELOG.md"),
  readme: path.join(ROOT, "README.md"),
};
const rel = (p) => path.relative(ROOT, p).split(path.sep).join("/");
const read = (p) => readFileSync(p, "utf8");
const nl = (s) => s.replace(/\r\n/g, "\n");

// ---------- 读取五处版本号 ----------

/** 从 Cargo.toml 的 [package] 段取第一个 version（其余 version 属于依赖，不能碰） */
function cargoVersionOrNull() {
  const m = read(F.cargo).match(/^version\s*=\s*"([^"]+)"/m);
  return m ? m[1] : null;
}

/**
 * `Cargo.lock` 里**本包自身**那条 `[[package]]` 的版本。
 *
 * ⛔ 这是第 5 处版本号，也是唯一一处**会被 `cargo build` 自动改写**的。
 * 早期只同步四处时踩过这个坑：升完版本不 build，`Cargo.lock` 还停在旧号，
 * 下一次 `cargo build` 又会把它"改成"新号 ⇒ 版本文件在"已一致 / 又不一致"之间来回摆，
 * 谁先谁后取决于有没有跑过构建。同步它，才真是"改了 A 也改了 B"。
 *
 * 只认 `[[package]]` 块里 `name` 恰好等于包名的那一条 —— 依赖各有各的版本，碰不得。
 */
const CARGO_LOCK_PKG = "easyop";

/** 返回「本包 [[package]] 块」的起始偏移（相对已归一化为 \n 的文本），找不到返回 null */
function cargoLockBlockStart(text) {
  const marker = `[[package]]\nname = "${CARGO_LOCK_PKG}"\n`;
  const at = text.indexOf(marker);
  return at < 0 ? null : at + marker.length;
}

function cargoLockVersionOrNull() {
  const text = nl(read(F.cargoLock));
  const from = cargoLockBlockStart(text);
  if (from === null) return null;
  const m = text.slice(from).match(/^version\s*=\s*"([^"]+)"/m);
  return m ? m[1] : null;
}

function writeCargoLockVersion(version) {
  const raw = read(F.cargoLock);
  const text = nl(raw);
  const from = cargoLockBlockStart(text);
  if (from === null) return false; // 本包还没进 lock（从未 build）⇒ 交给 cargo 自生，不硬造
  const rest = text.slice(from);
  const m = rest.match(/^version\s*=\s*"[^"]+"/m);
  if (!m || m[0] === `version = "${version}"`) return false;
  writeKeepEol(F.cargoLock, text.slice(0, from) + rest.replace(m[0], `version = "${version}"`), raw);
  return true;
}

function readVersions() {
  return [
    { label: "package.json", file: rel(F.pkg), version: JSON.parse(read(F.pkg)).version, src: "真源" },
    { label: "tauri.conf.json", file: rel(F.conf), version: JSON.parse(read(F.conf)).version },
    { label: "Cargo.toml", file: rel(F.cargo), version: cargoVersionOrNull() },
    { label: "package-lock.json", file: rel(F.lock), version: JSON.parse(read(F.lock)).version },
    { label: "Cargo.lock", file: rel(F.cargoLock), version: cargoLockVersionOrNull() },
  ];
}

// ---------- 版本号运算 ----------

const parse = (v) => {
  const m = String(v).trim().match(/^(\d+)\.(\d+)\.(\d+)(?:[-+].*)?$/);
  if (!m) throw new Error(`版本号格式非法：${v}（要求 x.y.z）`);
  return { major: +m[1], minor: +m[2], patch: +m[3] };
};
const cmp = (a, b) => {
  const x = parse(a), y = parse(b);
  return x.major - y.major || x.minor - y.minor || x.patch - y.patch;
};
function nextVersion(current, spec) {
  if (/^\d+\.\d+\.\d+$/.test(spec)) return spec; // 显式指定
  const v = parse(current);
  if (spec === "major") return `${v.major + 1}.0.0`;
  if (spec === "minor") return `${v.major}.${v.minor + 1}.0`;
  if (spec === "patch") return `${v.major}.${v.minor}.${v.patch + 1}`;
  throw new Error(`未知的版本增量说明：${spec}（可用 major | minor | patch | x.y.z）`);
}

const today = () => {
  const d = new Date();
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
};

// ---------- 写入 ----------

/**
 * 保留原文件的换行风格再落盘。
 * 多 agent 共用一个工作树时，凭空把 CRLF 改成 LF 会造出整文件级假 diff，
 * 既淹没真实改动，也让并行编辑更容易冲突 —— 所以换行一律"随原件"。
 */
function writeKeepEol(file, text, raw) {
  const eol = raw.includes("\r\n") ? "\r\n" : "\n";
  const body = text.replace(/\r\n/g, "\n").split("\n").join(eol);
  writeFileSync(file, body, "utf8");
}

function writeJsonVersion(file, version) {
  const raw = read(file);
  const obj = JSON.parse(raw);
  // package-lock.json 的根包版本是第二处，必须一起改，否则 npm 会报 lock 与 pkg 不一致
  const needRoot = obj.version !== version;
  const needLockPkg = !!(obj.packages && obj.packages[""] && obj.packages[""].version !== version);
  if (!needRoot && !needLockPkg) return false; // 无实质变化就不写，避免无谓 diff
  if (needRoot) obj.version = version;
  if (needLockPkg) obj.packages[""].version = version;
  writeKeepEol(file, JSON.stringify(obj, null, 2) + "\n", raw);
  return true;
}

function writeCargoVersion(version) {
  const raw = read(F.cargo);
  const next = raw.replace(/^version\s*=\s*"[^"]+"/m, `version = "${version}"`);
  if (next === raw) return false;
  writeFileSync(F.cargo, next, "utf8");
  return true;
}

export function sync(version) {
  const done = [];
  if (writeJsonVersion(F.pkg, version)) done.push(rel(F.pkg));
  if (writeJsonVersion(F.conf, version)) done.push(rel(F.conf));
  if (writeCargoVersion(version)) done.push(rel(F.cargo));
  if (writeJsonVersion(F.lock, version)) done.push(rel(F.lock));
  if (writeCargoLockVersion(version)) done.push(rel(F.cargoLock));
  return done;
}

/**
 * 署名：优先取 --author，其次取环境变量 EASYOP_AUTHOR，最后回落到 git 用户名。
 * 格式约定：宿主/项目/会话名，例如 workbuddy/easyinput-prototype/AI小助手
 */
function resolveAuthor(explicit) {
  const raw = (explicit || process.env.EASYOP_AUTHOR || "").trim();
  if (raw) return raw;
  return null;
}

/** 取 git user.name 作为兜底署名（拿不到就返回空，交由 check 报错） */
function gitUserName() {
  try {
    return execFileSync("git", ["config", "user.name"], {
      cwd: ROOT,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
  } catch {
    return "";
  }
}

/** 取某个版本节的署名行 */
function authorOf(section) {
  const m = section.match(/^>\s*署名\s*[:：]\s*(.+)$/m);
  return m ? m[1].trim() : "";
}

/** 在 CHANGELOG 首个版本节之前插入新节骨架 */
function insertChangelog(version, author) {
  const raw = read(F.changelog);
  if (raw.includes(`## [v${version}]`)) return false;
  const at = raw.search(/^## \[/m);
  if (at < 0) throw new Error("CHANGELOG.md 找不到版本节（## [vX.Y.Z]），请先修正文件结构");
  const who = author ? author : "（待填写：宿主/项目/会话名，例 workbuddy/easyinput-prototype/AI小助手）";
  const block =
    `## [v${version}] — ${today()}\n\n` +
    `> 署名：${who}\n\n` +
    `### 新增\n\n- （待填写：新增了什么能力，为什么加）\n\n` +
    `### 变更\n\n- （待填写：改动了什么行为，会不会影响既有配置）\n\n` +
    `### 修复\n\n- （待填写：修了什么问题，根因是什么）\n\n` +
    `### ⚠️ 未完成\n\n- （待填写：缺什么、卡在哪、下一步走哪条路；没有就写"无"）\n\n` +
    `---\n\n`;
  writeKeepEol(F.changelog, raw.slice(0, at) + block + raw.slice(at), raw);
  return true;
}

/** 在 README 的「版本历史」下插入新版本摘要骨架 */
function insertReadme(version, author) {
  const raw = read(F.readme);
  if (raw.includes(`### v${version}\n`)) return false;
  const head = raw.search(/^## 版本历史\s*$/m);
  if (head < 0) throw new Error("README.md 找不到「## 版本历史」小节，请先修正文件结构");
  const bodyFrom = head + raw.slice(head).indexOf("\n");
  const next = raw.slice(bodyFrom).search(/^### /m);
  const at = next < 0 ? raw.length : bodyFrom + next;
  const who = author ? ` —— 署名：${author}` : "";
  const block =
    `### v${version}\n\n` +
    `- （待填写：一句话概括本版对用户的影响）${who}\n\n`;
  writeKeepEol(F.readme, raw.slice(0, at) + block + raw.slice(at), raw);
  return true;
}

// ---------- 校验 ----------

function check() {
  const rows = readVersions();
  const pkgV = rows[0].version;
  const bad = rows.filter((r) => r.version !== pkgV);

  console.log("\n版本号分布（真源 = package.json）\n");
  for (const r of rows) {
    const mark = r.version === pkgV ? "✅" : "❌";
    console.log(`  ${mark} ${r.version.padEnd(10)} ${r.label}${r.src ? `  (${r.src})` : ""}`);
  }

  const ch = read(F.changelog);
  const rd = read(F.readme);
  const hasCh = new RegExp(`^## \\[v${pkgV.replace(/\./g, "\\.")}\\]`, "m").test(ch);
  const hasRd = rd.includes(`### v${pkgV}\n`);
  console.log("\n变更说明齐备性\n");
  console.log(`  ${hasCh ? "✅" : "❌"} CHANGELOG.md 存在 [v${pkgV}] 条目`);
  console.log(`  ${hasRd ? "✅" : "❌"} README.md 版本历史存在 v${pkgV} 条目`);

  // ---- 署名校验：只校验当前版本节，历史节不追溯 ----
  let author = "";
  let authorOk = false;
  if (hasCh) {
    const from = ch.search(new RegExp(`^## \\[v${pkgV.replace(/\./g, "\\.")}\\]`, "m"));
    const rest = ch.slice(from);
    const nextIdx = rest.slice(1).search(/^## \[/m);
    const section = nextIdx < 0 ? rest : rest.slice(0, nextIdx + 1);
    author = authorOf(section);
    authorOk = !!author && !/待填写/.test(author);
    const shown = authorOk ? author : author || "（缺失）";
    console.log(`  ${authorOk ? "✅" : "❌"} CHANGELOG [v${pkgV}] 已署名：${shown}`);
    const pending = /^- \（待填写/m.test(section) || /（待填写/.test(section);
    if (pending) console.log(`  ⚠️ [v${pkgV}] 节内仍有「待填写」占位符，必须填成真内容`);
  }

  const ok = bad.length === 0 && hasCh && hasRd && authorOk;
  if (!ok) {
    console.log("\n结论：❌ 版本纪律未满足。");
    if (bad.length) console.log(`  · 执行 \`node scripts/version.mjs sync\` 可把其余四处拉到 ${pkgV}`);
    if (!hasCh) console.log(`  · 到 CHANGELOG.md 补 [v${pkgV}] 条目（新增 / 变更 / 修复）`);
    if (!hasRd) console.log(`  · 到 README.md §版本历史 补 v${pkgV} 摘要`);
    if (!authorOk)
      console.log(`  · 到 CHANGELOG.md [v${pkgV}] 节补署名行：\`> 署名：宿主/项目/会话名\``);
  } else {
    console.log(`\n结论：✅ 五处版本号一致，变更说明齐备，署名已填（v${pkgV} · ${author}）。`);
  }
  return ok;
}

// ---------- 入口 ----------

const argv = process.argv.slice(2);
const cmd = argv[0];
const arg = argv[1];
// 署名来源优先级：--author "..." > EASYOP_AUTHOR 环境变量 > （都不给则写入占位符，check 会拦）
const authorIdx = argv.indexOf("--author");
const explicitAuthor = authorIdx >= 0 ? argv[authorIdx + 1] : undefined;

try {
  if (cmd === "check" || !cmd) {
    process.exit(check() ? 0 : 1);
  } else if (cmd === "sync") {
    const v = JSON.parse(read(F.pkg)).version;
    const done = sync(v);
    console.log(done.length ? `已同步到 v${v}：\n  · ${done.join("\n  · ")}` : `四处版本号已一致（v${v}），无需改动`);
    process.exit(check() ? 0 : 1);
  } else if (cmd === "bump") {
    if (!arg) throw new Error("用法：node scripts/version.mjs bump <major|minor|patch|x.y.z> [--author \"署名\"]");
    const cur = JSON.parse(read(F.pkg)).version;
    const v = nextVersion(cur, arg);
    if (cmp(v, cur) <= 0) throw new Error(`新版本 ${v} 未高于当前 ${cur}`);
    const author = resolveAuthor(explicitAuthor) || gitUserName();
    console.log(`版本：v${cur} → v${v}`);
    console.log(`署名：${author || "（未提供，骨架将留占位符，check 会拦）"}`);
    const done = sync(v);
    console.log(`版本号已写入：\n  · ${done.join("\n  · ")}`);
    const ch = insertChangelog(v, author);
    const rd = insertReadme(v, author);
    console.log(ch ? "CHANGELOG.md 已插入新版本节骨架" : "CHANGELOG.md 已存在该版本节，跳过");
    console.log(rd ? "README.md 已插入版本历史骨架" : "README.md 已存在该版本，跳过");
    console.log("\n⚠️ 骨架只是占位，必须逐项填成真内容；填完跑 `node scripts/version.mjs check` 验收。");
    process.exit(0);
  } else {
    console.log("用法：node scripts/version.mjs <check|sync|bump> [spec] [--author \"署名\"]");
    process.exit(2);
  }
} catch (e) {
  console.error(`❌ ${e.message}`);
  process.exit(2);
}
