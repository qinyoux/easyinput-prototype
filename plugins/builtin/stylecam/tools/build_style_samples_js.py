#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""把 test/artifacts/style-samples/ 里的 1K 风格原图压缩成缩略图，内嵌为 src/samples.js。

产物：src/samples.js（export const STYLE_SAMPLES，data:image/webp;base64）
说明：插件入口是 blob ESM 单文件（相对路径不可用），样品图只能内嵌进 bundle。
      缩略图 240px / WebP q70，单张约 8-15KB，10 张约 +100-150KB。
用法：
  python tools/build_style_samples_js.py            # 全部 10 组
  python tools/build_style_samples_js.py --only ink-china
依赖 Pillow（含 WebP）。
"""
from __future__ import annotations

import argparse
import base64
import io
import sys
from datetime import datetime
from pathlib import Path

try:
    from PIL import Image
except ImportError:
    print("[FAIL] 需要 Pillow：python -m pip install pillow")
    sys.exit(2)

PLUGIN_DIR = Path(__file__).resolve().parent.parent
SRC_DIR = PLUGIN_DIR / "test" / "artifacts" / "style-samples"
OUT_FILE = PLUGIN_DIR / "src" / "samples.js"

STYLE_IDS = [
    "cyber-neon", "retro-film", "jp-fresh", "ink-china", "min-nordic",
    "vaporwave", "dark-gothic", "pop-art", "clay-3d", "mono-doc",
]


def latest_png(style_id: str) -> Path | None:
    hits = sorted(SRC_DIR.glob(f"*_{style_id}.png"))
    return hits[-1] if hits else None


def make_thumb(path: Path, width: int = 240, quality: int = 70) -> bytes:
    im = Image.open(path)
    im = im.convert("RGB")
    ratio = width / im.width
    im = im.resize((width, max(1, round(im.height * ratio))), Image.LANCZOS)
    buf = io.BytesIO()
    im.save(buf, "WEBP", quality=quality, method=6)
    return buf.getvalue()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", default="")
    ap.add_argument("--width", type=int, default=240)
    ap.add_argument("--quality", type=int, default=70)
    args = ap.parse_args()
    only = {s.strip() for s in args.only.split(",") if s.strip()}

    entries, total = [], 0
    for sid in STYLE_IDS:
        if only and sid not in only:
            continue
        png = latest_png(sid)
        if not png:
            print(f"[SKIP] {sid}：没有原图（{SRC_DIR}/*_{sid}.png）")
            continue
        webp = make_thumb(png, args.width, args.quality)
        b64 = base64.b64encode(webp).decode("ascii")
        total += len(webp)
        entries.append((sid, b64))
        print(f"[OK ] {sid}  {png.name}  webp={len(webp) / 1024:.1f}KB")

    if not entries:
        print("[FAIL] 没有任何可用原图")
        return 2

    lines = [
        "/**",
        " * 风格样品缩略图 —— 由 tools/build_style_samples_js.py 自动生成，勿手改。",
        " *",
        " * 数据来源：猫型轮足机器人形象.jpg 作为基础素材，",
        " * 10 组风格各按 strength=100（大改风格档）经 EasySU gpt-image-2.5-flare 图生图，",
        f" * 1K 原图在 test/artifacts/style-samples/；此处为 {args.width}px WebP 内嵌缩略图。",
        f" * 生成时间：{datetime.now().strftime('%Y-%m-%d %H:%M')}",
        " */",
        "",
        "export const STYLE_SAMPLES = {",
    ]
    for sid, b64 in entries:
        lines.append(f"  '{sid}': 'data:image/webp;base64,{b64}',")
    lines += ["};", ""]
    OUT_FILE.write_text("\n".join(lines), encoding="utf-8")
    print(f"[DONE] {len(entries)} 组  原始 webp 合计 {total / 1024:.1f}KB  ->  {OUT_FILE.name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
