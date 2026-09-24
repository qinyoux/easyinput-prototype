"""判别矩阵：在真 mintty 里逐个试不同的发键组合，每次截屏读回。

已验证的前提：截屏回读有效（普通字母 x 确实出现在提示符后 ⇒ 注入管子是通的）。

要辨别的问题：
  T1 Ctrl+V 且 **scan=0**  ⇒ mintty 有专门的"剪贴板历史注入 Ctrl+V"兼容分支
                             （源码 wininput.c:2756 `key=='V' && mods==MDK_CTRL && !scancode`），
                             这条能通过就说明：剪贴板是对的、Ctrl 修饰键能被识别、win_paste 是好的。
  T2 Shift+Insert scan=0   ⇒ 修复前的发法（现场失败的组合）
  T3 左Shift(0xA0)+Insert  ⇒ 探"泛用 VK_SHIFT(0x10) 的注入会不会让 GetKeyState(VK_SHIFT) 看不到"
  T4 VK_SHIFT(0x10)+Insert 扫描码+E0 ⇒ 本次改动后的发法
  T5 Ctrl+V 且 scan≠0      ⇒ 看本次改动是否**破坏了** T1 那条兼容分支（回归检查）

# 前两版脚手架的教训（写在这里省得下次再踩）

- **v1：靠 pty 读回（`stty raw -echo` + `head`）** —— `stty` 返回 1 就是这个信号：
  `mintty -e bash -c ...` 起来的 bash，**stdin 不是 pty**（`[ -t 0 ]` = NO）。
  于是"打进去的字"既不回显也读不到，测出来"连普通字母都没到"，看起来像注入失败，
  **其实是我自己的脚手架坏了**。⛔ 判据失效时先怀疑脚手架，别急着怀疑被测对象。
- **v2：只补一个回车让规范模式交付一行** —— 管子这才通（字母 x 出现）。
  另发现：每步之间焦点可能被别的窗口抢走（用户就在机器前），
  必须**每一步发送前重新核对前台**，否则键会落到别人窗口里。
- **v3（本脚本）：截屏直读 + 每步激活复核** —— 抓窗口矩形画面，前台拿不到就先
  `SetForegroundWindow`、再"移动鼠标到窗口中心点一下"；每步 `SendInput` 前复核前台。

# 09-21 在本机 mintty 上跑出来的结论

| 发法 | 结果 |
|---|---|
| 普通字母 `x`（`wVk` + `wScan`） | ✅ 到达（证明注入管子是通的） |
| `Shift+Insert`（`wScan = 0`） | ❌ 什么都不发生 |
| `Shift+Insert`（真实扫描码 + `E0` 扩展位） | ❌ 什么都不发生（补扩展位无效） |
| **`Ctrl+V`（`wScan = 0`）** | ✅ **粘贴** ← easyop 现在对 mintty 用的就是这条 |
| `Ctrl+V`（真实扫描码 `0x2F`） | ❌ 什么都不发生（补扫描码**破坏了**兼容分支） |

⇒ `text_delivery::TERMINAL_PASTE_KEYS` 里 mintty 一项必须是 `CtrlV`，
且 `key_input` 的 `wScan` **必须留 0**。源码证据见 `text_delivery.rs` 文件头。

安全：只操作本脚本 spawn 的那个 mintty，结束杀掉进程树。
⛔ 会在屏幕上弹出测试终端窗口几秒 —— 跑之前先停手别动键鼠，否则焦点被抢会让这一步作废。
"""

import ctypes
import ctypes.wintypes as w
import os
import subprocess
import sys
import time

from PIL import Image

user32 = ctypes.WinDLL("user32", use_last_error=True)
gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

VK_SHIFT, VK_LSHIFT, VK_INSERT, VK_CONTROL, VK_V = 0x10, 0xA0, 0x2D, 0x11, 0x56
EXT, UP = 0x0001, 0x0002
INPUT_KEYBOARD, CF_UNICODETEXT, GMEM_MOVEABLE = 1, 13, 0x0002
SRCCOPY, DIB_RGB_COLORS = 0x00CC0020, 0
TITLE = "EASYOP-PASTE-E2E"
OUTDIR = r"E:\MY_AI\trae\ESP32_S3_BOT\.wb-tmp\matrix"

ULONG_PTR = ctypes.c_ulonglong if ctypes.sizeof(ctypes.c_void_p) == 8 else ctypes.c_ulong


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [("wVk", w.WORD), ("wScan", w.WORD), ("dwFlags", w.DWORD),
                ("time", w.DWORD), ("dwExtraInfo", ULONG_PTR)]


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [("dx", w.LONG), ("dy", w.LONG), ("mouseData", w.DWORD),
                ("dwFlags", w.DWORD), ("time", w.DWORD), ("dwExtraInfo", ULONG_PTR)]


class HARDWAREINPUT(ctypes.Structure):
    _fields_ = [("uMsg", w.DWORD), ("wParamL", w.WORD), ("wParamH", w.WORD)]


class _U(ctypes.Union):
    _fields_ = [("ki", KEYBDINPUT), ("mi", MOUSEINPUT), ("hi", HARDWAREINPUT)]


