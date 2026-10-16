#!/usr/bin/env node
// easyop 本地激活码生成器（作者专用）。私钥离线保存，绝不入库（已被 .gitignore 的 *.key 排除）。
//
// 用法：
//   node scripts/gen_license.mjs genkey                      生成密钥对，写 scripts/.license_private.key，打印公钥 hex
//   node scripts/gen_license.mjs pub                         打印当前公钥 hex（用于嵌入 Rust）
//   node scripts/gen_license.mjs sign <edition> [expISO] [note] [mid]
//                                                   生成激活码；edition 如 pro/trial，
//                                                   expISO 如 2027-12-31（留空=永不过期），
//                                                   mid 留空=不绑定机器。
//
// 协议（必须与 src-tauri/src/license.rs 完全一致）：
//   载荷 JSON 字段顺序固定为 v, edition, exp, note, mid。
//   签名 = Ed25519(载荷 JSON 的 UTF-8 字节, 私钥)。
//   激活码 = base64url(载荷) + "~" + base64url(签名)，无填充、无额外分隔符。

import { generateKeyPairSync, sign, createPrivateKey, createPublicKey } from "node:crypto";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const KEY_PATH = join(__dirname, ".license_private.key"); // gitignored (*.key)

function loadPrivate() {
  if (!existsSync(KEY_PATH)) {
    console.error("私钥不存在，请先运行：node scripts/gen_license.mjs genkey");
    process.exit(1);
  }
  const der = Buffer.from(readFileSync(KEY_PATH, "utf8").trim(), "hex");
  return createPrivateKey({ key: der, format: "der", type: "pkcs8" });
}

function genkey() {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const privDer = privateKey.export({ format: "der", type: "pkcs8" });
  const pubRaw = Buffer.from(publicKey.export({ format: "jwk" }).x, "base64url");
  writeFileSync(KEY_PATH, Buffer.from(privDer).toString("hex") + "\n");
  console.error("已写入私钥（gitignored）：" + KEY_PATH);
  console.error("请将以下公钥嵌入 src-tauri/src/license.rs 的 LICENSE_PUBLIC_KEY：");
  console.log(pubRaw.toString("hex"));
}

function pub() {
  const pubRaw = Buffer.from(createPublicKey(loadPrivate()).export({ format: "jwk" }).x, "base64url");
  console.log(pubRaw.toString("hex"));
}

function signCmd(edition, expISO, note, mid) {
  const priv = loadPrivate();
  const exp = expISO ? Date.parse(expISO) : null;
  if (expISO && (exp === null || Number.isNaN(exp))) {
    console.error("expISO 解析失败，示例：2027-12-31");
    process.exit(1);
  }
  const payload = { v: 1, edition: edition || "trial", exp, note: note || "", mid: mid || null };
  const msg = Buffer.from(JSON.stringify(payload), "utf8");
  const sig = sign(null, msg, priv);
  const code = msg.toString("base64url") + "~" + sig.toString("base64url");
  console.log(code);
}

const [cmd, ...args] = process.argv.slice(2);
switch (cmd) {
  case "genkey":
    genkey();
    break;
  case "pub":
    pub();
    break;
  case "sign":
    signCmd(args[0], args[1], args[2], args[3]);
    break;
  default:
    console.error("用法：genkey | pub | sign <edition> [expISO] [note] [mid]");
    process.exit(1);
}
