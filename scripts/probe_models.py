# -*- coding: utf-8 -*-
"""easyop 模型配置连通性探测（语音编辑 LLM / 流式 ASR / 任务台 TTS / 任务台 driver）。

# 为什么要有这个脚本

配置写对了没有、key 有没有过期、端点前缀（如 `/plan/`）能不能通 ——
这些问题的答案**只能靠真发一次请求拿到**，读代码读不出来。
而它们的失败形态又都很像"没反应"，靠界面上的报错去猜成本极高。

所以这里把每一处配置**逐条真打一次**，并且明确区分三档结论：

| 结论 | 含义 |
|---|---|
| `OK` | 真拿到了业务结果（模型回了字 / 音频有采样 / 识别回了包） |
| `AUTH` | 连上了但被拒（key 无效、resource 未开通、套餐不含） |
| `FAIL` | 连不上、超时、协议不匹配、或返回体解析不出预期字段 |

`AUTH` 与 `FAIL` 必须分开：前者要改账号/套餐，后者要改代码/网络 —— 混成一句
"不可用"会把排查方向彻底带偏。

# 用法

```bash
python scripts/probe_models.py              # 全部
python scripts/probe_models.py tts asr      # 只跑指定项

# 换一组 TTS 参数试（不影响已保存的配置，用来找可用音色）
EO_PROBE_TTS_VOICE=zh_female_cancan_uranus_bigtts python scripts/probe_models.py tts
EO_PROBE_TTS_RESOURCE=seed-tts-1.0 EO_PROBE_TTS_VOICE=zh_female_cancan_mars_bigtts \
    python scripts/probe_models.py tts
```

密钥一律从 `%APPDATA%\\com.easyop.app\\*.json` 现读现用，**不落盘、不进日志**，
输出里只打印掩码后的长度，便于确认"填的是哪一把"而不泄露内容。

# ⚠️ 一条实测教训：`max_tokens` 给小了会把"通"误判成"空"

本脚本对"只回复两个字"这种探针请求给了 **256** 的上限，而不是 16。
原因：`deepseek-flash` 这类**带思维链的模型**会把 `max_tokens` 全部花在
`reasoning_content` 上 —— 16 个 token 时 `content` 是空串、`finish_reason=length`，
看起来像"API 坏了"；`max_tokens=200` 时同一请求正常返回 `收到`。
识别这种"假失败"的信号是响应里的 `completion_tokens_details.reasoning_tokens > 0`。
"""

import json
import os
import ssl
import struct
import sys
import time
import uuid
import urllib.error
import urllib.request

try:
    import websocket  # websocket-client
except ImportError:
    websocket = None

APP_DIR = os.path.join(os.environ.get("APPDATA", ""), "com.easyop.app")
TIMEOUT_HTTP = 25
TIMEOUT_WS = 12

# 探针请求的输出上限。**不能给小**：带思维链的模型会把它全花在推理上，
# 于是"接口是通的"被读成"返回空"（详见文件头）。
PROBE_MAX_TOKENS = 256

OK, AUTH, FAIL = "OK", "AUTH", "FAIL"


# ---------------------------------------------------------------------------
# 基础设施
# ---------------------------------------------------------------------------

def load_json(name):
    path = os.path.join(APP_DIR, name)
    if not os.path.exists(path):
        raise SystemExit("找不到配置文件：%s" % path)
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def mask(key):
    """掩码：能看到前缀和长度，看不出内容。"""
    k = (key or "").strip()
    if not k:
        return "（空）"
    return "%s…%s（%d 字符）" % (k[:8], k[-4:], len(k))


def http_post(url, body, headers, timeout=TIMEOUT_HTTP):
    """返回 (status, text)。网络层异常返回 (None, 原因)。"""
    data = json.dumps(body, ensure_ascii=False).encode("utf-8")
    req = urllib.request.Request(url, data=data, method="POST")
    req.add_header("Content-Type", "application/json")
    for k, v in headers.items():
        req.add_header(k, v)
    ctx = ssl.create_default_context()
    try:
        with urllib.request.urlopen(req, timeout=timeout, context=ctx) as r:
            return r.status, r.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode("utf-8", "replace")
    except Exception as e:  # 超时 / DNS / TLS
        return None, "%s: %s" % (type(e).__name__, e)


