"""Manual GUI e2e: add app, toggle tunnel on via daemon, expect live URL.
Run: xvfb-run -a python3 gui/tests/manual_e2e_xvfb.py
Isolated via temp config + temp daemon socket; serializes on the bore lock.
"""
import fcntl
import os
import subprocess
import sys
import tempfile
import time

tmp = tempfile.mkdtemp(prefix="lf-gui-e2e-")
os.environ["LOCALFLY_CONFIG"] = tmp
os.environ["LOCALFLY_SOCKET"] = os.path.join(tmp, "localfly.sock")
os.environ["LOCALFLY_IDLE_EXIT_SECS"] = "30"
os.environ["LOCALFLY_IDLE_CHECK_SECS"] = "5"
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import localfly_backend as B

BORE = os.environ.get(
    "BORE_BIN", os.path.expanduser("~/.cargo/bin/bore")
)
if not os.path.isfile(BORE):
    BORE = "/tmp/opencode/bore-test/bin/bore"

lock = open(os.path.join(tempfile.gettempdir(), "localfly-bore-test.lock"), "w")
fcntl.flock(lock.fileno(), fcntl.LOCK_EX)

httpd = subprocess.Popen(
    [sys.executable, "-m", "http.server", "18341"],
    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
)
bore = subprocess.Popen(
    [BORE, "server", "--bind-addr", "127.0.0.1", "--bind-tunnels", "127.0.0.1"],
    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
)
time.sleep(1)
# Point the daemon config at the local relay (default would be bore.pub).
r = B.run_cli("config", "--relay", "127.0.0.1")
assert r.returncode == 0, r.stderr

from gi.repository import GLib
from localfly_gui import LocalflyLinuxApp

results = {}
app = LocalflyLinuxApp()
# Unique bus name so a running user session doesn't swallow this test run.
app.set_application_id("com.localfly.Localfly.E2E")
win = None


def on_activate(a):
    GLib.timeout_add(500, step_add)


def step_add():
    global win
    wins = app.get_windows()
    assert wins, "no windows"
    win = wins[0]
    B.ensure_daemon()
    import uuid

    reply = B.daemon_call(
        "add",
        app={
            "id": str(uuid.uuid4()),
            "name": "web",
            "port": 18341,
            "start_on_launch": False,
            "hue": 0.1,
            "public_port": None,
            "public_address": None,
        },
    )
    assert reply.get("ok"), reply
    GLib.timeout_add(3500, step_on)  # wait for a refresh tick to pick it up
    return False


def step_on():
    assert len(win.states) == 1, f"states={win.states}"
    app_id = win.states[0].app.id
    win.start_tunnel(app_id)
    results["id"] = app_id
    GLib.timeout_add(5000, step_assert_live)
    return False


def step_assert_live():
    tun = win.tunnels.get(results["id"], {})
    url = B.display_url(win.states[0].app, tun) if tun else None
    assert tun and tun.get("status") == "live" and url, f"tunnel={tun}"
    print("LIVE-URL:", url, flush=True)
    assert url.startswith("http://127.0.0.1:"), url
    win.stop_tunnel(results["id"])
    GLib.timeout_add(2000, step_quit)
    return False


def step_quit():
    assert results["id"] not in {k for k, v in win.tunnels.items() if isinstance(v, dict) and v.get("status") == "live"}, "tunnel should be stopped"
    print("GUI-E2E-OK", flush=True)
    win.quit_app()
    return False


app.connect("activate", on_activate)
GLib.timeout_add(
    25000, lambda: (print("TIMEOUT", flush=True), win.quit_app() if win else app.quit(), False)[2]
)
code = app.run([sys.argv[0]])
httpd.terminate()
bore.terminate()
sys.exit(code)
