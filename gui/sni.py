"""StatusNotifierItem tray for the Localfly GUI (pure Gio D-Bus, GTK4-safe).

Implements org.kde.StatusNotifierItem + com.canonical.dbusmenu over the
session bus so the tray shows wherever a StatusNotifierWatcher runs
(KDE, sway/waybar, GNOME with the AppIndicator extension). No Gtk2/3 needed.
"""
from __future__ import annotations

import os
from dataclasses import dataclass, field

from gi.repository import Gio, GLib

SNI_XML = """
<node>
  <interface name="org.kde.StatusNotifierItem">
    <method name="ContextMenu"><arg name="x" type="i" direction="in"/><arg name="y" type="i" direction="in"/></method>
    <method name="Activate"><arg name="x" type="i" direction="in"/><arg name="y" type="i" direction="in"/></method>
    <method name="SecondaryActivate"><arg name="x" type="i" direction="in"/><arg name="y" type="i" direction="in"/></method>
    <method name="Scroll"><arg name="delta" type="i" direction="in"/><arg name="orientation" type="s" direction="in"/></method>
    <property name="Category" type="s" access="read"/>
    <property name="Id" type="s" access="read"/>
    <property name="Title" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="IconName" type="s" access="read"/>
    <property name="ItemIsMenu" type="b" access="read"/>
    <property name="Menu" type="o" access="read"/>
    <property name="ToolTip" type="(sa(iiay)av)" access="read"/>
    <signal name="NewTitle"/>
    <signal name="NewIcon"/>
    <signal name="NewToolTip"/>
  </interface>
</node>
"""

MENU_XML = """
<node>
  <interface name="com.canonical.dbusmenu">
    <method name="GetLayout">
      <arg name="parentId" type="i" direction="in"/>
      <arg name="recursionDepth" type="i" direction="in"/>
      <arg name="propertyNames" type="as" direction="in"/>
      <!-- Declared as two out args (not one struct) so Gio validates the
           reply body as (u(ia{sv}av)) — the exact wire format renderers
           like libdbusmenu expect. -->
      <arg name="revision" type="u" direction="out"/>
      <arg name="layout" type="(ia{sv}av)" direction="out"/>
    </method>
    <method name="GetGroupProperties">
      <arg name="ids" type="ai" direction="in"/>
      <arg name="propertyNames" type="as" direction="in"/>
      <arg name="properties" type="a(ia{sv})" direction="out"/>
    </method>
    <method name="GetChildren">
      <arg name="id" type="i" direction="in"/>
      <arg name="propertyNames" type="as" direction="in"/>
      <arg name="children" type="a(ia{sv}av)" direction="out"/>
    </method>
    <method name="Event">
      <arg name="id" type="i" direction="in"/>
      <arg name="eventId" type="s" direction="in"/>
      <arg name="data" type="v" direction="in"/>
      <arg name="timestamp" type="u" direction="in"/>
    </method>
    <method name="EventGroup">
      <arg name="events" type="a(isvu)" direction="in"/>
      <arg name="idErrors" type="ai" direction="out"/>
    </method>
    <method name="AboutToShow">
      <arg name="id" type="i" direction="in"/>
      <arg name="needUpdate" type="b" direction="out"/>
    </method>
    <method name="AboutToShowGroup">
      <arg name="ids" type="ai" direction="in"/>
      <arg name="updatesNeeded" type="ai" direction="out"/>
      <arg name="idErrors" type="ai" direction="out"/>
    </method>
    <property name="Version" type="u" access="read"/>
    <property name="Status" type="s" access="read"/>
    <signal name="LayoutUpdated">
      <arg name="revision" type="u"/>
      <arg name="parent" type="i"/>
    </signal>
    <signal name="ItemsPropertiesUpdated">
      <arg name="updatedProps" type="a(ia{sv})"/>
      <arg name="removedProps" type="a(ias)"/>
    </signal>
  </interface>
</node>
"""


