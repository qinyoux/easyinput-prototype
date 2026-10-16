#!/usr/bin/env python3
"""
读浏览器 **getComputedStyle 的实际结果**，验证"某个选择器最终生效的颜色"。

# 为什么需要它

`scripts/audit_contrast.py` 算的是"**规则声明的**背景色 + 文字色"。
这在规则赢了的前提下是准的，但存在第二类缺陷：
**规则声明得完全正确，却根本没赢**。

v0.41.12 的真实事故：

    .desk-prefer-item { color: var(--on-hi) }      ← 声明正确
    .field label      { color: var(--text-dim) }   ← 特异度 0,1,1，实际赢

那个元素是 `<label class="desk-prefer-item">` 且外层套着 `.field`，
于是文字实际是 `--text-dim` 压在橙底上，几乎看不见 ——
而静态审计**报"0 问题"**，因为它读的正是那条"声明正确"的规则。

⚠️ 静态推断在这里连续失败过三次（详见 audit_contrast.py 里的记录）：
① 只比"元素类型相同"  → 误报 11 组
② 要求祖先 class 是后代子集 → 永远 False，漏报
③ 去 TSX 扫固定字符窗口找祖先 → 仍漏报
根因：**祖先关系属于 DOM，不属于 CSS**，选择器文本里没有祖先信息。
⇒ 唯一可靠依据是问浏览器。

# 用法

    # 起应用时带调试端口
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222" \
      src-tauri/target/debug/easyop.exe

    # 探全部文字元素
    python scripts/probe_computed.py

    # 只探某个选择器（最常用：验证某个修复是否真的生效）
    python scripts/probe_computed.py --sel ".desk-prefer-item"

# 退出码

    0 = 全部达标；1 = 有 < 4.5:1 的项；2 = 连不上 CDP

# ⚠️ 已知局限（勿当成完整覆盖）

探针只覆盖**当前页面当前可见**的元素。要覆盖全应用需逐页导航，
本版没做自动化遍历。它是"定点复查工具"，不是"全站扫描器"。

一个刻意保留的行为：`bgOf()` **优先用元素自身的背景**，只有自身透明才往父级找。
反过来（一律跳过自身）会让所有按钮类元素丢掉真正的底色，误报一片 1.00:1。
"""
import json
import sys
import urllib.request

try:
    import websocket
except ImportError:
    print("需要 websocket-client：pip install websocket-client", file=sys.stderr)
    sys.exit(2)

HOST = "127.0.0.1:9222"
MAIN = "http://localhost:1420/"

# 在页面里跑的探针。%s 处填 JSON 编码的选择器（null = 全部）。
PROBE = r"""
(() => {
  const lum = (rgb) => {
    const [r,g,b] = rgb;
    const f = (c) => { c/=255; return c<=0.04045 ? c/12.92 : Math.pow((c+0.055)/1.055, 2.4); };
    return 0.2126*f(r) + 0.7152*f(g) + 0.0722*f(b);
  };
  const parse = (s) => {
    const m = String(s).match(/rgba?\(([^)]+)\)/);
    if (!m) return null;
    const p = m[1].split(",").map(x => parseFloat(x));
    if (p.length < 3) return null;
    if (p.length === 4 && p[3] === 0) return null;   // 全透明
    return [p[0], p[1], p[2]];
  };
  const cr = (a,b) => {
    const x = lum(a), y = lum(b);
    return (Math.max(x,y)+0.05)/(Math.min(x,y)+0.05);
  };
  // 自身有背景就用自身的（它就是那个色块，文字压在它上面），
  // 自身透明才往父级找。一律跳过自身会让按钮类元素误报 1.00:1。
  const bgOf = (el) => {
    const own = parse(getComputedStyle(el).backgroundColor);
    if (own) return own;
    let n = el.parentElement;
    while (n && n !== document.documentElement) {
      const c = parse(getComputedStyle(n).backgroundColor);
      if (c) return c;
      n = n.parentElement;
    }
    return parse(getComputedStyle(document.body).backgroundColor) || [255,255,255];
  };
  const out = [];
  for (const el of document.querySelectorAll(%s)) {
    const txt = Array.from(el.childNodes)
      .filter(n => n.nodeType === 3)
      .map(n => n.textContent.trim()).join("");
    if (!txt) continue;
    const st = getComputedStyle(el);
    if (st.visibility === "hidden" || st.display === "none") continue;
    if (parseFloat(st.opacity) < 0.15) continue;
    const fg = parse(st.color);
    if (!fg) continue;
    const bg = bgOf(el);
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) continue;
    out.push({
      sel: el.tagName.toLowerCase() + (typeof el.className === "string" && el.className.trim()
              ? "." + el.className.trim().split(/\s+/).join(".") : ""),
      text: txt.slice(0, 20),
      fg: st.color, bg: `rgb(${bg.join(",")})`,
      ratio: Math.round(cr(fg, bg) * 100) / 100,
    });
  }
  return out;
})()
"""