def http_get(url, headers, timeout=TIMEOUT_HTTP):
    req = urllib.request.Request(url, method="GET")
    for k, v in headers.items():
        req.add_header(k, v)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, r.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode("utf-8", "replace")
    except Exception as e:
        return None, "%s: %s" % (type(e).__name__, e)


def fetch_error(payload):
    """从错误体里挖出人类可读的原因（火山两种形状都认）。"""
    try:
        v = json.loads(payload)
    except Exception:
        return payload.strip()[:200] or "(空响应体)"
    for holder in (v.get("header") or {}, v, v.get("error") or {}):
        if not isinstance(holder, dict):
            continue
        code = holder.get("code")
        if code is not None:
            return "code=%s message=%s" % (code, holder.get("message", ""))
        if "error" in holder and isinstance(holder["error"], str):
            return holder["error"]
    return json.dumps(v, ensure_ascii=False)[:200]


# ---------------------------------------------------------------------------
# 火山二进制帧（ASR / TTS 共用同一套外层协议）
# ---------------------------------------------------------------------------
#
# 帧头 4 字节 + 可选 event(4) + session_id 长度(4) + session_id + payload 长度(4) + payload
#
# ⚠️ 常见误读：**事件号长得像长度**（350/351/352 落在 300 上下），
# 把它当 payload 长度读会得到一段"看起来像音频、其实错位"的数据 ——
# 听不出错，只会发现音频比预期短一大截。所以这里按字段名逐个取，不靠猜。

MT_FULL_CLIENT = 0b0001
MT_AUDIO_ONLY_CLIENT = 0b0010
MT_FULL_SERVER = 0b1001
MT_AUDIO_ONLY_SERVER = 0b1011
MT_ERROR = 0b1111

FLAG_HAS_SEQUENCE = 0b0001
FLAG_LAST_PACKAGE = 0b0010
FLAG_HAS_EVENT = 0b0100

SER_RAW, SER_JSON = 0b0000, 0b0001

EVENT_NAMES = {
    1: "StartConnection", 2: "FinishConnection",
    50: "ConnectionStarted", 51: "ConnectionFailed", 52: "ConnectionFinished",
    100: "StartSession", 102: "FinishSession",
    150: "SessionStarted", 152: "SessionFinished", 153: "SessionFailed",
    350: "TTSSessionStarted", 351: "TTSSessionFinished",
    352: "TTSSentenceStart", 353: "TTSSentenceEnd", 359: "TTSEnded",
}


def build_frame(message_type, flags, serialization, sequence, payload):
    out = bytearray([0x11, (message_type << 4) | flags, serialization << 4, 0x00])
    if sequence is not None:
        out += struct.pack(">i", sequence)
    out += struct.pack(">I", len(payload))
    out += payload
    return bytes(out)


