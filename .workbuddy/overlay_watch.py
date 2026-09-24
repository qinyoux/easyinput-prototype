"""只读观察器：盯住「easyop 字幕」窗口的可见性，一旦可见就截图。

不点鼠标、不抢焦点、不发按键 —— 纯粹观察 + 截浮层那一小块（含少量周边上下文）。

用法: python overlay_watch.py <输出目录> <观察秒数>
"""
import ctypes
import ctypes.wintypes as wt
import os
import sys
import time

user32 = ctypes.windll.user32
user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))

from PIL import ImageGrab

OUT = sys.argv[1] if len(sys.argv) > 1 else "."
SECS = float(sys.argv[2]) if len(sys.argv) > 2 else 180.0
os.makedirs(OUT, exist_ok=True)
LOG = os.path.join(OUT, "timeline.txt")
PAD = 200
MAX_SHOTS = 40

WNDENUMPROC = ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)


def overlay_info():
    out = []

    def cb(hwnd, _):
        buf = ctypes.create_unicode_buffer(512)
        user32.GetWindowTextW(hwnd, buf, 512)
        if buf.value == "easyop 字幕":
            r = wt.RECT()
            user32.GetWindowRect(hwnd, ctypes.byref(r))
            out.append((hwnd, bool(user32.IsWindowVisible(hwnd)),
                        (r.left, r.top, r.right, r.bottom)))
        return True

    user32.EnumWindows(WNDENUMPROC(cb), 0)
    return out[0] if out else None


def log(line):
    with open(LOG, "a", encoding="utf-8") as f:
        f.write(f"[{time.strftime('%H:%M:%S')}.{int(time.time()*1000)%1000:03d}] {line}\n")


def shot(name, rect):
    l, t, r, b = rect
    box = (max(0, l - PAD), max(0, t - PAD), r + PAD, b + PAD)
    p = os.path.join(OUT, name)
    ImageGrab.grab(bbox=box, all_screens=True).save(p)
    return p


log(f"=== 开始观察 {SECS:.0f}s（只读，不点击） ===")
info = overlay_info()
log(f"初始：{'找到浮层' if info else '未找到浮层'} " +
    (f"visible={info[1]} rect={info[2]}" if info else ""))

t0 = time.time()
last_vis = info[1] if info else None
episode = 0
shots = 0
while time.time() - t0 < SECS:
    info = overlay_info()
    if not info:
        time.sleep(0.08)
        continue
    hwnd, vis, rect = info
    if vis != last_vis:
        episode += 1
        log(f"★ 可见性变化 → visible={vis} rect={rect}")
        if vis and shots < MAX_SHOTS:
            p = shot(f"vis_{episode:02d}_{shots:02d}.png", rect)
            shots += 1
            log(f"   已截图 {p}")
        last_vis = vis
    elif vis and shots < MAX_SHOTS:
        # 可见期间持续取证（每 ~0.35s 一张），捕捉状态变化
        if int(time.time() * 1000) % 350 < 90:
            p = shot(f"vis_{episode:02d}_{shots:02d}.png", rect)
            shots += 1
            log(f"   可见中截图 {p}")
    time.sleep(0.08)

log(f"=== 观察结束，共截 {shots} 张 ===")
print(f"DONE shots={shots}")
