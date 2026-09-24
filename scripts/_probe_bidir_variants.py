# -*- coding: utf-8 -*-
"""双向流式 **StartSession 帧形状** 变体实验。

线索：`StartConnection` 用的是 `[头][event][payload_size][payload]`，服务端正常回了
`ConnectionStarted(50)`。但同一形状的 `StartSession` 被拒：

    decode ws request failed: unable to decode V1 protocol message:
    parse payload size failed: body too short

把这句和字节数对上就破了案 —— 我们发的 StartSession 帧共 246 字节：

    [头 4][event 4][size(234) 4][payload 234]  = 246

服务端若按 `[头][event][sid_len][session_id][size][payload]` 读：
跳过 4+4 后剩 238；把 234 当 sid_len 读掉后在尝试读 size 时**正好剩 0 字节** → "body too short"。
⇒ 会话类上行帧**必须带 session_id 段**（哪怕为空）。

本脚本把几种候选形状逐个真打，看哪个能过 StartSession。
"""

import json
import os
import struct
import sys
import time
import uuid

import websocket

APP_DIR = os.path.join(os.environ.get("APPDATA", ""), "com.easyop.app")

MT_FULL_CLIENT = 0b0001
MT_ERROR = 0b1111
FLAG_HAS_EVENT = 0b0100
FLAG_HAS_SEQUENCE = 0b0001
SER_JSON = 0b0001

EV_START_CONNECTION, EV_FINISH_CONNECTION = 1, 2
EV_CONNECTION_STARTED = 50
EV_START_SESSION, EV_FINISH_SESSION = 100, 102
EV_SESSION_STARTED, EV_SESSION_FINISHED = 150, 152
EV_TASK_REQUEST = 200

NAMES = {1: "StartConnection", 2: "FinishConnection", 50: "ConnectionStarted",
         51: "ConnectionFailed", 52: "ConnectionFinished", 100: "StartSession",
         101: "CancelSession", 102: "FinishSession", 150: "SessionStarted",
         151: "SessionCanceled", 152: "SessionFinished", 153: "SessionFailed",
         200: "TaskRequest", 350: "TTSSentenceStart", 351: "TTSSentenceEnd",
         352: "TTSResponse"}


def head(mt=MT_FULL_CLIENT, flags=FLAG_HAS_EVENT, ser=SER_JSON):
    return bytearray([0x11, (mt << 4) | flags, ser << 4, 0x00])


def f_no_sid(event, payload: bytes, sid: str = ""):
    """变体 1：现状 —— [ev][size][payload]"""
    return bytes(head()) + struct.pack(">I", event) + struct.pack(">I", len(payload)) + payload


def f_zero_sid(event, payload: bytes, sid: str = ""):
    """变体 2：显式空 sid —— [ev][0][size][payload]"""
    return (bytes(head()) + struct.pack(">I", event) + struct.pack(">I", 0)
            + struct.pack(">I", len(payload)) + payload)


def f_real_sid(event, payload: bytes, sid: str = ""):
    """变体 3：带真 sid —— [ev][len][sid][size][payload]"""
    b = sid.encode()
    return (bytes(head()) + struct.pack(">I", event) + struct.pack(">I", len(b))
            + b + struct.pack(">I", len(payload)) + payload)


def f_sid_after_payload(event, payload: bytes, sid: str = ""):
    """变体 4：[ev][size][payload][len][sid]"""
    b = sid.encode()
    return (bytes(head()) + struct.pack(">I", event) + struct.pack(">I", len(payload))
            + payload + struct.pack(">I", len(b)) + b)


def parse(d):
    if isinstance(d, str):
        return {"mt": -1, "event": None, "code": None, "payload": d.encode(), "text": True}
    if len(d) < 8:
        return {"mt": -2, "event": None, "code": None, "payload": d}
    mt, flags = d[1] >> 4, d[1] & 0x0F
    if mt == MT_ERROR:
        return {"mt": mt, "event": None, "code": struct.unpack(">I", d[4:8])[0],
                "payload": d[12:] if len(d) > 12 else b""}
    off = 4
    ev = None
    if flags & FLAG_HAS_EVENT:
        ev = struct.unpack(">I", d[off:off + 4])[0]
        off += 4
    if off + 8 <= len(d):
        sid_len = struct.unpack(">I", d[off:off + 4])[0]
        if 1 <= sid_len <= 128 and off + 4 + sid_len + 4 <= len(d):
            size = struct.unpack(">I", d[off + 4 + sid_len:off + 8 + sid_len])[0]
            if off + 4 + sid_len + 4 + size == len(d):
                off += 4 + sid_len
    size = struct.unpack(">I", d[off:off + 4])[0]
    return {"mt": mt, "event": ev, "code": None, "payload": d[off + 4:off + 4 + size]}


