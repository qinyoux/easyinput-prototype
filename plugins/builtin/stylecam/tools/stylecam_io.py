#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""StyleCam 配置 v2 与生图协议（纯标准库）。"""
from __future__ import annotations

import argparse
import base64
import json
import os
import random
import re
import shutil
import socket
import sys
import tempfile
import time
import traceback
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime
from pathlib import Path

for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass

PROVIDERS = {
    "ark": {
        "name": "火山方舟", "endpoint": "https://ark.cn-beijing.volces.com/api/v3/images/generations",
        "models": ["doubao-seedream-5-0-250128", "doubao-seedream-4-5-251128", "doubao-seedream-5-0-lite-260128"],
        "i2i": "image_array", "size_mode": "tier", "default_size": "2K", "response": "b64_json_field",
        "formats": ["auto", "jpeg", "png"], "quality": False, "max_n": 1,
        "extra": {"response_format": "b64_json", "watermark": False}, "easyop_fallback": True,
    },
    "openai_compat": {
        "name": "OpenAI 兼容", "endpoint": "https://api.openai.com/v1/images/generations",
        "models": ["gpt-image-1", "dall-e-3"], "i2i": None, "size_mode": "px",
        "default_size": "1024x1024", "response": "b64_or_url", "formats": ["auto", "jpeg", "png"],
        "quality": "model", "max_n": 4, "extra": {"response_format": "b64_json"}, "easyop_fallback": False,
    },
    "zhipu": {
        "name": "智谱", "endpoint": "https://open.bigmodel.cn/api/paas/v4/images/generations",
        "models": ["cogview-4", "glm-image"], "i2i": None, "size_mode": "px", "default_size": "1024x1024",
        "response": "url_field", "formats": ["auto"], "quality": False, "max_n": 1, "extra": {},
        "easyop_fallback": False,
    },
    "easysu": {
        "name": "EasySU 中转", "endpoint": "https://aigc.easysu.cn/v1/images/generations",
        "i2i_endpoint": "https://aigc.easysu.cn/v1/images/edits", "models": ["gpt-image-2.5-flare"],
        "i2i": "edits_json", "i2i_field": "image", "size_mode": "res_ratio", "default_size": "1K",
        "default_ratio": "1:1", "resolutions": ["1K", "2K", "4K"],
        "ratios": ["1:1", "16:9", "9:16", "4:3", "3:4"], "response": "b64_or_url",
        "formats": ["auto", "jpeg", "png"], "quality": True, "max_n": 4,
        "extra": {"response_format": "b64_json"}, "easyop_fallback": False,
    },
}
DEFAULT_PROVIDER = "easysu"
CONFIG_VERSION = 2
CONFIG_DEFAULTS = {
    "v": 2, "provider": "easysu", "baseUrl": "", "model": "", "resolution": "1K",
    "width": 1024, "height": 1024, "format": "auto", "quality": None, "n": 1,
    "httpTimeoutSecs": 120, "retryCount": 1, "outDir": "", "hwKeys": True,
    "privacyAcked": False, "hwIntroSeen": False, "pyExe": "",
}
ENV_FILE_NAME = "stylecam_io.env"
STORAGE_FILE = "storage.json"
EASYOP_CONFIG = Path(os.environ.get("APPDATA", "")) / "com.easyop.app" / "config.json"
ENV_KEY_NEW, ENV_KEY_LEGACY = "IMAGE_API_KEY", "ARK_API_KEY"
ENV_MODEL_NEW, ENV_MODEL_LEGACY = "IMAGE_MODEL", "MODEL"
ENV_PROVIDER, ENV_BASE_URL = "PROVIDER", "IMAGE_BASE_URL"
ENV_MAP = {
    "provider": "PROVIDER", "baseUrl": "IMAGE_BASE_URL", "model": "IMAGE_MODEL", "resolution": "RESOLUTION",
    "width": "WIDTH", "height": "HEIGHT", "format": "FORMAT", "quality": "QUALITY", "n": "N",
    "httpTimeoutSecs": "HTTP_TIMEOUT_SECS", "retryCount": "RETRY_COUNT", "outDir": "OUT_DIR",
}
CHUNK_DEFAULT = 65536
MAX_B64_BYTES = 12 * 1024 * 1024
MAX_DL_BYTES = 12 * 1024 * 1024
MAX_IMAGES = 15
RETRYABLE_STATUS = {408, 425, 429, 500, 502, 503, 504}
ERRS = {"E_ARGS", "E_NO_JOB", "E_STORAGE", "E_NO_KEY", "E_IMG", "E_API", "E_NET", "E_IO", "E_PARSE", "E_SECRET"}
EXIT_OK, EXIT_PRE, EXIT_GEN, EXIT_IO = 0, 2, 3, 4
RATIOS = [("1:1", 1.0), ("16:9", 16 / 9), ("9:16", 9 / 16), ("4:3", 4 / 3), ("3:4", 3 / 4)]


