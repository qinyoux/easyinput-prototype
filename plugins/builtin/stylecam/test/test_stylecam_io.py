import importlib.util
import io
import json
import tempfile
import unittest
import urllib.error
from contextlib import redirect_stdout
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

MODULE_PATH = Path(__file__).resolve().parents[1] / "tools" / "stylecam_io.py"
SPEC = importlib.util.spec_from_file_location("stylecam_io", MODULE_PATH)
io_mod = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(io_mod)


class ConfigTests(unittest.TestCase):
    def test_default_provider_is_easysu(self):
        self.assertEqual(io_mod.DEFAULT_PROVIDER, "easysu")

    def test_migrate_legacy_tier_and_pixels(self):
        self.assertEqual(io_mod.migrate_config({"v": 1, "size": "2K"})["resolution"], "2K")
        cfg = io_mod.migrate_config({"v": 1, "size": "1280x768"})
        self.assertEqual((cfg["width"], cfg["height"]), (1280, 768))

    def test_priority_cli_job_config_env_default(self):
        env = {"PROVIDER": "zhipu", "WIDTH": "512"}
        cfg, src = io_mod.merge_effective_config({"width": 2048}, {"width": 1536}, {"width": 1024}, env)
        self.assertEqual(cfg["width"], 2048)
        self.assertEqual(src["width"], "cli")
        cfg, src = io_mod.merge_effective_config({}, {"width": 1536}, {"width": 1024}, env)
        self.assertEqual((cfg["width"], src["width"]), (1536, "job"))
        cfg, src = io_mod.merge_effective_config({}, {}, {}, env)
        self.assertEqual((cfg["width"], src["width"]), (512, "env"))

    def test_strict_dimensions(self):
        self.assertEqual(io_mod.map_dimensions("easysu", 1024, 1024, "1K")["ratio"], "1:1")
        with self.assertRaisesRegex(RuntimeError, "DIMENSION_UNREPRESENTABLE"):
            io_mod.map_dimensions("easysu", 1024, 704, "1K")
        with self.assertRaisesRegex(RuntimeError, "方形"):
            io_mod.map_dimensions("ark", 1024, 768, "2K")

    def test_capability_rules(self):
        cfg = io_mod.migrate_config({"provider": "ark", "format": "jpeg", "n": 2})
        with self.assertRaisesRegex(RuntimeError, "CAPABILITY_UNSUPPORTED"):
            io_mod.validate_capabilities(cfg, "doubao-seedream-5-0-250128")
        cfg = io_mod.migrate_config({"provider": "openai_compat", "format": "jpeg", "quality": 80, "n": 2})
        io_mod.validate_capabilities(cfg, "gpt-image-1")
        with self.assertRaisesRegex(RuntimeError, "CAPABILITY_UNSUPPORTED"):
            io_mod.validate_capabilities(cfg, "dall-e-3")

    def test_all_config_ranges_and_url_policy(self):
        for key, bad in (("width", 255), ("height", 4100), ("n", 5), ("httpTimeoutSecs", 9), ("retryCount", 4)):
            with self.subTest(key=key):
                with self.assertRaises(RuntimeError):
                    io_mod.validate_config(io_mod.migrate_config({key: bad}))
        with self.assertRaisesRegex(RuntimeError, "HTTPS"):
            io_mod.validate_config(io_mod.migrate_config({"baseUrl": "http://example.com/v1"}))
        io_mod.validate_config(io_mod.migrate_config({"baseUrl": "http://localhost:8000/v1"}))

    def test_provider_capability_matrix(self):
        expected = {
            "easysu": ({"auto", "jpeg", "png"}, True, 4),
            "ark": ({"auto", "jpeg", "png"}, False, 1),
            "zhipu": ({"auto"}, False, 1),
        }
        for provider, (formats, quality, max_n) in expected.items():
            cap = io_mod.provider_capabilities(provider, io_mod.PROVIDERS[provider]["models"][0])
            self.assertEqual(set(cap["formats"]), formats)
            self.assertEqual(cap["quality"], quality)
            self.assertEqual(cap["maxN"], max_n)
        unknown = io_mod.provider_capabilities("openai_compat", "custom-model")
        self.assertEqual((unknown["formats"], unknown["quality"], unknown["maxN"]), (["auto"], False, 1))


class PayloadTests(unittest.TestCase):
    def job(self):
        return {"prompt": "test", "images": [{"b64": "AA=="}]}

    def test_four_provider_payloads(self):
        for provider in io_mod.PROVIDERS:
            with self.subTest(provider=provider):
                cfg = io_mod.migrate_config({"provider": provider})
                if provider == "ark": cfg.update({"resolution": "2K", "format": "jpeg"})
                model = io_mod.PROVIDERS[provider]["models"][0]
                payload, i2i, _, endpoint = io_mod.build_payload(self.job(), cfg, io_mod.PROVIDERS[provider], model, io_mod.PROVIDERS[provider]["endpoint"])
                self.assertEqual(payload["model"], model)
                self.assertIn("n", payload)
                if provider == "easysu":
                    self.assertNotIn("size", payload); self.assertEqual(payload["ratio"], "1:1"); self.assertTrue(i2i)
                elif provider == "ark":
                    self.assertEqual(payload["size"], "2K"); self.assertTrue(i2i)
                else:
                    self.assertEqual(payload["size"], "1024x1024"); self.assertFalse(i2i)
                self.assertTrue(endpoint.startswith("https://"))

    def test_extract_multiple_images(self):
        value = {"data": [{"b64_json": "YWJj"}, {"b64_json": "ZGVm"}]}
        raws = io_mod.extract_images(value, io_mod.PROVIDERS["easysu"], "x", "https://x", 10)
        self.assertEqual(raws, [b"abc", b"def"])


