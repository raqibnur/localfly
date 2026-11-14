# Localfly for Linux

Your localhost, on the internet. A free, open-source CLI that gives any app on your Linux machine a public link.

Save an app by its port, expose it, and share the address. Test your site on your phone, show work to a client, or receive webhooks, all without deploying anything.

- **Knows what's running**: `list` shows whether your dev server is up, and `on` won't open a tunnel to an empty port.
- **Spots dev servers**: `scan` finds common ports like `:3000` and `:5173` ready to add.
- **Live traffic**: bytes in/out and connection counts per tunnel.
- **Reconnects by itself** with backoff and asks for the same address back.
- **HTTPS on your own domain** when you [run your own relay](docs/self-hosting.md).
- **Private by default**: nothing is exposed until you expose it. No account, no analytics. The relay secret lives in a `0600` file.

Localfly speaks the [bore](https://github.com/ekzhang/bore) tunnel protocol (v0.6). Your machine opens an outgoing connection to a relay server. When someone visits the public address, the relay passes the connection through to your machine. Nothing on your router or firewall changes.

By default it uses **bore.pub**, a free public relay. For stable `https://` addresses on your own domain, set up your own relay with [the self-hosting guide](docs/self-hosting.md).

> **Heads-up:** anyone with a tunnel's address can reach that app while it's on. Only share what you mean to share.

## Install

```bash
cargo install --path .
# binary: ~/.cargo/bin/localfly
```

Requires a C toolchain + cargo (Arch: `pacman -S rust`; Debian: `apt install cargo`). No GTK/Qt dependency.

## Use

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

## License

[MIT](LICENSE).