def out(line): print(line, flush=True)
def msg(text): out("MSG=" + str(text).replace("\r", " ").replace("\n", " "))
def die(code, text, exit_code):
    out("OK=0"); out("ERR=" + code); msg(text); return exit_code


def paths():
    script = Path(__file__).resolve(); plugin = script.parent.parent
    return {"plugin_dir": plugin, "storage": plugin / STORAGE_FILE, "env": plugin / "tools" / ENV_FILE_NAME, "script": script}


def default_out_dir():
    return Path(os.environ.get("USERPROFILE") or os.path.expanduser("~")) / "Pictures" / "easyop-stylecam"


def read_env_file(path):
    result = {}
    try: text = Path(path).read_text(encoding="utf-8")
    except OSError: return result
    for line in text.splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            key, value = line.split("=", 1); result[key.strip()] = value.strip().strip('"').strip("'")
    return result


def load_storage(path):
    try: data = json.loads(Path(path).read_text(encoding="utf-8"))
    except OSError as exc: raise RuntimeError("E_STORAGE: 读不了 storage.json：%s" % exc)
    except json.JSONDecodeError as exc: raise RuntimeError("E_STORAGE: storage.json 不是合法 JSON：%s" % exc)
    if not isinstance(data, dict): raise RuntimeError("E_STORAGE: storage.json 顶层不是对象")
    return data


def get_job(path, key):
    job = load_storage(path).get(key)
    if not isinstance(job, dict): raise RuntimeError("E_NO_JOB: storage.json 里没有对象 key=%s" % key)
    return job


def migrate_config(raw):
    src = raw if isinstance(raw, dict) else {}
    cfg = dict(CONFIG_DEFAULTS)
    for key in cfg:
        if key in src: cfg[key] = src[key]
    size = str(src.get("size") or "").strip()
    if "resolution" not in src and size in ("1K", "2K", "4K"): cfg["resolution"] = size
    match = re.fullmatch(r"(\d+)x(\d+)", size, re.I)
    if match:
        if "width" not in src: cfg["width"] = int(match.group(1))
        if "height" not in src: cfg["height"] = int(match.group(2))
    cfg["v"] = CONFIG_VERSION
    return cfg


def _coerce_env_value(key, value):
    if value in (None, ""): return None
    if key in ("width", "height", "quality", "n", "httpTimeoutSecs", "retryCount"):
        if key == "quality" and str(value).lower() in ("auto", "null", "none"): return None
        try: return int(value)
        except (TypeError, ValueError): return value
    return value


def merge_effective_config(cli, job, stored, env):
    cfg = migrate_config(stored)
    sources = {key: "config" if isinstance(stored, dict) and key in stored else "provider_default" for key in cfg}
    for key, env_name in ENV_MAP.items():
        if env.get(env_name) not in (None, "") and (not isinstance(stored, dict) or key not in stored):
            cfg[key] = _coerce_env_value(key, env[env_name]); sources[key] = "env"
    for source_name, source in (("job", job or {}), ("cli", cli or {})):
        for key in cfg:
            if key in source and source[key] not in (None, ""):
                cfg[key] = source[key]; sources[key] = source_name
    cfg["v"] = CONFIG_VERSION
    return cfg, sources


def validate_base_url(value):
    if not value: return
    if len(value) > 2048: raise RuntimeError("E_ARGS: baseUrl 超过 2048 字符")
    parsed = urllib.parse.urlsplit(value)
    if parsed.scheme == "https" and parsed.netloc: return
    if parsed.scheme == "http" and parsed.hostname in ("localhost", "127.0.0.1", "::1"): return
    raise RuntimeError("E_ARGS: baseUrl 必须是 HTTPS；仅 localhost 可使用 HTTP")


