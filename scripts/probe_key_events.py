"""零副作用探针：观察 SendInput 生成的 Insert / Shift 事件在低级钩子里长什么样。

目的：mintty 按 **scan code** 解键（Insert 的扩展码 0x52 与 Numpad0 的 0x52 撞车），
所以要看 easyop 现在的 `send_combo`（wVk + wScan=0 + 无 KEYEVENTF_EXTENDEDKEY）
到底给目标窗口送去了什么。

安全设计：本进程自己装的钩子把**自己注入的** Insert/Shift 事件全部吞掉（return 1），
所以任何应用都收不到这些按键 —— 不会污染用户当前窗口、不会误粘贴。

# ⚠️ 它只能回答一半的问题（09-21 的教训）

它回答的是"**OS 把这条 SendInput 变成了什么**"（vk / scan / extended / 上下），
**不回答**"目标程序拿它做了什么"。这两件事之间隔着目标程序自己的判断逻辑。

09-21 就栽在这里：探针证明"`Insert` 不带 `KEYEVENTF_EXTENDEDKEY` 时目标收到
`extended = false`"，于是推断"mintty 因此把 Insert 当成了小键盘 KP0、粘贴不触发"。
前半句对，后半句错 —— mintty 的粘贴走的是**另一条硬编码分支**（`wininput.c`：

    if (cfg.clip_shortcuts && key == VK_INSERT && mods && !alt) { if (shift) win_paste(); }

它只看 `VK_INSERT` 与修饰键，**根本不看扩展位**）。真正的失败原因在 `mods`：
它由 `get_mods()` → `GetKeyState(VK_SHIFT)`（**泛用** VK）算出，而注入的 shift 到目标时是
`VK_LSHIFT`，泛用位不会被置上 ⇒ `shift == false` ⇒ 分支进了、什么也不做。

⇒ **判据必须在"目标程序"那一层闭环**（见 `e2e_terminal_paste_key.py`）。
本脚本的定位是"拿第一手事实"，不是"下结论"。
"""

import ctypes
import ctypes.wintypes as w
import sys
import threading
import time

user32 = ctypes.WinDLL("user32", use_last_error=True)

WH_KEYBOARD_LL = 13
WM_KEYDOWN, WM_KEYUP = 0x0100, 0x0101
WM_SYSKEYDOWN, WM_SYSKEYUP = 0x0104, 0x0105
LLKHF_EXTENDED = 0x01
LLKHF_INJECTED = 0x10

VK_SHIFT, VK_INSERT, VK_V = 0x10, 0x2D, 0x56
KEYEVENTF_EXTENDEDKEY = 0x0001
KEYEVENTF_KEYUP = 0x0002
KEYEVENTF_SCANCODE = 0x0008

ULONG_PTR = ctypes.c_ulonglong if ctypes.sizeof(ctypes.c_void_p) == 8 else ctypes.c_ulong
LRESULT = ctypes.c_ssize_t


class KBDLLHOOKSTRUCT(ctypes.Structure):
    _fields_ = [
        ("vkCode", w.DWORD),
        ("scanCode", w.DWORD),
        ("flags", w.DWORD),
        ("time", w.DWORD),
        ("dwExtraInfo", ULONG_PTR),
    ]


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [
        ("wVk", w.WORD),
        ("wScan", w.WORD),
        ("dwFlags", w.DWORD),
        ("time", w.DWORD),
        ("dwExtraInfo", ULONG_PTR),
    ]


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [
        ("dx", w.LONG),
        ("dy", w.LONG),
        ("mouseData", w.DWORD),
        ("dwFlags", w.DWORD),
        ("time", w.DWORD),
        ("dwExtraInfo", ULONG_PTR),
    ]


class HARDWAREINPUT(ctypes.Structure):
    _fields_ = [("uMsg", w.DWORD), ("wParamL", w.WORD), ("wParamH", w.WORD)]


class _INPUTunion(ctypes.Union):
    _fields_ = [("ki", KEYBDINPUT), ("mi", MOUSEINPUT), ("hi", HARDWAREINPUT)]


class INPUT(ctypes.Structure):
    _fields_ = [("type", w.DWORD), ("u", _INPUTunion)]


INPUT_KEYBOARD = 1

HOOKPROC = ctypes.WINFUNCTYPE(LRESULT, ctypes.c_int, w.WPARAM, w.LPARAM)

# 只吞"我们自己注入的" Insert / Shift，其余一律放行。
SWALLOW_VKS = {VK_INSERT, VK_SHIFT, 0xA0, 0xA1}

records = []
current = {"tag": None}
_keep_alive = None


def _hook(nCode, wParam, lParam):
    if nCode == 0:
        kb = ctypes.cast(lParam, ctypes.POINTER(KBDLLHOOKSTRUCT)).contents
        if (kb.flags & LLKHF_INJECTED) and kb.vkCode in SWALLOW_VKS:
            records.append(
                {
                    "tag": current["tag"],
                    "msg": wParam,
                    "vk": kb.vkCode,
                    "scan": kb.scanCode,
                    "extended": bool(kb.flags & LLKHF_EXTENDED),
                    "flags": kb.flags,
                }
            )
            return 1  # 吞掉：任何应用都收不到 ⇒ 零副作用
    return user32.CallNextHookEx(None, nCode, wParam, lParam)


