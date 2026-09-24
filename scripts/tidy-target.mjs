#!/usr/bin/env node
// 回收 `src-tauri/target/` 里**我们自己 crate 的死产物**，保留依赖缓存。
//
// 为什么需要它 —— Cargo 的 `target/` 是**只增不减**的：
//   产物按 fingerprint（rustc 版本 / feature / profile / 环境变量…）寻址，
//   fingerprint 一变就换一套新文件名，**旧的那套 Cargo 永远不会删**。
//   于是每轮编译都在 `deps/` 落下几百个 `.rcgu.o`（代码生成单元的中间目标文件）
//   和一个上百 MB 的 `.pdb`，只要有一轮没跑完整（存盘触发重编、Ctrl+C、强杀），
//   它们就永久留下。实测本项目 6 天积到 54 GB，其中 30 GB 就是这么来的。
//
//   本脚本做两件事，顺序不能反：
//     1. `cargo clean -p easyop` —— 官方动作：删掉本包**全部**产物与 fingerprint，
//        下次编译本包从零开始（依赖不动，所以只慢 2~4 分钟，不是 20 分钟）。
//     2. 扫掉 deps 下的孤儿 `.rcgu.o` 与 `*.long-type-*.txt`（rustc 的"类型名太长"
//        诊断转储）。第 1 步匹配不到那些 fingerprint 已经消失的残留 —— 实测会漏
//        几百个 —— 但因为本包此刻已是全脏状态、下次必全量重编，它们一定用不上。
//
// ⚠️ 不会碰依赖缓存（`target/*/deps/*.rlib` 等）、不会碰 `target/release` 的成品。
//    真要连依赖一起清，用 `npm run cache:purge`（`cargo clean`，下次全编 15~20 分钟）。
//
// 用法：
//   npm run cache:tidy          # 干完直接报告回收了多少
//   npm run cache:tidy -- --dry # 只看会做什么，不动磁盘

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const MANIFEST = path.join(ROOT, "src-tauri", "Cargo.toml");
const TARGET = path.join(ROOT, "src-tauri", "target");

const DRY = process.argv.includes("--dry");
const PKG = "easyop";

/** 纯 JS 递归求目录字节数（不依赖 du / PowerShell，跨平台一致）。 */
function dirSize(dir) {
  let total = 0;
  let stack = [dir];
  while (stack.length) {
    const cur = stack.pop();
    let entries;
    try {
      entries = fs.readdirSync(cur, { withFileTypes: true });
    } catch {
      continue; // 并发删除/无权限的目录直接跳过
    }
    for (const e of entries) {
      const p = path.join(cur, e.name);
      if (e.isDirectory()) stack.push(p);
      else if (e.isFile()) {
        try {
          total += fs.statSync(p).size;
        } catch {
          /* 已被删掉，忽略 */
        }
      }
    }
  }
  return total;
}

const mb = (n) => (n / 1024 / 1024).toFixed(1) + " MB";
const gb = (n) => (n / 1024 / 1024 / 1024).toFixed(2) + " GB";
const human = (n) => (n >= 1024 ** 3 ? gb(n) : mb(n));

/** 把某个 profile 目录下 deps/ 里的死产物删掉，返回 [删了几个文件, 多少字节]。 */
function sweepOrphans(profileDir) {
  const deps = path.join(profileDir, "deps");
  if (!fs.existsSync(deps)) return [0, 0];

  const isOrphan = (name) =>
    name.endsWith(".rcgu.o") ||
    // rustc 在报错信息里写不下超长类型名时，会把完整名字转储到这种文件里。
    // 每次报错都换一个哈希名，于是也随每轮编译堆积。
    (name.startsWith(PKG) && name.includes(".long-type-") && name.endsWith(".txt"));

  let count = 0;
  let bytes = 0;
  for (const e of fs.readdirSync(deps, { withFileTypes: true })) {
    if (!e.isFile() || !isOrphan(e.name)) continue;
    const p = path.join(deps, e.name);
    let size = 0;
    try {
      size = fs.statSync(p).size;
    } catch {
      continue;
    }
    if (DRY) {
      count++;
      bytes += size;
      continue;
    }
    try {
      fs.unlinkSync(p);
      count++;
      bytes += size;
    } catch {
      /* 占用中/已删，跳过；不是致命问题 */
    }
  }
  return [count, bytes];
}

if (!fs.existsSync(TARGET)) {
  console.log("target/ 不存在（还没编译过），无需处理。");
  process.exit(0);
}

const before = dirSize(TARGET);
console.log(`target/ 当前占用：${human(before)}`);

console.log(`\n[1/2] cargo clean -p ${PKG}${DRY ? "（--dry，跳过）" : ""}`);
if (!DRY) {
  // 注意用 spawnSync 而不是 execFileSync：后者只回 stdout，而 cargo 的进度与
  // "Removed N files" 统计是走 **stderr** 的，用 execFileSync 会永远读不到。
  const r = spawnSync("cargo", ["clean", "-p", PKG, "--manifest-path", MANIFEST], {
    encoding: "utf8",
  });
  if (r.error) {
    console.error("  起不了 cargo（工具链没装好？）：", r.error.message);
    process.exit(1);
  }
  const tail = (r.stderr || r.stdout || "")
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean)
    .pop();
  if (tail) console.log("  " + tail);
  if (r.status !== 0) {
    console.error(`  cargo clean 退出码 ${r.status}，先手工跑一遍看完整报错。`);
    process.exit(1);
  }
}

console.log(`\n[2/2] 扫掉 deps 下的孤儿 .rcgu.o / long-type 转储`);
let files = 0;
let bytes = 0;
for (const profile of ["debug", "release"]) {
  const [n, b] = sweepOrphans(path.join(TARGET, profile));
  if (n) console.log(`  ${profile}: ${n} 个文件 / ${human(b)}`);
  files += n;
  bytes += b;
}
if (!files) console.log("  （无残留）");

const after = DRY ? before : dirSize(TARGET);
const freed = before - after;

console.log("\n" + "─".repeat(52));
if (DRY) {
  console.log(`干跑结束：将删除 ${files} 个孤儿文件 / ${human(bytes)}，并重置 ${PKG} 的编译产物。`);
  console.log("去掉 --dry 才会真正执行。");
} else {
  console.log(`回收 ${human(freed)}（${human(before)} → ${human(after)}）`);
  console.log(`下次 npm run tauri dev 会重编本包自身，约 2~4 分钟；依赖缓存已保留。`);
}
