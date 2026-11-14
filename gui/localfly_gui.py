#!/usr/bin/env python3
"""Localfly GUI for Linux: Adwaita window + tray, mirroring the macOS app.

Main window (like MainWindow: sidebar Apps/Relay/General/About), app cards
with on/off switches, URL copy/open, live stats, add/edit dialogs, relay and
general settings. Closing the window keeps tunnels running (like macOS);
Quit stops them. Backend tunnels run via the `localfly` CLI binary.
"""
from __future__ import annotations

import os
import subprocess
import sys
import threading

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import localfly_backend as B

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, Gio, GLib, Gtk

try:
    from sni import MenuItem, Tray

    HAVE_TRAY = True
except ImportError:
    HAVE_TRAY = False

APP_ID = "com.localfly.Localfly"
VERSION = "0.1.0"


def css():
    provider = Gtk.CssProvider()
    provider.load_from_string(
        """
        .localfly-dot { font-size: 14px; }
        .localfly-url { font-family: monospace; font-size: 12px; }
        .localfly-dim { opacity: 0.65; }
        .localfly-hero { font-size: 30px; font-weight: 800; }
        .localfly-monogram {
            background: @accent_bg_color; color: @accent_fg_color;
            border-radius: 9999px; font-weight: 800;
        }
        """
    )
    Gtk.StyleContext.add_provider_for_display(
        Gdk.Display.get_default(), provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
    )


class AppRow(Gtk.Box):
    """One saved app card (like macOS AppCard)."""

    def __init__(self, win: "MainWindow", state: B.AppState):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.win = win
        self.app_id = state.app.id
        self.add_css_class("card")
        self.set_margin_top(6)
        self.set_margin_bottom(6)

        top = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        top.set_margin_top(12)
        top.set_margin_bottom(12)
        top.set_margin_start(14)
        top.set_margin_end(14)
        self.append(top)

        self.dot = Gtk.Label()
        self.dot.add_css_class("localfly-dot")
        top.append(self.dot)

        titles = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        titles.set_hexpand(True)
        top.append(titles)
        self.name_label = Gtk.Label(xalign=0)
        self.name_label.add_css_class("heading")
        titles.append(self.name_label)
        self.port_label = Gtk.Label(xalign=0)
        self.port_label.add_css_class("localfly-dim")
        self.port_label.add_css_class("localfly-url")
        titles.append(self.port_label)

        self.url_label = Gtk.Label(xalign=0, selectable=True)
        self.url_label.add_css_class("localfly-url")
        top.append(self.url_label)

        self.copy_btn = Gtk.Button(icon_name="edit-copy-symbolic", tooltip_text="Copy public URL")
        self.copy_btn.add_css_class("flat")
        self.copy_btn.connect("clicked", self.on_copy)
        top.append(self.copy_btn)
        self.open_btn = Gtk.Button(icon_name="document-open-symbolic", tooltip_text="Open in browser")
        self.open_btn.add_css_class("flat")
        self.open_btn.connect("clicked", self.on_open)
        top.append(self.open_btn)

        self.switch = Gtk.Switch(valign=Gtk.Align.CENTER, tooltip_text="Expose this app")
        self.switch.connect("state-set", self.on_toggle)
        top.append(self.switch)

        bottom = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        bottom.set_margin_start(14)
        bottom.set_margin_end(14)
        bottom.set_margin_bottom(10)
        self.append(bottom)
        self.status_label = Gtk.Label(xalign=0)
        self.status_label.add_css_class("localfly-dim")
        self.status_label.set_hexpand(True)
        bottom.append(self.status_label)
        edit_btn = Gtk.Button(label="Edit")
        edit_btn.add_css_class("flat")
        edit_btn.connect("clicked", lambda *_: self.win.open_editor(self.app_id))
        bottom.append(edit_btn)
        del_btn = Gtk.Button(label="Delete")
        del_btn.add_css_class("flat")
        del_btn.connect("clicked", lambda *_: self.win.delete_app(self.app_id))
        bottom.append(del_btn)

        self._url = None
        self.refresh(state, None)

    def refresh(self, state: B.AppState, tunnel: B.TunnelProc | None):
        app = state.app
        self.name_label.set_text(app.name)
        extra = " · start-on-launch" if app.start_on_launch else ""
        self.port_label.set_text(f"localhost:{app.port}{extra}")
        listening = state.listening
        if tunnel and tunnel.url:
            dot, tip = "●", "live"
            color = "#2ec27e"
        elif tunnel:
            dot, tip = "●", "connecting"
            color = "#e5a50a"
        elif listening:
            dot, tip = "●", "listening"
            color = "#2ec27e"
        elif listening is False:
            dot, tip = "○", "nothing listening"
            color = "#9a9996"
        else:
            dot, tip = "●", ""
            color = "#9a9996"
        self.dot.set_markup(f'<span color="{color}">{dot}</span>')
        self.dot.set_tooltip_text(tip)

        live = bool(tunnel and tunnel.url)
        self._url = tunnel.url if tunnel else None
        self.url_label.set_text(self._url or "")
        self.url_label.set_visible(live)
        self.copy_btn.set_visible(live)
        self.open_btn.set_visible(live)
        self.switch.set_state(live)
        # Block signal feedback while syncing: state-set already returned; use set_active guard.
        self.switch.set_active(live)

        if tunnel and tunnel.url:
            self.status_label.set_text("Live — close the window and the tunnel keeps running")
        elif tunnel:
            tail = (tunnel.lines[-1] if tunnel.lines else "connecting…").strip()
            self.status_label.set_text(tail[-120:])
        elif listening is False:
            self.status_label.set_text(f"Nothing running on :{app.port} — start your dev server first")
        else:
            self.status_label.set_text("")

    def on_toggle(self, switch, value):
        if value:
            self.win.start_tunnel(self.app_id)
        else:
            self.win.stop_tunnel(self.app_id)
        return True  # we sync visual state on refresh

    def on_copy(self, _btn):
        if self._url:
            self.win.get_clipboard().set(self._url)
            self.win.toast("Public URL copied")

    def on_open(self, _btn):
        if self._url:
            Gio.AppInfo.launch_default_for_uri(self._url, None)


