#!/usr/bin/env python3
"""Localfly Linux GUI: Adwaita window + tray, mirroring the macOS app.

Main window (like MainWindow: sidebar Apps/Relay/General/About), app cards
with on/off switches, URL copy/open, live stats, add/edit dialogs, relay and
general settings. Closing the window keeps tunnels running (like macOS);
Quit stops them. Backend tunnels run via the `localfly` CLI binary.
"""
from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import uuid

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
VERSION = "0.1.1"


def css():
    provider = Gtk.CssProvider()
    provider.load_from_string(
        """
        /* Localfly Linux accent: Arch blue. The official logo and the
           green live indicators stay untouched; blue means interactive. */
        @define-color accent_bg_color #1793d1;
        @define-color accent_fg_color #ffffff;
        @define-color accent_color #1793d1;
        switch:checked { background-color: #1793d1; }
        switch:checked slider { background: #ffffff; }
        button.suggested-action {
            background: #1793d1; color: #ffffff;
        }
        button.suggested-action:hover { background: shade(#1793d1, 1.08); }
        button.suggested-action:active { background: shade(#1793d1, 0.92); }
        .localfly-dot { font-size: 14px; }
        .localfly-url { font-family: monospace; font-size: 12px; }
        .localfly-dim { opacity: 0.65; }
        .localfly-hero { font-size: 30px; font-weight: 800; }
        .localfly-monogram {
            color: #ffffff; font-weight: 800; font-size: 16px;
            border-radius: 9999px;
        }
        .localfly-pill {
            background-color: rgba(46, 194, 126, 0.12);
            border: 1px solid rgba(46, 194, 126, 0.4);
            border-radius: 9999px;
            padding-left: 12px; padding-right: 4px;
            padding-top: 2px; padding-bottom: 2px;
        }
        .localfly-pill-label { color: #2ec27e; font-family: monospace; font-size: 12px; }
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

        overlay = Gtk.Overlay()
        top.append(overlay)

        self.monogram = Gtk.Label()
        self.monogram.add_css_class("localfly-monogram")
        self.monogram.set_size_request(40, 40)
        self.monogram.set_halign(Gtk.Align.CENTER)
        self.monogram.set_valign(Gtk.Align.CENTER)
        self._mg_class = None
        self._mg_provider = None
        overlay.set_child(self.monogram)

        self.dot = Gtk.Label()
        self.dot.add_css_class("localfly-dot")
        self.dot.set_halign(Gtk.Align.END)
        self.dot.set_valign(Gtk.Align.END)
        overlay.add_overlay(self.dot)

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

        self.pill = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=2)
        self.pill.add_css_class("localfly-pill")
        self.pill.set_valign(Gtk.Align.CENTER)
        top.append(self.pill)
        self.url_label = Gtk.Label(xalign=0, selectable=True)
        self.url_label.add_css_class("localfly-pill-label")
        self.pill.append(self.url_label)

        self.copy_btn = Gtk.Button(icon_name="edit-copy-symbolic", tooltip_text="Copy public URL")
        self.copy_btn.add_css_class("flat")
        self.copy_btn.connect("clicked", self.on_copy)
        self.pill.append(self.copy_btn)
        self.open_btn = Gtk.Button(icon_name="document-open-symbolic", tooltip_text="Open in browser")
        self.open_btn.add_css_class("flat")
        self.open_btn.connect("clicked", self.on_open)
        self.pill.append(self.open_btn)

        self.qr_btn = Gtk.Button(icon_name="view-grid-symbolic", tooltip_text="Show QR code for phones")
        self.qr_btn.add_css_class("flat")
        self.qr_btn.connect("clicked", self.on_qr)
        self.pill.append(self.qr_btn)
        self._has_qr = shutil.which("qrencode") is not None

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

    def _paint_monogram(self, app):
        initial = (app.name.strip()[:1].upper() or "•")
        key = (initial, round(float(app.hue or 0.07), 4))
        if key == getattr(self, "_mg_key", None):
            return
        self._mg_key = key
        self.monogram.set_text(initial)
        if self._mg_provider is not None:
            Gtk.StyleContext.remove_provider_for_display(
                Gdk.Display.get_default(), self._mg_provider
            )
        if self._mg_class is not None:
            self.monogram.remove_css_class(self._mg_class)
        r, g, b = B.hue_color(app.hue)
        self._mg_class = f"mg{id(self)}"
        self._mg_provider = Gtk.CssProvider()
        self._mg_provider.load_from_string(
            f".{self._mg_class} {{ background-color: rgb({r},{g},{b}); }}"
        )
        Gtk.StyleContext.add_provider_for_display(
            Gdk.Display.get_default(),
            self._mg_provider,
            Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION,
        )
        self.monogram.add_css_class(self._mg_class)

    def refresh(self, state: B.AppState, tunnel: dict | None):
        app = state.app
        self._paint_monogram(app)
        self.name_label.set_text(app.name)
        extra = " · start-on-launch" if app.start_on_launch else ""
        self.port_label.set_text(f"localhost:{app.port}{extra}")
        listening = state.listening
        status = (tunnel or {}).get("status")
        if status == "live":
            dot, tip = "●", "live"
            color = "#2ec27e"
        elif status == "connecting" or (tunnel and self.app_id in self.win._pending):
            dot, tip = "●", "connecting"
            color = "#e5a50a"
        elif status == "error":
            dot, tip = "●", "error"
            color = "#e5484d"
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

        live = status == "live"
        self._url = B.display_url(app, tunnel)
        self.url_label.set_text(self._url or "")
        self.pill.set_visible(live)
        self.qr_btn.set_visible(live and self._has_qr)
        self.switch.set_state(live)
        # Block signal feedback while syncing: state-set already returned; use set_active guard.
        self.switch.set_active(live)

        if live:
            assert tunnel is not None
            parts = []
            epoch = tunnel.get("live_since_epoch")
            if epoch:
                parts.append(B.format_uptime(max(0.0, time.time() - epoch)))
            st_active = tunnel.get("active_connections", 0)
            st_total = tunnel.get("total_connections", 0)
            parts.append(f"{st_active} open · {st_total} total")
            parts.append(f"↓{B.format_bytes(tunnel.get('bytes_in', 0))} ↑{B.format_bytes(tunnel.get('bytes_out', 0))}")
            self.status_label.set_text(" · ".join(parts))
        elif status == "error":
            reason = (tunnel or {}).get("reason") or "tunnel failed"
            self.status_label.set_text(str(reason)[-140:])
        elif status == "connecting" or self.app_id in self.win._pending:
            attempt = (tunnel or {}).get("retry_attempt", 0)
            self.status_label.set_text(f"connecting… (attempt {attempt})" if attempt else "connecting…")
        elif listening is False:
            self.status_label.set_text(f"Nothing running on :{app.port} - start your dev server first")
        else:
            self.status_label.set_text("")

    def on_qr(self, _btn):
        if self._url:
            self.win.show_qr(self.app_id)

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
        self.start_row = Gtk.CheckButton(label="Start tunnel when Localfly Linux opens")
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
        try:
            B.ensure_daemon()
            if self.edit_id:
                st = next((s for s in self.win.states if s.app.id == self.edit_id), None)
                if st is None:
                    return self.fail("App no longer exists.")
                app = st.app
                was_live = bool(self.win.tunnels.get(app.id, {}).get("status") == "live")
                old_port, old_public = app.port, app.public_port
                app.name = name
                app.hue = B.hue_for(name)
                app.port = port
                app.public_port = int(pub_t) if pub_t else None
                app.public_address = addr or None
                app.start_on_launch = self.start_row.get_active()
                reply = B.daemon_call("update", app=app.__dict__)
                if was_live and (app.port != old_port or app.public_port != old_public):
                    B.daemon_call("stop", id=app.id)
            else:
                import uuid as _uuid

                app = B.SavedApp(
                    id=str(_uuid.uuid4()),
                    name=name,
                    port=port,
                    start_on_launch=self.start_row.get_active(),
                    hue=B.hue_for(name),
                    public_port=int(pub_t) if pub_t else None,
                    public_address=addr or None,
                )
                reply = B.daemon_call("add", app=app.__dict__)
            if not reply.get("ok"):
                return self.fail(str(reply.get("error") or "Could not save."))
        except Exception as e:
            return self.fail(str(e))
        self.win.toast("App saved")
        self.destroy()
        self.win.refresh_soon()


class MainWindow(Adw.ApplicationWindow):
    def __init__(self, app: Adw.Application):
        super().__init__(application=app, title="Localfly Linux")
        self.set_default_size(900, 600)
        # Live tunnel info dicts from the daemon, keyed by app id.
        self.tunnels: dict[str, dict] = {}
        # Ids with a start request in flight (shown as connecting).
        self._pending: set[str] = set()
        # For desktop notifications on transitions.
        self._notified_live: set[str] = set()
        self._notified_err: dict[str, str] = {}
        self.states: list[B.AppState] = []
        self.detected: list[int] = []
        self.page = "apps"
        self._rows: dict[str, AppRow] = {}
        self._refresh_lock = threading.Lock()
        self._refresh_pending = False
        self._relay = "bore.pub"
        self._relay_ms: float | None = None
        self._relay_ms_at: float = 0.0
        self._scan_tick = 0

        css()
        self.toast_overlay = Adw.ToastOverlay()
        self.set_content(self.toast_overlay)

        split = Adw.NavigationSplitView()
        self.toast_overlay.set_child(split)

        side_page = Adw.NavigationPage(title="Localfly Linux")
        side_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        side_page.set_child(side_box)
        split.set_sidebar(side_page)

        brand = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        brand.set_margin_top(14)
        brand.set_margin_bottom(6)
        brand.set_margin_start(14)
        brand.set_margin_end(14)
        brand_logo = self.logo_image(36)
        if brand_logo is not None:
            brand.append(brand_logo)
        brand_text = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=1)
        brand_text.set_valign(Gtk.Align.CENTER)
        brand_title = Gtk.Label(label="Localfly Linux", xalign=0)
        brand_title.add_css_class("heading")
        brand_text.append(brand_title)
        brand_sub = Gtk.Label(label="localhost, everywhere", xalign=0)
        brand_sub.add_css_class("caption")
        brand_sub.add_css_class("localfly-dim")
        brand_text.append(brand_sub)
        brand.append(brand_text)
        side_box.append(brand)

        sidebar = Gtk.ListBox(selection_mode=Gtk.SelectionMode.SINGLE)
        sidebar.add_css_class("navigation-sidebar")
        side_box.append(sidebar)

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

    def logo_image(self, size: int):
        """Official logo from the icon theme, or None if not installed."""
        theme = Gtk.IconTheme.get_for_display(Gdk.Display.get_default())
        if theme is None or not theme.has_icon("localfly"):
            return None
        img = Gtk.Image.new_from_icon_name("localfly")
        img.set_pixel_size(size)
        return img

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
            "Description=Localfly Linux - expose saved localhost apps via bore\n"
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
        group = Adw.PreferencesGroup()
        page.add(group)
        logo_row = Adw.ActionRow(
            title="Localfly Linux", subtitle="Your localhost, on the internet"
        )
        logo = self.logo_image(48)
        if logo is not None:
            logo_row.add_prefix(logo)
        group.add(logo_row)
        group.add(Adw.ActionRow(title="Version", subtitle=VERSION))
        maintainer = Adw.ActionRow(
            title="Maintainer",
            subtitle="9M2PJU - hamradio.my. Click to open.",
        )
        maintainer.set_activatable(True)
        maintainer.connect(
            "activated",
            lambda *_: Gio.AppInfo.launch_default_for_uri("https://hamradio.my", None),
        )
        group.add(maintainer)
        credit = Adw.ActionRow(
            title="Original macOS app",
            subtitle="Raqib Nur - raqibnur/localfly (MIT). Click to open.",
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
        daemon_error = None
        try:
            states = self._daemon_snapshot()
        except Exception as e:
            import traceback

            traceback.print_exc()
            states = []
            self._last_refresh_error = str(e)[:200]
            daemon_error = str(e)[:120]
        self._scan_tick += 1
        if self._scan_tick % 5 == 1:
            try:
                found = B.scan()
            except Exception:
                found = []
            self._scan_cache = found
        found = getattr(self, "_scan_cache", [])
        cfg = B.load_effective_config()
        relay = str(cfg.get("relay") or "bore.pub")
        # Relay latency, cached 20 s like the macOS poller.
        now = time.monotonic()
        if now - self._relay_ms_at > 20:
            try:
                self._relay_ms = B.relay_latency_ms(relay, timeout=2.0)
            except Exception:
                self._relay_ms = None
            self._relay_ms_at = now
        latency = self._relay_ms
        GLib.idle_add(self._apply_refresh, states, found, relay, latency, daemon_error)
        with self._refresh_lock:
            self._refresh_pending = False

    def _daemon_snapshot(self) -> list[B.AppState]:
        """One daemon round-trip gives apps + liveness + tunnel state."""
        B.ensure_daemon()
        reply = B.daemon_call("list", timeout=15)
        out = []
        tunnels = {}
        for item in reply.get("apps", []) or []:
            app_d = item.get("app", {})
            app = B.SavedApp(
                id=str(app_d.get("id", "")),
                name=str(app_d.get("name", "")),
                port=int(app_d.get("port", 0)),
                start_on_launch=bool(app_d.get("start_on_launch", False)),
                hue=app_d.get("hue"),
                public_port=app_d.get("public_port"),
                public_address=app_d.get("public_address"),
            )
            out.append(B.AppState(app=app, listening=item.get("listening")))
            if item.get("tunnel"):
                tunnels[app.id] = item["tunnel"]
        # Stash tunnels for tray + notifications.
        self._tunnel_map = tunnels
        return out

    def _apply_refresh(self, states, found, relay, latency, daemon_error=None):
        self.states = states
        self.detected = found or []
        self._relay = relay
        tunnels = getattr(self, "_tunnel_map", {})
        self.tunnels = {k: v for k, v in tunnels.items() if v and v.get("status") in ("live", "connecting", "error")}
        live_ids = {k for k, v in self.tunnels.items() if v.get("status") == "live"}
        self._notify_transitions(live_ids)
        self.live_badge.set_text(f"{len(live_ids)} live" if live_ids else "")
        n = len(states)
        summary = "Nothing saved yet." if n == 0 else ("One app saved:" if n == 1 else f"{n} apps saved:")
        if daemon_error:
            via = f"{summary}  daemon unreachable ({daemon_error})"
        elif latency is None:
            via = f"{summary}  via {relay} (unreachable)"
        else:
            via = f"{summary}  via {relay} · {latency:.0f} ms"
        self.summary_label.set_text(via)
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

    def _notify_transitions(self, live_ids: set[str]):
        new_live = live_ids - self._notified_live
        gone = self._notified_live - live_ids
        for app_id in new_live:
            st = next((s for s in self.states if s.app.id == app_id), None)
            if st is None:
                continue
            url = B.display_url(st.app, self.tunnels.get(app_id)) or ""
            self.notify(f"{st.app.name} is live", url or "tunnel is up")
        for app_id in gone:
            self._notified_err.pop(app_id, None)
        self._notified_live = set(live_ids)
        # Errors worth surfacing once per message.
        for app_id, tun in self.tunnels.items():
            if isinstance(tun, dict) and tun.get("status") == "error":
                reason = str(tun.get("reason") or "tunnel failed")
                if self._notified_err.get(app_id) != reason:
                    self._notified_err[app_id] = reason
                    st = next((s for s in self.states if s.app.id == app_id), None)
                    name = st.app.name if st else "App"
                    self.notify(f"{name} tunnel failed", reason[-140:])

    def notify(self, title: str, body: str):
        try:
            note = Gio.Notification.new(title)
            note.set_body(body)
            note.set_icon(Gio.ThemedIcon.new("localfly"))
            self.get_application().send_notification("localfly-tunnel", note)
        except Exception:
            pass

    def _on_quick_add(self, _btn, port):
        self.open_editor(None, preset_port=port)

    # ----- tunnels (all owned by localflyd; the GUI only asks) -----
    def start_tunnel(self, app_id):
        if app_id in self._pending:
            return
        st = next((s for s in self.states if s.app.id == app_id), None)
        if st and st.listening is False:
            self.toast(f"Nothing running on :{st.app.port} - start your dev server first")
            self.refresh_soon()
            return
        self._pending.add(app_id)
        self.refresh_soon()
        threading.Thread(target=self._start_bg, args=(app_id,), daemon=True).start()

    def _start_bg(self, app_id):
        try:
            B.ensure_daemon()
            st = next((s for s in self.states if s.app.id == app_id), None)
            if st is None:
                return
            options = B.effective_options()
            B.daemon_call(
                "start", timeout=30, id=app_id,
                local_port=st.app.port,
                remote_port=st.app.public_port or 0,
                options=options,
            )
        except Exception as e:
            self.toast(f"Could not start tunnel: {e}")
        finally:
            self._pending.discard(app_id)
            GLib.idle_add(self._idle_refresh)

    def _idle_refresh(self):
        self.refresh_soon()
        return False

    def stop_tunnel(self, app_id):
        self._pending.discard(app_id)
        threading.Thread(target=self._stop_bg, args=(app_id,), daemon=True).start()
        self.refresh_soon()

    def _stop_bg(self, app_id):
        try:
            B.ensure_daemon()
            B.daemon_call("stop", id=app_id)
        except Exception:
            pass
        GLib.idle_add(self._idle_refresh)

    def stop_all_tunnels(self):
        try:
            B.ensure_daemon()
            B.daemon_call("stop_all")
        except Exception:
            pass

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
                    B.ensure_daemon()
                    reply = B.daemon_call("remove", id=app_id)
                    if not reply.get("ok"):
                        raise RuntimeError(str(reply.get("error") or "remove failed"))
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
            # Official logo when installed in the icon theme, generic otherwise.
            theme = Gtk.IconTheme.get_for_display(Gdk.Display.get_default())
            self._has_tray_icon = bool(theme and theme.has_icon("localfly"))
            self._has_tray_live_icon = bool(theme and theme.has_icon("localfly-live"))
            icon = "localfly" if self._has_tray_icon else "network-transmit-receive"
            self.tray = Tray(on_show_menu=self.show_from_tray, icon_name=icon)
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
        live_ids = {k for k, v in self.tunnels.items() if v.get("status") == "live"}
        total = len(self.states)
        # Green-dot icon while anything is live (falls back if theme lacks it).
        base = "localfly" if self._has_tray_icon else "network-transmit-receive"
        icon = "localfly-live" if (live_ids and self._has_tray_live_icon) else base
        if icon != getattr(self.tray, "icon_name", None):
            self.tray.set_icon(icon)
        items = [MenuItem(id=1, label=f"Localfly Linux - {len(live_ids)}/{total} live", enabled=False)]
        next_id = 10
        self._tray_app_ids = {}
        for st in self.states:
            app = st.app
            on = app.id in live_ids
            iid = next_id
            next_id += 1
            self._tray_app_ids[iid] = app.id
            items.append(
                MenuItem(id=iid, label=f"{'●' if on else '○'} {app.name} :{app.port}",
                         checkmark=True, checked=on, on_clicked=self._tray_toggle_cb(app.id))
            )
        items.append(MenuItem(id=2, separator=True, label=""))
        items.append(MenuItem(id=10000, label="Open Localfly Linux", on_clicked=self.show_from_tray))
        items.append(MenuItem(id=10001, label="Quit Localfly Linux", on_clicked=self.quit_app))
        self.tray.set_menu(
            items,
            status="Active",
            tooltip=f"{len(live_ids)}/{total} tunnels live",
        )

    def _tray_toggle_cb(self, app_id):
        def cb():
            tun = self.tunnels.get(app_id)
            if isinstance(tun, dict) and tun.get("status") in ("live", "connecting"):
                self.stop_tunnel(app_id)
            else:
                self.start_tunnel(app_id)

        return cb

    def show_qr(self, app_id):
        st = next((s for s in self.states if s.app.id == app_id), None)
        url = B.display_url(st.app, self.tunnels.get(app_id)) if st else None
        if not url:
            self.toast("No live URL to encode")
            return
        path = os.path.join(tempfile.gettempdir(), f"localfly-qr-{app_id[:8]}.png")
        if not B.qrencode_png(url, path):
            self.toast("QR needs the qrencode tool installed")
            return
        dlg = Gtk.Window(title=f"QR - {st.app.name if st else 'tunnel'}", transient_for=self, modal=True)
        dlg.set_default_size(300, 360)
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        box.set_margin_top(18)
        box.set_margin_bottom(18)
        box.set_margin_start(18)
        box.set_margin_end(18)
        dlg.set_child(box)
        pic = Gtk.Picture.new_for_filename(path)
        pic.set_size_request(240, 240)
        box.append(pic)
        link = Gtk.Label(label=url, selectable=True, wrap=True, xalign=0.5)
        link.add_css_class("localfly-pill-label")
        box.append(link)
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        row.set_halign(Gtk.Align.CENTER)
        box.append(row)
        copy = Gtk.Button(label="Copy URL")
        copy.connect("clicked", lambda *_: (self.get_clipboard().set(url), self.toast("Public URL copied")))
        row.append(copy)
        close = Gtk.Button(label="Close")
        close.connect("clicked", lambda *_: dlg.destroy())
        row.append(close)

        def _cleanup(*_args):
            try:
                os.remove(path)
            except OSError:
                pass

        dlg.connect("close-request", _cleanup)
        dlg.present()

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


class LocalflyLinuxApp(Adw.Application):
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
    app = LocalflyLinuxApp()
    return app.run([sys.argv[0], *argv])


if __name__ == "__main__":
    sys.exit(main())
