# -*- coding: utf-8 -*-
"""抓双向流式回包的**真实十六进制**，供 Rust 单测当夹具用。

为什么要抓真帧当夹具：解析器最怕的是"读错位"，而错位的字节照样能被切成 i16 ——
交付出去是一段听不出解码失败、却短了一百倍的音频。用真帧钉死偏移量，
以后服务端一旦改了布局，单测立刻红，而不是等用户听到半句话。
"""

import json
import os
import struct
import sys
import uuid

import websocket

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from probe_tts_bidirection import (  # noqa: E402
    EV_FINISH_CONNECTION, EV_FINISH_SESSION, EV_START_CONNECTION, EV_START_SESSION,
    EV_TASK_REQUEST, build_event_frame)

APP_DIR = os.path.join(os.environ.get("APPDATA", ""), "com.easyop.app")

with open(os.path.join(APP_DIR, "agent_desk.json"), encoding="utf-8") as f:
    T = json.load(f)["tts"]

ws = websocket.create_connection(
    T["endpoint"],
    header=["X-Api-Key: " + T["api_key"],
            "X-Api-Resource-Id: " + T["resource_id"],
            "X-Api-Connect-Id: " + str(uuid.uuid4())],
    timeout=12, sslopt={"cert_reqs": 1})

sid = str(uuid.uuid4())
want = {}
print("端点: %s" % T["endpoint"])


def keep(name, d):
    if name in want:
        return
    want[name] = d
    print("%-22s %s" % (name, d.hex(" ") if isinstance(d, bytes) else repr(d)))


ws.send_binary(build_event_frame(EV_START_CONNECTION, b"{}"))
keep("ConnectionStarted_50", ws.recv())

params = {"speaker": T["voice"],
          "audio_params": {"format": "pcm", "sample_rate": int(T.get("target_sample_rate", 48000))}}
ws.send_binary(build_event_frame(EV_START_SESSION, json.dumps(
    {"event": EV_START_SESSION, "namespace": "BidirectionalTTS", "req_params": params},
    ensure_ascii=False).encode(), sid))
keep("SessionStarted_150", ws.recv())

ws.send_binary(build_event_frame(EV_TASK_REQUEST, json.dumps(
    {"event": EV_TASK_REQUEST, "req_params": {"text": "好的。"}},
    ensure_ascii=False).encode(), sid))
ws.send_binary(build_event_frame(EV_FINISH_SESSION, b"{}", sid))

n = 0
while n < 40:
    d = ws.recv()
    if isinstance(d, str):
        print("文本帧:", d[:120])
        continue
    n += 1
    ev = struct.unpack(">I", d[4:8])[0] if len(d) >= 8 else -1
    mt = d[1] >> 4
    if mt == 0b1111:
        keep("ErrorFrame", d)
    elif ev == 350:
        keep("TTSSentenceStart_350", d)
    elif ev == 352:
        keep("TTSResponse_352_first", d)
    elif ev == 351:
        keep("TTSSentenceEnd_351", d)
        break

ws.send_binary(build_event_frame(EV_FINISH_CONNECTION, b"{}"))
try:
    d = ws.recv()
    if isinstance(d, bytes):
        keep("ConnectionFinished_52", d)
except Exception:
    pass
ws.close()

print()
print("=== 夹具（Rust 单测用）===")
for k, v in want.items():
    if k == "TTSResponse_352_first":
        # 音频帧太长，只留头部 12 字节 + 长度信息，并单独给一个"裁剪成 8 字节音频"的可测帧
        body_len = len(v) - 4 - 4 - 4 - 36 - 4
        truncated = v[:4] + v[4:8] + v[8:12] + v[12:48] + struct.pack(">I", 8) + v[48:56]
        print("%-22s 原长 %d，音频 %d 字节" % (k, len(v), body_len))
        print("    %s" % truncated.hex(" "))
    else:
        print("%-22s %s" % (k, v.hex(" ")))