class RetryTests(unittest.TestCase):
    def test_retry_status_then_success(self):
        replies = [(429, b"", {"Retry-After": "0"}), (200, b"{}", {})]
        with mock.patch.object(io_mod, "http_call_once", side_effect=replies):
            status, _, meta = io_mod.http_call("https://x", "k", {}, retry_count=1, sleep_fn=lambda _: None)
        self.assertEqual(status, 200); self.assertEqual(meta["attempts"], 2); self.assertEqual(meta["retried"], 1)

    def test_nonretryable_status(self):
        with mock.patch.object(io_mod, "http_call_once", return_value=(401, b"", {})) as call:
            status, _, meta = io_mod.http_call("https://x", "k", {}, retry_count=3, sleep_fn=lambda _: None)
        self.assertEqual(status, 401); self.assertEqual(meta["attempts"], 1); self.assertEqual(call.call_count, 1)

    def test_post_timeout_not_retried(self):
        with mock.patch.object(io_mod, "http_call_once", side_effect=TimeoutError("read timeout")) as call:
            with self.assertRaisesRegex(RuntimeError, "E_NET"):
                io_mod.http_call("https://x", "k", {}, retry_count=3, sleep_fn=lambda _: None)
        self.assertEqual(call.call_count, 1)

    def test_retryable_and_nonretryable_status_matrix(self):
        for status in sorted(io_mod.RETRYABLE_STATUS):
            replies = [(status, b"", {"Retry-After": "0"}), (200, b"{}", {})]
            with self.subTest(status=status), mock.patch.object(io_mod, "http_call_once", side_effect=replies) as call:
                result, _, meta = io_mod.http_call("https://x", "k", {}, retry_count=1, sleep_fn=lambda _: None)
                self.assertEqual((result, meta["attempts"], call.call_count), (200, 2, 2))
        for status in (400, 401, 403, 404, 405, 409, 413, 422):
            with self.subTest(status=status), mock.patch.object(io_mod, "http_call_once", return_value=(status, b"", {})) as call:
                result, _, meta = io_mod.http_call("https://x", "k", {}, retry_count=3, sleep_fn=lambda _: None)
                self.assertEqual((result, meta["attempts"], call.call_count), (status, 1, 1))

    def test_connection_failure_can_retry(self):
        replies = [urllib.error.URLError("connect failed"), (200, b"{}", {})]
        with mock.patch.object(io_mod, "http_call_once", side_effect=replies) as call:
            status, _, meta = io_mod.http_call("https://x", "k", {}, retry_count=1, sleep_fn=lambda _: None)
        self.assertEqual((status, meta["attempts"], meta["retried"], call.call_count), (200, 2, 1, 2))


class SecretTests(unittest.TestCase):
    def test_secret_consumed_and_env_atomic(self):
        with tempfile.TemporaryDirectory() as td:
            storage = Path(td) / "storage.json"; env = Path(td) / "stylecam_io.env"
            storage.write_text(json.dumps({"stylecam.secret": {"v": 1, "nonce": "n", "apiKey": "secret-key-123", "createdAt": 1000}}), encoding="utf-8")
            value = io_mod.consume_storage_secret(storage, "stylecam.secret", "n", now=1000)
            self.assertEqual(value, "secret-key-123")
            self.assertIsNone(json.loads(storage.read_text(encoding="utf-8"))["stylecam.secret"])
            io_mod.write_env_value(env, "IMAGE_API_KEY", value)
            self.assertIn("IMAGE_API_KEY=secret-key-123", env.read_text(encoding="utf-8"))
            self.assertFalse(list(Path(td).glob("*.tmp")))

    def test_nonce_failure_still_clears_secret(self):
        with tempfile.TemporaryDirectory() as td:
            storage = Path(td) / "storage.json"
            storage.write_text(json.dumps({"stylecam.secret": {"nonce": "good", "apiKey": "secret-key-123", "createdAt": 1000}}), encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "nonce"):
                io_mod.consume_storage_secret(storage, "stylecam.secret", "bad", now=1000)
            self.assertIsNone(json.loads(storage.read_text(encoding="utf-8"))["stylecam.secret"])


class CliTests(unittest.TestCase):
    def test_provider_list_is_read_only(self):
        buf = io.StringIO()
        with redirect_stdout(buf): rc = io_mod.main(["provider", "list"])
        self.assertEqual(rc, 0); self.assertIn("PROVIDERS=", buf.getvalue())

    def test_dry_run_easysu_no_network(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td); storage = root / "storage.json"; env = root / "env"; outdir = root / "out"
            storage.write_text(json.dumps({"stylecam.job": {"prompt": "p", "images": [{"b64": "AA=="}]}}), encoding="utf-8")
            env.write_text("IMAGE_API_KEY=test-key-123\n", encoding="utf-8")
            buf = io.StringIO()
            with mock.patch.object(io_mod, "http_call_once", side_effect=AssertionError("network")):
                with redirect_stdout(buf):
                    rc = io_mod.main(["generate", "--dry", "--storage", str(storage), "--env", str(env), "--out-dir", str(outdir)])
            self.assertEqual(rc, 0); self.assertIn("PROVIDER=easysu", buf.getvalue()); self.assertIn("DRY=1", buf.getvalue())


if __name__ == "__main__":
    unittest.main()
