import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import localfly_backend as B


class ParseLiveLine(unittest.TestCase):
    def test_valid(self):
        self.assertEqual(
            B.parse_live_line("web live: localhost:3000 -> http://127.0.0.1:41234"),
            ("web", "http://127.0.0.1:41234"),
        )

    def test_status_lines_ignored(self):
        for line in ["connecting… (attempt 0)", "live http://x (uptime", "error: boom", ""]:
            self.assertIsNone(B.parse_live_line(line))


class ConfigDir(unittest.TestCase):
    def test_env_override(self):
        os.environ["LOCALFLY_CONFIG"] = "/tmp/lf-gui-test-config"
        try:
            self.assertEqual(B.config_dir(), "/tmp/lf-gui-test-config")
        finally:
            del os.environ["LOCALFLY_CONFIG"]


class ListAppsEmpty(unittest.TestCase):
    def test_empty_config_lists_nothing(self):
        import tempfile

        with tempfile.TemporaryDirectory() as d:
            os.environ["LOCALFLY_CONFIG"] = d
            os.environ["LOCALFLY_SOCKET"] = os.path.join(d, "localfly.sock")
            os.environ["LOCALFLY_IDLE_EXIT_SECS"] = "2"
            os.environ["LOCALFLY_IDLE_CHECK_SECS"] = "1"
            try:
                self.assertEqual(B.list_apps(), [])
            finally:
                for key in ("LOCALFLY_CONFIG", "LOCALFLY_SOCKET", "LOCALFLY_IDLE_EXIT_SECS", "LOCALFLY_IDLE_CHECK_SECS"):
                    os.environ.pop(key, None)
                # The spawned test daemon idle-exits on its own.


class HueParity(unittest.TestCase):
    def test_matches_rust_djb2(self):
        # Cross-language pin: Rust hue_for("web") prints the same value.
        self.assertEqual(B.hue_for("web"), 0.8305555555555556)
        self.assertEqual(B.hue_for("Web"), B.hue_for("web"))
        self.assertTrue(0.0 <= B.hue_for("anything") < 1.0)


class QrPng(unittest.TestCase):
    def test_renders_or_skips_gracefully(self):
        import shutil
        import tempfile

        if shutil.which("qrencode") is None:
            self.assertFalse(B.qrencode_png("http://example.test/", "/tmp/nope.png"))
            return
        with tempfile.TemporaryDirectory() as d:
            out = os.path.join(d, "qr.png")
            self.assertTrue(B.qrencode_png("http://bore.pub:1234", out))
            self.assertGreater(os.path.getsize(out), 100)


if __name__ == "__main__":
    unittest.main()
