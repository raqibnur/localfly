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
    proc = run_cli("list", "--json")
    if proc.returncode != 0:
        raise RuntimeError(proc.stderr.strip() or "localfly list failed")
    try:
        raw = json.loads(proc.stdout or "[]")
    except json.JSONDecodeError as e:
        raise RuntimeError(f"could not parse localfly list output: {e}")
    apps = []
    for item in raw:
        apps.append(
            SavedApp(
                id=str(item.get("id", "")),
                name=str(item.get("name", "")),
                port=int(item.get("port", 0)),
                start_on_launch=bool(item.get("start_on_launch", False)),
                public_port=item.get("public_port"),
                public_address=item.get("public_address"),
                hue=item.get("hue"),
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
    """Parse `web live: localhost:3000 -> http://127.0.0.1:41234`."""
    if " live: " not in line or " -> " not in line:
        return None
    left, url = line.split(" -> ", 1)
    name = left.split(" live: ", 1)[0].strip()
    url = url.strip()
    if not url or not name:
        return None
    return name, url


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
