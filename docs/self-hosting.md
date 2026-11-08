# Self-hosting a relay

By default Localfly uses the free public relay at bore.pub. Running your own relay gets you:

- **HTTPS addresses on your domain**, like `https://web.yourdomain.com`
- **Addresses that never change**, so webhooks and shared links keep working
- **Privacy**: only people with your secret can open tunnels

You need a small Linux server (about $5 a month from any VPS provider) and a domain.

## 1. Point DNS at the server

Create an A record for each name you want, or one wildcard record:

```
*.yourdomain.com   A   <your server's IP>
```

## 2. Open the firewall

| Port | For |
|---|---|
| 7835/tcp | Localfly connecting to the relay |
| 80/tcp, 443/tcp | Caddy (HTTPS, and the certificate challenge) |

## 3. Start the relay

Install Docker, then on the server:

```bash
git clone https://github.com/raqibnur/localfly.git
cd localfly/deploy
cp .env.example .env
sed -i "s/change-me/$(openssl rand -hex 24)/" .env   # generates the secret
nano Caddyfile                                        # replace web.example.com with your names
docker compose up -d
```

`docker compose logs -f` shows Caddy fetching certificates. After editing `Caddyfile`, apply it with `docker compose restart caddy`.

## 4. Connect Localfly

1. **Relay**: set **Server** to your domain or the server's IP, and set **Secret** to the `BORE_SECRET` from `.env`.
2. For each app, choose **Edit…**, set **Public port** to the port in its Caddy block (e.g. `40001`), and set **Address** to `https://web.yourdomain.com`.
3. Turn the app on. The lock icon and your address appear when it's live.

## Without HTTPS

If plain `http://your-server:40001` is enough, set `BIND_TUNNELS=0.0.0.0` in `.env`, open ports 40000–40100 in the firewall, and run only the relay:

```bash
docker compose up -d bore
```

## Without Docker

Download `bore` from [its releases](https://github.com/ekzhang/bore/releases/tag/v0.6.0) and run:

```bash
BORE_SECRET=… bore server --bind-tunnels 127.0.0.1 --min-port 40000 --max-port 40100
```

Then install [Caddy](https://caddyserver.com/docs/install) with the same `Caddyfile`. Use systemd to keep both running.

## Troubleshooting

| Localfly says | Meaning |
|---|---|
| Couldn’t reach relay | Port 7835 is blocked, or the server address is wrong. |
| This relay requires a secret | Enter the secret under **Relay**. |
| Relay didn’t ask for a secret | The relay runs without `BORE_SECRET`; clear the secret in Localfly. |
| Relay refused the tunnel: port already in use | Another tunnel holds that public port. |
| Relay refused the tunnel: client port number not in allowed range | Pick a public port between MIN_PORT and MAX_PORT. |