def parse_frame(data):
    """解析一帧，返回 dict（字段逐个取，取不出来就 raise，绝不静默错位）。"""
    if len(data) < 8:
        raise ValueError("帧短于 8 字节：%s" % data[:16].hex(" "))
    f = {
        "mt": data[1] >> 4,
        "flags": data[1] & 0x0F,
        "ser": data[2] >> 4,
        "comp": data[2] & 0x0F,
        "event": None,
        "seq": None,
        "session_id": None,
        "code": None,
        "payload": b"",
    }
    off = 4
    if f["mt"] == MT_ERROR:
        if len(data) < off + 8:
            raise ValueError("错误帧过短")
        f["code"] = struct.unpack(">I", data[off:off + 4])[0]
        off += 4
    else:
        if f["flags"] & FLAG_HAS_EVENT:
            if len(data) < off + 4:
                raise ValueError("标了 event 但字节不够")
            f["event"] = struct.unpack(">I", data[off:off + 4])[0]
            off += 4
        if f["flags"] & FLAG_HAS_SEQUENCE:
            if len(data) < off + 4:
                raise ValueError("标了 sequence 但字节不够")
            f["seq"] = struct.unpack(">i", data[off:off + 4])[0]
            off += 4
        if f["event"] is not None and off + 4 <= len(data):
            # 服务端回包带 session_id：先长度、再内容
            sid_len = struct.unpack(">I", data[off:off + 4])[0]
            if 0 < sid_len <= 128 and off + 4 + sid_len + 4 <= len(data):
                off += 4
                f["session_id"] = data[off:off + sid_len].decode("utf-8", "replace")
                off += sid_len
    if len(data) < off + 4:
        raise ValueError("缺少 payload 长度字段")
    size = struct.unpack(">I", data[off:off + 4])[0]
    off += 4
    if size > len(data) - off:
        raise ValueError("payload 长度 %d 超过剩余字节 %d" % (size, len(data) - off))
    f["payload"] = data[off:off + size]
    f["raw_len"] = len(data)
    return f


def event_label(ev):
    if ev is None:
        return "-"
    return "%d(%s)" % (ev, EVENT_NAMES.get(ev, "?"))


def ws_connect(url, headers):
    if websocket is None:
        raise SystemExit("缺少 websocket-client，请先 pip install websocket-client")
    hdr = ["%s: %s" % (k, v) for k, v in headers.items()]
    return websocket.create_connection(
        url, header=hdr, timeout=TIMEOUT_WS, sslopt={"cert_reqs": ssl.CERT_NONE}
    )


# ---------------------------------------------------------------------------
# 各项探测
# ---------------------------------------------------------------------------

def probe_chat_endpoint(url, model, key, label):
    """对任意 OpenAI 兼容端点发一次探针请求，返回 (结论, 说明)。

    `probe_llm_edit` 与 `probe_driver` 共用它 —— 两处各写一份的结果是：
    一处修了"思维链吃满 token"的判读、另一处没修，于是同一个模型在两条路径上
    得到相反结论。这种不一致比不探测更糟。
    """
    body = {
        "model": model,
        "messages": [{"role": "user", "content": "只回复两个字：收到"}],
        "max_tokens": PROBE_MAX_TOKENS,
        "stream": False,
    }
    status, text = http_post(url, body, {"Authorization": "Bearer %s" % key})
    if status is None:
        return FAIL, "%s 端点=%s model=%s key=%s｜%s" % (label, url, model, mask(key), text)
    if status != 200:
        return (AUTH if status in (401, 403) else FAIL), \
            "%s 端点=%s model=%s key=%s｜HTTP %s %s" % (
                label, url, model, mask(key), status, fetch_error(text))
    try:
        v = json.loads(text)
        choice = v["choices"][0]
        content = (choice.get("message") or {}).get("content") or ""
        usage = v.get("usage") or {}
        reasoning = (usage.get("completion_tokens_details") or {}).get("reasoning_tokens")
        head = "%s 端点=%s model=%s key=%s" % (label, url, model, mask(key))
        if not content.strip():
            # 空正文 + 思维链吃满 token ⇒ 是"上限给小了"，不是"接口不通"。
            # 这个区分很重要：前者改调用参数，后者要改账号/网络 —— 报错方向完全不同。
            if choice.get("finish_reason") == "length" and reasoning:
                return FAIL, ("%s｜**接口是通的**，但 %d 个 token 全被思维链\
（reasoning_tokens=%s）吃满，正文为空 —— 把调用方的 max_tokens 调大即可。"
                              % (head, PROBE_MAX_TOKENS, reasoning))
            return FAIL, "%s｜200 但正文为空（finish_reason=%s）" % (
                head, choice.get("finish_reason"))
        return OK, "%s｜回字=%r tokens=%s" % (head, content[:30], usage.get("total_tokens", "?"))
    except Exception as e:
        return FAIL, "%s 端点=%s｜返回体解析不了：%s｜%s" % (label, url, e, text[:180])


