# Localfly Linux

<p align="center">
  <img src="packaging/icons/localfly.png" width="96" height="96" alt="Localfly Linux logo">
</p>

Your localhost, on the internet. A free, open-source app that gives any app on your Linux machine a public link - an Adwaita window + system tray, like the macOS menu bar app, over a Rust tunnel engine.

Save an app by its port, flip the switch, and share the address. Test your site on your phone, show work to a client, or receive webhooks, all without deploying anything.

- **Window + tray first**: every app, its status and a copy button, one click away. Closing the window keeps tunnels running.
- **Knows what's running**: a dot shows whether your dev server is up, and Localfly Linux won't open a tunnel to an empty port.
- **Spots dev servers**: common ports like `:3000` and `:5173` show up ready to add.
- **Live traffic**: bytes in and out for each tunnel.
- **Reconnects by itself** after Wi-Fi drops, and asks for the same address back.
- **HTTPS on your own domain** when you [run your own relay](docs/self-hosting.md).
- **Private by default**: nothing is exposed until you switch it on. No account, no analytics. The relay secret lives in a `0600` file.

Localfly Linux speaks the [bore](https://github.com/ekzhang/bore) tunnel protocol (v0.6). Your machine opens an outgoing connection to a relay server. When someone visits the public address, the relay passes the connection through to your machine. Nothing on your router or firewall changes.

By default it uses **bore.pub**, a free public relay. For stable `https://` addresses on your own domain, set up your own relay with [the self-hosting guide](docs/self-hosting.md).

> **Heads-up:** anyone with a tunnel's address can reach that app while it's on. Only share what you mean to share.

<p align="center">
  <img src="docs/images/app.png" width="760" alt="The Localfly Linux window with one saved app, live at a bore.pub address">
</p>

## Install

One-liner (user-local, no root):

```bash
curl -fsSL https://raw.githubusercontent.com/9M2PJU/localfly-linux/main/install.sh | sh
```

Or pick your distro:

| Method | Command |
|---|---|
| Arch (AUR, prebuilt) | `yay -S localfly-linux-bin` (or `localfly-linux-git` to build from source) |
| Debian/Ubuntu (.deb) | download `localfly-linux_*.deb` from [releases](https://github.com/9M2PJU/localfly-linux/releases) then `sudo apt install ./localfly-linux_*.deb` |
| Fedora/RHEL (.rpm) | download `localfly-linux-*.rpm` from [releases](https://github.com/9M2PJU/localfly-linux/releases) then `sudo dnf install ./localfly-linux-*.rpm` |
| Any distro (AppImage) | download `Localfly-*.AppImage` from [releases](https://github.com/9M2PJU/localfly-linux/releases), `chmod +x`, run |
| From source | `cargo install --path .` |

### Arch Linux

```bash
yay -S localfly-linux-bin   # prebuilt, fast
# or: yay -S localfly-linux-git  # builds from git main
```

No AUR helper? Grab `localfly-linux-bin-*.pkg.tar.zst` from
[releases](https://github.com/9M2PJU/localfly-linux/releases) and:

```bash
sudo pacman -U ./localfly-linux-bin-*.pkg.tar.zst
```

Or build manually:

```bash
git clone https://aur.archlinux.org/localfly-linux-bin.git  # or localfly-linux-git
cd localfly-linux-bin
makepkg -si
```

This installs the CLI (`localfly`), the GUI launcher (`localfly-gui`),
the desktop entry, the official icon, and the systemd user unit.
Start the GUI from your app menu, or run `localfly-gui` from a terminal.
Optional: enable tunnels at login from the app's General page
(which enables `localfly.service --user`), or manually:

```bash
systemctl --user enable --now localfly.service
```

Requires a C toolchain + cargo for source builds (Arch: `pacman -S rust`; Debian: `apt install cargo`).
The GUI additionally needs `python3-gi` + GTK4 + libadwaita
(Arch: `python-gobject gtk4 libadwaita`; Debian: `python3-gi gir1.2-gtk-4.0 gir1.2-adw-1`).

## GUI (window + tray)

```bash
# system packages (Arch; Debian: python3-gi gir1.2-gtk-4.0 gir1.2-adw-1)
sudo pacman -S --needed python-gobject gtk4 libadwaita
cargo install --path .   # provides the `localfly` backend the GUI drives

./gui/localfly-gui
```

The window mirrors the macOS app: sidebar (Apps / Relay / General / About),
app cards with on/off switches, URL copy + open buttons, live status, add/edit
dialogs, relay settings (server, secret, latency check) and start-on-login via
a systemd user unit. Closing the window keeps tunnels running; Quit stops them.

The tray uses StatusNotifierItem, so it shows in KDE, sway/waybar and GNOME
(with the AppIndicator extension). Toggles per app, Open, Quit.

Install both launchers:

```bash
install -Dm755 target/release/localfly ~/.local/bin/localfly
install -Dm755 gui/localfly-gui ~/.local/bin/localfly-gui
install -Dm644 com.localfly.Localfly.desktop ~/.local/share/applications/com.localfly.Localfly.desktop
```

## CLI use (headless / scripting)

```bash
localfly add --name web --port 3000
localfly on web                 # foreground; Ctrl-C stops
localfly expose 5173 --relay bore.pub

localfly edit web --public-port 40001
localfly config --relay example.com:7835 --secret "$BORE_SECRET"
localfly up                     # every --start-on-launch app, foreground

localfly list
localfly scan                   # common dev ports: 3000, 5173, 8000, …
localfly probe 3000
localfly relay-status
localfly update-check
```

`on`/`expose` refuse empty ports (`Nothing running on :3000`). `up` skips apps with nothing listening and keeps the rest up.

## Where things live

| Setting | Location |
|---|---|
| Saved apps | `~/.config/localfly/apps.json` (`$LOCALFLY_CONFIG` overrides) |
| Relay secret | `~/.config/localfly/secret` mode `0600` (`$LOCALFLY_SECRET` env wins) |
| Relay / auto-reconnect | `~/.config/localfly/config.json` via `localfly config` |
| Start on login | `localfly.service` → `systemctl --user enable --now` (runs `localfly up`) |
| Copy / open URL | stdout URL (pipe to `wl-copy`/`xclip`, open with `xdg-open`) |

## Self-hosting

See [docs/self-hosting.md](docs/self-hosting.md). In short: run the relay in `deploy/` (`bore server` on `7835` + Caddy), then `localfly config --relay my.example.com --secret "$BORE_SECRET"`.

## Verify

```bash
cargo test
```

7 unit tests (bore protocol vectors) + 4 live-relay integration tests against a real `bore server` 0.6.0 on `127.0.0.1:7835`, covering traffic round-trip, secret auth, fixed ports, and unreachable-relay errors.

## Project layout

```text
src/            tunnel engine + CLI (bore protocol over tokio, no UI toolkit)
tests/          live-relay integration tests
deploy/         relay for self-hosting: bore server + Caddy (docker compose)
docs/           self-hosting guide
```

## Contributing

Issues and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md). Report security problems privately as described in [SECURITY.md](SECURITY.md).

Maintained by [9M2PJU](https://hamradio.my). Original macOS app by [Raqib Nur](https://github.com/raqibnur) ([raqibnur/localfly](https://github.com/raqibnur/localfly)).

## License

[MIT](LICENSE) - © Raqib Nur for the original app; Linux port contributions under the same license.

## Donate

If Localfly Linux saves you time, consider supporting the maintainer:

<p align="center">
  <img src="docs/images/donation-qr.png" width="220" alt="Donation QR code">
</p>

- [Buy me a coffee](https://www.buymeacoffee.com/9m2pju)
- [PayPal](https://paypal.me/9M2PJU)
- [Wise](https://wise.com/pay/me/faizulz13)