def cfg():
    with open(os.path.join(APP_DIR, "agent_desk.json"), encoding="utf-8") as f:
        return json.load(f)["tts"]


def connect(t):
    return websocket.create_connection(
        t["endpoint"],
        header=["X-Api-Key: " + t["api_key"],
                "X-Api-Resource-Id: " + t["resource_id"],
                "X-Api-Connect-Id: " + str(uuid.uuid4())],
        timeout=10, sslopt={"cert_reqs": 1})


def label(ev):
    return "%s(%s)" % (ev, NAMES.get(ev, "?")) if ev is not None else "-"


def try_variant(name, maker, t, text, sid_seed=None):
    print("=" * 78)
    print("变体：%s" % name)
    try:
        ws = connect(t)
    except Exception as e:
        print("  握手失败：%s" % e)
        return None
    try:
        ws.send_binary(f_no_sid(EV_START_CONNECTION, b"{}"))
        r = parse(ws.recv())
        conn_id = None
        print("  ← %s" % label(r["event"]))
        # 从回包里挖 connection_id（供变体 3 用）
        if r["mt"] != MT_ERROR and r["event"] == EV_CONNECTION_STARTED:
            raw = ws.__dict__.get("_last_raw")
        payload = json.dumps({
            "event": EV_START_SESSION, "namespace": "BidirectionalTTS",
            "req_params": {"text": text, "speaker": t["voice"],
                           "audio_params": {"format": "pcm",
                                            "sample_rate": int(t.get("target_sample_rate", 48000))}},
        }, ensure_ascii=False).encode()
        # connection_id 从原始回帧里重新取一次（用一次独立 recv 不现实，改为固定 uuid）
        sid = sid_seed or str(uuid.uuid4())
        ws.send_binary(maker(EV_START_SESSION, payload, sid))
        r = parse(ws.recv())
        if r["mt"] == MT_ERROR:
            print("  ← ✗ 错误帧 code=%s %s" % (r["code"], r["payload"].decode("utf-8", "replace")[:220]))
            ok = False
        else:
            print("  ← %s ✓" % label(r["event"]))
            ok = r["event"] == EV_SESSION_STARTED

        if ok:
            # 喂文本 → 收音频（用与 StartSession **同一个** maker，保证帧形状一致）
            ws.send_binary(maker(EV_TASK_REQUEST, json.dumps(
                {"event": EV_TASK_REQUEST, "req_params": {"text": text}},
                ensure_ascii=False).encode(), sid))
            pcm = 0
            evs = []
            deadline = time.time() + 10
            while time.time() < deadline:
                try:
                    d = ws.recv()
                except Exception:
                    break
                r2 = parse(d)
                if r2["mt"] == MT_ERROR:
                    print("  ← 错误帧 code=%s %s" % (r2["code"], r2["payload"].decode("utf-8", "replace")[:200]))
                    break
                if r2["event"] is not None:
                    evs.append(r2["event"])
                if r2["mt"] == 0b1011:
                    pcm += len(r2["payload"])
                if r2["event"] in (351, 152):
                    break
            print("  音频 %d 字节；事件 %s" % (pcm, ",".join(label(e) for e in evs[:8])))
        return ok
    finally:
        try:
            ws.close()
        except Exception:
            pass


def main():
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    t = cfg()
    text = "测试文本。"
    print("端点：%s" % t["endpoint"])
    print()
    results = []
    results.append(("1 无 sid（现状）", try_variant("1 无 sid（现状）", f_no_sid, t, text)))
    results.append(("2 显式空 sid", try_variant("2 显式空 sid", f_zero_sid, t, text)))
    results.append(("3 真 sid(uuid)", try_variant("3 真 sid(uuid)", f_real_sid, t, text)))
    results.append(("4 sid 在 payload 之后", try_variant("4 sid 在 payload 之后", f_sid_after_payload, t, text)))
    print()
    print("=" * 78)
    for n, ok in results:
        print("%-24s %s" % (n, "通过 StartSession ✓" if ok else "失败 ✗"))


if __name__ == "__main__":
    main()