def pick_target():
    with urllib.request.urlopen(f"http://{HOST}/json/list", timeout=5) as r:
        pages = [p for p in json.load(r) if p.get("type") == "page"]
    for p in pages:
        if p.get("url") == MAIN:
            return p
    for p in pages:
        if p.get("url", "").startswith(MAIN):
            return p
    raise SystemExit("找不到主窗口页面（应用是否带 --remote-debugging-port=9222 启动？）")


def evaluate(ws, expr):
    ws.send(json.dumps({
        "id": 1,
        "method": "Runtime.evaluate",
        "params": {"expression": expr, "returnByValue": True, "awaitPromise": True},
    }))
    while True:
        msg = json.loads(ws.recv())
        if msg.get("id") != 1:
            continue
        if "error" in msg:
            raise SystemExit("CDP 错误: " + json.dumps(msg["error"]))
        res = msg.get("result", {})
        if "exceptionDetails" in res:
            raise SystemExit("页面异常: " + json.dumps(res["exceptionDetails"])[:400])
        return res.get("result", {}).get("value")


def main():
    sel = None
    if "--sel" in sys.argv:
        sel = sys.argv[sys.argv.index("--sel") + 1]
    try:
        tgt = pick_target()
    except SystemExit as e:
        print(f"⛔ {e}", file=sys.stderr)
        sys.exit(2)
    # WebView2/Chromium 拒绝"非浏览器来源"的 CDP WebSocket。
    # 绕法：suppress_origin=True（不发 Origin 头），免去重启应用。
    ws = websocket.create_connection(
        tgt["webSocketDebuggerUrl"], timeout=20, suppress_origin=True
    )
    try:
        val = evaluate(ws, PROBE % json.dumps(sel) if sel else "*")
    finally:
        ws.close()

    if not isinstance(val, list) or not val:
        print("未取到元素（当前页面可能没有可见文字，或该选择器没命中）")
        return 0
    bad = [x for x in val if x["ratio"] < 4.5]
    warn = [x for x in val if 4.5 <= x["ratio"] < 7]
    scope = f"{sel} " if sel else ""
    print(f"\n运行时配色自检 · {scope}采样 {len(val)} 个文字元素（浏览器实际计算值）")
    print(f"  ✅ ≥7:1    {len(val) - len(bad) - len(warn)}")
    print(f"  △ 4.5–7:1 {len(warn)}")
    print(f"  ✗ <4.5:1  {len(bad)}")
    if bad:
        print("\n不达标明细：")
        for b in sorted(bad, key=lambda x: x["ratio"])[:25]:
            print(f"  ✗ {b['ratio']:6.2f}:1  {b['sel'][:42]:44} fg={b['fg']:20} bg={b['bg']:20} \"{b['text']}\"")
        print("\n  ⇒ 这是浏览器**算出来的**颜色，不是 CSS 声明值。")
        print("    若声明是对的但结果不对 ⇒ 有更高特异度的规则覆盖了它。")
        sys.exit(1)
    print("\n✅ 全部达标\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