class INPUT(ctypes.Structure):
    _fields_ = [("type", w.DWORD), ("u", _U)]


class BMIH(ctypes.Structure):
    _fields_ = [("biSize", w.DWORD), ("biWidth", w.LONG), ("biHeight", w.LONG),
                ("biPlanes", w.WORD), ("biBitCount", w.WORD), ("biCompression", w.DWORD),
                ("biSizeImage", w.DWORD), ("biXPelsPerMeter", w.LONG),
                ("biYPelsPerMeter", w.LONG), ("biClrUsed", w.DWORD), ("biClrImportant", w.DWORD)]


user32.MapVirtualKeyW.restype = w.UINT
user32.MapVirtualKeyW.argtypes = [w.UINT, w.UINT]
kernel32.GlobalAlloc.restype = ctypes.c_void_p
kernel32.GlobalAlloc.argtypes = [w.UINT, ctypes.c_size_t]
kernel32.GlobalLock.restype = ctypes.c_void_p
kernel32.GlobalLock.argtypes = [ctypes.c_void_p]
kernel32.GlobalUnlock.argtypes = [ctypes.c_void_p]
user32.SetClipboardData.restype = ctypes.c_void_p
user32.SetClipboardData.argtypes = [w.UINT, ctypes.c_void_p]
user32.EmptyClipboard.restype = w.BOOL
user32.OpenClipboard.argtypes = [w.HWND]
user32.CloseClipboard.restype = w.BOOL
gdi32.CreateCompatibleDC.restype = ctypes.c_void_p
gdi32.CreateCompatibleDC.argtypes = [ctypes.c_void_p]
gdi32.CreateCompatibleBitmap.restype = ctypes.c_void_p
gdi32.CreateCompatibleBitmap.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
gdi32.SelectObject.restype = ctypes.c_void_p
gdi32.SelectObject.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
gdi32.BitBlt.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                         ctypes.c_int, ctypes.c_void_p, ctypes.c_int, ctypes.c_int, w.DWORD]
gdi32.GetDIBits.argtypes = [ctypes.c_void_p, ctypes.c_void_p, w.UINT, w.UINT,
                            ctypes.c_void_p, ctypes.POINTER(BMIH), w.UINT]
gdi32.GetDIBits.restype = ctypes.c_int
gdi32.DeleteObject.argtypes = [ctypes.c_void_p]
gdi32.DeleteDC.argtypes = [ctypes.c_void_p]
user32.GetDC.restype = ctypes.c_void_p
user32.GetDC.argtypes = [w.HWND]
user32.ReleaseDC.argtypes = [w.HWND, ctypes.c_void_p]


def scan_of(vk):
    return user32.MapVirtualKeyW(vk, 0)


def send(vk, scan, flags):
    arr = (INPUT * 1)()
    arr[0].type = INPUT_KEYBOARD
    arr[0].u.ki = KEYBDINPUT(wVk=vk, wScan=scan, dwFlags=flags, time=0, dwExtraInfo=0)
    return user32.SendInput(1, arr, ctypes.sizeof(INPUT))


def set_clipboard(text):
    if not user32.OpenClipboard(None):
        raise RuntimeError("打不开剪贴板")
    user32.EmptyClipboard()
    units = text.encode("utf-16-le") + b"\x00\x00"
    h = kernel32.GlobalAlloc(GMEM_MOVEABLE, len(units))
    ctypes.memmove(kernel32.GlobalLock(h), units, len(units))
    kernel32.GlobalUnlock(h)
    if not user32.SetClipboardData(CF_UNICODETEXT, h):
        raise RuntimeError("SetClipboardData 失败")
    user32.CloseClipboard()


def windows(title):
    found = []

    @ctypes.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
    def cb(hwnd, _):
        cls = ctypes.create_unicode_buffer(256)
        user32.GetClassNameW(hwnd, cls, 256)
        if cls.value == "mintty" and user32.IsWindowVisible(hwnd):
            txt = ctypes.create_unicode_buffer(512)
            user32.GetWindowTextW(hwnd, txt, 512)
            if txt.value == title:
                owner = w.DWORD()
                user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
                found.append((hwnd, owner.value))
        return True

    user32.EnumWindows(cb, 0)
    return found


def kill_tree(pid):
    subprocess.run(["taskkill", "/PID", str(pid), "/T", "/F"], capture_output=True)


def click_center(hwnd):
    r = w.RECT()
    if not user32.GetWindowRect(hwnd, ctypes.byref(r)):
        return False
    user32.SetCursorPos((r.left + r.right) // 2, (r.top + r.bottom) // 2)
    time.sleep(0.1)
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.05)
    user32.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(0.25)
    return user32.GetForegroundWindow() == hwnd


def activate(hwnd, tries=4):
    user32.AllowSetForegroundWindow(0xFFFFFFFF)
    for _ in range(tries):
        if user32.GetForegroundWindow() == hwnd:
            return True
        user32.ShowWindow(hwnd, 9)
        user32.SetForegroundWindow(hwnd)
        time.sleep(0.15)
        if user32.GetForegroundWindow() == hwnd:
            return True
        if click_center(hwnd):
            return True
    return False