def validate_config(cfg):
    if cfg.get("provider") not in PROVIDERS: raise RuntimeError("E_ARGS: 未知 provider=%s" % cfg.get("provider"))
    if cfg.get("resolution") not in ("1K", "2K", "4K"): raise RuntimeError("E_ARGS: resolution 必须是 1K/2K/4K")
    for key in ("width", "height"):
        value = cfg.get(key)
        if not isinstance(value, int) or isinstance(value, bool) or not 256 <= value <= 4096 or value % 64:
            raise RuntimeError("E_ARGS: %s 必须是 256..4096 且为 64 的倍数" % key)
    if cfg["width"] * cfg["height"] > 16777216: raise RuntimeError("E_ARGS: width * height 超过 16777216")
    if cfg.get("format") not in ("auto", "jpeg", "png"): raise RuntimeError("E_ARGS: format 必须是 auto/jpeg/png")
    quality = cfg.get("quality")
    if quality is not None and (not isinstance(quality, int) or isinstance(quality, bool) or not 1 <= quality <= 100):
        raise RuntimeError("E_ARGS: quality 必须是 null 或 1..100 整数")
    for key, low, high in (("n", 1, 4), ("httpTimeoutSecs", 10, 240), ("retryCount", 0, 3)):
        value = cfg.get(key)
        if not isinstance(value, int) or isinstance(value, bool) or not low <= value <= high:
            raise RuntimeError("E_ARGS: %s 必须是 %d..%d 整数" % (key, low, high))
    validate_base_url(str(cfg.get("baseUrl") or ""))
    if len(str(cfg.get("model") or "")) > 256: raise RuntimeError("E_ARGS: model 超过 256 字符")
    if len(str(cfg.get("outDir") or "")) > 2048: raise RuntimeError("E_ARGS: outDir 超过 2048 字符")
    return cfg


def provider_capabilities(provider_key, model=""):
    prov = PROVIDERS[provider_key]
    formats, quality, max_n = list(prov["formats"]), prov["quality"], prov["max_n"]
    if provider_key == "openai_compat":
        if model == "dall-e-3" or (model and not model.startswith("gpt-image-")):
            formats, quality, max_n = ["auto"], False, 1
        else:
            quality = True
    return {"formats": formats, "quality": bool(quality), "maxN": max_n, "i2i": bool(prov.get("i2i")),
            "sizeMode": prov["size_mode"]}


def validate_capabilities(cfg, model):
    cap = provider_capabilities(cfg["provider"], model)
    if cfg["format"] not in cap["formats"]: raise RuntimeError("E_ARGS: CAPABILITY_UNSUPPORTED format=%s" % cfg["format"])
    if cfg["quality"] is not None:
        if not cap["quality"]: raise RuntimeError("E_ARGS: CAPABILITY_UNSUPPORTED quality")
        if cfg["format"] != "jpeg": raise RuntimeError("E_ARGS: quality 仅在 format=jpeg 时可用")
    if cfg["n"] > cap["maxN"]: raise RuntimeError("E_ARGS: CAPABILITY_UNSUPPORTED n 最大为 %d" % cap["maxN"])
    return cap


def map_dimensions(provider_key, width, height, resolution):
    if provider_key == "easysu":
        actual = width / height; best = RATIOS[0]; error = float("inf")
        for candidate in RATIOS:
            rel = abs(actual - candidate[1]) / candidate[1]
            if rel < error: best, error = candidate, rel
        if error > 0.03:
            raise RuntimeError("E_ARGS: DIMENSION_UNREPRESENTABLE %dx%d 最近 %s 误差 %.2f%%" % (width, height, best[0], error * 100))
        return {"resolution": resolution, "ratio": best[0]}
    if provider_key == "ark":
        if width != height: raise RuntimeError("E_ARGS: DIMENSION_UNREPRESENTABLE Ark 只接受方形")
        return {"size": resolution}
    return {"size": "%dx%d" % (width, height)}


def resolve_provider(cli_provider, env_kv, job=None, config=None):
    name = str(cli_provider or (job or {}).get("provider") or (config or {}).get("provider") or env_kv.get(ENV_PROVIDER) or DEFAULT_PROVIDER).strip().lower()
    if name not in PROVIDERS: raise RuntimeError("E_ARGS: 未知 provider=%s" % name)
    return name, PROVIDERS[name]


def resolve_endpoint(prov, cli_base, env_kv, job=None, config=None):
    base = str(cli_base or (job or {}).get("baseUrl") or (config or {}).get("baseUrl") or env_kv.get(ENV_BASE_URL) or "").strip().rstrip("/")
    if not base: return prov["endpoint"]
    validate_base_url(base)
    if base.endswith("/images/generations") or base.endswith("/images/edits"): return base
    return base + "/images/generations"


def resolve_model(cli_model, job, env_kv, prov, config=None):
    for candidate in (cli_model, (job or {}).get("model"), (config or {}).get("model"), env_kv.get(ENV_MODEL_NEW), env_kv.get(ENV_MODEL_LEGACY)):
        if str(candidate or "").strip(): return str(candidate).strip()
    return prov["models"][0]


def read_easyop_key():
    try: data = json.loads(EASYOP_CONFIG.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError): return ""
    return str(data.get("llm_api_key") or "").strip() if isinstance(data, dict) else ""


