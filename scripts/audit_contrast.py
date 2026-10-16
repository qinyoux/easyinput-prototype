#!/usr/bin/env python3
"""
审计 styles.css 里所有「背景色 + 前景色」配对的实际对比度。

为什么需要这个脚本（而不是人眼看）：
「实心填充 + 文字压在上面」的组合，**可读性完全取决于两套主题下这两个值的相对亮度**。
项目里 `--accent-solid` 在两套主题下亮度相反（深色=亮橙 #ff6b35，亮色=深橙 #c2410c），
而 `--on-accent` 两边都是白字 —— 凡是复用 `--on-accent` 的规则，
**在深色主题下必然对比度不足**。这是肉眼扫不出来的规律性问题。

输出：每条规则 × 两个主题的实际 WCAG 对比度，低于 4.5:1 标 FAIL。
"""
import re
import sys
from pathlib import Path

CSS = Path(__file__).resolve().parent.parent / "src" / "styles.css"

# ---- WCAG 相对亮度 ----
def srgb_to_lin(c: float) -> float:
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4

def luminance(hexstr: str) -> float:
    h = hexstr.lstrip("#")
    if len(h) == 3:
        h = "".join(ch * 2 for ch in h)
    r, g, b = (int(h[i : i + 2], 16) for i in (0, 2, 4))
    return (
        0.2126 * srgb_to_lin(r / 255)
        + 0.7152 * srgb_to_lin(g / 255)
        + 0.0722 * srgb_to_lin(b / 255)
    )

def contrast(fg: str, bg: str) -> float:
    a, b = luminance(fg), luminance(bg)
    hi, lo = max(a, b), min(a, b)
    return (hi + 0.05) / (lo + 0.05)

# ---- 解析两个 [data-theme] 块的变量 ----
text = CSS.read_text(encoding="utf-8")

def strip_comments(s: str) -> str:
    return re.sub(r"/\*.*?\*/", " ", s, flags=re.S)

def theme_vars(marker: str) -> dict:
    m = re.search(rf'\[data-theme="{marker}"\]\s*\{{(.*?)\n\}}', text, re.S)
    if not m:
        return {}
    out = {}
    for line in m.group(1).splitlines():
        vm = re.match(r"\s*(--[a-z0-9-]+)\s*:\s*(#[0-9a-fA-F]{3,6})\s*;", line)
        if vm:
            out[vm.group(1)] = vm.group(2)
    return out

themes = {"dark": theme_vars("dark"), "light": theme_vars("light")}
# 未在某个块里显式定义 ⇒ 继承 :root
root = {}
mroot = re.search(r"\n:root\s*\{(.*?)\n\}", text, re.S)
if mroot:
    for line in mroot.group(1).splitlines():
        vm = re.match(r"\s*(--[a-z0-9-]+)\s*:\s*(#[0-9a-fA-F]{3,6})\s*;", line)
        if vm:
            root[vm.group(1)] = vm.group(2)

def resolve(val: str, tname: str) -> str | None:
    """把 `var(--x)` 解析成该主题下的实际 hex。`transparent` 之类复合值返回 None。"""
    val = val.strip()
    m = re.fullmatch(r"var\(\s*(--[a-z0-9-]+)\s*\)", val, re.I)
    if not m:
        return None  # 只处理单变量：渐变/calc/rgba 一律跳过
    name = m.group(1)
    t = themes[tname]
    if name in t:
        return t[name]
    return root.get(name)

# ---- 抽取规则块 ----
# 用**括号配对**扫描，而不是正则找 `{...}`：CSS 里到处是 @media 嵌套，
# 正则 `[^{}]*` 只会抓到最内层，导致顶层规则全部漏掉。
def iter_rules(src: str):
    """产出 (选择器, 声明块) —— 跳过 @media/@keyframes 的名字，保留其内部规则。"""
    i, n = 0, len(src)
    while i < n:
        if src[i] != "{":
            i += 1
            continue
        # 往回找这个 '{' 之前最近的选择器
        j = i - 1
        while j >= 0 and src[j] in " \t\r\n":
            j -= 1
        end = j
        while j >= 0 and src[j] not in "{};":
            j -= 1
        sel = " ".join(src[j + 1 : end + 1].split())
        # 配对找 '}'
        depth, k = 1, i + 1
        while k < n and depth:
            if src[k] == "{":
                depth += 1
            elif src[k] == "}":
                depth -= 1
            k += 1
        yield sel, src[i + 1 : k - 1]
        i = k

rules = []
for sel, body in iter_rules(text):
    # 注释里常含花括号，会把选择器解析得乱七八糟 —— 先把注释从文本里剥掉。
    sel = strip_comments(sel)
    body = strip_comments(body)
    if sel.startswith("@") or not sel or "{" in sel:
        continue
    bgm = re.search(r"(?<![\w-])background(?:-color)?\s*:\s*([^;]+);", body)
    if not bgm:
        continue
    bg = bgm.group(1).strip()
    cm = re.search(r"(?<![\w-])color\s*:\s*([^;]+);", body)
    if not cm:
        continue
    if "gradient" in bg:
        # 渐变也要查：取**两端中最浅的一端**算保守值。
        # 漏掉它 = 侧边栏 logo 那类"白字压浅橙 2.1:1"的问题会永远测不出来。
        gm = re.search(r"var\(\s*(--[a-z0-9-]+)\s*\)\s*,\s*var\(\s*(--[a-z0-9-]+)\s*\)", bg, re.I)
        if gm:
            rules.append((sel, (gm.group(1), gm.group(2)), cm.group(1).strip()))
        continue
    rules.append((sel, bg, cm.group(1).strip()))