def shot(hwnd, path):
    r = w.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    ww, hh = r.right - r.left, r.bottom - r.top
    src = user32.GetDC(None)
    mem = gdi32.CreateCompatibleDC(src)
    bmp = gdi32.CreateCompatibleBitmap(src, ww, hh)
    old = gdi32.SelectObject(mem, bmp)
    gdi32.BitBlt(mem, 0, 0, ww, hh, src, r.left, r.top, SRCCOPY)
    bi = BMIH()
    bi.biSize = ctypes.sizeof(BMIH)
    bi.biWidth, bi.biHeight = ww, -hh
    bi.biPlanes, bi.biBitCount, bi.biCompression = 1, 32, 0
    buf = ctypes.create_string_buffer(ww * hh * 4)
    gdi32.GetDIBits(mem, bmp, 0, hh, buf, ctypes.byref(bi), DIB_RGB_COLORS)
    gdi32.SelectObject(mem, old)
    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mem)
    user32.ReleaseDC(None, src)
    Image.frombuffer("RGBA", (ww, hh), buf, "raw", "BGRA", 0, 1).convert("RGB").save(path)


def main():
    os.makedirs(OUTDIR, exist_ok=True)
    for hwnd, pid in windows(TITLE):
        kill_tree(pid)
    time.sleep(0.3)
    before = {h for h, _ in windows(TITLE)}
    subprocess.Popen([r"C:\Program Files\Git\usr\bin\mintty.exe",
                      "-t", TITLE, "-e", "bash", "--norc", "-i"])
    hwnd = winpid = None
    for _ in range(40):
        for h, p in windows(TITLE):
            if h not in before:
                hwnd, winpid = h, p
                break
        if hwnd:
            break
        time.sleep(0.2)
    if not hwnd:
        print("✗ 没找到测试窗口")
        return 3
    time.sleep(1.2)
    if not activate(hwnd):
        kill_tree(winpid)
        print("✗ 拿不到前台")
        return 5
    print(f"窗口 0x{hwnd:x} pid={winpid} 前台就绪\n")

    sh, lsh, ins, ctl, v = (scan_of(VK_SHIFT), scan_of(VK_LSHIFT), scan_of(VK_INSERT),
                            scan_of(VK_CONTROL), scan_of(VK_V))
    print(f"扫描码 Shift=0x{sh:02X} LShift=0x{lsh:02X} Insert=0x{ins:02X} "
          f"Ctrl=0x{ctl:02X} V=0x{v:02X}\n")

    cases = [
        ("T1-CtrlV-scan0",
         "T1 Ctrl+V（scan=0，mintty 有专门兼容分支）",
         [(VK_CONTROL, 0, 0), (VK_V, 0, 0), (VK_V, 0, UP), (VK_CONTROL, 0, UP)],
         "MARK-T1-CTRLV-SCAN0"),
        ("T2-ShiftIns-scan0",
         "T2 Shift+Insert（scan=0，修复前 easyop 的发法）",
         [(VK_SHIFT, 0, 0), (VK_INSERT, 0, 0), (VK_INSERT, 0, UP), (VK_SHIFT, 0, UP)],
         "MARK-T2-SHIFTINS-SCAN0"),
        ("T3-LShift-Ins",
         "T3 左Shift(0xA0)+Insert（扫描码+扩展位）",
         [(VK_LSHIFT, lsh, 0), (VK_INSERT, ins, EXT), (VK_INSERT, ins, EXT | UP),
          (VK_LSHIFT, lsh, UP)],
         "MARK-T3-LSHIFT"),
        ("T4-Shift-Ins-ok",
         "T4 Shift(0x10)+Insert（扫描码+扩展位，本次改动后的发法）",
         [(VK_SHIFT, sh, 0), (VK_INSERT, ins, EXT), (VK_INSERT, ins, EXT | UP),
          (VK_SHIFT, sh, UP)],
         "MARK-T4-SHIFTINS-NEW"),
        ("T5-CtrlV-scan",
         "T5 Ctrl+V（scan=0x2F，检查本次改动是否破坏了 T1）",
         [(VK_CONTROL, ctl, 0), (VK_V, v, 0), (VK_V, v, UP), (VK_CONTROL, ctl, UP)],
         "MARK-T5-CTRLV-SCAN"),
    ]

    for tag, name, keys, marker in cases:
        if not activate(hwnd):
            print(f"  ✗ {name}：拿不到前台，跳过")
            continue
        set_clipboard(marker)
        time.sleep(0.2)
        if user32.GetForegroundWindow() != hwnd:
            print(f"  ✗ {name}：前台被抢走，跳过")
            continue
        for vk, sc, fl in keys:
            send(vk, sc, fl)
            time.sleep(0.03)
        time.sleep(1.0)
        p = os.path.join(OUTDIR, f"{tag}.png")
        shot(hwnd, p)
        print(f"  {name} → {p}")

    kill_tree(winpid)
    time.sleep(0.3)
    print("\n截图目录：", OUTDIR)
    return 0


if __name__ == "__main__":
    sys.exit(main())
