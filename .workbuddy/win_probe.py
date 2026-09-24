"""枚举某个进程的全部顶层窗口 + 可见性。用于诊断"浮层窗口到底显示了没有"。

用法:
  python win_probe.py once            # 打印当前窗口列表
  python win_probe.py watch 60        # 轮询 60 秒，只在可见性/位置变化时打印
进程名默认 easyop.exe，可用 --exe 覆盖。
"""
import ctypes
import ctypes.wintypes as wt
import sys
import time

user32 = ctypes.windll.user32
WNDENUMPROC = ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)


def pids_of(exe_name):
    """用 tasklist 拿进程号（避免引入 psutil）。"""
    import subprocess
    out = subprocess.run(
        ["tasklist", "/FI", f"IMAGENAME eq {exe_name}", "/FO", "CSV", "/NH"],
        capture_output=True, text=True, errors="ignore",
    ).stdout
    pids = []
    for line in out.splitlines():
        parts = [p.strip('"') for p in line.split('","')]
        if len(parts) >= 2 and parts[0].lower() == exe_name.lower():
            try:
                pids.append(int(parts[1]))
            except ValueError:
                pass
    return pids


def windows_of(pid):
    found = []

    def cb(hwnd, _):
        p = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(p))
        if p.value == pid:
            buf = ctypes.create_unicode_buffer(512)
            user32.GetWindowTextW(hwnd, buf, 512)
            r = wt.RECT()
            user32.GetWindowRect(hwnd, ctypes.byref(r))
            found.append({
                "hwnd": hwnd,
                "visible": bool(user32.IsWindowVisible(hwnd)),
                "rect": (r.left, r.top, r.right - r.left, r.bottom - r.top),
                "title": buf.value,
            })
        return True

    user32.EnumWindows(WNDENUMPROC(cb), 0)
    return found


def snapshot(exe):
    rows = []
    for pid in pids_of(exe):
        for w in windows_of(pid):
            rows.append((pid, w))
    return rows


def fmt(rows):
    if not rows:
        return "（无窗口）"
    return "\n".join(
        f"  pid={pid} hwnd={w['hwnd']} {'VISIBLE' if w['visible'] else 'hidden '} "
        f"{w['rect'][0]},{w['rect'][1]} {w['rect'][2]}x{w['rect'][3]} 「{w['title']}」"
        for pid, w in rows
    )


def key(rows):
    return tuple((pid, w["visible"], w["rect"], w["title"]) for pid, w in rows)


def main():
    args = sys.argv[1:]
    exe = "easyop.exe"
    if "--exe" in args:
        i = args.index("--exe")
        exe = args[i + 1]
        del args[i:i + 2]
    mode = args[0] if args else "once"

    if mode == "once":
        print(f"[{time.strftime('%H:%M:%S')}] {exe} 的顶层窗口：")
        print(fmt(snapshot(exe)))
        return

    if mode == "watch":
        secs = float(args[1]) if len(args) > 1 else 60.0
        t0 = time.time()
        last = None
        while time.time() - t0 < secs:
            rows = snapshot(exe)
            k = key(rows)
            if k != last:
                print(f"\n[{time.strftime('%H:%M:%S')}] 窗口状态变化：")
                print(fmt(rows), flush=True)
                last = k
            time.sleep(0.12)
        print(f"\n[{time.strftime('%H:%M:%S')}] 观察结束")
        return

    print("未知模式，用 once / watch <秒>")


if __name__ == "__main__":
    main()
