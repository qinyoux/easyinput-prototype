# -*- coding: utf-8 -*-
"""豆包 TTS **双向流式**（`/api/v3/tts/bidirection`）协议探测 —— 权威版。

# 为什么单独有一个脚本

`probe_models.py` 里的 TTS 探测走**单向流式**帧型：

    上行：[头 4B][sequence 4B][payload_size 4B][payload]      ← flags = 0b0001

双向流式端点（`/tts/bidirection`）的帧型**完全不同**，且要求先握手再喂文本：

    上行：[头 4B][event 4B][session_id_len 4B][session_id][payload_size 4B][payload]
                                                                 ↑ flags = 0b0100

    StartConnection(1) → ConnectionStarted(50)
    → StartSession(100) → SessionStarted(150)
    → TaskRequest(200)  ← 文本在这一帧生效
    → 350/351/352 …    → FinishSession(102) → SessionFinished(152)
    → FinishConnection(2) → ConnectionFinished(52)

# 两个已经实测钉死的坑（都在本脚本里复现过）

**坑一：把单向帧发给双向端点 = 端点通、但一帧都不回。**
钦友 2026-09-23 17:22 的日志就是它：

    建连成功（HTTP 200）→ 10 秒后「等待音频超时（已收 0 帧）」→ 退回固定语料

服务端握手照收，读完帧发现没有合法 event，就**静静等**。`--repro-old` 可复现。

**坑二：双向流式的 `StartSession` 不带 `session_id` 必被拒。**
这一步只有真打才知道，文档里"可选字段"的说法不足以推出结论：

| 发法 | 服务端回 |
|---|---|
| `[ev][size][payload]`（无 sid 段） | `parse payload size failed: body too short` |
| `[ev][0][size][payload]`（空 sid） | `session ID length is zero` |
| **`[ev][len][sid][size][payload]`** | **`SessionStarted(150)` ✓ 随后拿到音频** |
| `[ev][size][payload][len][sid]` | `marshal ws msg body to JSON failed` |

⇒ **上行会话类帧必须带非空 session_id**（客户端自己生成 uuid 即可，不必等服务端下发）。

# 用法

```bash
python scripts/probe_tts_bidirection.py             # 用配置里的端点，跑完整流程
python scripts/probe_tts_bidirection.py --text-only-in-task   # 文本只放 TaskRequest
python scripts/probe_tts_bidirection.py --repro-old           # 复现坑一

EO_PROBE_TTS_VOICE=zh_female_xiaohe_uranus_bigtts \
EO_PROBE_TTS_ENDPOINT=wss://openspeech.bytedance.com/api/v3/tts/bidirection \
    python scripts/probe_tts_bidirection.py
```

密钥现读现用，只打掩码。
"""

import json
import os
import struct
import sys
import time
import uuid

try:
    import websocket  # websocket-client
except ImportError:
    websocket = None

APP_DIR = os.path.join(os.environ.get("APPDATA", ""), "com.easyop.app")
TIMEOUT_WS = 12
IDLE_WS = 6

OK, AUTH, FAIL = "OK", "AUTH", "FAIL"

# ---- 帧常量（官方文档 §4.1 / §4.2）----
MT_FULL_CLIENT = 0b0001
MT_AUDIO_ONLY_SERVER = 0b1011
MT_ERROR = 0b1111
FLAG_HAS_SEQUENCE = 0b0001
FLAG_HAS_EVENT = 0b0100
SER_JSON = 0b0001

# ---- 事件号（官方文档 §5）----
EV_START_CONNECTION = 1
EV_FINISH_CONNECTION = 2
EV_CONNECTION_STARTED = 50
EV_CONNECTION_FAILED = 51
EV_CONNECTION_FINISHED = 52
EV_START_SESSION = 100
EV_FINISH_SESSION = 102
EV_SESSION_STARTED = 150
EV_SESSION_FINISHED = 152
EV_SESSION_FAILED = 153
EV_TASK_REQUEST = 200
EV_TTS_SENTENCE_START = 350
EV_TTS_SENTENCE_END = 351
EV_TTS_RESPONSE = 352

EVENT_NAMES = {
    1: "StartConnection", 2: "FinishConnection",
    50: "ConnectionStarted", 51: "ConnectionFailed", 52: "ConnectionFinished",
    100: "StartSession", 101: "CancelSession", 102: "FinishSession",
    150: "SessionStarted", 151: "SessionCanceled", 152: "SessionFinished",
    153: "SessionFailed", 200: "TaskRequest",
    350: "TTSSentenceStart", 351: "TTSSentenceEnd", 352: "TTSResponse",
}


def mask(key):
    k = (key or "").strip()
    return "%s…%s（%d 字符）" % (k[:8], k[-4:], len(k)) if k else "（空）"