class AppEditor(Gtk.Window):
    """Add/edit dialog (like macOS AppEditor sheet)."""

    def __init__(self, win: "MainWindow", app: B.SavedApp | None):
        kind = "Edit App" if app else "Add App"
        super().__init__(title=kind, transient_for=win, modal=True)
        self.win = win
        self.edit_id = app.id if app else None
        self.set_default_size(420, -1)

        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        box.set_margin_top(18)
        box.set_margin_bottom(18)
        box.set_margin_start(18)
        box.set_margin_end(18)
        self.set_child(box)

        grid = Gtk.Grid(column_spacing=12, row_spacing=10)
        box.append(grid)

        def row(i, label, widget):
            lab = Gtk.Label(label=label, xalign=1)
            lab.add_css_class("localfly-dim")
            grid.attach(lab, 0, i, 1, 1)
            grid.attach(widget, 1, i, 1, 1)
            return widget

        self.name_entry = row(0, "Name", Gtk.Entry(placeholder_text="e.g. web"))
        self.port_entry = row(1, "Port", Gtk.Entry(placeholder_text="3000"))
        self.start_row = Gtk.CheckButton(label="Start tunnel when Localfly opens")
        grid.attach(self.start_row, 1, 2, 1, 1)
        self.pubport_entry = row(3, "Public port", Gtk.Entry(placeholder_text="Random"))
        self.addr_entry = row(4, "Address", Gtk.Entry(placeholder_text="https://web.yourdomain.com"))

        hint = Gtk.Label(
            label="Public port / address need a self-hosted relay (see Relay page).",
            wrap=True,
            xalign=0,
        )
        hint.add_css_class("localfly-dim")
        box.append(hint)
        self.error_label = Gtk.Label(xalign=0, wrap=True)
        box.append(self.error_label)

        actions = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        actions.set_halign(Gtk.Align.END)
        box.append(actions)
        cancel = Gtk.Button(label="Cancel")
        cancel.connect("clicked", lambda *_: self.destroy())
        actions.append(cancel)
        self.save_btn = Gtk.Button(label="Save")
        self.save_btn.add_css_class("suggested-action")
        self.save_btn.connect("clicked", self.on_save)
        actions.append(self.save_btn)

        if app:
            self.name_entry.set_text(app.name)
            self.port_entry.set_text(str(app.port))
            self.start_row.set_active(app.start_on_launch)
            self.pubport_entry.set_text(str(app.public_port or ""))
            self.addr_entry.set_text(app.public_address or "")
        else:
            first = win.first_detected() or 3000
            self.port_entry.set_text(str(first))
        self.name_entry.grab_focus()

    def fail(self, msg):
        self.error_label.set_text(msg)

    def on_save(self, _btn):
        name = self.name_entry.get_text().strip()
        port_t = "".join(c for c in self.port_entry.get_text() if c.isdigit())[:5]
        pub_t = "".join(c for c in self.pubport_entry.get_text() if c.isdigit())[:5]
        addr = self.addr_entry.get_text().strip()
        try:
            port = int(port_t)
            if not 1 <= port <= 65535:
                raise ValueError
        except ValueError:
            return self.fail("Port must be a TCP port (1–65535).")
        if not name:
            return self.fail("Name is required.")
        args = ["--name", name, "--port", str(port)]
        if pub_t:
            args += ["--public-port", pub_t]
        if addr:
            args += ["--public-address", addr]
        if self.start_row.get_active():
            args += ["--start-on-launch"]
        if self.edit_id:
            try:
                B.run_cli("rm", self.edit_id[:8])
            except Exception as e:
                return self.fail(str(e))
        try:
            proc = B.run_cli("add", *args)
        except Exception as e:
            return self.fail(str(e))
        if proc.returncode != 0:
            return self.fail((proc.stderr or proc.stdout).strip() or "Could not save.")
        self.win.toast("App saved")
        self.destroy()
        self.win.refresh_soon()


