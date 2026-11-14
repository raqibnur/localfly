"""Manual GUI e2e: needs xvfb + bore + display. Run: xvfb-run -a python3 gui/tests/manual_e2e_xvfb.py"""
"""Scripted GUI e2e (runs under xvfb): add app, toggle tunnel on, expect live URL."""
import os
import subprocess
import sys
import tempfile
import time

tmp = tempfile.mkdtemp(prefix="lf-gui-e2e-")
os.environ["LOCALFLY_CONFIG"] = tmp
sys.path.insert(0, "gui")

import localfly_backend as B

BORE = "/tmp/opencode/bore-test/bin/bore"

httpd = subprocess.Popen([sys.executable, "-m", "http.server", "18341"],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
bore = subprocess.Popen([BORE, "server", "--bind-addr", "127.0.0.1",
                         "--bind-tunnels", "127.0.0.1"],
                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
time.sleep(1)
# Point the CLI at the local relay (default would be bore.pub).
r = B.run_cli("config", "--relay", "127.0.0.1")
assert r.returncode == 0, r.stderr

from gi.repository import GLib
from localfly_gui import LocalflyLinuxApp

results = {}
app = LocalflyLinuxApp()
win = None


def on_activate(a):
    GLib.timeout_add(500, step_add)


def step_add():
    global win
    wins = app.get_windows()
    assert wins, "no windows"
    win = wins[0]
    r = B.run_cli("add", "--name", "web", "--port", "18341")
    assert r.returncode == 0, r.stderr
    GLib.timeout_add(3500, step_on)  # wait for a refresh tick to pick it up
    return False


def step_on():
    assert len(win.states) == 1, f"states={win.states}"
    app_id = win.states[0].app.id
    win.start_tunnel(app_id)
    results["id"] = app_id
    GLib.timeout_add(4000, step_assert_live)
    return False


def step_assert_live():
    tun = win.tunnels.get(results["id"])
    assert tun is not None and tun.url, f"tunnel={tun}"
    print("LIVE-URL:", tun.url)
    assert tun.url.startswith("http://127.0.0.1:"), tun.url
    win.stop_tunnel(results["id"])
    GLib.timeout_add(1000, step_quit)
    return False


def step_quit():
    assert results["id"] not in win.tunnels, "tunnel should be stopped"
    print("GUI-E2E-OK")
    win.quit_app()
    return False


app.connect("activate", on_activate)
GLib.timeout_add(15000, lambda: (print("TIMEOUT"), win.quit_app() if win else app.quit(), False)[2])
code = app.run([sys.argv[0]])
httpd.terminate()
bore.terminate()
sys.exit(code)
