"""Backend for the Localfly Linux GUI: drives the `localfly` CLI binary.

Pure stdlib (no Gtk) so it can be unit-tested headless in CI.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
from dataclasses import dataclass, field


COMMON_PORTS = [3000, 3001, 3333, 4000, 4200, 4321, 5173, 5174, 8000, 8080, 8081, 8888, 1313]


def find_binary() -> str:
    for cand in (
        shutil.which("localfly"),
        os.path.expanduser("~/.local/bin/localfly"),
        "/usr/local/bin/localfly",
    ):
        if cand and os.path.isfile(cand) and os.access(cand, os.X_OK):
            return cand
    # Dev fallback: repo checkout layout.
    here = os.path.dirname(os.path.abspath(__file__))
    for cand in (
        os.path.join(os.path.dirname(here), "target", "release", "localfly"),
        os.path.join(os.path.dirname(here), "target", "debug", "localfly"),
    ):
        if os.path.isfile(cand) and os.access(cand, os.X_OK):
            return cand
    return "localfly"


def config_dir() -> str:
    override = os.environ.get("LOCALFLY_CONFIG")
    if override:
        return override
    xdg = os.environ.get("XDG_CONFIG_HOME") or os.path.expanduser("~/.config")
    return os.path.join(xdg, "localfly")


def secret_configured() -> bool:
    """Whether a relay secret is stored (env wins, like the CLI)."""
    if os.environ.get("LOCALFLY_SECRET"):
        return True
    try:
        with open(os.path.join(config_dir(), "secret"), "rb") as f:
            return bool(f.read().strip())
    except OSError:
        return False


@dataclass
class SavedApp:
    id: str
    name: str
    port: int
    start_on_launch: bool = False
    public_port: int | None = None
    public_address: str | None = None
    hue: float | None = None


@dataclass
class AppState:
    app: SavedApp
    listening: bool | None = None  # None = not probed yet


def run_cli(*args: str, timeout: int = 15) -> subprocess.CompletedProcess:
    return subprocess.run(
        [find_binary(), *args],
        capture_output=True,
        text=True,
        timeout=timeout,
    )


def list_apps() -> list[SavedApp]:
    """Saved apps via the daemon (single source of truth; spawns it if needed)."""
    ensure_daemon()
    reply = daemon_call("list", timeout=15)
    apps = []
    for item in reply.get("apps", []) or []:
        app = item.get("app", {})
        apps.append(
            SavedApp(
                id=str(app.get("id", "")),
                name=str(app.get("name", "")),
                port=int(app.get("port", 0)),
                start_on_launch=bool(app.get("start_on_launch", False)),
                public_port=app.get("public_port"),
                public_address=app.get("public_address"),
                hue=app.get("hue"),
            )
        )
    return apps


def probe(port: int, timeout: int = 2) -> bool:
    try:
        proc = run_cli("probe", str(port), timeout=timeout)
    except (OSError, subprocess.SubprocessError):
        return False
    return proc.returncode == 0


def probe_tcp(port: int, host: str = "localhost", timeout: float = 0.5) -> bool:
    """Cheap in-process TCP liveness check (no subprocess) for GUI polling."""
    import socket

    targets = [(host, port)]
    if host == "localhost":
        targets = [("127.0.0.1", port), ("::1", port)]
    for target in targets:
        try:
            with socket.create_connection(target, timeout=timeout):
                return True
        except OSError:
            continue
    return False


def scan() -> list[int]:
    """Ports from COMMON_PORTS with something listening (via one CLI call)."""
    try:
        proc = run_cli("scan", timeout=30)
    except (OSError, subprocess.SubprocessError):
        return []
    if proc.returncode != 0:
        return []
    found = []
    for line in proc.stdout.splitlines():
        line = line.strip().removesuffix(" (saved)")
        if line.startswith("localhost:"):
            try:
                found.append(int(line.split(":")[1]))
            except ValueError:
                pass
    return sorted(found)


def relay_status(relay: str | None = None) -> tuple[bool, str]:
    args = ["relay-status"]
    if relay:
        args += ["--relay", relay]
    try:
        proc = run_cli(*args, timeout=20)
    except (OSError, subprocess.SubprocessError) as e:
        return False, str(e)
    out = (proc.stdout + proc.stderr).strip()
    return proc.returncode == 0, out


def load_effective_config() -> dict:
    """Read ~/.config/localfly/config.json (relay/auto_reconnect/local_host)."""
    path = os.path.join(config_dir(), "config.json")
    try:
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
        return data if isinstance(data, dict) else {}
    except (OSError, json.JSONDecodeError):
        return {}


def parse_stats_line(line: str) -> dict | None:
    """Parse `live URL (...; bytes_in=N bytes_out=N conns=A/B)` from CLI stderr."""
    import re

    m = re.search(
        r"^live \S+ \(.*bytes_in=(\d+) bytes_out=(\d+) conns=(\d+)/(\d+)\)", line.strip()
    )
    if not m:
        return None
    return {
        "bytes_in": int(m.group(1)),
        "bytes_out": int(m.group(2)),
        "active": int(m.group(3)),
        "total": int(m.group(4)),
    }


def format_bytes(n: int) -> str:
    n = max(0, int(n))
    if n < 1024:
        return f"{n} B"
    if n < 1024 * 1024:
        v = n / 1024
        return f"{v:.0f} KB" if v >= 10 else f"{v:.1f} KB"
    if n < 1024 * 1024 * 1024:
        v = n / (1024 * 1024)
        return f"{v:.0f} MB" if v >= 10 else f"{v:.1f} MB"
    v = n / (1024 * 1024 * 1024)
    return f"{v:.0f} GB" if v >= 10 else f"{v:.1f} GB"


def format_uptime(seconds: float) -> str:
    s = max(0, int(seconds))
    if s < 60:
        return f"{s}s"
    if s < 3600:
        return f"{s // 60}m"
    return f"{s // 3600}h"


def hue_for(name: str) -> float:
    """Stable 0..1 hue for a name (djb2, mirrors the Rust core)."""
    h = 5381
    for b in name.lower().encode("utf-8"):
        h = ((h << 5) + h + b) & 0xFFFFFFFF
    return (h % 360) / 360.0


def hue_color(hue: float | None) -> tuple[int, int, int]:
    """Pastel tile color from the app's stable hue (like macOS monograms)."""
    import colorsys

    try:
        h = float(hue) % 1.0
    except (TypeError, ValueError):
        h = 0.07
    r, g, b = colorsys.hsv_to_rgb(h, 0.55, 0.88)
    return round(r * 255), round(g * 255), round(b * 255)


