"""语音识别链路探测：只做一次 WebSocket 握手，判断"服务端鉴权/网络"这一环通不通。

为什么要单独有这么一个脚本：语音识别"没反应"可能死在很多地方
（麦克风没采到数据 / 板子没推流 / 网络不通 / Key 失效 / 额度用尽 / 服务端不响应），
而从现象上它们**长得一模一样**。这个脚本只回答其中一个问题：
**连着这个 Key、从这台机器出去，语音服务收不收。**

配置全部从应用自己的配置里读（不在这里写死任何密钥）：
    %APPDATA%\\com.easyop.app\\config.json

判读：
    101 Switching Protocols  → 网络与鉴权都通，问题在本机录音侧（看 mic_source / 板子推流）
    401 / 403                → Key 失效、无权限或被停用
    其他 4xx/5xx             → 服务端或域名问题
    PROBE_FAILED             → 本机到服务端根本连不上（网络/代理/DNS）

用法：
    python scripts/probe_asr.py
"""

import base64
import json
import os
import socket
import ssl
import sys
import uuid
from urllib.parse import urlparse

CONFIG = os.path.join(
    os.environ.get("APPDATA", ""), "com.easyop.app", "config.json"
)


def load_config() -> dict:
    if not os.path.exists(CONFIG):
        print(f"找不到配置：{CONFIG}")
        sys.exit(2)
    with open(CONFIG, "r", encoding="utf-8") as f:
        return json.load(f)


def main() -> int:
    cfg = load_config()
    url = urlparse(cfg.get("asr_url", ""))
    api_key = cfg.get("api_key", "")
    resource_id = cfg.get("resource_id", "")

    if not url.hostname or url.scheme != "wss":
        print(f"asr_url 不是 wss 地址：{cfg.get('asr_url')!r}")
        return 2
    if not api_key:
        print("配置里没有 api_key —— 先在应用的设置里填好再说。")
        return 2

    print(f"目标：{url.hostname}{url.path}")
    print(f"Resource-Id：{resource_id}")

    key_b64 = base64.b64encode(os.urandom(16)).decode()
    req = (
        f"GET {url.path} HTTP/1.1\r\n"
        f"Host: {url.hostname}\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key_b64}\r\n"
        "Sec-WebSocket-Version: 13\r\n"
        f"X-Api-Key: {api_key}\r\n"
        f"X-Api-Resource-Id: {resource_id}\r\n"
        f"X-Api-Request-Id: {uuid.uuid4()}\r\n"
        "X-Api-Sequence: -1\r\n"
        "\r\n"
    )

    try:
        raw = socket.create_connection((url.hostname, url.port or 443), timeout=15)
        with ssl.create_default_context().wrap_socket(
            raw, server_hostname=url.hostname
        ) as sock:
            sock.sendall(req.encode())
            data = sock.recv(4096).decode("utf-8", "replace")
    except Exception as e:  # noqa: BLE001 — 探测脚本，任何异常都是"连不上"
        print(f"PROBE_FAILED: {type(e).__name__}: {e}")
        print("→ 本机到服务端不通（网络 / 代理 / DNS）。")
        return 1

    status = data.split("\r\n", 1)[0]
    print(f"HANDSHAKE: {status}")
    head, _, body = data.partition("\r\n\r\n")
    if body.strip():
        print(f"BODY: {body[:600]}")

    if "101" in status:
        print("→ 网络与鉴权都通。若应用里还是识别不出字，问题在本机录音侧。")
        return 0
    print("→ 服务端拒绝了这次握手，按上面的状态码判断（401/403 = Key 问题）。")
    return 1


if __name__ == "__main__":
    sys.exit(main())