def probe_llm_edit(cfg):
    """语音编辑用的 LLM（config.json 的 llm_* 三项）。"""
    return probe_chat_endpoint(cfg["llm_endpoint"], cfg["llm_model"], cfg["llm_api_key"], "语音编辑 LLM")


def probe_asr(cfg):
    """流式 ASR：连上 → 发首包 → 发静音 → 发负包 → 看回包。"""
    url, key = cfg["asr_url"], cfg["api_key"]
    res = cfg["resource_id"]
    try:
        ws = ws_connect(url, {
            "X-Api-Key": key,
            "X-Api-Resource-Id": res,
            "X-Api-Request-Id": str(uuid.uuid4()),
            "X-Api-Sequence": "-1",
        })
    except Exception as e:
        tail = getattr(e, "status_code", "") or ""
        return AUTH if "403" in str(e) or "401" in str(e) else FAIL, \
            "端点=%s resource=%s key=%s｜握手失败 %s %s" % (url, res, mask(key), tail, e)

    first = {
        "audio": {"format": "pcm", "codec": "raw", "rate": 16000, "bits": 16, "channel": 1},
        "request": {"model_name": "bigmodel", "enable_itn": True, "enable_punc": True,
                    "result_type": "full", "language": "zh-CN"},
    }
    ws.send_binary(build_frame(MT_FULL_CLIENT, FLAG_HAS_SEQUENCE, SER_JSON, 1,
                               json.dumps(first, ensure_ascii=False).encode()))
    silence = b"\x00\x00" * 8000  # 0.5 秒静音（16k/16bit）
    for i in range(0, len(silence), 3200):
        ws.send_binary(build_frame(MT_AUDIO_ONLY_CLIENT, FLAG_HAS_SEQUENCE, SER_RAW,
                                   2 + i // 3200, silence[i:i + 3200]))
    ws.send_binary(build_frame(MT_AUDIO_ONLY_CLIENT, FLAG_LAST_PACKAGE, SER_RAW, None, b""))

    frames, events, err, shape_err = 0, [], None, None
    deadline = time.time() + 10
    while time.time() < deadline:
        try:
            d = ws.recv()
        except Exception:
            break
        if d is None:
            break
        if isinstance(d, str):
            frames += 1
            continue
        frames += 1
        try:
            f = parse_frame(d)
        except ValueError as e:
            shape_err = "%s head=%s" % (e, d[:20].hex(" "))
            break
        if f["mt"] == MT_ERROR:
            err = "code=%s %s" % (f["code"], f["payload"].decode("utf-8", "replace")[:160])
            break
        if f["event"] is not None:
            events.append(f["event"])
        # 收到末包即停
        if f["flags"] & FLAG_LAST_PACKAGE:
            break
    ws.close()

    detail = "端点=%s resource=%s key=%s｜回包 %d 帧 events=%s" % (
        url, res, mask(key), frames, ",".join(event_label(e) for e in events[:6]) or "-")
    if err:
        return AUTH if "403" in err or "45000010" in err else FAIL, detail + "｜错误帧 " + err
    if shape_err:
        return FAIL, detail + "｜帧结构不符：" + shape_err
    if frames == 0:
        return FAIL, detail + "｜服务端一个包都没回（连上了但不处理）"
    return OK, detail + "｜服务端受理（静音输入无文本属正常）"


def probe_tts(cfg, out_dir):
    """任务台 TTS：按端点协议走 HTTP 或 WebSocket。"""
    t = cfg["tts"]
    url, key = t["endpoint"], t["api_key"]
    res, voice = t.get("resource_id", ""), t["voice"]
    rate = int(t.get("target_sample_rate", 48000))
    text = "任务台已经接通，这是一段语音合成测试。"

    if url.startswith("ws://") or url.startswith("wss://"):
        return _probe_tts_ws(url, key, res, voice, rate, text, out_dir)
    return _probe_tts_http(url, key, res, voice, rate, text, out_dir)


def _probe_tts_ws(url, key, res, voice, rate, text, out_dir):
    try:
        ws = ws_connect(url, {
            "X-Api-Key": key,
            "X-Api-Resource-Id": res,
            "X-Api-Request-Id": str(uuid.uuid4()),
        })
    except Exception as e:
        return AUTH if ("403" in str(e) or "401" in str(e)) else FAIL, \
            "端点=%s resource=%s voice=%s｜握手失败 %s" % (url, res, voice, e)

    body = {"user": {"uid": "easyop"}, "req_params": {
        "text": text, "speaker": voice,
        "audio_params": {"format": "pcm", "sample_rate": rate}}}
    ws.send_binary(build_frame(MT_FULL_CLIENT, FLAG_HAS_SEQUENCE, SER_JSON, 1,
                               json.dumps(body, ensure_ascii=False).encode()))

    pcm = bytearray()
    frames, events, err, shape_err = 0, [], None, None
    deadline = time.time() + TIMEOUT_WS
    while time.time() < deadline:
        try:
            d = ws.recv()
        except Exception:
            break
        if d is None:
            break
        frames += 1
        try:
            f = parse_frame(d)
        except ValueError as e:
            shape_err = "%s head=%s" % (e, d[:20].hex(" "))
            break
        if f["mt"] == MT_ERROR:
            err = "code=%s %s" % (f["code"], f["payload"].decode("utf-8", "replace")[:200])
            break
        if f["event"] is not None:
            events.append(f["event"])
        if f["mt"] == MT_AUDIO_ONLY_SERVER:
            pcm += f["payload"]
        if f["event"] in (351, 152):  # SessionFinished 系：音频到此为止
            break
    ws.close()

    duration = len(pcm) / 2.0 / rate if rate else 0
    detail = "端点=%s resource=%s voice=%s key=%s｜%d 帧 events=%s｜音频 %.2f 秒" % (
        url, res, voice, mask(key), frames,
        ",".join(event_label(e) for e in events[:6]) or "-", duration)
    if err:
        # 55000000 = 音色与 resource 不同版本；45000010 = key 无效
        return (AUTH if "45000010" in err else FAIL), detail + "｜错误帧 " + err
    if shape_err:
        return FAIL, detail + "｜帧结构不符：" + shape_err
    if not pcm:
        return FAIL, detail + "｜0 个采样"
    path = os.path.join(out_dir, "probe_tts_ws.wav")
    _write_wav(path, pcm, rate)
    peak = _peak(pcm)
    return OK, detail + "｜已落盘 %s 峰值=%.3f" % (path, peak)


def _probe_tts_http(url, key, res, voice, rate, text, out_dir):
    body = ({"user": {"uid": "easyop"}, "req_params": {
        "text": text, "speaker": voice,
        "audio_params": {"format": "pcm", "sample_rate": rate}}})
    status, txt = http_post(url, body,
                            {"X-Api-Key": key, "X-Api-Resource-Id": res,
                             "X-Api-Request-Id": str(uuid.uuid4())})
    if status is None:
        return FAIL, "端点=%s｜%s" % (url, txt)
    if status != 200:
        return AUTH if status in (401, 403) else FAIL, \
            "端点=%s｜HTTP %s %s" % (url, status, fetch_error(txt))
    pcm = bytearray()
    err = None
    for line in txt.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            v = json.loads(line)
        except Exception:
            continue
        code = v.get("code", 0)
        if code not in (0, None) and code != 20000000:
            err = "code=%s %s" % (code, v.get("message", ""))
            break
        data = v.get("data")
        if data:
            import base64
            pcm += base64.b64decode(data + "=" * (-len(data) % 4))
    duration = len(pcm) / 2.0 / rate if rate else 0
    detail = "端点=%s resource=%s voice=%s｜音频 %.2f 秒" % (url, res, voice, duration)
    if err:
        return FAIL, detail + "｜" + err
    if not pcm:
        return FAIL, detail + "｜0 个采样"
    path = os.path.join(out_dir, "probe_tts_http.wav")
    _write_wav(path, pcm, rate)
    return OK, detail + "｜已落盘 %s" % path


def probe_driver(drv):
    """任务台 Api driver：按 endpoint 直发 chat/completions。

    ⚠️ driver 的 endpoint 会被**原样 POST**（见 `driver/api_driver.rs`：它把
    `driver.endpoint` 直接交给 `chat_stream`），所以**必须写全到 `/chat/completions`**。
    少写这一段的配置在界面上完全看不出问题，真跑起来是 404 —— 实测过，
    所以这里在裸端点失败时会自动补一段再试一次，并把"补哪一段才通"写进结论。
    """
    if drv.get("kind") != "api":
        return None
    url, model = drv["endpoint"], drv["model"]
    key = drv.get("api_key", "")
    if not drv.get("enabled"):
        return "SKIP", "id=%s（未启用，本次不验证）端点=%s model=%s" % (drv["id"], url, model)
    if not model.strip():
        return FAIL, "id=%s 未填模型名" % drv["id"]

    if url.rstrip("/").endswith("/chat/completions"):
        return probe_chat_endpoint(url, model, key, "driver:%s" % drv["id"])

    verdict, detail = probe_chat_endpoint(url, model, key, "driver:%s" % drv["id"])
    if verdict == OK:
        return OK, detail

    # 裸端点失败 → 补 /chat/completions 再试一次，把话说明白。
    fixed = url.rstrip("/") + "/chat/completions"
    verdict2, detail2 = probe_chat_endpoint(fixed, model, key, "driver:%s" % drv["id"])
    if verdict2 == OK:
        return FAIL, ("id=%s 端点写的是 %s，实际要 POST 的是 %s —— 少写 `/chat/completions` "
                      "会 404。补上之后是通的：%s" % (drv["id"], url, fixed, detail2))
    return verdict2, "%s｜补 `/chat/completions` 后仍失败：%s" % (detail, detail2)


# ---------------------------------------------------------------------------
# 小工具
# ---------------------------------------------------------------------------

def _write_wav(path, pcm_bytes, rate):
    import wave
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(bytes(pcm_bytes))


def _peak(pcm_bytes):
    n = len(pcm_bytes) // 2
    if n == 0:
        return 0.0
    samples = struct.unpack("<%dh" % n, pcm_bytes[:n * 2])
    return max(abs(s) for s in samples) / 32768.0


def main():
    which = [a.lower() for a in sys.argv[1:]] or ["llm", "asr", "tts", "drivers"]
    out_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "_probe_out")
    os.makedirs(out_dir, exist_ok=True)

    cfg = load_json("config.json")
    desk = load_json("agent_desk.json")
    print("配置目录：%s" % APP_DIR)
    print("=" * 78)

    rows = []
    if "llm" in which:
        rows.append(("语音编辑 LLM",) + probe_llm_edit(cfg))
    if "asr" in which:
        rows.append(("流式 ASR",) + probe_asr(cfg))
    if "tts" in which:
        rows.append(("任务台 TTS",) + probe_tts(desk, out_dir))
    if "drivers" in which:
        for d in desk.get("drivers", []):
            r = probe_driver(d)
            if r:
                rows.append(("driver:%s" % d["id"],) + r)

    width = max(len(r[0]) for r in rows) if rows else 10
    bad = 0
    for name, verdict, detail in rows:
        if verdict in (FAIL, "AUTH"):
            bad += 1
        print("[%s] %-*s  %s" % (verdict, width, name, detail))
    print("=" * 78)
    print("结论：%d/%d 通过；未通过 %d" % (len(rows) - bad, len(rows), bad))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
