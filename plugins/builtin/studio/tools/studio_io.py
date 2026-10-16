#!/usr/bin/env python3
# -*- coding: utf-8 -*-
r"""内容台 ↔ 磁盘的窄腰（studio_io）。

## 为什么需要它

easyop 的插件走 `eo.*` 能力集，**当前没有「写文件」能力**（`eo.storage` 只往
`plugins/<id>/storage.json` 里写 KV）。而内容台必须把稿件落成 `.md` 才能交给
`pub.py check` / `pub.py pack`。于是把「读台账 → 组稿 → 写盘」这一小段放在
Python 侧：插件用 `eo.run` 调本脚本，脚本从已知位置读 `storage.json`。

> 若将来 `eo.*` 补上 `fs.write` / `paths.plugin_dir`，「组稿」可以搬回 JS，
> 本脚本即可退役。这是**一期妥协**，不是长期设计。

## 约定

- 台账文件：`%APPDATA%\com.easyop.app\plugins\<plugin>\storage.json`
  形如 `{"topics":[...],"meta":{...}}`（`eo.storage` 的实现细节）。
- 成功时 stdout 打印一行 `POST=<绝对路径>`，供插件解析。
- **纯标准库**——不能因为没装 playwright 就调不动（`pub.py` 才需要 playwright）。

## 用法

    python studio_io.py compose --topic-id <id> --slug <slug> [--out-dir <dir>]
    python studio_io.py list
    python studio_io.py show --topic-id <id>
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from datetime import datetime
from pathlib import Path

DEFAULT_PLUGIN = "studio"
DEFAULT_OUT_DIR = Path(os.environ.get("EASYOP_DRAFTS_DIR") or str(Path.home() / "Documents" / "easyop-drafts"))


def storage_path(plugin: str, override: str | None) -> Path:
    if override:
        return Path(override).expanduser().resolve()
    appdata = os.environ.get("APPDATA")
    if not appdata:
        raise SystemExit("找不到 %APPDATA%，无法定位 storage.json（可用 --storage 显式指定）")
    return Path(appdata) / "com.easyop.app" / "plugins" / plugin / "storage.json"


def load_store(path: Path) -> dict:
    if not path.is_file():
        raise SystemExit(f"台账不存在：{path}\n（先在 easyop 的内容台里建一条选题）")
    raw = path.read_text(encoding="utf-8")
    try:
        data = json.loads(raw)
    except json.JSONDecodeError as e:
        raise SystemExit(f"台账不是合法 JSON：{path}\n{e}")
    if not isinstance(data, dict):
        raise SystemExit(f"台账结构异常（应为对象）：{path}")
    return data


def topics_of(store: dict) -> list[dict]:
    topics = store.get("topics")
    if topics is None:
        return []
    if not isinstance(topics, list):
        raise SystemExit("台账里的 topics 不是数组，可能被外部改坏了")
    return [t for t in topics if isinstance(t, dict)]


def pick_topic(topics: list[dict], topic_id: str | None, meta: dict) -> dict:
    if not topics:
        raise SystemExit("台账里还没有选题")
    if topic_id:
        for t in topics:
            if t.get("id") == topic_id:
                return t
        raise SystemExit(f"找不到选题 id={topic_id}（现有：{[t.get('id') for t in topics]}）")
    cur = meta.get("currentId")
    for t in topics:
        if t.get("id") == cur:
            return t
    return topics[0]


def body_of(topic: dict) -> str:
    for key in ("humanized", "body"):
        v = (topic.get(key) or "").strip()
        if v:
            return v
    return ""


def render_md(topic: dict) -> str:
    """把一条选题渲染成交付用的 Markdown。"""
    title = (topic.get("chosenTitle") or topic.get("title") or "未命名").strip()
    body = body_of(topic)
    if not body:
        raise SystemExit("这条选题还没有正文（先走「生成正文」）")

    parts: list[str] = [f"# {title}", "", body.strip(), ""]

    figures = topic.get("figures") or []
    if isinstance(figures, list) and figures:
        parts.append("## 配图")
        parts.append("")
        for i, f in enumerate(figures, 1):
            if isinstance(f, dict):
                alt = f.get("alt") or f.get("caption") or f"图{i}"
                src = f.get("path") or f.get("file") or ""
            else:
                alt, src = f"图{i}", str(f)
            parts.append(f"![{alt}]({src})")
        parts.append("")

    source = (topic.get("source") or "").strip()
    if source:
        parts.append(f"> 来源：{source}")
        parts.append("")

    return "\n".join(parts).rstrip() + "\n"


def cmd_compose(args: argparse.Namespace) -> int:
    path = storage_path(args.plugin, args.storage)
    store = load_store(path)
    topic = pick_topic(topics_of(store), args.topic_id, store.get("meta") or {})

    md = render_md(topic)
    out_dir = Path(args.out_dir) if args.out_dir else DEFAULT_OUT_DIR
    out_dir.mkdir(parents=True, exist_ok=True)

    base = args.slug or (topic.get("chosenTitle") or topic.get("title") or "untitled")
    safe = "".join(ch for ch in str(base) if ch not in '\\/:*?"<>|').strip() or "untitled"
    out = out_dir / f"{safe[:60]}.md"
    out.write_text(md, encoding="utf-8")

    print(f"POST={out.resolve()}")
    print(f"字数（去空白）：{len(md.replace(chr(10), '').replace(' ', ''))}")
    print(f"阶段：{topic.get('stage')}")
    return 0


def cmd_list(args: argparse.Namespace) -> int:
    store = load_store(storage_path(args.plugin, args.storage))
    meta = store.get("meta") or {}
    topics = topics_of(store)
    if not topics:
        print("（台账为空）")
        return 0
    for t in topics:
        mark = "*" if t.get("id") == meta.get("currentId") else " "
        print(f"{mark} [{t.get('stage')}] {t.get('title') or '(无标题)'}  id={t.get('id')}")
    return 0


def cmd_show(args: argparse.Namespace) -> int:
    store = load_store(storage_path(args.plugin, args.storage))
    topic = pick_topic(topics_of(store), args.topic_id, store.get("meta") or {})
    print(json.dumps(topic, ensure_ascii=False, indent=2))
    return 0


def cmd_write_html(args: argparse.Namespace) -> int:
    """把排版好的 HTML 写成公众号本地投放包（登录态不可用时的降级产物）。"""
    html = args.html
    if not html:
        raise SystemExit("write-html 需要 --html 传入 HTML 内容")
    title = args.title or "未命名"
    slug = args.slug or "untitled"
    safe = "".join(ch for ch in str(slug) if ch not in '\\/:*?"<>|').strip() or "untitled"

    out_dir = Path(args.out_dir) if args.out_dir else DEFAULT_OUT_DIR
    pkg_dir = out_dir / f"{safe[:60]}-wechat投放包"
    pkg_dir.mkdir(parents=True, exist_ok=True)

    (pkg_dir / "正文.html").write_text(html, encoding="utf-8")

    note = [
        "# 公众号手动投放说明",
        "",
        "## 步骤",
        "1. 用浏览器打开 `正文.html`",
        "2. 选中预览内容（跳过顶部黄色提示条）",
        "3. 复制（Ctrl+A 全选后 Ctrl+C，或鼠标选中内容区域）",
        "4. 打开微信公众号后台 → 新建图文",
        "5. 在正文编辑区粘贴（Ctrl+V）",
        "6. 检查排版，按需微调后发布",
        "",
        "## 稿件信息",
        f"- 标题：{title}",
        f"- 字数：{args.word_count or '?'}",
        f"- 生成时间：{datetime.now():%Y-%m-%d %H:%M}",
        "",
        "## 注意",
        "- 本投放包是 +公众号 的默认产物，用于复制粘贴到公众号后台",
        "- 如需自动填草稿（可选），请先在终端运行 `pub.py login wechat` 扫码登录后开启 autoDraft",
    ]
    (pkg_dir / "投放说明.md").write_text("\n".join(note) + "\n", encoding="utf-8")

    manifest = {
        "title": title,
        "wordCount": args.word_count,
        "htmlFile": "正文.html",
        "generatedAt": datetime.now().isoformat(),
    }
    (pkg_dir / "_manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")

    print(f"HTML={pkg_dir / '正文.html'}")
    print(f"投放包={pkg_dir}")
    return 0


def cmd_write_html_file(args: argparse.Namespace) -> int:
    """只落一个 HTML 文件（平台发布专用）。

    与 write-html 的区别：不建投放包目录，不写投放说明 / 正文.txt / _manifest.json
    等任何附加文件——产物就是一个可直接用浏览器打开、全选复制粘贴到公众号的 HTML。
    """
    html = args.html
    if not html:
        raise SystemExit("write-html-file 需要 --html 传入 HTML 内容")

    out_dir = Path(args.out_dir) if args.out_dir else DEFAULT_OUT_DIR
    out_dir.mkdir(parents=True, exist_ok=True)

    base = args.slug or args.title or "untitled"
    safe = "".join(ch for ch in str(base) if ch not in '\\/:*?"<>|').strip() or "untitled"
    fp = out_dir / f"{safe[:60]}.html"
    fp.write_text(html, encoding="utf-8")

    print(f"HTML={fp.resolve()}")
    return 0


def cmd_read_layout_guide(args: argparse.Namespace) -> int:
    """读取公众号排版规范（wechat-writer-pro 的 layout-guide.md）并打印到 stdout。

    找不到时以非零退出，让插件侧优雅降级到内置兜底提示词。
    """
    import os as _os

    roots = [_os.environ.get("USERPROFILE"), _os.environ.get("HOME")]
    candidates: list[Path] = []
    # 允许显式覆盖（测试 / 换机器用）
    override = _os.environ.get("LAYOUT_GUIDE_PATH")
    if override:
        candidates.append(Path(override).expanduser())
    for r in roots:
        if not r:
            continue
        candidates.append(
            Path(r).expanduser()
            / ".workbuddy"
            / "skills"
            / "wechat-writer-pro"
            / "references"
            / "layout-guide.md"
        )

    for c in candidates:
        if c.is_file():
            text = c.read_text(encoding="utf-8").strip()
            if text:
                print(text)
                return 0
    raise SystemExit(
        "找不到公众号排版规范 layout-guide.md（已搜索："
        + "；".join(str(c) for c in candidates[:2])
        + "）"
    )


# ---------------------------------------------------------------------------
# 本地 markdown → 内联样式 HTML（LLM 排版失败时的兜底）
# ---------------------------------------------------------------------------
# 为什么要有它：公众号排版的「锦上添花」依赖 eo.chat（火山方舟）。当代理抖动 /
# key 失效 / 网络不通时，eo.chat 会 error sending request，整条 +公众号 链路直接死。
# 但「产出可复制粘贴的 HTML」是用户刚需，不该绑死在外部 LLM 上。于是这里提供
# 一套零依赖的本地转换器：即使 LLM 全挂，也能产出可读、可粘贴的公众号 HTML。
# 样式严格对齐 wechat-writer-pro/layout-guide.md 的核心约束：
#   移动端优先 / 正文深灰 #333333（不用纯黑 #000）/ 系统字体（不强制苹方，用通用 sans 回退链）
#   / 章节蓝色强调（#3b82f6）/ 图片全宽带圆角。

_SHELL_BODY_STYLE = 'max-width:680px;margin:0 auto;padding:16px 18px;background:#fff;min-height:100vh'
_BLOCK_CENTER = 'margin:0 0 16px;text-align:justify'
_P_STYLE = 'color:#333333;font-size:16px;line-height:1.75;letter-spacing:0.3px;' + _BLOCK_CENTER
_H1_STYLE = 'color:#1a1a1a;font-size:22px;font-weight:700;text-align:center;margin:24px 0 16px;line-height:1.4'
_H2_STYLE = 'color:#1a1a1a;font-size:18px;font-weight:700;margin:24px 0 10px;padding-left:10px;border-left:4px solid #3b82f6;line-height:1.4'
_H3_STYLE = 'color:#1a1a1a;font-size:16px;font-weight:700;margin:18px 0 8px'
_QUOTE_STYLE = 'margin:0 0 16px;padding:10px 14px;background:#f7f8fa;border-left:4px solid #ddd;color:#666;font-size:15px;border-radius:4px'
_IMG_STYLE = 'display:block;width:100%;max-width:100%;border-radius:6px;margin:8px 0 16px'
_LIST_STYLE = 'margin:0 0 16px;padding-left:1.4em;color:#333333;font-size:16px;line-height:1.75'
_LINK_STYLE = 'color:#3b82f6;text-decoration:none'
_CODE_STYLE = 'background:#f2f3f5;padding:2px 5px;border-radius:3px;font-size:14px;font-family:monospace'


def _inline(text: str) -> str:
    '''行内格式：**粗体** / *斜体* / `code` / [text](url) / ![alt](src)。'''
    text = text.replace('&', '&amp;').replace('<', '&lt;').replace('>', '&gt;')
    # 图片（先于链接，避免被链接规则误吞）
    text = re.sub(
        r'!\[([^\]]*)\]\(([^)]+)\)',
        lambda m: '<img src="' + m.group(2) + '" alt="' + m.group(1) + '" style="' + _IMG_STYLE + '">',
        text,
    )
    # 链接
    text = re.sub(
        r'(?<!!)\[([^\]]+)\]\(([^)]+)\)',
        lambda m: '<a href="' + m.group(2) + '" style="' + _LINK_STYLE + '">' + m.group(1) + '</a>',
        text,
    )
    # 行内 code
    text = re.sub(
        r'`([^`]+)`',
        lambda m: '<code style="' + _CODE_STYLE + '">' + m.group(1) + '</code>',
        text,
    )
    # 粗体 → 深色加粗（不抢戏，用 #1a1a1a 而非亮蓝）
    text = re.sub(
        r'\*\*(.+?)\*\*',
        lambda m: '<strong style="color:#1a1a1a;font-weight:700">' + m.group(1) + '</strong>',
        text,
    )
    # 斜体
    text = re.sub(r'\*(.+?)\*', lambda m: '<em style="font-style:italic">' + m.group(1) + '</em>', text)
    return text


def _extract_title(md: str) -> str:
    for line in md.split('\n'):
        m = re.match(r'^#\s+(.+)$', line.strip())
        if m:
            return m.group(1).strip()
    return '未命名'


def _html_shell(title: str, body: str) -> str:
    return (
        '<!DOCTYPE html>\n<html lang="zh-CN">\n<head>\n<meta charset="utf-8">\n'
        '<meta name="viewport" content="width=device-width,initial-scale=1">\n'
        '<title>' + title + '</title>\n</head>\n'
        '<body style="margin:0;background:#f5f5f5;font-family:-apple-system,BlinkMacSystemFont,Roboto,Helvetica,Arial,sans-serif">\n'
        '<div style="' + _SHELL_BODY_STYLE + '">\n' + body + '\n</div>\n</body>\n</html>'
    )


def markdown_to_html(md: str) -> str:
    '''零依赖：把 Markdown 转成全内联样式的公众号 HTML 文档。

    支持：#/##/### 标题、段落、有序/无序列表、引用、图片（独占行或行内）、
    分割线、链接、行内 code、粗体/斜体。不支持表格等复杂块（够用即可）。
    '''
    lines = md.split('\n')
    out = []
    n = len(lines)
    i = 0
    while i < n:
        line = lines[i].rstrip()
        stripped = line.strip()

        # 分割线
        if re.match(r'^-{3,}$', stripped):
            out.append('<hr style="border:none;border-top:1px solid #eee;margin:24px 0">')
            i += 1
            continue

        # 标题
        m = re.match(r'^(#{1,3})\s+(.*)$', line)
        if m:
            level = len(m.group(1))
            content = _inline(m.group(2).strip())
            style = {1: _H1_STYLE, 2: _H2_STYLE, 3: _H3_STYLE}[level]
            tag = 'h' + str(level)
            out.append('<' + tag + ' style="' + style + '">' + content + '</' + tag + '>')
            i += 1
            continue

        # 引用（连续 > ）
        if stripped.startswith('> '):
            quotes = []
            while i < n and lines[i].strip().startswith('> '):
                quotes.append(lines[i].strip()[2:])
                i += 1
            out.append('<blockquote style="' + _QUOTE_STYLE + '">' + _inline(' '.join(quotes)) + '</blockquote>')
            continue

        # 列表
        if re.match(r'^[-*]\s+', stripped) or re.match(r'^\d+\.\s+', stripped):
            ordered = bool(re.match(r'^\d+\.\s+', stripped))
            items = []
            while i < n:
                ml = lines[i].strip()
                mm = re.match(r'^([-*]|\d+\.)\s+(.*)$', ml)
                if not mm:
                    break
                items.append(mm.group(2))
                i += 1
            inner = ''.join('<li style="margin:4px 0">' + _inline(it) + '</li>' for it in items)
            tag = 'ol' if ordered else 'ul'
            out.append('<' + tag + ' style="' + _LIST_STYLE + '">' + inner + '</' + tag + '>')
            continue

        # 图片独占一行
        img_m = re.match(r'^!\[([^\]]*)\]\(([^)]+)\)$', stripped)
        if img_m:
            out.append('<img src="' + img_m.group(2) + '" alt="' + img_m.group(1) + '" style="' + _IMG_STYLE + '">')
            i += 1
            continue

        # 空行
        if not stripped:
            i += 1
            continue

        # 段落：累积到下一个块级元素或空行
        para = []
        while i < n:
            ll = lines[i].strip()
            if (
                not ll
                or re.match(r'^(#{1,3})\s+', ll)
                or ll.startswith('> ')
                or re.match(r'^-{3,}$', ll)
                or re.match(r'^([-*]|\d+\.)\s+', ll)
                or re.match(r'^!\[([^\]]*)\]\(([^)]+)\)$', ll)
            ):
                break
            para.append(ll)
            i += 1
        if para:
            out.append('<p style="' + _P_STYLE + '">' + _inline(' '.join(para)) + '</p>')

    return _html_shell(_extract_title(md), '\n'.join(out))


def cmd_markdown_to_html(args: argparse.Namespace) -> int:
    '''把选题/articles markdown 转成公众号 HTML（LLM 排版兜底，零依赖）。

    输入优先级：--topic-id（复用 render_md，含配图）> --in 文件 > stdin。
    输出：HTML 打印到 stdout；给了 --out 则同时写文件。
    '''
    if args.topic_id:
        store = load_store(storage_path(args.plugin, args.storage))
        topic = pick_topic(topics_of(store), args.topic_id, store.get('meta') or {})
        md = render_md(topic)
    elif args.in_file:
        md = Path(args.in_file).read_text(encoding='utf-8')
    else:
        md = sys.stdin.read()

    if not md.strip():
        raise SystemExit('没有可转换的内容（stdin 为空且未给 --topic-id / --in）')

    html = markdown_to_html(md)
    if args.out:
        out = Path(args.out)
        out.write_text(html, encoding='utf-8')
        print('HTML=' + str(out.resolve()))
    else:
        print(html)
    return 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description="内容台 ↔ 磁盘的窄腰（组稿 / 查看台账）")
    p.add_argument("--plugin", default=DEFAULT_PLUGIN, help="插件 id（默认 studio）")
    p.add_argument("--storage", default=None, help="直接指定 storage.json 路径（测试用）")
    sub = p.add_subparsers(dest="cmd", required=True)

    c = sub.add_parser("compose", help="把选题渲染成稿件 .md 并落盘")
    c.add_argument("--topic-id", default=None)
    c.add_argument("--slug", default=None, help="输出文件名（不含扩展名）")
    c.add_argument("--out-dir", default=None, help="输出目录（默认 content-publisher/drafts）")
    c.set_defaults(func=cmd_compose)

    l = sub.add_parser("list", help="列出台账选题")
    l.set_defaults(func=cmd_list)

    s = sub.add_parser("show", help="打印某条选题的 JSON")
    s.add_argument("--topic-id", default=None)
    s.set_defaults(func=cmd_show)

    w = sub.add_parser("write-html", help="把排版好的 HTML 写成公众号本地投放包")
    w.add_argument("--html", required=True, help="HTML 内容（完整文档，含外壳）")
    w.add_argument("--slug", default=None, help="输出目录名前缀")
    w.add_argument("--title", default=None, help="稿件标题")
    w.add_argument("--word-count", default=None, help="正文字数")
    w.add_argument("--out-dir", default=None, help="输出目录（默认 ~/Documents/easyop-drafts）")
    w.set_defaults(func=cmd_write_html)

    wf = sub.add_parser("write-html-file", help="只写一个 HTML 文件（平台发布专用，不产生任何附加文件）")
    wf.add_argument("--html", required=True, help="HTML 内容（完整文档，含外壳）")
    wf.add_argument("--slug", default=None, help="输出文件名（不含扩展名）")
    wf.add_argument("--title", default=None, help="稿件标题")
    wf.add_argument("--word-count", default=None, help="正文字数")
    wf.add_argument("--out-dir", default=None, help="输出目录（默认 ~/Documents/easyop-drafts）")
    wf.set_defaults(func=cmd_write_html_file)

    g = sub.add_parser("read-layout-guide", help="读取公众号排版规范（wechat-writer-pro/layout-guide.md）打印到 stdout")
    g.set_defaults(func=cmd_read_layout_guide)

    m = sub.add_parser("markdown-to-html", help="把 markdown（选题/文件/stdin）转成公众号 HTML（LLM 排版的本地兜底，零依赖）")
    m.add_argument("--topic-id", default=None, help="从台账读取该选题并复用 render_md 渲染（含配图）")
    m.add_argument("--in-file", default=None, help="从文件读取 markdown")
    m.add_argument("--out", default=None, help="同时把 HTML 写入该文件")
    m.set_defaults(func=cmd_markdown_to_html)

    return p


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return args.func(args)
    except SystemExit:
        raise
    except Exception as e:  # noqa: BLE001 - 顶层兜底：绝不把 traceback 甩给插件
        print(f"ERROR: {type(e).__name__}: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