def resolve_key(cli_key, env_kv, prov):
    if str(cli_key or "").strip(): return str(cli_key).strip(), "cli"
    value = str(env_kv.get(ENV_KEY_NEW) or env_kv.get(ENV_KEY_LEGACY) or "").strip()
    if value and not value.startswith("在这里填"): return value, "env"
    if prov.get("easyop_fallback"):
        value = read_easyop_key()
        if value: return value, "easyop"
    return "", "none"


def mask_key(key): return (key[:4] + "***") if key else ""


def check_outdir(path):
    try: path.mkdir(parents=True, exist_ok=True)
    except OSError as exc: raise RuntimeError("E_IO: 建不了输出目录 %s：%s" % (path, exc))
    if not os.access(str(path), os.W_OK): raise RuntimeError("E_IO: 输出目录不可写：%s" % path)
    return path


def write_env_value(env_path, key_name, value):
    env_path = Path(env_path); lines = []
    try: lines = env_path.read_text(encoding="utf-8").splitlines()
    except OSError: pass
    output, hit = [], False
    for line in lines:
        if line.strip().startswith(key_name + "="):
            output.append("%s=%s" % (key_name, value)); hit = True
        else: output.append(line)
    if not hit: output.append("%s=%s" % (key_name, value))
    env_path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(prefix=env_path.name + ".", suffix=".tmp", dir=str(env_path.parent))
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as stream: stream.write("\n".join(output) + "\n")
        os.replace(tmp, env_path)
    except Exception:
        try: os.unlink(tmp)
        except OSError: pass
        raise


def atomic_update_storage(path, data):
    path = Path(path); path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(prefix=path.name + ".", suffix=".tmp", dir=str(path.parent))
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as stream: json.dump(data, stream, ensure_ascii=False, separators=(",", ":"))
        os.replace(tmp, path)
    except Exception:
        try: os.unlink(tmp)
        except OSError: pass
        raise


def consume_storage_secret(storage_path, storage_key, nonce, now=None):
    data = load_storage(storage_path); secret = data.get(storage_key); data[storage_key] = None
    try: atomic_update_storage(storage_path, data)
    except OSError as exc: raise RuntimeError("E_SECRET: SECRET_CLEANUP_FAILED %s" % exc)
    if not isinstance(secret, dict): raise RuntimeError("E_SECRET: secret 不存在")
    if str(secret.get("nonce") or "") != str(nonce or ""): raise RuntimeError("E_SECRET: nonce 不匹配")
    created = secret.get("createdAt", secret.get("at", 0)); current = time.time() * 1000 if now is None else now
    if not isinstance(created, (int, float)) or abs(current - created) > 5 * 60 * 1000: raise RuntimeError("E_SECRET: secret 已过期")
    value = str(secret.get("apiKey", secret.get("value", "")) or "").strip()
    if not 8 <= len(value) < 4096 or "\n" in value or "\r" in value: raise RuntimeError("E_SECRET: Key 长度或格式无效")
    return value


def http_call_once(url, key, payload=None, timeout=120, method=None):
    data = json.dumps(payload, ensure_ascii=False).encode("utf-8") if payload is not None else None
    request = urllib.request.Request(url, data=data, method=method or ("POST" if data is not None else "GET"))
    if key: request.add_header("Authorization", "Bearer " + key)
    request.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return int(response.status), response.read(), dict(response.headers.items())
    except urllib.error.HTTPError as exc:
        try: body = exc.read()
        except Exception: body = b""
        return int(exc.code), body, dict(exc.headers.items()) if exc.headers else {}


def http_call(url, key, payload=None, timeout=120, method=None, retry_count=0, sleep_fn=time.sleep, random_fn=random.random):
    attempts, reason = 0, ""
    for attempt in range(retry_count + 1):
        attempts += 1
        try:
            status, body, headers = http_call_once(url, key, payload, timeout, method)
        except (urllib.error.URLError, ConnectionError, socket.gaierror) as exc:
            reason = str(getattr(exc, "reason", exc))
            if attempt >= retry_count: raise RuntimeError("E_NET: %s" % reason)
        except (TimeoutError, socket.timeout, OSError) as exc:
            # POST 发送后的读超时/中断可能重复计费，保守不重试。
            raise RuntimeError("E_NET: %s" % exc)
        else:
            if status not in RETRYABLE_STATUS or attempt >= retry_count:
                return status, body, {"attempts": attempts, "retried": attempts - 1, "reason": reason or (str(status) if attempts > 1 else "")}
            reason = str(status)
            retry_after = headers.get("Retry-After", "")
            try: delay = min(30.0, max(0.0, float(retry_after))) if retry_after else (2 ** attempt) + random_fn() * 0.25
            except ValueError: delay = (2 ** attempt) + random_fn() * 0.25
            sleep_fn(delay)
    raise RuntimeError("E_NET: RETRY_EXHAUSTED")