# ---- 评估 ----
rows = []
skipped = []
for sel, bg, fg in rules:
    for tname in ("dark", "light"):
        f = resolve(fg, tname)
        if not f or not f.startswith("#"):
            skipped.append((sel, tname, fg, bg, None, None))
            continue
        # 渐变：取两端里对比度**更低**（更刺眼）的那个
        if isinstance(bg, tuple):
            ratios = []
            ok = True
            for var in bg:
                v = resolve(var, tname)
                if not v or not v.startswith("#"):
                    ok = False
                    break
                ratios.append((contrast(f, v), v))
            if not ok:
                skipped.append((sel, tname, fg, bg, None, None))
                continue
            ratio, bhex = min(ratios, key=lambda x: x[0])
            bg_label = "渐变:" + "/".join(v for _, v in ratios)
        else:
            b = resolve(bg, tname)
            if not b or not b.startswith("#"):
                skipped.append((sel, tname, fg, bg, None, None))
                continue
            ratio, bhex, bg_label = contrast(f, b), b, bg
        rows.append((sel, tname, fg, bg_label, f, bhex, ratio))

if not rows:
    print("⛔ 没有可评估的规则 —— 解析环节有问题，不是「没有对比度问题」。")
    print(f"   共抽到 {len(rules)} 条候选规则，全部因变量无法解析被跳过，前 8 条：")
    for s in skipped[:8]:
        print(f"   - {s[0][:50]:52} fg={s[2]:24} bg={s[3]:24} 解析得 fg={s[4]!r} bg={s[5]!r}")
    sys.exit(1)

print(f"{'选择器':46} {'主题':6} {'前景':26} {'背景':24} {'对比度':>7}")
print("-" * 118)
fails = {"dark": [], "light": []}
for sel, tname, fgraw, bgraw, f, b, ratio in sorted(rows, key=lambda r: r[6]):
    flag = "FAIL" if ratio < 4.5 else ("warn" if ratio < 7 else "ok")
    if ratio < 4.5:
        fails[tname].append((sel, fgraw, bgraw, ratio))
    mark = {"FAIL": "✗", "warn": "△", "ok": " "}[flag]
    sel_s = sel if len(sel) <= 44 else sel[:41] + "..."
    print(f"{mark}{sel_s:45} {tname:6} {fgraw:26} {bgraw:24} {ratio:6.2f}:1")

print()
print(f"（另跳过 {len(skipped)} 条无法单值解析的规则，含渐变背景与复合值）")

# ==========================================================================
# 第二类缺陷：特异度冲突 —— 「规则声明的组合」≠「实际生效的组合」
# ==========================================================================
# v0.41.12 的真实事故：`.desk-prefer-item` 声明了 `color: var(--on-hi)`，
# 但它是 `<label class="desk-prefer-item">` 且外层套着 `.field`，
# 于是 `.field label`（0,1,1）压过它（0,1,0），`--text-dim` 实际生效。
#
# ⚠️⚠️ **静态推断在这里连续失败了三次，不要再尝试**：
#   ① 只比"元素类型相同"        → 误报 11 组（`.desk-counters .warn strong`
#                                   与 `.desk-light-verdict strong` 要求不同祖先，
#                                   一个元素不可能同时在两处，本就不是冲突）
#   ② 要求祖先 class 是后代子集  → 永远 False，漏报（`.field` 与
#                                   `.desk-prefer-item` 互不包含，却确实同命中一个元素）
#   ③ 去 TSX 扫 400 字符窗口找祖先 → 仍漏报（`.field` 在窗口之外）
#
#   根因：**祖先关系是 DOM 属性，不是 CSS 属性**。CSS 选择器本身不含祖先信息，
#   任何"从 CSS 猜 DOM"的方案都会在复杂页面失效。
#   ⇒ 唯一可靠依据是**问浏览器要 getComputedStyle 的结果**。
#   所以这块改为运行时检查：scripts/check_computed.mjs（用项目已有的 CDP 通道），
#   它读的是浏览器真正算出的值，不存在推断。
#
# 本脚本只保留它**真正擅长**的那件事：按主题变量枚举"声明的组合"并算对比度。
# 两者的分工写在 README §配色自检里。

print()
print("=" * 72)
print("【第二类】特异度冲突：静态分析已放弃（见上方注释）")
print("  ⇒ 改由运行时检查：node scripts/check_computed.mjs")
print("     （读浏览器 getComputedStyle，不做 DOM 推断）")
print("=" * 60)
for tname in ("dark", "light"):
    uniq = sorted({f[0] for f in fails[tname]})
    print(f"[{tname}] 低于 4.5:1 的规则 {len(fails[tname])} 条，去重选择器 {len(uniq)} 个：")
    for s in uniq:
        print(f"   - {s}")
    print()

sys.exit(1 if fails["dark"] or fails["light"] else 0)
