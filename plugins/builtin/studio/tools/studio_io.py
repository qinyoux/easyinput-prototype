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
        "- 本投放包因公众号登录态未就绪而生成",
        "- 如需自动填草稿，请先在终端运行 `pub.py login wechat` 扫码登录",
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