def same_host(url, endpoint):
    return urllib.parse.urlsplit(url).netloc.lower() == urllib.parse.urlsplit(endpoint).netloc.lower()


def download_image(url, key, endpoint, timeout):
    req = urllib.request.Request(url, method="GET")
    if key and same_host(url, endpoint): req.add_header("Authorization", "Bearer " + key)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response: raw = response.read(MAX_DL_BYTES + 1)
    except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError, OSError) as exc: raise RuntimeError("E_NET: 下载图片失败：%s" % exc)
    if len(raw) > MAX_DL_BYTES: raise RuntimeError("E_PARSE: 下载图片超过上限")
    return raw


def parse_api_error(body):
    try:
        value = json.loads(body.decode("utf-8", "replace")); error = value.get("error") or {}
        return str(error.get("message") or error.get("code") or value)
    except Exception: return body.decode("utf-8", "replace")[:400]


def sniff_ext(raw):
    if raw[:4] == b"\x89PNG": return "png"
    if raw[:3] == b"\xff\xd8\xff": return "jpg"
    if raw[:3] == b"GIF": return "gif"
    if raw[:4] == b"RIFF" and raw[8:12] == b"WEBP": return "webp"
    return "bin"


def extract_images(value, prov, key, endpoint, timeout):
    items = value.get("data") or []
    if not isinstance(items, list) or not items: raise ValueError("响应里没有 data")
    result = []
    for item in items:
        item = item if isinstance(item, dict) else {}; encoded, url = item.get("b64_json") or "", item.get("url") or ""
        if encoded: result.append(base64.b64decode(encoded))
        elif url and prov["response"] in ("url_field", "b64_or_url"): result.append(download_image(url, key, endpoint, timeout))
        else: raise ValueError("data[] 里没有可用的 b64_json/url")
    return result


def build_payload(job, cfg, prov, model, endpoint):
    prompt = str(job.get("prompt") or "").strip()
    if not prompt: raise RuntimeError("E_ARGS: job 里没有 prompt")
    images = job.get("images") or []
    if not isinstance(images, list) or not images: raise RuntimeError("E_IMG: job.images 为空")
    b64s = []
    for item in images:
        value = (item or {}).get("b64") or (item or {}).get("data") or "" if isinstance(item, dict) else ""
        if value.startswith("data:") and "," in value: value = value.split(",", 1)[1]
        if value: b64s.append(value)
    if not b64s: raise RuntimeError("E_IMG: job.images 没有 base64")
    validate_capabilities(cfg, model)
    payload = {"model": model, "prompt": prompt, "n": cfg["n"]}
    payload.update(map_dimensions(cfg["provider"], cfg["width"], cfg["height"], cfg["resolution"]))
    payload.update(dict(prov.get("extra") or {}))
    if cfg["format"] != "auto": payload["output_format"] = cfg["format"]
    elif cfg["provider"] == "ark": payload["output_format"] = "jpeg"
    if cfg["quality"] is not None: payload["quality"] = cfg["quality"]
    i2i, endpoint_out = 0, endpoint
    if prov.get("i2i") == "image_array": payload["image"] = b64s; i2i = 1
    elif str(prov.get("i2i") or "").startswith("edits_"):
        payload[prov.get("i2i_field") or "image"] = b64s[0]; i2i = 1; endpoint_out = prov.get("i2i_endpoint") or endpoint
    return payload, i2i, len(b64s), endpoint_out