def load_tts_cfg():
    path = os.path.join(APP_DIR, "agent_desk.json")
    if not os.path.exists(path):
        raise SystemExit("找不到配置文件：%s" % path)
    with open(path, encoding="utf-8") as f:
        cfg = json.load(f)["tts"]
    cfg["endpoint"] = os.environ.get("EO_PROBE_TTS_ENDPOINT", cfg["endpoint"])
    cfg["resource_id"] = os.environ.get("EO_PROBE_TTS_RESOURCE", cfg.get("resource_id", ""))
    cfg["voice"] = os.environ.get("EO_PROBE_TTS_VOICE", cfg["voice"])
    return cfg


# ---------------------------------------------------------------------------
# 帧编解码
# ---------------------------------------------------------------------------

def head(mt=MT_FULL_CLIENT, flags=FLAG_HAS_EVENT, ser=SER_JSON):
    return bytearray([0x11, (mt << 4) | flags, ser << 4, 0x00])


def build_event_frame(event, payload: bytes, session_id: str = ""):
    """双向流式上行帧。

    `session_id` **非空才写该段** —— 连接类事件（StartConnection/FinishConnection）
    不带 sid，会话类（StartSession/TaskRequest/FinishSession）必须带。实测过：
    会话类帧少了 sid 段会被解析成"size 错位"，带了空 sid 段则直接被拒。
    """
    out = head() + struct.pack(">I", event)
    if session_id:
        b = session_id.encode()
        out += struct.pack(">I", len(b)) + b
    out += struct.pack(">I", len(payload)) + payload
    return bytes(out)


def build_sequence_frame(payload: bytes, sequence: int = 1):
    """单向流式上行帧（**只为复现坑一**）。"""
    return (bytes(head(flags=FLAG_HAS_SEQUENCE)) + struct.pack(">i", sequence)
            + struct.pack(">I", len(payload)) + payload)


def parse_frame(data):
    """解析一帧。

    id 段是否存在的判据是**硬**的：让 payload 长度正好吃到帧尾
    （`off + 4 + id_len + 4 + size == len(data)`）。只看"长度字段像不像小数"会在
    服务端布局变化时静默错位 —— 而错位的数据照样能被切成 i16，交付出去是一段
    听不出解码失败、却短了一百倍的音频。
    """
    if isinstance(data, str):
        return {"text": True, "mt": -1, "event": None, "code": None, "payload": data.encode()}
    if len(data) < 8:
        raise ValueError("帧短于 8 字节：%s" % data[:16].hex(" "))
    f = {"text": False, "mt": data[1] >> 4, "flags": data[1] & 0x0F,
         "event": None, "id": None, "id_kind": None, "code": None,
         "payload": b"", "raw_len": len(data)}
    off = 4
    if f["mt"] == MT_ERROR:
        f["code"] = struct.unpack(">I", data[off:off + 4])[0]
        off += 4
    else:
        if f["flags"] & FLAG_HAS_EVENT:
            f["event"] = struct.unpack(">I", data[off:off + 4])[0]
            off += 4
        if f["flags"] & FLAG_HAS_SEQUENCE:
            off += 4
        if off + 8 <= len(data):
            id_len = struct.unpack(">I", data[off:off + 4])[0]
            if 1 <= id_len <= 128 and off + 4 + id_len + 4 <= len(data):
                size = struct.unpack(">I", data[off + 4 + id_len:off + 8 + id_len])[0]
                if off + 4 + id_len + 4 + size == len(data):
                    f["id"] = data[off + 4:off + 4 + id_len].decode("utf-8", "replace")
                    f["id_kind"] = "connection_id" if f["event"] in (
                        EV_CONNECTION_STARTED, EV_CONNECTION_FAILED,
                        EV_CONNECTION_FINISHED) else "session_id"
                    off += 4 + id_len
    size = struct.unpack(">I", data[off:off + 4])[0]
    f["payload"] = data[off + 4:off + 4 + size]
    return f


def ev_label(ev):
    return "%d(%s)" % (ev, EVENT_NAMES.get(ev, "?")) if ev is not None else "-"


def trace(f, indent="    ← "):
    if f["text"]:
        print("%s[文本帧] %s" % (indent, f["payload"].decode("utf-8", "replace")[:180]))
        return
    if f["mt"] == MT_ERROR:
        print("%s[错误帧] code=%s %s" % (indent, f["code"],
                                        f["payload"].decode("utf-8", "replace")[:220]))
        return
    body = f["payload"]
    if f["mt"] == MT_AUDIO_ONLY_SERVER:
        preview = "<裸音频 %d 字节>" % len(body)
    else:
        preview = body.decode("utf-8", "replace")[:160]
        if len(body) > 160:
            preview += "…"
    sid = (" %s=%s" % (f["id_kind"], f["id"][:20])) if f["id"] else ""
    print("%smt=%s ev=%-24s len=%-6d%s %s" % (indent, bin(f["mt"]), ev_label(f["event"]),
                                              f["raw_len"], sid, preview))