def parse_live_line(line: str) -> tuple[str, str] | None:
    """Parse `web live: localhost:3000 -> http://127.0.0.1:41234` (CLI output)."""
    if " live: " not in line or " -> " not in line:
        return None
    left, url = line.split(" -> ", 1)
    name = left.split(" live: ", 1)[0].strip()
    url = url.strip()
    if not url or not name:
        return None
    return name, url


def socket_path() -> str:
    """Mirror of Rust daemon::socket_path + LOCALFLY_SOCKET override."""
    override = os.environ.get("LOCALFLY_SOCKET")
    if override and override.strip():
        return override
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    if runtime and runtime.strip():
        return os.path.join(runtime, "localfly.sock")
    import tempfile

    return os.path.join(tempfile.gettempdir(), "localfly.sock")


def display_url(app: "SavedApp", tunnel: dict | None) -> str | None:
    """Public URL, preferring a self-hosted Caddy address like the CLI does."""
    if not tunnel or tunnel.get("status") != "live":
        return None
    if app.public_address:
        return app.public_address
    host = tunnel.get("relay_host")
    port = tunnel.get("remote_port")
    if host and port:
        return f"http://{host}:{port}"
    return None


def parse_relay(relay: str) -> tuple[str, int]:
    """Split `host` or `host:port` (control port, default 7835)."""
    text = (relay or "").strip()
    if ":" in text:
        host, _, tail = text.rpartition(":")
        try:
            return host or "bore.pub", int(tail)
        except ValueError:
            pass
    return (text or "bore.pub"), 7835


def relay_latency_ms(relay: str, timeout: float = 2.0) -> float | None:
    """TCP connect time to the relay control port in ms, or None."""
    import socket as _socket
    import time as _time

    host, port = parse_relay(relay)
    start = _time.monotonic()
    for family in (_socket.AF_INET, _socket.AF_INET6):
        try:
            infos = _socket.getaddrinfo(host, port, family, _socket.SOCK_STREAM)
        except OSError:
            continue
        for info in infos:
            try:
                with _socket.socket(info[0], info[1], info[2]) as s:
                    s.settimeout(timeout)
                    s.connect(info[4])
                    return ( _time.monotonic() - start) * 1000.0
            except OSError:
                continue
    return None


def read_secret() -> str:
    """Relay secret: env wins, then the 0600 file (Rust also mirrors keyring)."""
    env = os.environ.get("LOCALFLY_SECRET")
    if env:
        return env
    try:
        with open(os.path.join(config_dir(), "secret"), encoding="utf-8") as f:
            return f.read().strip()
    except OSError:
        return ""


def effective_options(auto_reconnect_default: bool = True) -> dict:
    """Tunnel options for daemon start, mirroring the CLI's effective_* logic."""
    cfg = load_effective_config()
    relay = os.environ.get("LOCALFLY_RELAY") or cfg.get("relay") or "bore.pub"
    secret = read_secret() or None
    local_host = cfg.get("local_host") or "localhost"
    auto = cfg.get("auto_reconnect", auto_reconnect_default)
    auto = bool(auto) if isinstance(auto, bool) else auto_reconnect_default
    return {
        "relay": relay,
        "secret": secret,
        "local_host": local_host,
        "auto_reconnect": auto,
    }