def emit_image(encoded, chunk):
    if len(encoded) > MAX_B64_BYTES: out("TOO_LARGE=1"); return
    count = max(1, (len(encoded) + chunk - 1) // chunk); out("BEGIN_IMAGE")
    for index in range(count): out("CHUNK=%d/%d:%s" % (index + 1, count, encoded[index * chunk:(index + 1) * chunk]))
    out("END_IMAGE")


def load_config_from_storage(storage_path):
    try: raw = load_storage(storage_path).get("stylecam.config")
    except RuntimeError: raw = None
    return raw if isinstance(raw, dict) else {}


def cmd_provider(args):
    rows = []
    for key, prov in PROVIDERS.items():
        rows.append({"id": key, "name": prov["name"], "defaultModel": prov["models"][0], "defaultEndpoint": prov["endpoint"],
                     "capabilities": provider_capabilities(key, prov["models"][0])})
    out("OK=1"); out("PROVIDERS=" + json.dumps(rows, ensure_ascii=False, separators=(",", ":"))); msg("provider 能力列表")
    return EXIT_OK


def cmd_config(args):
    storage = Path(args.storage).expanduser() if args.storage else paths()["storage"]
    env = read_env_file(Path(args.env).expanduser() if args.env else paths()["env"])
    stored = load_config_from_storage(storage); cfg, sources = merge_effective_config({}, {}, stored, env)
    try: validate_config(cfg); model = resolve_model("", {}, env, PROVIDERS[cfg["provider"]], cfg); validate_capabilities(cfg, model); map_dimensions(cfg["provider"], cfg["width"], cfg["height"], cfg["resolution"])
    except RuntimeError as exc: return die("E_ARGS", str(exc).split(": ", 1)[-1], EXIT_PRE)
    out("OK=1"); out("CONFIG=" + json.dumps(cfg, ensure_ascii=False, separators=(",", ":")))
    out("SOURCES=" + json.dumps(sources, ensure_ascii=False, separators=(",", ":"))); msg("配置有效")
    return EXIT_OK


def cmd_key(args):
    env_path = Path(args.env).expanduser() if args.env else paths()["env"]; env = read_env_file(env_path)
    prov_key, prov = resolve_provider("", env); action = args.action
    if action == "status":
        key, src = resolve_key(args.api_key, env, prov); out("PROVIDER=" + prov_key); out("KEY=%d" % bool(key)); out("KEY_SRC=" + src)
        out("KEY_MASKED=" + mask_key(key)); out("KEY_LEN=%d" % len(key)); msg("Key 来源：" + src); return EXIT_OK if key else EXIT_PRE
    if action == "from-easyop":
        value = read_easyop_key()
        if not value: return die("E_NO_KEY", "easyop 配置里没有 llm_api_key", EXIT_PRE)
    elif args.from_storage_key:
        if not args.nonce: return die("E_ARGS", "--from-storage-key 需要 --nonce", EXIT_PRE)
        try: value = consume_storage_secret(Path(args.storage).expanduser() if args.storage else paths()["storage"], args.from_storage_key, args.nonce)
        except RuntimeError as exc: return die("E_SECRET", str(exc).split(": ", 1)[-1], EXIT_PRE)
    else:
        value = str(args.value or "").strip()
        if not value: return die("E_ARGS", "set-env 需要 --value 或 --from-storage-key", EXIT_PRE)
    try: write_env_value(env_path, ENV_KEY_NEW, value)
    except OSError as exc: return die("E_IO", "SECRET_WRITE_FAILED %s" % exc, EXIT_IO)
    out("OK=1"); out("KEY_SRC=env"); out("KEY_MASKED=" + mask_key(value)); out("KEY_LEN=%d" % len(value)); msg("已安全写入 Key")
    return EXIT_OK


def cmd_dir(args):
    try: directory = check_outdir(Path(args.out_dir).expanduser() if args.out_dir else default_out_dir())
    except RuntimeError as exc: return die("E_IO", str(exc).split(": ", 1)[-1], EXIT_IO)
    out("OK=1"); out("OUTDIR=" + str(directory)); out("OUTDIR_WRITABLE=1"); msg("目录可写")
    return EXIT_OK


def cmd_doctor(args):
    env_path = Path(args.env).expanduser() if args.env else paths()["env"]; env = read_env_file(env_path)
    stored = load_config_from_storage(Path(args.storage).expanduser() if args.storage else paths()["storage"])
    cfg, _ = merge_effective_config(vars(args), {}, stored, env); cfg["provider"] = args.provider or cfg["provider"]; cfg["baseUrl"] = args.base_url or cfg["baseUrl"]
    try: validate_config(cfg); prov_key, prov = resolve_provider(cfg["provider"], env, config=cfg); endpoint = resolve_endpoint(prov, cfg["baseUrl"], env, config=cfg); model = resolve_model(args.model, {}, env, prov, cfg)
    except RuntimeError as exc: return die("E_ARGS", str(exc).split(": ", 1)[-1], EXIT_PRE)
    key, src = resolve_key(args.api_key, env, prov)
    out("PY=1"); out("PROVIDER=" + prov_key); out("PROVIDER_NAME=" + prov["name"]); out("ENDPOINT=" + endpoint); out("MODEL=" + model)
    out("I2I=%d" % bool(prov.get("i2i"))); out("KEY=%d" % bool(key)); out("KEY_SRC=" + src); out("KEY_MASKED=" + mask_key(key)); out("KEY_LEN=%d" % len(key))
    if not key: return die("E_NO_KEY", "没有可用生图 Key", EXIT_PRE)
    if args.probe: return die("E_ARGS", "本阶段 doctor --probe 未启用，避免意外计费", EXIT_PRE)
    out("OK=1"); msg("未联网探测")
    return EXIT_OK


def cmd_generate(args):
    storage = Path(args.storage).expanduser() if args.storage else paths()["storage"]; env_path = Path(args.env).expanduser() if args.env else paths()["env"]
    try: job = get_job(storage, args.job_key)
    except RuntimeError as exc: return die(str(exc).split(":", 1)[0], str(exc).split(": ", 1)[-1], EXIT_PRE)
    env = read_env_file(env_path); stored = load_config_from_storage(storage)
    cli = {"provider": args.provider, "baseUrl": args.base_url, "model": args.model, "resolution": args.resolution,
           "width": args.width, "height": args.height, "format": args.format, "quality": args.quality, "n": args.n,
           "httpTimeoutSecs": args.http_timeout, "retryCount": args.retry, "outDir": args.out_dir}
    if args.size:
        if args.size in ("1K", "2K", "4K"): cli["resolution"] = args.size
        else:
            match = re.fullmatch(r"(\d+)x(\d+)", args.size, re.I)
            if match: cli["width"], cli["height"] = int(match.group(1)), int(match.group(2))
    cfg, _ = merge_effective_config(cli, job, stored, env)
    try:
        validate_config(cfg); prov_key, prov = resolve_provider(cfg["provider"], env, job, cfg); cfg["provider"] = prov_key
        endpoint = resolve_endpoint(prov, cfg["baseUrl"], env, job, cfg); model = resolve_model(cfg["model"], job, env, prov, cfg)
        payload, i2i, image_count, endpoint = build_payload(job, cfg, prov, model, endpoint)
    except RuntimeError as exc: return die("E_ARGS", str(exc).split(": ", 1)[-1], EXIT_PRE)
    key, _ = resolve_key(args.api_key, env, prov)
    if not key: return die("E_NO_KEY", "没有 provider=%s 可用的生图 Key" % prov_key, EXIT_PRE)
    try: out_dir = check_outdir(Path(cfg["outDir"]).expanduser() if cfg["outDir"] else default_out_dir())
    except RuntimeError as exc: return die("E_IO", str(exc).split(": ", 1)[-1], EXIT_IO)
    if args.dry:
        out("OK=1"); out("DRY=1"); out("PROVIDER=" + prov_key); out("MODEL=" + model); out("ENDPOINT=" + endpoint)
        out("SIZE=" + str(payload.get("size") or payload.get("resolution") or "")); out("RATIO=" + str(payload.get("ratio") or ""))
        out("FORMAT=" + cfg["format"]); out("QUALITY=" + ("" if cfg["quality"] is None else str(cfg["quality"]))); out("N=%d" % cfg["n"])
        out("IMAGES=%d" % image_count); out("I2I=%d" % i2i); out("OUTDIR=" + str(out_dir)); msg("dry-run：不联网")
        return EXIT_OK
    t0 = time.time()
    try: status, body, retry_meta = http_call(endpoint, key, payload, cfg["httpTimeoutSecs"], retry_count=cfg["retryCount"])
    except RuntimeError as exc: return die("E_NET", str(exc).split(": ", 1)[-1], EXIT_GEN)
    out("ATTEMPTS=%d" % retry_meta["attempts"]); out("RETRIED=%d" % retry_meta["retried"]); out("RETRY_REASON=" + retry_meta["reason"])
    if status != 200: out("STATUS=%d" % status); return die("E_API", "%d：%s" % (status, parse_api_error(body)), EXIT_GEN)
    try: raws = extract_images(json.loads(body.decode("utf-8", "replace")), prov, key, endpoint, cfg["httpTimeoutSecs"])
    except (ValueError, json.JSONDecodeError, RuntimeError) as exc: return die("E_PARSE", "解析不了返回：%s" % exc, EXIT_GEN)
    style_id = args.style or job.get("styleId") or "custom"; stamp = datetime.now().strftime("%Y%m%d-%H%M%S"); images = []
    for index, raw in enumerate(raws):
        ext = sniff_ext(raw); suffix = "" if index == 0 else "_%d" % (index + 1); target = out_dir / ("%s_%s%s.%s" % (stamp, style_id, suffix, ext))
        try: target.write_bytes(raw)
        except OSError as exc: return die("E_IO", "写不了文件：%s" % exc, EXIT_IO)
        images.append({"path": str(target), "bytes": len(raw), "ext": ext})
    elapsed = int((time.time() - t0) * 1000); first = images[0]
    out("OK=1"); out("PROVIDER=" + prov_key); out("MODEL=" + model); out("I2I=%d" % i2i); out("PATH=" + first["path"])
    emit_image(base64.b64encode(raws[0]).decode("ascii"), args.chunk)
    meta = {"jobId": job.get("jobId") or "", "styleId": style_id, "provider": prov_key, "model": model,
            "strength": args.strength if args.strength is not None else job.get("strength"), "size": payload.get("size") or payload.get("resolution") or "",
            "resolution": payload.get("resolution") or "", "ratio": payload.get("ratio") or "", "i2i": i2i,
            "elapsedMs": elapsed, "bytes": first["bytes"], "ext": first["ext"], "images": images}
    out("META=" + json.dumps(meta, ensure_ascii=False, separators=(",", ":"))); msg("生成完成：%d 张" % len(images)); return EXIT_OK


def cmd_save(args):
    source = Path(args.from_path).expanduser() if args.from_path else None
    if not source or not source.is_file(): return die("E_ARGS", "save 需要存在的 --from", EXIT_PRE)
    try: directory = check_outdir(Path(args.out_dir).expanduser() if args.out_dir else default_out_dir()); target = directory / (args.name or source.name); shutil.copy2(source, target)
    except OSError as exc: return die("E_IO", "复制失败：%s" % exc, EXIT_IO)
    out("OK=1"); out("PATH=" + str(target)); out("BYTES=%d" % target.stat().st_size); return EXIT_OK


def build_parser():
    parser = argparse.ArgumentParser(prog="stylecam_io"); sub = parser.add_subparsers(dest="cmd")
    doctor = sub.add_parser("doctor"); doctor.add_argument("--probe", action="store_true"); doctor.add_argument("--provider", default=""); doctor.add_argument("--model", default=""); doctor.add_argument("--base-url", dest="base_url", default=""); doctor.add_argument("--out-dir", dest="outDir", default=""); doctor.add_argument("--env", default=""); doctor.add_argument("--storage", default=""); doctor.add_argument("--api-key", dest="api_key", default="")
    gen = sub.add_parser("generate"); gen.add_argument("--job-key", dest="job_key", default="stylecam.job"); gen.add_argument("--storage", default=""); gen.add_argument("--style", default=""); gen.add_argument("--prompt", default=""); gen.add_argument("--strength", type=int, default=None); gen.add_argument("--size", default=""); gen.add_argument("--ratio", default=""); gen.add_argument("--resolution", default=""); gen.add_argument("--width", type=int, default=None); gen.add_argument("--height", type=int, default=None); gen.add_argument("--n", type=int, default=None); gen.add_argument("--seed", type=int, default=None); gen.add_argument("--provider", default=""); gen.add_argument("--model", default=""); gen.add_argument("--base-url", dest="base_url", default=""); gen.add_argument("--out-dir", dest="out_dir", default=""); gen.add_argument("--chunk", type=int, default=CHUNK_DEFAULT); gen.add_argument("--format", default=""); gen.add_argument("--quality", type=int, default=None); gen.add_argument("--http-timeout", dest="http_timeout", type=int, default=None); gen.add_argument("--retry", type=int, default=None); gen.add_argument("--dry", action="store_true"); gen.add_argument("--env", default=""); gen.add_argument("--api-key", dest="api_key", default="")
    save = sub.add_parser("save"); save.add_argument("--from", dest="from_path", default=""); save.add_argument("--from-key", dest="from_key", default=""); save.add_argument("--out-dir", dest="out_dir", default=""); save.add_argument("--name", default=""); save.add_argument("--storage", default="")
    directory = sub.add_parser("dir"); directory.add_argument("--out-dir", dest="out_dir", default="")
    key = sub.add_parser("key"); key.add_argument("action", choices=["status", "from-easyop", "set-env"]); key.add_argument("--value", default=""); key.add_argument("--from-storage-key", default=""); key.add_argument("--nonce", default=""); key.add_argument("--storage", default=""); key.add_argument("--env", default=""); key.add_argument("--api-key", dest="api_key", default="")
    config = sub.add_parser("config"); config.add_argument("action", choices=["get", "validate"], default="get"); config.add_argument("--storage", default=""); config.add_argument("--env", default="")
    provider = sub.add_parser("provider"); provider.add_argument("action", choices=["list"], default="list")
    return parser


def main(argv=None):
    args = build_parser().parse_args(argv)
    if not args.cmd: return EXIT_PRE
    try:
        return {"doctor": cmd_doctor, "generate": cmd_generate, "save": cmd_save, "dir": cmd_dir, "key": cmd_key,
                "config": cmd_config, "provider": cmd_provider}[args.cmd](args)
    except RuntimeError as exc:
        text = str(exc); code, _, detail = text.partition(":"); code = code if code in ERRS else "E_ARGS"
        return die(code, detail.strip() or text, EXIT_GEN if code in ("E_API", "E_NET", "E_PARSE") else EXIT_PRE)
    except Exception:
        traceback.print_exc(file=sys.stderr); return die("E_ARGS", "脚本内部异常（详见 stderr）", EXIT_PRE)


if __name__ == "__main__": sys.exit(main())