# ---------------------------------------------------------------------------
# 探测
# ---------------------------------------------------------------------------

def build_req_params(cfg, text):
    p = {"speaker": cfg["voice"],
         "audio_params": {"format": "pcm",
                          "sample_rate": int(cfg.get("target_sample_rate", 48000))}}
    if text:
        p["text"] = text
    if cfg.get("explicit_language", "").strip():
        # additions 必须是**字符串**（内容是 JSON 文本），传嵌套对象会被服务端拒
        p["additions"] = json.dumps({"explicit_language": cfg["explicit_language"].strip()},
                                    ensure_ascii=False)
    return p


def connect(cfg):
    return websocket.create_connection(
        cfg["endpoint"],
        header=["X-Api-Key: " + cfg["api_key"],
                "X-Api-Resource-Id: " + cfg["resource_id"],
                "X-Api-Connect-Id: " + str(uuid.uuid4())],
        timeout=TIMEOUT_WS, sslopt={"cert_reqs": 1})


def probe(cfg, text, text_only_in_task=False, until_152=False, verbose=True):
    url = cfg["endpoint"]
    rate = int(cfg.get("target_sample_rate", 48000))
    if verbose:
        print("端点    : %s" % url)
        print("resource: %s" % cfg["resource_id"])
        print("音色    : %s" % cfg["voice"])
        print("key     : %s" % mask(cfg["api_key"]))
        print("文本    : %s（%d 字）" % (text, len(text)))
        print("文本位置: %s" % ("只放 TaskRequest" if text_only_in_task
                                else "StartSession 与 TaskRequest 都放"))
        print()
        if "bidirection" not in url:
            print("⚠️ 端点路径里没有 `bidirection` —— 它多半是单向流式端点，本流程不适用。\n")

    try:
        ws = connect(cfg)
    except Exception as e:
        return (AUTH if ("403" in str(e) or "401" in str(e)) else FAIL), "握手失败：%s" % e

    sid = str(uuid.uuid4())
    pcm = bytearray()
    events, frames, err, server_sid = [], 0, None, None
    try:
        # 1) StartConnection（连接类：不带 sid）
        print("  → StartConnection(1) {}")
        ws.send_binary(build_event_frame(EV_START_CONNECTION, b"{}"))
        f = parse_frame(ws.recv())
        frames += 1
        if verbose:
            trace(f, "    ← ")
        if f["mt"] == MT_ERROR:
            return FAIL, "StartConnection 错误 code=%s" % f["code"]
        if f["event"] != EV_CONNECTION_STARTED:
            return FAIL, "StartConnection 未回 50，而是 %s" % ev_label(f["event"])
        conn_id = f["id"]

        # 2) StartSession（会话类：必须带非空 sid）
        body = {"event": EV_START_SESSION, "namespace": "BidirectionalTTS",
                "req_params": build_req_params(cfg, "" if text_only_in_task else text)}
        raw = json.dumps(body, ensure_ascii=False).encode()
        print("  → StartSession(100) sid=%s payload=%d 字节" % (sid[:13], len(raw)))
        ws.send_binary(build_event_frame(EV_START_SESSION, raw, sid))
        f = parse_frame(ws.recv())
        frames += 1
        if verbose:
            trace(f, "    ← ")
        if f["mt"] == MT_ERROR:
            return FAIL, "StartSession 错误 code=%s %s" % (
                f["code"], f["payload"].decode("utf-8", "replace")[:200])
        if f["event"] != EV_SESSION_STARTED:
            return FAIL, "StartSession 未回 150，而是 %s" % ev_label(f["event"])
        server_sid = f["id"]

        # 3) TaskRequest（文本在这里生效）
        body = {"event": EV_TASK_REQUEST, "req_params": {"text": text}}
        raw = json.dumps(body, ensure_ascii=False).encode()
        print("  → TaskRequest(200) payload=%d 字节" % len(raw))
        ws.send_binary(build_event_frame(EV_TASK_REQUEST, raw, sid))

        # 4) FinishSession（告诉服务端"文本给完了"）
        print("  → FinishSession(102) {}")
        ws.send_binary(build_event_frame(EV_FINISH_SESSION, b"{}", sid))

        # 5) 收音频直到 SessionFinished(152) / TTSSentenceEnd(351)
        deadline = time.time() + TIMEOUT_WS
        last = time.time()
        while time.time() < deadline:
            try:
                d = ws.recv()
            except Exception as e:
                if verbose:
                    print("    ← [recv 结束] %s" % e)
                break
            last = time.time()
            try:
                f = parse_frame(d)
            except ValueError as e:
                err = "帧结构不符：%s head=%s" % (e, bytes(d[:20]).hex(" "))
                print("    ← [解析失败] %s" % err)
                break
            frames += 1
            if verbose:
                trace(f, "    ← ")
            if f["mt"] == MT_ERROR:
                err = "code=%s %s" % (f["code"], f["payload"].decode("utf-8", "replace")[:200])
                break
            if f["event"] is not None:
                events.append(f["event"])
            if f["mt"] == MT_AUDIO_ONLY_SERVER:
                pcm += f["payload"]
            elif f["event"] == EV_TTS_RESPONSE:
                # TTS2.0 下音频也可能挂在"整包应答"上（载荷为混合数据）
                pcm += f["payload"]
            if f["event"] in (EV_TTS_SENTENCE_END, EV_SESSION_FINISHED):
                # `--until-152` 时不把 351 当终点：TTS1.0 下 350/351 可能**每句各来一次**，
                # 拿 351 收尾会把后面几句的音频整段丢掉。官方文档只在 SessionFinished(152)
                # 上写了"必须收到后再开下一个 session"，那才是无歧义的终点。
                if until_152 and f["event"] == EV_TTS_SENTENCE_END:
                    continue
                break

        # 6) FinishConnection（收尾，失败不影响结论）
        try:
            print("  → FinishConnection(2) {}")
            ws.send_binary(build_event_frame(EV_FINISH_CONNECTION, b"{}"))
            f = parse_frame(ws.recv())
            frames += 1
            if verbose:
                trace(f, "    ← ")
        except Exception:
            pass
    finally:
        try:
            ws.close()
        except Exception:
            pass

    dur = len(pcm) / 2.0 / rate if rate else 0
    print()
    print("  连接 id=%s  服务端 session_id=%s" % (conn_id, server_sid))
    print("  %d 帧；事件 %s" % (frames, ",".join(ev_label(e) for e in events) or "-"))
    print("  音频 %.3f 秒（%d 字节 @ %d Hz）" % (dur, len(pcm), rate))

    if err:
        return FAIL, "端点=%s｜%s" % (url, err)
    if not pcm:
        return FAIL, "端点=%s｜握手走完但 0 个采样" % url
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "_probe_out")
    os.makedirs(out, exist_ok=True)
    tag = "task_only" if text_only_in_task else "both"
    path = os.path.join(out, "probe_tts_bidirection_%s.wav" % tag)
    write_wav(path, pcm, rate)
    print("  已落盘 %s（峰值 %.3f）" % (path, peak(pcm)))
    return OK, "端点=%s｜%d 帧｜%.3f 秒｜%s" % (url, frames, dur, path)


