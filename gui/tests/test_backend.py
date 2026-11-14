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
        with tempfile.TemporaryDirectory() as d:
            os.environ["LOCALFLY_CONFIG"] = d
            try:
                self.assertEqual(B.list_apps(), [])
            finally:
                del os.environ["LOCALFLY_CONFIG"]


if __name__ == "__main__":
    unittest.main()
