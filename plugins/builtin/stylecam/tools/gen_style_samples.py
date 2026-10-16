#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""批量生成 10 组风格样品图（强度 100），用一张固定基础素材。

用途：给风格轨道卡片提供直观样品缩略图（samples.js 的数据来源）。
prompt 拼装与 src/logic.js buildPrompt(style, 100, >=1) 保持一致：
  <style.prompt>, style intensity: 大改风格 (100/100), keep the composition
  and subject of the main reference image, high quality, no text overlay, no watermark

用法（在插件目录下）：
  python tools/gen_style_samples.py --src <基础图片路径> [--only id1,id2]

产物：test/artifacts/style-samples/<时间戳>_<styleId>.png（1K 原图）
依赖：复用 stylecam_io.py，Key 从 tools/stylecam_io.env 读。
"""
from __future__ import annotations

import argparse
import base64
import json
import subprocess
import sys
import time
from pathlib import Path

PLUGIN_DIR = Path(__file__).resolve().parent.parent
IO_SCRIPT = PLUGIN_DIR / "tools" / "stylecam_io.py"
ENV_FILE = PLUGIN_DIR / "tools" / "stylecam_io.env"
OUT_DIR = PLUGIN_DIR / "test" / "artifacts" / "style-samples"
WORK_DIR = OUT_DIR / "_work"

# 与 src/constants.js STYLES 保持一致（id + prompt）
STYLES = [
    ("cyber-neon", "cyberpunk neon lighting, magenta and cyan rim light on deep blue-violet base, glowing shadows, rainy night cyber city, high contrast, cinematic"),
    ("retro-film", "kodak film look, warm yellow color cast, visible film grain, slightly faded highlights, 90s family photo album, soft contrast, nostalgic"),
    ("jp-fresh", "japanese fresh style, low contrast high key, soft teal green and sky blue, airy and translucent, j-drama summer mood, gentle diffused light"),
    ("ink-china", "chinese ink wash painting, monochrome black grey white, generous negative space, ink bleed on rice paper, xieyi freehand brushwork, elegant"),
    ("min-nordic", "minimalist nordic style, off-white light grey and natural wood tones, lots of white space, clean lines, interior magazine spread, calm and tidy"),
    ("vaporwave", "vaporwave aesthetic, pink purple gradient with cyan, glitch scanlines, retro grid and old computer UI, dreamy nostalgic 90s digital art"),
    ("dark-gothic", "dark gothic style, low key cold tone, strong chiaroscuro contrast, deep crimson and dark gold accents, classical oil painting in a cathedral, moody"),
    ("pop-art", "pop art style, high saturation clashing colors, halftone dot texture, bold black outlines, andy warhol screen print, flat graphic poster"),
    ("clay-3d", "3d clay render, rounded chunky clay texture, soft studio lighting, stop-motion figurine look, matte surface, cute and tactile"),
    ("mono-doc", "black and white documentary photography, heavy grain, hard light, strong structure and emotion, photojournalism, timeless monochrome"),
]

BAND_NAME = "maximum restyle"  # 与 logic.js buildPrompt 的英文档位词保持一致（中文名会被画进图里）
SUFFIX = "keep the composition and subject of the main reference image, high quality, no text overlay, no watermark"


def build_prompt(style_prompt: str) -> str:
    return f"{style_prompt}, style intensity: {BAND_NAME} (100/100), {SUFFIX}"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", required=True, help="基础素材图片路径")
    ap.add_argument("--only", default="", help="只生成这些 id（逗号分隔）")
    ap.add_argument("--resolution", default="1K")
    ap.add_argument("--http-timeout", type=int, default=240)
    ap.add_argument("--retry", type=int, default=1)
    args = ap.parse_args()

    src = Path(args.src).expanduser()
    if not src.is_file():
        print(f"[FAIL] 素材不存在：{src}")
        return 2
    only = {s.strip() for s in args.only.split(",") if s.strip()}
    todo = [x for x in STYLES if not only or x[0] in only]

    b64 = base64.b64encode(src.read_bytes()).decode("ascii")
    WORK_DIR.mkdir(parents=True, exist_ok=True)
    OUT_DIR.mkdir(parents=True, exist_ok=True)

    results = {}
    for sid, sp in todo:
        job = {
            "jobId": f"sample-{sid}",
            "styleId": sid,
            "strength": 100,
            "prompt": build_prompt(sp),
            "images": [{"b64": b64}],
        }
        storage = WORK_DIR / f"storage_{sid}.json"
        storage.write_text(json.dumps({"stylecam.job": job}), encoding="utf-8")
        cmd = [
            sys.executable, str(IO_SCRIPT), "generate",
            "--storage", str(storage),
            "--style", sid,
            "--strength", "100",
            "--resolution", args.resolution,
            "--out-dir", str(OUT_DIR),
            "--env", str(ENV_FILE),
            "--http-timeout", str(args.http_timeout),
            "--retry", str(args.retry),
        ]
        t0 = time.time()
        print(f"[GEN] {sid} …", flush=True)
        proc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=args.http_timeout * (args.retry + 1) + 120)
        elapsed = int(time.time() - t0)
        path_line = next((ln for ln in proc.stdout.splitlines() if ln.startswith("PATH=")), "")
        if proc.returncode == 0 and path_line:
            out_path = path_line.split("=", 1)[1]
            results[sid] = {"ok": True, "path": out_path, "secs": elapsed}
            print(f"[OK ] {sid}  {elapsed}s  {out_path}", flush=True)
        else:
            results[sid] = {"ok": False, "code": proc.returncode, "secs": elapsed}
            tail = (proc.stdout or "").strip().splitlines()[-3:]
            print(f"[ERR] {sid}  exit={proc.returncode}  {elapsed}s", flush=True)
            for ln in tail:
                print(f"      | {ln}", flush=True)
            for ln in (proc.stderr or "").strip().splitlines()[-3:]:
                print(f"      ! {ln}", flush=True)
        time.sleep(2)  # 轻微间隔，避免连续打点

    ok = sum(1 for r in results.values() if r["ok"])
    print(f"[DONE] {ok}/{len(results)} 成功")
    (WORK_DIR / "last_run.json").write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8")
    return 0 if ok == len(results) else 1


if __name__ == "__main__":
    sys.exit(main())