class MainWindow(Adw.ApplicationWindow):
    def __init__(self, app: Adw.Application):
        super().__init__(application=app, title="Localfly")
        self.set_default_size(900, 600)
        self.tunnels: dict[str, B.TunnelProc] = {}
        self.states: list[B.AppState] = []
        self.detected: list[int] = []
        self.page = "apps"
        self._rows: dict[str, AppRow] = {}
        self._refresh_lock = threading.Lock()
        self._refresh_pending = False
        self._relay = "bore.pub"

        css()
        self.toast_overlay = Adw.ToastOverlay()
        self.set_content(self.toast_overlay)

        split = Adw.NavigationSplitView()
        self.toast_overlay.set_child(split)

        side_page = Adw.NavigationPage(title="Localfly")
        sidebar = Gtk.ListBox(selection_mode=Gtk.SelectionMode.SINGLE)
        sidebar.add_css_class("navigation-sidebar")
        side_page.set_child(sidebar)
        split.set_sidebar(side_page)

        self.side_rows: dict[str, Gtk.ListBoxRow] = {}
        for key, title in [("apps", "Apps"), ("relay", "Relay"), ("general", "General"), ("about", "About")]:
            row = Gtk.ListBoxRow()
            box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
            box.set_margin_top(8)
            box.set_margin_bottom(8)
            box.set_margin_start(12)
            box.set_margin_end(12)
            box.append(Gtk.Label(label=title, xalign=0, hexpand=True))
            if key == "apps":
                self.live_badge = Gtk.Label()
                self.live_badge.add_css_class("caption")
                self.live_badge.add_css_class("localfly-dim")
                box.append(self.live_badge)
            row.set_child(box)
            row.key = key  # type: ignore[attr-defined]
            sidebar.append(row)
            self.side_rows[key] = row
        self.content_title = None
        self.stack = None
        sidebar.connect("row-selected", self.on_navigate)
        sidebar.select_row(self.side_rows["apps"])

        content_page = Adw.NavigationPage(title="Apps")
        self.content_title = content_page
        split.set_content(content_page)
        self.stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.CROSSFADE)

        self.apps_scroll, self.apps_list = self.build_apps_page()
        self.stack.add_named(self.apps_scroll, "apps")
        self.stack.add_named(self.build_relay_page(), "relay")
        self.stack.add_named(self.build_general_page(), "general")
        self.stack.add_named(self.build_about_page(), "about")

        header = Adw.HeaderBar()
        add_btn = Gtk.Button(icon_name="list-add-symbolic", tooltip_text="Add app")
        add_btn.connect("clicked", lambda *_: self.open_editor(None))
        header.pack_start(add_btn)
        menu = Gio.Menu()
        menu.append("Open in Terminal", "app.terminal")
        menu.append("Quit", "app.quit")
        menu_btn = Gtk.MenuButton(menu_model=menu, icon_name="open-menu-symbolic")
        header.pack_end(menu_btn)
        # HeaderBar on NavigationSplitView content pages:
        content_page.set_child(self._wrap_with_header(self.stack, header))

        self.connect("close-request", self.on_close_request)
        self.setup_tray()
        self.refresh_soon()
        GLib.timeout_add_seconds(3, self.on_poll_tick)

    def _wrap_with_header(self, child, header):
        view = Adw.ToolbarView()
        view.add_top_bar(header)
        view.set_content(child)
        return view

    # ----- navigation -----
    def on_navigate(self, _box, row):
        if row is None or self.content_title is None or self.stack is None:
            return
        self.page = row.key
        titles = {"apps": "Apps", "relay": "Relay", "general": "General", "about": "About"}
        self.content_title.set_title(titles[self.page])
        self.stack.set_visible_child_name(self.page)
        if self.page == "relay":
            self.update_relay_page()
        if self.page == "general":
            self.update_general_page()

    # ----- apps page -----
    def build_apps_page(self):
        scroll = Gtk.ScrolledWindow(hscrollbar_policy=Gtk.PolicyType.NEVER)
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        outer.set_margin_top(12)
        outer.set_margin_bottom(24)
        outer.set_margin_start(24)
        outer.set_margin_end(24)
        scroll.set_child(outer)

        title = Gtk.Label(label="Apps", xalign=0)
        title.add_css_class("localfly-hero")
        outer.append(title)
        self.summary_label = Gtk.Label(xalign=0)
        self.summary_label.add_css_class("localfly-dim")
        outer.append(self.summary_label)

        self.empty_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        empty = Gtk.Label(label="Put a local app on the internet.\nSave an app by its port, flip the switch, and share the address.")
        empty.set_justify(Gtk.Justification.CENTER)
        self.empty_box.append(empty)
        add_first = Gtk.Button(label="Add Your First App")
        add_first.add_css_class("suggested-action")
        add_first.connect("clicked", lambda *_: self.open_editor(None))
        add_first.set_halign(Gtk.Align.CENTER)
        self.empty_box.append(add_first)
        outer.append(self.empty_box)

        self.rows_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.append(self.rows_box)

        self.detected_label = Gtk.Label(label="ALSO RUNNING HERE", xalign=0)
        self.detected_label.add_css_class("caption")
        self.detected_label.add_css_class("localfly-dim")
        outer.append(self.detected_label)
        self.detected_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        outer.append(self.detected_box)
        return scroll, outer

    def first_detected(self):
        saved = {s.app.port for s in self.states}
        for p in self.detected:
            if p not in saved:
                return p
        return None

    # ----- relay page -----
    def build_relay_page(self):
        page = Adw.PreferencesPage()
        group = Adw.PreferencesGroup(title="Relay", description="bore server on TCP 7835. Default: bore.pub")
        page.add(group)
        self.relay_entry = Adw.EntryRow(title="Server")
        group.add(self.relay_entry)
        self.secret_entry = Adw.PasswordEntryRow(title="Secret")
        group.add(self.secret_entry)
        btns = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        btns.set_margin_top(8)
        btns.set_margin_start(12)
        save = Gtk.Button(label="Save relay settings")
        save.add_css_class("suggested-action")
        save.connect("clicked", self.on_save_relay)
        btns.append(save)
        check = Gtk.Button(label="Check latency")
        check.connect("clicked", self.on_check_relay)
        btns.append(check)
        group.add(btns)
        self.relay_status = Gtk.Label(xalign=0)
        self.relay_status.add_css_class("localfly-dim")
        group.add(self.relay_status)
        return page

    def update_relay_page(self):
        cfg = B.load_effective_config()
        relay = cfg.get("relay") or "bore.pub"
        self._relay = relay
        self.relay_entry.set_text(relay)
        self.secret_entry.set_text("")
        self.secret_entry.set_title("Secret (%s)" % ("saved" if B.secret_configured() else "not set"))
        self.on_check_relay(None)

    def on_save_relay(self, _btn):
        relay = self.relay_entry.get_text().strip()
        secret = self.secret_entry.get_text()
        args = []
        if relay:
            args += ["--relay", relay]
        if secret:
            args += ["--secret", secret]
        if not args:
            return self.toast("Nothing to save")
        try:
            proc = B.run_cli("config", *args)
        except Exception as e:
            return self.toast(f"Save failed: {e}")
        if proc.returncode != 0:
            return self.toast((proc.stderr or proc.stdout).strip() or "Save failed")
        self._relay = relay or self._relay
        self.toast("Relay settings saved")
        self.refresh_soon()

    def on_check_relay(self, _btn):
        relay = self.relay_entry.get_text().strip() if hasattr(self, "relay_entry") else None
        threading.Thread(target=self._check_relay_bg, args=(relay,), daemon=True).start()

    def _check_relay_bg(self, relay):
        ok, out = B.relay_status(relay or None)
        GLib.idle_add(lambda: self.relay_status.set_text(out) or False)

    # ----- general page -----
    def build_general_page(self):
        page = Adw.PreferencesPage()
        group = Adw.PreferencesGroup(title="General")
        page.add(group)
        self.reconnect_row = Adw.SwitchRow(title="Reconnect automatically", subtitle="Retry with backoff after drops")
        self.reconnect_row.connect("notify::active", self.on_reconnect_changed)
        group.add(self.reconnect_row)
        self.login_row = Adw.SwitchRow(title="Start on login", subtitle="Run saved start-on-launch apps via systemd")
        self.login_row.connect("notify::active", self.on_login_changed)
        group.add(self.login_row)
        self._login_guard = False
        btn = Gtk.Button(label="Check for updates")
        btn.connect("clicked", self.on_update_check)
        group.add(btn)
        self.update_label = Gtk.Label(xalign=0)
        self.update_label.add_css_class("localfly-dim")
        group.add(self.update_label)
        return page

    def update_general_page(self):
        cfg = B.load_effective_config()
        self.reconnect_row.set_active(bool(cfg.get("auto_reconnect", True)))
        self._login_guard = True
        try:
            self.login_row.set_active(self.login_enabled())
        finally:
            self._login_guard = False

    def on_reconnect_changed(self, row, _pspec):
        try:
            B.run_cli("config", "--auto-reconnect", "true" if row.get_active() else "false")
        except Exception as e:
            self.toast(f"Save failed: {e}")

    def systemd_unit_path(self):
        return os.path.expanduser("~/.config/systemd/user/localfly.service")

    def unit_template_path(self):
        # Kept for reference; the installed unit is generated (see unit_content)
        # so login works regardless of where the GUI is launched from.
        here = os.path.realpath(__file__)
        return os.path.join(os.path.dirname(os.path.dirname(here)), "localfly.service")

    def unit_content(self):
        return (
            "[Unit]\n"
            "Description=Localfly — expose saved localhost apps via bore\n"
            "After=network-online.target\n"
            "Wants=network-online.target\n"
            "\n"
            "[Service]\n"
            f"ExecStart={B.find_binary()} up\n"
            "Restart=on-failure\n"
            "RestartSec=5\n"
            "\n"
            "[Install]\n"
            "WantedBy=default.target\n"
        )

    def login_enabled(self):
        try:
            proc = subprocess.run(
                ["systemctl", "--user", "is-enabled", "localfly.service"],
                capture_output=True, text=True, timeout=10,
            )
            return proc.returncode == 0 and proc.stdout.strip() == "enabled"
        except (OSError, subprocess.SubprocessError):
            return False

    def on_login_changed(self, row, _pspec):
        if self._login_guard:
            return
        enable = row.get_active()
        try:
            if enable:
                os.makedirs(os.path.dirname(self.systemd_unit_path()), exist_ok=True)
                with open(self.systemd_unit_path(), "w") as f:
                    f.write(self.unit_content())
                proc = subprocess.run(
                    ["systemctl", "--user", "enable", "localfly.service"],
                    capture_output=True, text=True, timeout=15,
                )
                if proc.returncode != 0:
                    raise RuntimeError((proc.stderr or proc.stdout).strip() or "systemctl enable failed")
            else:
                proc = subprocess.run(
                    ["systemctl", "--user", "disable", "localfly.service"],
                    capture_output=True, text=True, timeout=15,
                )
                if proc.returncode != 0:
                    raise RuntimeError((proc.stderr or proc.stdout).strip() or "systemctl disable failed")
            self.toast("Start on login on" if enable else "Start on login off")
        except Exception as e:
            self.toast(f"Login setting failed: {e}")
            self._login_guard = True
            row.set_active(not enable)
            self._login_guard = False

    def on_update_check(self, _btn):
        threading.Thread(target=self._update_bg, daemon=True).start()

    def _update_bg(self):
        try:
            proc = B.run_cli("update-check", timeout=30)
            out = (proc.stdout + proc.stderr).strip()
        except Exception as e:
            out = f"Update check failed: {e}"
        GLib.idle_add(lambda: self.update_label.set_text(out) or False)

    # ----- about page -----
    def build_about_page(self):
        page = Adw.PreferencesPage()
        group = Adw.PreferencesGroup(title="Localfly", description="Your localhost, on the internet.")
        page.add(group)
        group.add(Adw.ActionRow(title="Version", subtitle=VERSION))
        group.add(Adw.ActionRow(title="Maintainer", subtitle="9M2PJU"))
        credit = Adw.ActionRow(
            title="Original macOS app",
            subtitle="Raqib Nur — raqibnur/localfly (MIT). Click to open.",
        )
        credit.set_activatable(True)
        credit.connect(
            "activated",
            lambda *_: Gio.AppInfo.launch_default_for_uri(
                "https://github.com/raqibnur/localfly", None
            ),
        )
        group.add(credit)
        group.add(Adw.ActionRow(title="Protocol", subtitle="bore v0.6 over an outgoing relay connection"))
        group.add(Adw.ActionRow(title="License", subtitle="MIT"))
        return page

    # ----- data -----
    def toast(self, msg):
        self.toast_overlay.add_toast(Adw.Toast(title=msg))

    def refresh_soon(self):
        with self._refresh_lock:
            if self._refresh_pending:
                return
            self._refresh_pending = True
        threading.Thread(target=self._refresh_bg, daemon=True).start()

    def on_poll_tick(self):
        self.refresh_soon()
        return True  # keep timer

    def _refresh_bg(self):
        try:
            apps = B.list_apps()
        except Exception:
            apps = []
        states = []
        for app in apps:
            states.append(B.AppState(app=app, listening=B.probe_tcp(app.port)))
        try:
            found = B.scan()
        except Exception:
            found = []
        cfg = B.load_effective_config()
        relay = str(cfg.get("relay") or "bore.pub")
        GLib.idle_add(self._apply_refresh, states, found, relay)
        with self._refresh_lock:
            self._refresh_pending = False

    def _apply_refresh(self, states, found, relay):
        self.states = states
        self.detected = found or []
        self._relay = relay
        live = sum(1 for t in self.tunnels.values() if t.url)
        self.live_badge.set_text(f"{live} live" if live else "")
        n = len(states)
        summary = "Nothing saved yet." if n == 0 else ("One app saved:" if n == 1 else f"{n} apps saved:")
        self.summary_label.set_text(f"{summary}  via {relay}")
        self.empty_box.set_visible(n == 0)

        seen = set()
        for st in states:
            seen.add(st.app.id)
            row = self._rows.get(st.app.id)
            if row is None:
                row = AppRow(self, st)
                self._rows[st.app.id] = row
                self.rows_box.append(row)
            row.refresh(st, self.tunnels.get(st.app.id))
        for app_id in [k for k in self._rows if k not in seen]:
            self.rows_box.remove(self._rows.pop(app_id))

        saved_ports = {s.app.port for s in states}
        new_ports = [p for p in self.detected if p not in saved_ports]
        self.detected_label.set_visible(bool(new_ports) and n > 0)
        while (child := self.detected_box.get_first_child()) is not None:
            self.detected_box.remove(child)
        for port in new_ports[:8]:
            btn = Gtk.Button(label=f"localhost:{port}  +")
            btn.connect("clicked", self._on_quick_add, port)
            self.detected_box.append(btn)
        self.update_tray_menu()
        return False

    def _on_quick_add(self, _btn, port):
        self.open_editor(None, preset_port=port)

    # ----- tunnels -----
    def start_tunnel(self, app_id):
        if app_id in self.tunnels:
            return
        st = next((s for s in self.states if s.app.id == app_id), None)
        if st and st.listening is False:
            self.toast(f"Nothing running on :{st.app.port} — start your dev server first")
            self.refresh_soon()
            return
        try:
            proc = subprocess.Popen(
                [B.find_binary(), "on", app_id[:8]],
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1,
            )
        except OSError as e:
            self.toast(f"Could not start tunnel: {e}")
            return
        tun = B.TunnelProc(app_id=app_id, proc=proc)
        self.tunnels[app_id] = tun
        threading.Thread(target=self._watch_tunnel, args=(tun,), daemon=True).start()
        self.refresh_soon()

    def _idle_refresh(self):
        self.refresh_soon()
        return False

    def _watch_tunnel(self, tun: B.TunnelProc):
        assert tun.proc.stdout is not None and tun.proc.stderr is not None
        for line in tun.proc.stdout:
            parsed = B.parse_live_line(line.strip())
            if parsed:
                tun.url = parsed[1]
                GLib.idle_add(self._idle_refresh)
        # stdout closed: collect stderr tail for the error message
        try:
            err = tun.proc.stderr.read() or ""
        except Exception:
            err = ""
        tun.lines.extend(err.strip().splitlines()[-3:])
        tun.proc.wait()
        # If still registered, the tunnel died on its own: keep record for error display briefly.
        GLib.idle_add(self._tunnel_exited, tun.app_id)

    def _tunnel_exited(self, app_id):
        tun = self.tunnels.get(app_id)
        if tun and tun.proc.poll() is not None and not tun.url:
            # First attempt failed and no auto-reconnect output followed: drop and show error.
            msg = (tun.lines[-1] if tun.lines else "tunnel exited").strip()
            del self.tunnels[app_id]
            self.toast(msg[-140:])
        self.refresh_soon()
        return False

    def stop_tunnel(self, app_id):
        tun = self.tunnels.pop(app_id, None)
        if tun and tun.proc.poll() is None:
            tun.proc.terminate()
            threading.Thread(target=self._kill_after, args=(tun,), daemon=True).start()
        self.refresh_soon()

    @staticmethod
    def _kill_after(tun: B.TunnelProc):
        try:
            tun.proc.wait(timeout=5)
        except Exception:
            try:
                tun.proc.kill()
            except Exception:
                pass

    def stop_all_tunnels(self):
        for app_id in list(self.tunnels):
            self.stop_tunnel(app_id)

    # ----- app actions -----
    def open_editor(self, app_id: str | None, preset_port: int | None = None):
        app = next((s.app for s in self.states if s.app.id == app_id), None) if app_id else None
        dlg = AppEditor(self, app)
        if preset_port is not None and app is None:
            dlg.port_entry.set_text(str(preset_port))
        dlg.present()

    def delete_app(self, app_id):
        st = next((s for s in self.states if s.app.id == app_id), None)
        name = st.app.name if st else app_id[:8]
        dlg = Adw.AlertDialog(heading=f"Delete {name}?", body="Its tunnel closes and the app is removed. Nothing on localhost changes.")
        dlg.add_response("cancel", "Cancel")
        dlg.add_response("delete", "Delete")
        dlg.set_response_appearance("delete", Adw.ResponseAppearance.DESTRUCTIVE)
        dlg.set_default_response("cancel")
        dlg.set_close_response("cancel")

        def on_response(_d, resp):
            if resp == "delete":
                self.stop_tunnel(app_id)
                try:
                    B.run_cli("rm", app_id[:8])
                except Exception as e:
                    self.toast(f"Delete failed: {e}")
                else:
                    self.toast(f"Removed {name}")
                self.refresh_soon()

        dlg.connect("response", on_response)
        dlg.present(self)

    # ----- tray (StatusNotifierItem; works with KDE, sway/waybar, GNOME+extension) -----
    def setup_tray(self):
        self.tray = None
        if not HAVE_TRAY:
            return
        try:
            self.tray = Tray(on_show_menu=self.show_from_tray)
        except GLib.Error:
            self.tray = None
            return
        if not self.tray.registered:
            # No watcher on the bus: tray would be invisible; drop it.
            self.tray.quit()
            self.tray = None
            return
        self._tray_ids = {"header": 1, "open": 10000, "quit": 10001}
        self.update_tray_menu()

    def show_from_tray(self):
        # Left-click toggles the window (ItemIsMenu=false routes button1 to
        # Activate); right-click opens the menu via waybar's dbusmenu popup.
        if self.is_visible():
            self.set_visible(False)
        else:
            self.set_visible(True)
            self.present()

    def update_tray_menu(self):
        if not getattr(self, "tray", None):
            return
        live = sum(1 for t in self.tunnels.values() if t.url)
        total = len(self.states)
        items = [MenuItem(id=1, label=f"Localfly — {live}/{total} live", enabled=False)]
        next_id = 10
        self._tray_app_ids = {}
        for st in self.states:
            app = st.app
            on = bool(self.tunnels.get(app.id) and self.tunnels[app.id].url)
            iid = next_id
            next_id += 1
            self._tray_app_ids[iid] = app.id
            items.append(
                MenuItem(id=iid, label=f"{'●' if on else '○'} {app.name} :{app.port}",
                         checkmark=True, checked=on, on_clicked=self._tray_toggle_cb(app.id))
            )
        items.append(MenuItem(id=2, separator=True, label=""))
        items.append(MenuItem(id=10000, label="Open Localfly", on_clicked=self.show_from_tray))
        items.append(MenuItem(id=10001, label="Quit Localfly", on_clicked=self.quit_app))
        self.tray.set_menu(
            items,
            status="Active" if live else "Active",
            tooltip=f"{live}/{total} tunnels live",
        )

    def _tray_toggle_cb(self, app_id):
        def cb():
            if app_id in self.tunnels:
                self.stop_tunnel(app_id)
            else:
                self.start_tunnel(app_id)

        return cb

    # ----- window lifecycle -----
    def on_close_request(self, _win):
        # Like macOS: closing the window keeps tunnels running in the background.
        self.set_visible(False)
        return True

    def quit_app(self):
        self.stop_all_tunnels()
        if getattr(self, "tray", None):
            try:
                self.tray.quit()
            except Exception:
                pass
            self.tray = None
        self.get_application().quit()


class LocalflyApp(Adw.Application):
    def __init__(self):
        super().__init__(application_id=APP_ID)
        act_quit = Gio.SimpleAction.new("quit", None)
        act_quit.connect("activate", lambda *_: self.quit_from_action())
        self.add_action(act_quit)
        act_term = Gio.SimpleAction.new("terminal", None)
        act_term.connect("activate", lambda *_: self.open_terminal())
        self.add_action(act_term)

    def quit_from_action(self):
        if self.props.active_window:
            self.props.active_window.quit_app()
        else:
            self.quit()

    def open_terminal(self):
        try:
            subprocess.Popen(["xdg-terminal-exec", "--", B.find_binary(), "list"])
        except OSError:
            self.toast("No terminal launcher found (xdg-terminal-exec missing)")

    def do_activate(self):
        win = self.props.active_window
        if win:
            win.set_visible(True)
            win.present()
        else:
            win = MainWindow(self)
            win.present()


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if "--version" in argv or "-V" in argv:
        print(f"localfly-gui {VERSION}")
        return 0
    app = LocalflyApp()
    return app.run([sys.argv[0], *argv])


if __name__ == "__main__":
    sys.exit(main())
