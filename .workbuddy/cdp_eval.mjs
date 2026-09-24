// 零依赖 CDP 驱动：往 WebView2 (--remote-debugging-port=9222) 的指定页面里求值。
//
//   node cdp_eval.mjs list
//   node cdp_eval.mjs eval   "<js表达式>"  --url <url子串>
//   node cdp_eval.mjs evalfile <js文件>    --url <url子串>
//
// 表达式/文件内容会被包进 async IIFE，因此可以写 top-level await 和 return。
// 返回值统一 JSON.stringify 后打印，便于直接看结果。

const args = process.argv.slice(2);
const mode = args[0];

const urlIdx = args.indexOf("--url");
const urlFilter = urlIdx >= 0 ? args[urlIdx + 1] : "";
const payloadArgs = args.slice(1).filter((a, i) => {
  // 去掉 --url 及其值
  const absIdx = i + 1;
  return absIdx !== urlIdx && absIdx !== urlIdx + 1;
});
const payload = payloadArgs.join(" ");

const HOST = "127.0.0.1:9222";

async function targets() {
  const r = await fetch(`http://${HOST}/json/list`);
  return await r.json();
}

function pick(list, filter) {
  const pages = list.filter((t) => t.type === "page" && t.webSocketDebuggerUrl);
  if (!filter) return pages[0];

  // `~正则` 形式：用于挑「主窗口」这种没有唯一文件名的目标（如 http://localhost:1420/）。
  if (filter.startsWith("~")) {
    const re = new RegExp(filter.slice(1));
    const hit = pages.filter((t) => re.test(t.url || ""));
    if (!hit.length) {
      throw new Error(
        `没有匹配 /${filter.slice(1)}/ 的页面。现有：\n` +
          pages.map((p) => `  ${p.url}`).join("\n")
      );
    }
    return hit[0];
  }

  const base = (u) => {
    try {
      return new URL(u).pathname.split("/").pop() || "";
    } catch {
      return "";
    }
  };

  // ⛔ 先按「路径最后一段**精确相等**」匹配。
  //    用 includes 会让 `overlay.html` 同时命中 `edit_overlay.html`，
  //    而 edit_overlay 排在前面 —— 于是命令打进了「语音编辑」窗，
  //    读回来的是/写进去的都不是「字幕」窗的数据，测试结论全错且毫无提示。
  const exact = pages.filter((t) => base(t.url) === filter);
  if (exact.length) return exact[0];

  const hit = pages.filter((t) => (t.url || "").includes(filter));
  if (!hit.length) {
    throw new Error(
      `没有匹配 "${filter}" 的页面。现有：\n` +
        pages.map((p) => `  ${p.url}`).join("\n")
    );
  }
  if (hit.length > 1) {
    console.error(
      `⚠️ "${filter}" 模糊命中 ${hit.length} 个页面，取第一个：${hit[0].url}\n` +
        hit.map((p) => `     ${p.url}`).join("\n")
    );
  }
  return hit[0];
}

async function evaluate(target, expression, awaitPromise = true) {
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((res, rej) => {
    ws.addEventListener("open", res, { once: true });
    ws.addEventListener("error", (e) => rej(new Error("WS 连接失败: " + e.message)), {
      once: true,
    });
  });

  const id = 1;
  const done = new Promise((res, rej) => {
    const timer = setTimeout(() => rej(new Error("CDP 求值超时")), 30000);
    ws.addEventListener("message", (ev) => {
      let msg;
      try {
        msg = JSON.parse(ev.data);
      } catch {
        return;
      }
      if (msg.id !== id) return;
      clearTimeout(timer);
      if (msg.error) return rej(new Error("CDP 错误: " + JSON.stringify(msg.error)));
      res(msg.result);
    });
  });

  ws.send(
    JSON.stringify({
      id,
      method: "Runtime.evaluate",
      params: {
        expression,
        awaitPromise,
        returnByValue: true,
        userGesture: true,
      },
    })
  );

  const result = await done;
  ws.close();
  return result;
}

function fmt(result) {
  if (result.exceptionDetails) {
    return (
      "!! 页面内异常: " +
      (result.exceptionDetails.exception?.description ||
        result.exceptionDetails.text)
    );
  }
  const v = result.result?.value;
  if (typeof v === "string") return v;
  return JSON.stringify(v);
}

try {
  if (mode === "list") {
    const list = await targets();
    for (const t of list) {
      console.log(`${t.type}\t${t.title}\t${t.url}`);
    }
    process.exit(0);
  }

  let expr;
  if (mode === "eval") {
    expr = payload;
  } else if (mode === "evalfile") {
    const { readFile } = await import("node:fs/promises");
    expr = await readFile(payload, "utf8");
  } else {
    console.error("用法: node cdp_eval.mjs list | eval <expr> | evalfile <file> [--url <子串>]");
    process.exit(2);
  }

  const list = await targets();
  const target = pick(list, urlFilter);
  const wrapped = `(async () => {\n${expr}\n})()`;
  const result = await evaluate(target, wrapped);
  console.log(fmt(result));
} catch (err) {
  console.error("FAIL: " + err.message);
  process.exit(1);
}
