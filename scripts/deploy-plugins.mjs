#!/usr/bin/env node
/**
 * 把「内置插件源」装进 easyop 的用户插件目录。
 *
 * 为什么需要这一步：`plugin_registry::scan_dir` 目前**只扫一层**
 * （`app_config_dir()/plugins/<id>/plugin.json`），而内置插件按规范放在
 * `plugins/builtin/<id>/`。于是从源码到可用之间差一次拷贝。
 *
 * 用法：
 *   node scripts/deploy-plugins.mjs            # 装全部
 *   node scripts/deploy-plugins.mjs studio     # 只装某个
 *   node scripts/deploy-plugins.mjs --list     # 看有哪些、装在哪
 *   node scripts/deploy-plugins.mjs --dry      # 只演练，不落盘
 *
 * 零依赖，Node 原生。
 */

import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, cpSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = resolve(fileURLToPath(import.meta.url), "..");
const REPO = resolve(HERE, "..");
const SRC_ROOT = join(REPO, "plugins", "builtin");

/** easyop 的应用配置目录（与 Rust 侧 `app.path().app_config_dir()` 一致）。 */
function appConfigDir() {
  const appdata = process.env.APPDATA;
  if (!appdata) {
    console.error("✘ 找不到 %APPDATA%，无法定位 easyop 配置目录");
    process.exit(2);
  }
  // Tauri 用 identifier 作为目录名
  return join(appdata, "com.easyop.app");
}

function listBuiltin() {
  if (!existsSync(SRC_ROOT)) return [];
  return readdirSync(SRC_ROOT)
    .filter((n) => statSync(join(SRC_ROOT, n)).isDirectory())
    .filter((n) => !n.startsWith(".")) // 跳过 .workbuddy 等非插件目录
    .sort();
}

function assertManifest(dir) {
  const mf = join(dir, "plugin.json");
  if (!existsSync(mf)) return { ok: false, why: "缺 plugin.json" };
  let m;
  try {
    m = JSON.parse(readFile(mf));
  } catch (e) {
    return { ok: false, why: `plugin.json 不是合法 JSON：${e.message}` };
  }
  if (!m.id || !m.name || !m.main) return { ok: false, why: "plugin.json 缺 id / name / main" };
  const main = join(dir, m.main);
  if (!existsSync(main)) return { ok: false, why: `入口不存在：${m.main}` };
  return { ok: true, manifest: m };
}

function readFile(p) {
  return readFileSync(p, "utf8");
}

/** 开发期目录/文件，不进运行时（用户不需要，且会把插件目录撑大）。 */
const EXCLUDE = new Set(["test", "src", "node_modules", ".git", "__pycache__"]);

function copyDir(from, to) {
  mkdirSync(to, { recursive: true });
  cpSync(from, to, {
    recursive: true,
    filter: (src) => !EXCLUDE.has(src.split(/[\\/]/).pop()),
  });
}

function main() {
  const args = process.argv.slice(2);
  const dry = args.includes("--dry");
  const names = args.filter((a) => !a.startsWith("--"));
  const target = join(appConfigDir(), "plugins");

  const available = listBuiltin();
  if (args.includes("--list")) {
    console.log(`内置插件源：${SRC_ROOT}`);
    console.log(`安装目标　：${target}`);
    console.log("");
    for (const n of available) {
      const info = assertManifest(join(SRC_ROOT, n));
      console.log(`  ${info.ok ? "✔" : "✘"} ${n}${info.ok ? ` — ${info.manifest.name} v${info.manifest.version || "?"}` : ` — ${info.why}`}`);
    }
    return;
  }

  const targets = names.length ? names : available;
  if (!targets.length) {
    console.error(`✘ ${SRC_ROOT} 下没有插件`);
    process.exit(2);
  }

  // 坏插件在部署阶段就拦下——比装进去让 easyop 静默跳过好。
  let firstPageTitle = "";
  for (const n of targets) {
    const src = join(SRC_ROOT, n);
    if (!existsSync(src)) {
      console.error(`✘ 源不存在：${src}`);
      process.exit(2);
    }
    const info = assertManifest(src);
    if (!info.ok) {
      if (names.length) {
        // 显式点名装某个插件时保持严格：装错了要立刻报错。
        console.error(`✘ ${n} 校验失败：${info.why}`);
        process.exit(2);
      }
      // 全量部署时跳过非插件目录（如 .workbuddy / 备份目录），不拦整批。
      console.warn(`⚠ 跳过 ${n}：${info.why}`);
      continue;
    }
    const dst = join(target, n);
    if (dry) {
      console.log(`[dry] ${src} → ${dst}`);
      continue;
    }
    // 覆盖前先把旧版整体备份：内置插件会随开发迭代，而运行目录那份可能是
    // 用户/agent 直接改过的唯一副本（曾发生过 v0.41.5 覆盖丢功能的事故）。
    if (existsSync(join(dst, "plugin.json"))) {
      const stamp = new Date().toISOString().replace(/[-:.TZ]/g, "").slice(0, 14);
      const bak = `${dst}.bak.${stamp}`;
      try {
        cpSync(dst, bak, { recursive: true });
        console.log(`  已备份旧版 → ${bak}`);
      } catch (e) {
        console.error(`✘ 备份失败，中止部署以防覆盖丢失：${e.message}`);
        process.exit(3);
      }
    }
    copyDir(src, dst);
    console.log(`✔ ${n} → ${dst}  （${info.manifest.name} v${info.manifest.version || "?"}）`);
    // 记住第一个页签标题：结尾提示要说清"侧栏会出现哪个页签"，别写死某个插件的名字。
    if (!firstPageTitle) {
      firstPageTitle = info.manifest?.contributes?.pages?.[0]?.title || "";
    }
  }

  if (dry) return;
  console.log("");
  console.log(
    `下一步：重启 easyop（或在插件管理页点刷新 → plugins_rescan），侧栏出现「${firstPageTitle || "插件"}」页签。`,
  );
}

main();
