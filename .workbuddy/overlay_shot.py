"""自己触发一次真实语音流程，并在过程中截屏浮层，验证"浮层到底显示了没有"。

关键点：
- 必须先 SetProcessDpiAwarenessContext(-4)，否则 GetWindowRect 给逻辑坐标、
  ImageGrab 按物理采样，两者差 2 倍（本机 200%）。
- 修饰键之间留 40ms，否则合成按键会静默降级（见 windows-dpi-window-capture skill）。
- 只按两次（开始 / 结束）。过程中保持安静 ⇒ ASR 返回空 ⇒ 走"空结果"分支，
  不会把任何文本注入到前台窗口。
"""
import ctypes
import ctypes.wintypes as wt
import os
import sys
import time

user32 = ctypes.windll.user32
user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))  # 必须在取任何尺寸之前

try:
    from PIL import ImageGrab
except ImportError:
    print("NO_PILLOW")
    sys.exit(9)

OUT = sys.argv[1] if len(sys.argv) > 1 else "."
os.makedirs(OUT, exist_ok=True)

WNDENUMPROC = ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)
VK_CONTROL, VK_SHIFT, VK_SPACE = 0x11, 0x10, 0x20
KEYUP = 0x0002


def find_overlay():
    """返回 (hwnd, visible, rect) —— 只看标题是「easyop 字幕」的窗口。"""
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


def crop_shot(name, pad=90):
    """按浮层当前 rect 截一块（含周边上下文）。"""
    info = find_overlay()
    if not info:
        print(f"  [{name}] 找不到浮层窗口")
        return None
    hwnd, vis, (l, t, r, b) = info
    box = (max(0, l - pad), max(0, t - pad), r + pad, b + pad)
    p = os.path.join(OUT, name)
    ImageGrab.grab(bbox=box, all_screens=True).save(p)
    print(f"  [{name}] visible={vis} rect=({l},{t})-({r},{b}) 尺寸={r-l}x{b-t} → {p}")
    return p


def tap_combo():
    user32.keybd_event(VK_CONTROL, 0, 0, 0); time.sleep(0.04)
    user32.keybd_event(VK_SHIFT, 0, 0, 0); time.sleep(0.04)
    user32.keybd_event(VK_SPACE, 0, 0, 0); time.sleep(0.04)
    user32.keybd_event(VK_SPACE, 0, KEYUP, 0); time.sleep(0.04)
    user32.keybd_event(VK_SHIFT, 0, KEYUP, 0); time.sleep(0.04)
    user32.keybd_event(VK_CONTROL, 0, KEYUP, 0)


print("== 0. 触发前 ==")
crop_shot("00_before.png")

print("== 1. 发 Ctrl+Shift+Space（开始录音）==")
tap_combo()
time.sleep(0.5)
crop_shot("01_start_500ms.png")
time.sleep(0.7)
crop_shot("02_start_1200ms.png")

print("== 2. 再发一次（结束录音）==")
tap_combo()
time.sleep(0.4)
crop_shot("03_stop_400ms.png")
time.sleep(1.2)
crop_shot("04_stop_1600ms.png")
time.sleep(1.5)
crop_shot("05_stop_3100ms.png")

print("== 3. 收尾状态 ==")
info = find_overlay()
if info:
    print(f"  浮层最终：找到了 visible={info[1]} rect={info[2]}")
else:
    print("  浮层最终：不存在")
