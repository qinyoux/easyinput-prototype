#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""端到端语音识别探针：用真实音频按 asr.rs 的协议真发一遍，看服务端到底回不回文字。

为什么需要它
------------
`scripts/probe_asr.py` 只做 WebSocket 握手（101）—— 那只能证明"地址通、Key 没被拒"，
**证明不了服务端会返回识别结果**。本脚本把完整链路走完：

    建连 → 首包(JSON 参数) → 逐帧上行 PCM(16k/mono/s16le) → 负包 → 收文本

于是三种病可以被分开：

  A. 握手失败 / 首包被拒   → 服务端或鉴权问题（与本机录音无关）
  B. 一路正常但**没有文本** → 音频内容不可识别，或服务端配额/模型问题
  C. 正常返回文本           → 服务端好的 ⇒ 问题在**本机录音侧**（麦克风/上行）

用法
----
    python scripts/probe_asr_e2e.py [音频.wav ...]

不给参数就用应用自己的 TTS 缓存音频（中文语音，48k 自动重采样到 16k）。
配置（Key / 地址 / Resource-Id）从应用 config.json 读，不在这里写死密钥。
"""

import base64
import glob
import json
import os
import socket
import ssl
import struct
import sys
import time
import uuid
from array import array
from urllib.parse import urlparse

# --- 协议常量（与 src-tauri/src/asr.rs 保持一致） -------------------------
MT_FULL_CLIENT_REQUEST = 0b0001
MT_AUDIO_ONLY_REQUEST = 0b0010
MT_FULL_SERVER_RESPONSE = 0b1001
MT_ERROR = 0b1111
FLAG_HAS_SEQUENCE = 0b0001
FLAG_LAST_PACKAGE = 0b0010
SER_JSON = 0b0001
SER_RAW = 0b0000

CONFIG = os.path.join(os.environ.get("APPDATA", ""), "com.easyop.app", "config.json")

# 负包写法的候选（用 --last-mode=N 选，默认 1 = 旧写法）。
LAST_MODE = 1


def load_config():
    with open(CONFIG, "r", encoding="utf-8") as f:
        return json.load(f)


# --- WAV → 16k/mono/s16le --------------------------------------------------
def read_wav_16k(path):
    """读任意 WAV，重采样到 16kHz 单声道 16bit PCM（含简单抗混叠平均）。"""
    import wave

    w = wave.open(path, "rb")
    ch, rate, sw, n = w.getnchannels(), w.getframerate(), w.getsampwidth(), w.getnframes()
    raw = w.readframes(n)
    w.close()

    if sw != 2:
        raise ValueError(f"只支持 16bit WAV，当前 {sw * 8}bit")

    samples = array("h")
    samples.frombytes(raw)
    if sys.byteorder == "big":
        samples.byteswap()

    if ch == 2:  # 混成单声道
        mono = array("h", [0]) * (len(samples) // 2)
        for i in range(len(mono)):
            mono[i] = (samples[2 * i] + samples[2 * i + 1]) // 2
        samples = mono

    if rate != 16000:
        ratio = rate / 16000.0
        step = rate // 16000 if rate % 16000 == 0 else None
        out = array("h")
        if step:
            # 整数倍降采样：先做 step 点滑动平均再抽取，压掉高频镜像。
            acc = 0
            for i, s in enumerate(samples):
                acc += s
                if (i + 1) % step == 0:
                    out.append(acc // step)
                    acc = 0
        else:
            count = int(len(samples) / ratio)
            for i in range(count):
                out.append(samples[int(i * ratio)])
        samples = out

    if sys.byteorder == "big":
        samples.byteswap()
    return samples.tobytes()


# --- 极简 WebSocket 客户端（零依赖；只做本探针需要的事） --------------------
class WS:
    def __init__(self, url, headers):
        u = urlparse(url)
        self.sock = socket.create_connection((u.hostname, u.port or 443), timeout=10)
        ctx = ssl.create_default_context()
        self.sock = ctx.wrap_socket(self.sock, server_hostname=u.hostname)
        key = base64.b64encode(os.urandom(16)).decode()
        path = u.path + (("?" + u.query) if u.query else "")
        req = [
            f"GET {path} HTTP/1.1",
            f"Host: {u.hostname}",
            "Upgrade: websocket",
            "Connection: Upgrade",
            f"Sec-WebSocket-Key: {key}",
            "Sec-WebSocket-Version: 13",
        ]
        req += [f"{k}: {v}" for k, v in headers.items()]
        self.sock.sendall(("\r\n".join(req) + "\r\n\r\n").encode())

        # 读握手响应头
        buf = b""
        while b"\r\n\r\n" not in buf:
            chunk = self.sock.recv(4096)
            if not chunk:
                break
            buf += chunk
        head, _, rest = buf.partition(b"\r\n\r\n")
        self.buf = rest
        status = head.split(b"\r\n")[0].decode("latin1")
        self.status = status
        self.handshake_headers = head.decode("latin1", "replace")

    def _read_exact(self, n):
        while len(self.buf) < n:
            chunk = self.sock.recv(65536)
            if not chunk:
                raise ConnectionError("服务端关闭连接")
            self.buf += chunk
        out, self.buf = self.buf[:n], self.buf[n:]
        return out

    def send_binary(self, payload):
        mask = os.urandom(4)
        n = len(payload)
        hdr = bytearray([0x82])  # FIN + binary
        if n < 126:
            hdr.append(0x80 | n)
        elif n < 65536:
            hdr.append(0x80 | 126)
            hdr += struct.pack(">H", n)
        else:
            hdr.append(0x80 | 127)
            hdr += struct.pack(">Q", n)
        hdr += mask
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        self.sock.sendall(bytes(hdr) + masked)

    def recv_message(self):
        """返回 (opcode, payload)；连接关闭返回 (None, None)。"""
        b0, b1 = self._read_exact(2)
        opcode = b0 & 0x0F
        masked = b1 & 0x80
        n = b1 & 0x7F
        if n == 126:
            n = struct.unpack(">H", self._read_exact(2))[0]
        elif n == 127:
            n = struct.unpack(">Q", self._read_exact(8))[0]
        mkey = self._read_exact(4) if masked else None
        data = self._read_exact(n) if n else b""
        if mkey:
            data = bytes(b ^ mkey[i % 4] for i, b in enumerate(data))
        if opcode == 0x8:
            return None, None
        if opcode == 0x9:  # ping → pong
            return "ping", data
        return opcode, data

    def close(self):
        try:
            self.sock.close()
        except Exception:
            pass


# --- 与 Rust 侧同构的帧构造 -------------------------------------------------
def build_frame(mt, flags, ser, seq, payload):
    v = bytearray([0x11, (mt << 4) | flags, ser << 4, 0x00])
    if seq is not None:
        v += struct.pack(">i", seq)
    v += struct.pack(">I", len(payload))
    v += payload
    return bytes(v)


def parse_frame(data):
    """→ (kind, text_or_error, is_last)；kind ∈ {server, error, other}"""
    if len(data) < 8:
        return "other", None, False
    mt = data[1] >> 4
    flags = data[1] & 0x0F
    off = 4
    seq = 0
    if flags & FLAG_HAS_SEQUENCE:
        seq = struct.unpack(">i", data[off:off + 4])[0]
        off += 4
    size = struct.unpack(">I", data[off:off + 4])[0]
    off += 4
    payload = data[off:off + size]
    is_last = bool(flags & FLAG_LAST_PACKAGE) or seq < 0
    if mt == MT_FULL_SERVER_RESPONSE:
        try:
            v = json.loads(payload)
        except Exception:
            return "server", repr(payload[:200]), is_last
        t = None
        for path in (("payload_msg", "result", "text"), ("result", "text")):
            cur = v
            for k in path:
                cur = cur.get(k) if isinstance(cur, dict) else None
            if isinstance(cur, str):
                t = cur
                break
        return "server", t if t is not None else json.dumps(v, ensure_ascii=False)[:300], is_last
    if mt == MT_ERROR:
        return "error", payload.decode("utf-8", "replace"), is_last
    return "other", None, is_last


def probe(wav, cfg, verbose=True):
    """跑一次完整识别，返回 (成功?, 说明)"""
    api_key = cfg["api_key"]
    url = cfg["asr_url"]
    rid = cfg["resource_id"]

    pcm = read_wav_16k(wav)
    print(f"\n{'=' * 72}")
    print(f"音频: {os.path.basename(wav)}  ({len(pcm)} 字节 PCM / 16k mono / "
          f"{len(pcm) / 32000:.2f}s)")
    print(f"端点: {url}")
    print("=" * 72)

    try:
        ws = WS(url, {
            "X-Api-Key": api_key,
            "X-Api-Resource-Id": rid,
            "X-Api-Request-Id": str(uuid.uuid4()),
            "X-Api-Sequence": "-1",
        })
    except Exception as e:
        print(f"❌ 建连失败: {e}")
        return False, f"建连失败: {e}"

    if "101" not in ws.status:
        print(f"❌ 握手非 101: {ws.status}")
        if verbose:
            print(ws.handshake_headers[:800])
        ws.close()
        return False, f"握手 {ws.status}"

    print(f"✅ 握手: {ws.status.strip()}")
    # 回显服务端握手头里的日志 ID，便于找对端工单
    for line in ws.handshake_headers.split("\r\n"):
        if any(k in line.lower() for k in ("x-tt-logid", "logid", "x-api")):
            print(f"   {line.strip()}")

    # 首包
    first_json = {
        "audio": {"format": "pcm", "codec": "raw", "rate": 16000, "bits": 16, "channel": 1},
        "request": {
            "model_name": "bigmodel",
            "enable_itn": True,
            "enable_punc": True,
            "enable_ddc": True,
            "result_type": "full",
            "language": "zh-CN",
        },
    }
    if verbose:
        print(f"→ 首包 JSON: {json.dumps(first_json, ensure_ascii=False)}")
    ws.send_binary(build_frame(MT_FULL_CLIENT_REQUEST, FLAG_HAS_SEQUENCE, SER_JSON, 1,
                               json.dumps(first_json).encode()))

    # 上行音频（按 100ms 节流，模拟实时流）+ 边收边打印
    chunk = 3200
    seq = 2
    frames = 0
    got_server = 0
    text = ""
    errors = []
    deadline = time.time() + 60

    def drain(timeout=0.05):
        nonlocal got_server, text, errors
        ws.sock.settimeout(timeout)
        try:
            while True:
                op, data = ws.recv_message()
                if op is None:
                    return False
                if op == "ping":
                    continue
                if op == 0x2:
                    got_server += 1
                    kind, val, last = parse_frame(data)
                    if kind == "server":
                        print(f"   ← 回包#{got_server} 文本: {val!r}")
                        if val:
                            text = val
                    elif kind == "error":
                        print(f"   ← ❌ 服务端错误帧: {val}")
                        errors.append(val)
                    else:
                        print(f"   ← 回包#{got_server} (其它): {data[:40].hex()}")
                elif op == 0x1:
                    print(f"   ← 文本帧: {data[:200]!r}")
        except socket.timeout:
            return True
        except ConnectionError:
            return False
        except ssl.SSLWantReadError:
            return True

    for i in range(0, len(pcm), chunk):
        ws.send_binary(build_frame(MT_AUDIO_ONLY_REQUEST, FLAG_HAS_SEQUENCE, SER_RAW, seq,
                                   pcm[i:i + chunk]))
        seq += 1
        frames += 1
        drain(0.02)
        time.sleep(0.1)
        if time.time() > deadline:
            print("⏰ 总时长超限，提前收尾")
            break

    # 负包。**这里正是本脚本存在的理由** —— 负包的 sequence 写法有多种候选，
    # 服务端只认其中一种，写错了不会立刻报错，而是"文本都识别出来了却整段作废"。
    if LAST_MODE == 1:
        last_seq, last_flags, desc = -1, FLAG_HAS_SEQUENCE | FLAG_LAST_PACKAGE, "seq=-1（旧写法）"
    elif LAST_MODE == 2:
        last_seq, last_flags, desc = -seq, FLAG_HAS_SEQUENCE | FLAG_LAST_PACKAGE, \
            f"seq={-seq}（下一个序号的负数）"
    elif LAST_MODE == 3:
        last_seq, last_flags, desc = None, FLAG_LAST_PACKAGE, "不带 sequence，仅 LAST 标志"
    elif LAST_MODE == 4:
        last_seq, last_flags, desc = seq, FLAG_HAS_SEQUENCE | FLAG_LAST_PACKAGE, \
            f"seq={seq}（正的下一序号）"
    else:
        last_seq, last_flags, desc = -1, FLAG_HAS_SEQUENCE | FLAG_LAST_PACKAGE, "seq=-1"
    print(f"→ 上行 {frames} 帧音频，发负包 [{desc}]")
    ws.send_binary(build_frame(MT_AUDIO_ONLY_REQUEST, last_flags, SER_RAW, last_seq, b""))

    # 收尾
    end = time.time() + 15
    while time.time() < end:
        if not drain(0.2):
            break

    ws.close()
    print(f"\n结果：回包 {got_server} 个，错误 {len(errors)} 个，文本 = {text!r}")
    if errors:
        return False, f"服务端返回错误：{errors[0]}"
    if not got_server:
        return False, "服务端一个回包都没有（首包被无视 / 音频未被接受）"
    if not text:
        return False, "服务端有回包但没有文本（音频不可识别）"
    return True, f"识别成功：{text}"


def main():
    global LAST_MODE
    argv = [a for a in sys.argv[1:] if not a.startswith("--")]
    modes = [1]
    for a in sys.argv[1:]:
        if a.startswith("--last-mode="):
            modes = [int(x) for x in a.split("=", 1)[1].split(",")]
        elif a == "--compare":
            modes = [1, 2, 3, 4]

    cfg = load_config()
    wavs = argv
    if not wavs:
        base = os.path.join(os.environ["APPDATA"], "com.easyop.app", "agent_desk_out")
        wavs = sorted(glob.glob(os.path.join(base, "tts_preview_*.wav")))[:1]
        wavs += sorted(glob.glob(os.path.join(base, "notice_assets", "*.wav")))[:1]
    if not wavs:
        print("找不到测试音频，请显式传入 .wav 路径")
        return 2

    verdicts = []
    for mode in modes:
        LAST_MODE = mode
        for w in wavs:
            try:
                ok, why = probe(w, cfg)
            except Exception as e:
                ok, why = False, f"异常: {e}"
            verdicts.append((f"[负包写法{mode}] {os.path.basename(w)}", ok, why))

    print("\n" + "=" * 72)
    print("判定")
    print("=" * 72)
    for name, ok, why in verdicts:
        print(f"{'✅' if ok else '❌'} {name}: {why}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