def send(seq):
    """seq: [(vk, scan, flags), ...]"""
    arr = (INPUT * len(seq))()
    for i, (vk, scan, flags) in enumerate(seq):
        arr[i].type = INPUT_KEYBOARD
        arr[i].u.ki = KEYBDINPUT(wVk=vk, wScan=scan, dwFlags=flags, time=0, dwExtraInfo=0)
    n = user32.SendInput(len(seq), arr, ctypes.sizeof(INPUT))
    return n


def run_case(tag, seq, gap=0.25):
    current["tag"] = tag
    before = len(records)
    sent = send(seq)
    time.sleep(gap)
    got = records[before:]
    print(f"\n--- {tag} ---")
    print(f"  SendInput 发送 {sent}/{len(seq)} 个事件")
    if not got:
        print("  (钩子未观察到注入事件)")
    for r in got:
        kind = "down" if r["msg"] in (WM_KEYDOWN, WM_SYSKEYDOWN) else "up"
        print(
            f"  vk=0x{r['vk']:02X}  scan=0x{r['scan']:02X}  "
            f"extended={r['extended']}  {kind}"
        )
    return got


def main():
    global _keep_alive

    user32.MapVirtualKeyW.restype = w.UINT
    user32.MapVirtualKeyW.argtypes = [w.UINT, w.UINT]
    vsc = user32.MapVirtualKeyW(VK_INSERT, 0)   # MAPVK_VK_TO_VSC
    vsc_ex = user32.MapVirtualKeyW(VK_INSERT, 4)  # MAPVK_VK_TO_VSC_EX
    print("MapVirtualKeyW(VK_INSERT):")
    print(f"  MAPVK_VK_TO_VSC    = 0x{vsc:04X}")
    print(f"  MAPVK_VK_TO_VSC_EX = 0x{vsc_ex:04X}   (0xE0xx = 带 E0 前缀的扩展键)")

    proc = HOOKPROC(_hook)
    _keep_alive = proc
    hhook = user32.SetWindowsHookExW(WH_KEYBOARD_LL, proc, None, 0)
    if not hhook:
        print("SetWindowsHookEx 失败:", ctypes.get_last_error())
        return 1
    print("\n钩子已装（只吞本进程注入的 Insert/Shift，其余放行）")

    t = threading.Thread(target=_run_cases, daemon=True)
    t.start()

    # 低级钩子回调投递到安装线程的消息队列 ⇒ 必须泵消息
    msg = w.MSG()
    deadline = time.time() + 4.0
    while time.time() < deadline:
        while user32.PeekMessageW(ctypes.byref(msg), None, 0, 0, 1):
            user32.TranslateMessage(ctypes.byref(msg))
            user32.DispatchMessageW(ctypes.byref(msg))
        time.sleep(0.01)
    t.join(timeout=1.0)
    user32.UnhookWindowsHookEx(hhook)
    print("\n钩子已卸载")
    return 0


def _run_cases():
    time.sleep(0.3)
    # ① easyop 现状：wVk + wScan=0 + 无扩展标志
    run_case("① 现状：Insert(wVk, scan=0, flags=0)", [(VK_INSERT, 0, 0)])
    run_case("①b 现状 keyup", [(VK_INSERT, 0, KEYEVENTF_KEYUP)])

    # ② 只加 KEYEVENTF_EXTENDEDKEY
    run_case("② Insert + KEYEVENTF_EXTENDEDKEY", [(VK_INSERT, 0, KEYEVENTF_EXTENDEDKEY)])
    run_case("②b + keyup", [(VK_INSERT, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP)])

    # ③ 只给 scan code
    run_case("③ Insert(scan=0x52, KEYEVENTF_SCANCODE)", [(0, 0x52, KEYEVENTF_SCANCODE)])

    # ④ scan code + 扩展
    run_case(
        "④ Insert(scan=0x52, SCANCODE|EXTENDEDKEY)",
        [(0, 0x52, KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY)],
    )

    # ⑤ 现状的完整 Shift+Insert 组合
    run_case(
        "⑤ 现状 Shift+Insert 组合",
        [(VK_SHIFT, 0, 0), (VK_INSERT, 0, 0), (VK_INSERT, 0, KEYEVENTF_KEYUP), (VK_SHIFT, 0, KEYEVENTF_KEYUP)],
    )

    # ⑥ 修正版 Shift+Insert
    run_case(
        "⑥ 修正 Shift+Insert（Insert 带扩展）",
        [
            (VK_SHIFT, 0, 0),
            (VK_INSERT, 0, KEYEVENTF_EXTENDEDKEY),
            (VK_INSERT, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP),
            (VK_SHIFT, 0, KEYEVENTF_KEYUP),
        ],
    )


if __name__ == "__main__":
    sys.exit(main())