@dataclass
class MenuItem:
    id: int
    label: str = ""
    enabled: bool = True
    visible: bool = True
    separator: bool = False
    checkmark: bool = False
    checked: bool = False
    on_clicked: object = None  # callable | None


class Tray:
    """Owns the bus name, SNI object and dbusmenu; call set_menu() to refresh."""

    def __init__(self, on_show_menu=None):
        self.bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        self.service = f"org.kde.StatusNotifierItem-{os.getpid()}-1"
        self.path = "/StatusNotifierItem"
        self.menu_path = "/Menu"
        self.revision = 1
        self.items: list[MenuItem] = []
        self.status = "Active"
        self.tooltip_title = "Localfly"
        self.tooltip_body = ""
        self.on_show_menu = on_show_menu  # called on left-click (Activate)
        self._ids: list[int] = []

        sni_info = Gio.DBusNodeInfo.new_for_xml(SNI_XML)
        sni_iface = sni_info.interfaces[0]
        menu_info = Gio.DBusNodeInfo.new_for_xml(MENU_XML)
        menu_iface = menu_info.interfaces[0]
        self._sni_info = sni_info  # keep alive
        self._menu_info = menu_info
        self._sni_id = self.bus.register_object(
            self.path, sni_iface, self._sni_method, self._sni_get_prop, None
        )
        self._menu_id = self.bus.register_object(
            self.menu_path, menu_iface, self._menu_method, self._menu_get_prop, None
        )
        # Own a well-known name so the watcher (and renderers) can find us.
        self.bus.call_sync(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "RequestName",
            GLib.Variant("(su)", (self.service, 0)),
            GLib.VariantType("(u)"),
            Gio.DBusCallFlags.NONE,
            5000,
            None,
        )
        # Register with the watcher (ignore failure: no watcher, no tray).
        try:
            self.bus.call_sync(
                "org.kde.StatusNotifierWatcher",
                "/StatusNotifierWatcher",
                "org.kde.StatusNotifierWatcher",
                "RegisterStatusNotifierItem",
                GLib.Variant("(s)", (self.service,)),
                None,
                Gio.DBusCallFlags.NONE,
                5000,
                None,
            )
            self.registered = True
        except GLib.Error:
            self.registered = False

    # ----- public -----
    def set_menu(self, items: list[MenuItem], status: str = "Active",
                 tooltip: str = "") -> None:
        self.items = items
        self.status = status
        self.tooltip_body = tooltip
        self.revision += 1
        self._emit_menu("LayoutUpdated", GLib.Variant("(ui)", (self.revision, 0)))
        self._emit_sni("NewTitle")

    def quit(self) -> None:
        try:
            self.bus.unregister_object(self._sni_id)
        except Exception:
            pass
        try:
            self.bus.unregister_object(self._menu_id)
        except Exception:
            pass

    # ----- SNI -----
    def _emit_sni(self, name: str) -> None:
        try:
            self.bus.emit_signal(None, self.path, "org.kde.StatusNotifierItem", name, None)
        except GLib.Error:
            pass

    def _emit_menu(self, name: str, params: GLib.Variant) -> None:
        try:
            self.bus.emit_signal(None, self.menu_path, "com.canonical.dbusmenu", name, params)
        except GLib.Error:
            pass

    def _sni_method(self, _conn, _sender, _path, _iface, method, params, invocation, *_extra):
        if method == "Activate":
            if self.on_show_menu:
                GLib.idle_add(lambda: (self.on_show_menu(), False)[1])
            invocation.return_value(None)
        elif method in ("ContextMenu", "SecondaryActivate", "Scroll"):
            invocation.return_value(None)
        else:
            invocation.return_dbus_error("org.kde.StatusNotifierItem.UnknownMethod", "unknown method")

    def _sni_get_prop(self, _conn, _sender, _path, _iface, name, *_extra):
        if name == "Category":
            return GLib.Variant("s", "ApplicationStatus")
        if name == "Id":
            return GLib.Variant("s", "localfly")
        if name == "Title":
            return GLib.Variant("s", "Localfly")
        if name == "Status":
            return GLib.Variant("s", self.status)
        if name == "IconName":
            return GLib.Variant("s", "network-transmit-receive")
        if name == "ItemIsMenu":
            return GLib.Variant("b", True)
        if name == "Menu":
            return GLib.Variant("o", self.menu_path)
        if name == "ToolTip":
            return GLib.Variant(
                "(sa(iiay)av)",
                ("Localfly", [], [GLib.Variant("s", self.tooltip_body or "Localfly")]),
            )
        return None

    @staticmethod
    def _prop(name: str, value: str):
        return (name, GLib.Variant("s", value))

    # ----- dbusmenu -----
    def _props(self, item: MenuItem):
        props = {
            "label": GLib.Variant("s", item.label),
            "enabled": GLib.Variant("b", item.enabled),
            "visible": GLib.Variant("b", item.visible),
        }
        if item.separator:
            props["type"] = GLib.Variant("s", "separator")
        if item.checkmark:
            props["toggle-type"] = GLib.Variant("s", "checkmark")
            props["toggle-state"] = GLib.Variant("i", 1 if item.checked else 0)
        return props

    def _item_variant(self, item: MenuItem):
        # av elements must be Variants; the root tuple stays plain python.
        return GLib.Variant("(ia{sv}av)", (item.id, self._props(item), []))

    def _item_tuple(self, item: MenuItem):
        # Plain-tuple form: PyGObject rejects pre-built Variants in a
        # top-level array (GetChildren), but accepts them nested (GetLayout).
        return (item.id, self._props(item), [])

    def _menu_method(self, _conn, _sender, _path, _iface, method, params, invocation, *_extra):
        try:
            self._menu_method_inner(method, params, invocation)
        except Exception as e:  # never leave a D-Bus call hanging
            import traceback

            traceback.print_exc()
            invocation.return_dbus_error(
                "com.canonical.dbusmenu.Error", f"{type(e).__name__}: {e}"
            )

    def _menu_method_inner(self, method, params, invocation):
        if method == "GetLayout":
            _parent, _depth, _names = params.unpack()
            children = [self._item_variant(i) for i in self.items if i.visible]
            root = (0, {"children-display": GLib.Variant("s", "submenu")}, children)
            invocation.return_value(GLib.Variant("(u(ia{sv}av))", (self.revision, root)))
        elif method == "GetGroupProperties":
            ids, _names = params.unpack()
            out = [(item.id, self._props(item)) for item in self.items if item.id in ids]
            invocation.return_value(GLib.Variant("(a(ia{sv}))", (out,)))
        elif method == "GetChildren":
            _pid, _names = params.unpack()
            invocation.return_value(
                GLib.Variant(
                    "(a(ia{sv}av))", ([self._item_tuple(i) for i in self.items],)
                )
            )
        elif method == "GetChildren":
            _pid, _names = params.unpack()
            invocation.return_value(
                GLib.Variant("(a(ia{sv}av))", ([self._item_tuple(i) for i in self.items],))
            )
        elif method == "Event":
            item_id, event_id, _data, _ts = params.unpack()
            if event_id == "clicked":
                for item in self.items:
                    if item.id == item_id and callable(item.on_clicked):
                        cb = item.on_clicked
                        GLib.idle_add(lambda c=cb: (c(), False)[1])
                        break
            invocation.return_value(None)
        elif method == "EventGroup":
            invocation.return_value(GLib.Variant("(ai)", ([],)))
        elif method == "AboutToShow":
            invocation.return_value(GLib.Variant("(b)", (False,)))
        elif method == "AboutToShowGroup":
            invocation.return_value(GLib.Variant("(aiai)", ([], [])))
        else:
            invocation.return_dbus_error("com.canonical.dbusmenu.UnknownMethod", "unknown method")

    def _menu_get_prop(self, _conn, _sender, _path, _iface, name, *_extra):
        if name == "Version":
            return GLib.Variant("u", 4)
        if name == "Status":
            return GLib.Variant("s", "normal")
        return None