def qrencode_png(url: str, out_path: str, size: int = 6) -> bool:
    """Render url as QR via the `qrencode` CLI. False when unavailable."""
    import subprocess as _subprocess

    if not shutil.which("qrencode"):
        return False
    try:
        r = _subprocess.run(
            ["qrencode", "-o", out_path, "-s", str(size), "-m", "2", url],
            timeout=10,
            capture_output=True,
        )
        return r.returncode == 0 and os.path.isfile(out_path)
    except (OSError, subprocess.SubprocessError):
        return False


def _send_recv(payload: dict, timeout: int = 20) -> dict:
    import json
    import socket as _socket

    data = (json.dumps(payload) + "\n").encode()
    with _socket.socket(_socket.AF_UNIX, _socket.SOCK_STREAM) as s:
        s.settimeout(timeout)
        s.connect(socket_path())
        s.sendall(data)
        chunks = []
        while True:
            part = s.recv(65536)
            if not part:
                break
            chunks.append(part)
            if b"\n" in part:
                break
        raw = b"".join(chunks).decode("utf-8", "replace").strip().splitlines()
        if not raw:
            raise ConnectionError("empty reply from localflyd")
        reply = json.loads(raw[0])
        if not isinstance(reply, dict):
            raise ConnectionError("bad reply from localflyd")
        return reply


def _localflyd_candidates() -> list:
    cands = []
    try:
        exe = find_binary()
        exe_dir = os.path.dirname(os.path.realpath(exe))
        sib = os.path.join(exe_dir, "localflyd")
        cands.append(sib)
    except Exception:
        pass
    here = os.path.dirname(os.path.realpath(__file__))
    repo = os.path.dirname(here)
    cands.append(os.path.join(repo, "target", "debug", "localflyd"))
    cands.append(os.path.join(repo, "target", "release", "localflyd"))
    which = shutil.which("localflyd")
    if which:
        cands.append(which)
    return cands


def ensure_daemon(timeout: float = 5.0) -> None:
    """Ping the daemon; spawn a detached one if nothing answers."""
    import socket as _socket
    import subprocess
    import time

    try:
        reply = _send_recv({"cmd": "ping"}, timeout=2)
        if reply.get("ok"):
            return
    except (OSError, ConnectionError, ValueError):
        pass
    binary = next(
        (c for c in _localflyd_candidates() if os.path.isfile(c) and os.access(c, os.X_OK)),
        "",
    )
    if not binary:
        raise ConnectionError("localflyd not found (reinstall Localfly Linux)")
    try:
        subprocess.Popen(
            [binary],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except OSError as e:
        raise ConnectionError(f"cannot start localflyd: {e}")
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            reply = _send_recv({"cmd": "ping"}, timeout=2)
            if reply.get("ok"):
                return
        except (OSError, ConnectionError, ValueError):
            time.sleep(0.1)
    raise ConnectionError("localflyd did not come up")


def daemon_call(cmd: str, timeout: int = 20, **fields) -> dict:
    """One request (auto-spawns the daemon). Raises on transport or daemon error."""
    ensure_daemon()
    payload = {"cmd": cmd, **fields}
    reply = _send_recv(payload, timeout=timeout)
    if not reply.get("ok", False):
        raise RuntimeError(reply.get("error") or "daemon refused the request")
    return reply


def subscribe_lines(timeout: int = 10):
    """Yield parsed {"info": ...} dicts until the server closes the stream."""
    import json
    import socket as _socket

    ensure_daemon()
    data = (json.dumps({"cmd": "subscribe"}) + "\n").encode()
    s = _socket.socket(_socket.AF_UNIX, _socket.SOCK_STREAM)
    s.settimeout(timeout)
    try:
        s.connect(socket_path())
        s.sendall(data)
        buf = b""
        while True:
            part = s.recv(65536)
            if not part:
                return
            buf += part
            while b"\n" in buf:
                line, buf = buf.split(b"\n", 1)
                line = line.strip()
                if not line:
                    continue
                try:
                    msg = json.loads(line.decode("utf-8", "replace"))
                except ValueError:
                    continue
                if isinstance(msg, dict) and "info" in msg:
                    yield msg["info"]
    finally:
        try:
            s.close()
        except OSError:
            pass


@dataclass
class TunnelProc:
    """A supervised `localfly on` child process."""

    app_id: str
    proc: subprocess.Popen
    url: str | None = None
    lines: list[str] = field(default_factory=list)
    since: float | None = None  # monotonic() when first live
    stats: dict = field(default_factory=lambda: {
        "bytes_in": 0, "bytes_out": 0, "active": 0, "total": 0,
    })