def repro_old(cfg):
    """复现坑一：把单向流式帧发给双向端点。"""
    print("== 复现：单向帧 → 双向端点 ==")
    print("端点：%s" % cfg["endpoint"])
    ws = connect(cfg)
    body = {"user": {"uid": "easyop"},
            "req_params": build_req_params(cfg, "测试。")}
    ws.send_binary(build_sequence_frame(json.dumps(body, ensure_ascii=False).encode()))
    print("  → 已发单向帧（flags=0b0001，带 sequence）")
    got = 0
    deadline = time.time() + 8
    while time.time() < deadline:
        try:
            d = ws.recv()
        except Exception:
            break
        got += 1
        trace(parse_frame(d), "    ← ")
    print("收到 %d 帧 ⇒ %s" % (got, "复现成功（一帧都不回）" if got == 0 else "竟然回了"))
    ws.close()


def write_wav(path, pcm_bytes, rate):
    n = len(pcm_bytes)
    with open(path, "wb") as f:
        f.write(b"RIFF" + struct.pack("<I", 36 + n) + b"WAVE")
        f.write(b"fmt " + struct.pack("<IHHIIHH", 16, 1, 1, rate, rate * 2, 2, 16))
        f.write(b"data" + struct.pack("<I", n) + pcm_bytes)


def peak(pcm_bytes):
    m = 0
    for i in range(0, max(0, len(pcm_bytes) - 1), 2):
        v = abs(struct.unpack("<h", pcm_bytes[i:i + 2])[0])
        m = max(m, v)
    return m / 32768.0


def main():
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if websocket is None:
        raise SystemExit("缺少 websocket-client：pip install websocket-client")
    cfg = load_tts_cfg()
    if "--repro-old" in sys.argv:
        repro_old(cfg)
        return
    text = os.environ.get("EO_PROBE_TTS_TEXT", "任务台已经接通，这是双向流式合成测试。")
    only_task = "--text-only-in-task" in sys.argv
    verdict, detail = probe(cfg, text, text_only_in_task=only_task,
                            until_152="--until-152" in sys.argv)
    print()
    print("== 结论 %s ==" % verdict)
    print(detail)


if __name__ == "__main__":
    main()
