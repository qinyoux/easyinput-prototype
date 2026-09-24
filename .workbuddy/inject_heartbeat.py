#!/usr/bin/env python3
"""往 easyop 的 UDP 17333 注入**合法 EIHB 心跳帧**，用于验证心跳判据状态机。

为什么需要它：开发板不在手边时，"超时 / 抖动 / 真离线"这类判据没法靠真硬件复现，
但它们**本来就只是逻辑**——只要喂进和固件同格式的帧，状态机就该给出同样的行为。
这比"改小常量再改回来"安全得多（不用为了测试动生产代码）。

⛔ 适用前提：**开发板不在场（或不发心跳）**。
   本脚本发到 `127.0.0.1`，而真板子从自己的 IP 发心跳；两者的作用都是刷新
   同一个 `last_heartbeat_ms`。只要真板子还在发包，就**永远造不出"离线"**——
   脚本停发的那段时间，真心跳会把时间戳续上，"超时判定"这一环根本不会被触发。
   （2026-09-24 第一次跑就踩了这个坑：以为在测状态机，其实板子一直在线上，
   脚本的阶段 2/3 测的是"板子没断"，PASS/FAIL 都无意义。）
   ⇒ **先确认板子不在发心跳**（看日志有没有 `收到心跳` / 查 `wifi_link_status`），
   再跑本脚本。

帧格式（与 `src-tauri/src/ei_protocol.rs::EihbHeartbeat::parse` 对齐）：
    [0..4]   "EIHB"
    [4]      版本
    [5]      flags：bit0 = streaming，bit1 = audio_ready
    [8..16]  session_id (LE u64, 0 = 未推流)
    [16..20] seq (LE u32)

时间表（按 `HEARTBEAT_TIMEOUT=15s`、`RESUMED_FLAP_MS=30s` 设计）：
    阶段 1  正常心跳 4 秒            → 首次连上（应计 1 次成功）
    阶段 2  停发 20 秒               → 15 秒后判离线；恢复时离线仅约 4~5 秒
                                     → **应判为抖动，成功次数不变**
    阶段 3  停发 50 秒               → 判离线后再等 30 秒以上
                                     → **应计一次成功，成功次数 +1**
"""

import json
import os
import socket
import struct
import time

HOST = "127.0.0.1"
PORT = 17333
KNOWN = os.path.join(
    os.environ.get("APPDATA", ""), "com.easyop.app", "wifi_known.json"
)

sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)


def hb(seq: int) -> bytes:
    buf = bytearray(20)
    buf[0:4] = b"EIHB"
    buf[4] = 1
    buf[5] = 0x02  # audio_ready，未推流
    struct.pack_into("<Q", buf, 8, 0)
    struct.pack_into("<I", buf, 16, seq)
    return bytes(buf)


def send(seq: int) -> None:
    sock.sendto(hb(seq), (HOST, PORT))


def snapshot(tag: str) -> dict:
    """读历史文件里的成功统计。**只读，不写** —— 写坏了就分不清是谁改的。"""
    try:
        with open(KNOWN, "r", encoding="utf-8") as f:
            data = json.load(f)
    except Exception as e:  # 文件可能还没生成
        return {"tag": tag, "error": str(e)}
    rows = [
        {
            "ssid": e.get("ssid", "")[:3] + "***",  # 打码：日志/终端会被贴出去
            "success": e.get("success_count", 0),
            "last_success_unix": e.get("last_success_unix", 0),
        }
        for e in data.get("entries", [])
    ]
    return {"tag": tag, "rows": rows}


def log(msg: str) -> None:
    print(f"[{time.strftime('%H:%M:%S')}] {msg}", flush=True)


log(f"目标 {HOST}:{PORT}｜历史文件 {KNOWN}")
base = snapshot("基线")
log(f"基线：{json.dumps(base, ensure_ascii=False)}")

# ── 阶段 1：建立在线
log("阶段1 注入心跳 4 秒（建立在线）")
for i in range(4):
    send(i)
    time.sleep(1)
time.sleep(2)
after1 = snapshot("阶段1后")
log(f"阶段1后：{json.dumps(after1, ensure_ascii=False)}")

# ── 阶段 2：短停 ⇒ 抖动
log("阶段2 停发 20 秒…")
time.sleep(20)
log("阶段2 恢复心跳（预期：判为抖动，成功次数不变）")
send(100)
time.sleep(1)
send(101)
time.sleep(2)
after2 = snapshot("阶段2后")
log(f"阶段2后：{json.dumps(after2, ensure_ascii=False)}")

# ── 阶段 3：长停 ⇒ 真重连
log("阶段3 停发 50 秒…")
time.sleep(50)
log("阶段3 恢复心跳（预期：计一次成功，成功次数 +1）")
send(200)
time.sleep(1)
send(201)
time.sleep(2)
after3 = snapshot("阶段3后")
log(f"阶段3后：{json.dumps(after3, ensure_ascii=False)}")

log("=== 判定 ===")
def total(snap: dict) -> int:
    return sum(r["success"] for r in snap.get("rows", []))

b, a1, a2, a3 = total(base), total(after1), total(after2), total(after3)
log(f"成功次数总计：基线 {b} → 阶段1 {a1}（应 +1）→ 阶段2 {a2}（应不变）→ 阶段3 {a3}（应 +1）")
log(f"阶段1 {'PASS' if a1 == b + 1 else 'FAIL'}")
log(f"阶段2 {'PASS' if a2 == a1 else 'FAIL'}")
log(f"阶段3 {'PASS' if a3 == a2 + 1 else 'FAIL'}")
