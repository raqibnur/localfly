use clap::Parser;
use localfly_lib::cli::{self, Cli, Cmd, DAEMON_TIMEOUT};
use localfly_lib::daemon::{self, OptionConfig, Request, TunnelDto};
use localfly_lib::manager::{TunnelManager, TunnelOptions, TunnelStatus};
use localfly_lib::{bore, probe, store};
use std::time::Duration;

async fn run_tunnel_foreground(
    app_name: &str,
    local_port: u16,
    requested: u16,
    options: TunnelOptions,
    public_address: Option<String>,
) -> Result<(), AnyErr> {
    if !probe::is_listening(local_port, &options.local_host, Duration::from_millis(1000)).await {
        eprintln!("Nothing running on :{local_port} - start your dev server first.");
        std::process::exit(2);
    }
    // One-shot path bypasses the daemon on purpose: the tunnel's lifetime
    // is tied to this process (Ctrl-C stops it). See `on` for the daemon path.
    let mgr = TunnelManager::new();
    let id = uuid::Uuid::new_v4();
    let mut rx = mgr.subscribe();
    let info = mgr.start(id, local_port, requested, options.clone()).await;
    match &info.status {
        TunnelStatus::Live => {
            let url = public_address
                .clone()
                .unwrap_or_else(|| info.web_url().unwrap_or_default());
            println!("{app_name} live: localhost:{local_port} -> {url}");
        }
        TunnelStatus::Error(e) if !options.auto_reconnect => {
            eprintln!("Relay refused the tunnel: {e}");
            std::process::exit(1);
        }
        TunnelStatus::Error(e) => {
            eprintln!("First attempt failed ({e}); retrying…");
        }
        _ => {}
    }
    let printer = tokio::spawn(async move {
        while let Ok(info) = rx.recv().await {
            if info.id != id {
                continue;
            }
            match &info.status {
                TunnelStatus::Live => {
                    let url = public_address.clone().unwrap_or_else(|| info.web_url().unwrap_or_default());
                    eprintln!(
                        "live {} (uptime/reconnects tracked in-process; bytes_in={} bytes_out={} conns={}/{})",
                        url,
                        info.stats.bytes_in,
                        info.stats.bytes_out,
                        info.stats.active_connections,
                        info.stats.total_connections
                    );
                }
                TunnelStatus::Connecting => {
                    eprintln!("connecting… (attempt {})", info.retry_attempt);
                }
                TunnelStatus::Error(e) => {
                    eprintln!("error: {e}");
                }
                TunnelStatus::Stopped => break,
            }
        }
    });
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = async {
            if let Some(s) = sigterm.as_mut() {
                let _ = s.recv().await;
            } else {
                std::future::pending::<()>().await;
            }
        } => {},
    }
    mgr.stop(id).await;
    printer.abort();
    Ok(())
}

type AnyErr = Box<dyn std::error::Error + Send + Sync>;

/// Follow daemon updates for one app (`on`) or all (`up`), printing like the
/// old foreground printer. Ctrl-C detaches (daemon keeps the tunnels);
/// SIGTERM stops the followed tunnel(s) and exits (lets supervisors and the
/// GUI, which terminates the child on toggle-off, behave like before).
async fn follow(app_id: Option<uuid::Uuid>, stop_all: bool) -> Result<(), AnyErr> {
    let mut lines = daemon::subscribe(Duration::from_secs(10)).await.map_err(|e| -> AnyErr {
        format!("subscribe: {e}").into()
    })?;
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                eprintln!("Detached - tunnel keeps running in localflyd (`localfly off` stops it).");
                return Ok(());
            }
            _ = async {
                if let Some(s) = sigterm.as_mut() {
                    let _ = s.recv().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                if stop_all {
                    let _ = daemon::call(&Request::StopAll, Duration::from_secs(5)).await;
                } else if let Some(id) = app_id {
                    let _ = daemon::call(&Request::Stop { id }, Duration::from_secs(5)).await;
                }
                return Ok(());
            }
            line = lines.next_line() => {
                let line = line.map_err(|e| -> AnyErr { format!("subscribe: {e}").into() })?;
                let Some(line) = line else { return Ok(()) };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
                let Some(info) = v.get("info") else { continue };
                let Ok(dto) = serde_json::from_value::<TunnelDto>(info.clone()) else { continue };
                if let Some(id) = app_id {
                    if dto.id != id {
                        continue;
                    }
                }
                print_dto(&dto);
            }
        }
    }
}

fn print_dto(dto: &TunnelDto) {
    match dto.status.as_str() {
        "live" => {
            let url = dto.web_url().unwrap_or_default();
            eprintln!(
                "live {} (bytes_in={} bytes_out={} conns={}/{})",
                url, dto.bytes_in, dto.bytes_out, dto.active_connections, dto.total_connections
            );
        }
        "connecting" => {
            eprintln!("connecting… (attempt {})", dto.retry_attempt);
        }
        "error" => {
            eprintln!("error: {}", dto.reason.as_deref().unwrap_or("unknown"));
        }
        _ => {}
    }
}

fn live_line(name: &str, local_port: u16, dto: &TunnelDto, public_address: Option<&str>) {
    let url = public_address
        .map(str::to_string)
        .or_else(|| dto.web_url())
        .unwrap_or_default();
    println!("{name} live: localhost:{local_port} -> {url}");
}

#[tokio::main]
async fn main() -> Result<(), AnyErr> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Add { name, port, public_port, public_address, start_on_launch } => {
            daemon::ensure_daemon().await.map_err(|e| -> AnyErr { e.into() })?;
            let list = daemon::call(&Request::List, DAEMON_TIMEOUT)
                .await
                .map_err(|e| -> AnyErr { e.into() })?;
            let apps = list.apps.unwrap_or_default();
            if apps.iter().any(|a| a.app.port == port && a.app.name == name) {
                eprintln!("Already saved.");
                std::process::exit(1);
            }
            let mut app = store::SavedApp::new(name, port);
            app.public_port = public_port;
            app.public_address = public_address;
            app.start_on_launch = start_on_launch;
            let name = app.name.clone();
            let port = app.port;
            let id = app.id;
            let r = daemon::call(&Request::Add { app }, DAEMON_TIMEOUT)
                .await
                .map_err(|e| -> AnyErr { e.into() })?;
            if !r.ok {
                eprintln!("{}", r.error.unwrap_or_else(|| "add failed".to_string()));
                std::process::exit(1);
            }
            println!("Saved {name} on :{port} (id {id})");
        }
        Cmd::List { json } => {
            // Read-only: prefer the daemon (has live state), fall back local.
            let apps = match daemon::call(&Request::List, Duration::from_secs(5)).await {
                Ok(r) if r.ok => Some(r.apps.unwrap_or_default()),
                _ => None,
            };
            if let Some(states) = apps {
                if json {
                    let plain: Vec<_> = states.iter().map(|s| &s.app).collect();
                    println!("{}", serde_json::to_string_pretty(&plain)?);
                    return Ok(());
                }
                if states.is_empty() {
                    println!("Nothing saved yet. Add one: localfly add --name web --port 3000");
                    return Ok(());
                }
                for s in &states {
                    let dot = match s.tunnel.as_ref().map(|t| t.status.as_str()) {
                        Some("live") => "●",
                        Some(_) => "◌",
                        None => match s.listening {
                            Some(true) => "●",
                            _ => "○",
                        },
                    };
                    let live = s
                        .tunnel
                        .as_ref()
                        .filter(|t| t.status == "live")
                        .and_then(|t| t.web_url())
                        .map(|u| format!(" -> {u}"))
                        .unwrap_or_default();
                    let extra = match (&s.app.public_port, &s.app.public_address) {
                        (Some(p), _) => format!(" public-port={p}"),
                        (_, Some(a)) => format!(" public-address={a}"),
                        _ => String::new(),
                    };
                    println!(
                        "{dot} {} :{}{} [{}]{}{}",
                        s.app.name,
                        s.app.port,
                        if s.app.start_on_launch { " (start-on-launch)" } else { "" },
                        s.app.id,
                        extra,
                        live,
                    );
                }
                return Ok(());
            }
            // Local fallback (no daemon running).
            let apps = store::load_apps();
            if json {
                println!("{}", serde_json::to_string_pretty(&apps)?);
                return Ok(());
            }
            if apps.is_empty() {
                println!("Nothing saved yet. Add one: localfly add --name web --port 3000");
                return Ok(());
            }
            let mut listening = std::collections::HashMap::new();
            let mut tasks = Vec::new();
            for app in &apps {
                let port = app.port;
                tasks.push(tokio::spawn(async move {
                    (port, probe::is_listening(port, "localhost", Duration::from_millis(600)).await)
                }));
            }
            for t in tasks {
                let (port, up) = t.await?;
                listening.insert(port, up);
            }
            for app in &apps {
                let dot = match listening.get(&app.port) {
                    Some(true) => "●",
                    Some(false) => "○",
                    None => "?",
                };
                let extra = match (&app.public_port, &app.public_address) {
                    (Some(p), _) => format!(" public-port={p}"),
                    (_, Some(a)) => format!(" public-address={a}"),
                    _ => String::new(),
                };
                println!(
                    "{dot} {} :{}{} [{}]{}",
                    app.name,
                    app.port,
                    if app.start_on_launch { " (start-on-launch)" } else { "" },
                    app.id,
                    extra
                );
            }
        }
        Cmd::Rm { selector } => {
            daemon::ensure_daemon().await.map_err(|e| -> AnyErr { e.into() })?;
            let list = daemon::call(&Request::List, DAEMON_TIMEOUT)
                .await
                .map_err(|e| -> AnyErr { e.into() })?;
            let apps: Vec<_> = list.apps.unwrap_or_default().into_iter().map(|s| s.app).collect();
            let found = daemon::find_app(&apps, &selector).cloned();
            match found {
                Some(app) => {
                    let r = daemon::call(&Request::Remove { id: app.id }, DAEMON_TIMEOUT)
                        .await
                        .map_err(|e| -> AnyErr { e.into() })?;
                    if !r.ok {
                        eprintln!("{}", r.error.unwrap_or_else(|| "remove failed".to_string()));
                        std::process::exit(1);
                    }
                    println!("Removed {} (:{})", app.name, app.port);
                }
                None => {
                    eprintln!("No app matching '{selector}'.");
                    std::process::exit(1);
                }
            }
        }
        Cmd::Edit { selector, name, port, public_port, no_public_port, public_address, no_public_address, start_on_launch } => {
            daemon::ensure_daemon().await.map_err(|e| -> AnyErr { e.into() })?;
            let list = daemon::call(&Request::List, DAEMON_TIMEOUT)
                .await
                .map_err(|e| -> AnyErr { e.into() })?;
            let states = list.apps.unwrap_or_default();
            let found = states.iter().find(|s| {
                daemon::find_app(std::slice::from_ref(&s.app), &selector).is_some()
            });
            let Some(state) = found else {
                eprintln!("No app matching '{selector}'.");
                std::process::exit(1);
            };
            let mut app = state.app.clone();
            let was_live = state.tunnel.as_ref().map(|t| t.status == "live").unwrap_or(false);
            let old_port = app.port;
            let old_public = app.public_port;
            if let Some(n) = name {
                app.name = n.clone();
                app.hue = store::hue_for(&n);
            }
            if let Some(p) = port {
                app.port = p;
            }
            if no_public_port {
                app.public_port = None;
            } else if let Some(p) = public_port {
                app.public_port = Some(p);
            }
            if no_public_address {
                app.public_address = None;
            } else if let Some(a) = public_address {
                app.public_address = Some(a);
            }
            if let Some(s) = start_on_launch {
                app.start_on_launch = s;
            }
            let r = daemon::call(&Request::Update { app: app.clone() }, DAEMON_TIMEOUT)
                .await
                .map_err(|e| -> AnyErr { e.into() })?;
            if !r.ok {
                eprintln!("{}", r.error.unwrap_or_else(|| "update failed".to_string()));
                std::process::exit(1);
            }
            // A running tunnel keeps its old ports until re-enabled; restart it.
            if was_live && (app.port != old_port || app.public_port != old_public) {
                let _ = daemon::call(&Request::Stop { id: app.id }, DAEMON_TIMEOUT).await;
                println!("Updated {} (:{}); tunnel stopped, turn it on again to apply.", app.name, app.port);
            } else {
                println!("Updated {} (:{})", app.name, app.port);
            }
        }
        Cmd::On { selector, relay, secret, local_host, no_reconnect } => {
            daemon::ensure_daemon().await.map_err(|e| -> AnyErr { e.into() })?;
            let list = daemon::call(&Request::List, DAEMON_TIMEOUT)
                .await
                .map_err(|e| -> AnyErr { e.into() })?;
            let states = list.apps.unwrap_or_default();
            let apps: Vec<_> = states.iter().map(|s| &s.app).collect();
            let found = apps.iter().find(|a| daemon::find_app(std::slice::from_ref(a), &selector).is_some());
            let Some(app) = found else {
                eprintln!("No app matching '{selector}'.");
                std::process::exit(1);
            };
            let app = (*app).clone();
            let state = states.iter().find(|s| s.app.id == app.id);
            if state.and_then(|s| s.listening) == Some(false) {
                eprintln!("Nothing running on :{} - start your dev server first.", app.port);
                std::process::exit(2);
            }
            let cfg = cli::load_config();
            let options = OptionConfig {
                relay: cli::effective_relay(relay, &cfg),
                secret: cli::effective_secret(secret),
                local_host: cli::effective_local_host(local_host, &cfg),
                auto_reconnect: cli::effective_auto_reconnect(no_reconnect, &cfg),
            };
            let r = daemon::call(
                &Request::Start { id: app.id, local_port: app.port, remote_port: app.public_port.unwrap_or(0), options: options.clone() },
                Duration::from_secs(30),
            )
            .await
            .map_err(|e| -> AnyErr { e.into() })?;
            let dto = r.info.expect("start returns info");
            match dto.status.as_str() {
                "live" => live_line(&app.name, app.port, &dto, app.public_address.as_deref()),
                "error" if options.auto_reconnect => {
                    eprintln!(
                        "First attempt failed ({}); retrying in the background…",
                        dto.reason.as_deref().unwrap_or("unknown")
                    );
                }
                _ => {
                    eprintln!("Relay refused the tunnel: {}", dto.reason.as_deref().unwrap_or("unknown"));
                    std::process::exit(1);
                }
            }
            follow(Some(app.id), false).await?;
        }
        Cmd::Off { selector, all } => {
            daemon::ensure_daemon().await.map_err(|e| -> AnyErr { e.into() })?;
            if all {
                daemon::call(&Request::StopAll, DAEMON_TIMEOUT)
                    .await
                    .map_err(|e| -> AnyErr { e.into() })?;
                println!("All tunnels stopped.");
                return Ok(());
            }
            let Some(sel) = selector else {
                eprintln!("Specify an app or --all.");
                std::process::exit(2);
            };
            let list = daemon::call(&Request::List, DAEMON_TIMEOUT)
                .await
                .map_err(|e| -> AnyErr { e.into() })?;
            let apps: Vec<_> = list.apps.unwrap_or_default().into_iter().map(|s| s.app).collect();
            match daemon::find_app(&apps, &sel) {
                Some(app) => {
                    daemon::call(&Request::Stop { id: app.id }, DAEMON_TIMEOUT)
                        .await
                        .map_err(|e| -> AnyErr { e.into() })?;
                    println!("Stopped {} (:{})", app.name, app.port);
                }
                None => {
                    eprintln!("No app matching '{sel}'.");
                    std::process::exit(1);
                }
            }
        }
        Cmd::Expose { port, remote_port, relay, secret, local_host, no_reconnect } => {
            let cfg = cli::load_config();
            let options = TunnelOptions {
                relay: cli::effective_relay(relay, &cfg),
                secret: cli::effective_secret(secret),
                local_host: cli::effective_local_host(local_host, &cfg),
                auto_reconnect: cli::effective_auto_reconnect(no_reconnect, &cfg),
            };
            run_tunnel_foreground(&format!(":{port}"), port, remote_port.unwrap_or(0), options, None).await?;
        }
        Cmd::Up { .. } => {
            daemon::ensure_daemon().await.map_err(|e| -> AnyErr { e.into() })?;
            let r = daemon::call(&Request::Up, Duration::from_secs(30))
                .await
                .map_err(|e| -> AnyErr { e.into() })?;
            let started = r.started.unwrap_or_default();
            if started.is_empty() {
                println!("Nothing to start (no start-on-launch apps listening).");
                return Ok(());
            }
            println!("Running {} app(s). Ctrl-C detaches; SIGTERM stops all.", started.len());
            follow(None, true).await?;
        }
        Cmd::Status { json } => {
            let list = match daemon::call(&Request::List, Duration::from_secs(5)).await {
                Ok(r) if r.ok => r.apps.unwrap_or_default(),
                _ => {
                    println!("localflyd is not running.");
                    return Ok(());
                }
            };
            let live: Vec<_> = list.iter().filter_map(|s| s.tunnel.as_ref().map(|t| (&s.app, t))).collect();
            if json {
                let out: Vec<_> = live
                    .iter()
                    .map(|(a, t)| {
                        serde_json::json!({
                            "name": a.name, "local_port": a.port,
                            "status": t.status, "remote_port": t.remote_port,
                            "url": t.web_url(),
                            "bytes_in": t.bytes_in, "bytes_out": t.bytes_out,
                            "active": t.active_connections, "total": t.total_connections,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&out)?);
                return Ok(());
            }
            if live.is_empty() {
                println!("No live tunnels.");
                return Ok(());
            }
            for (app, t) in live {
                let url = t.web_url().unwrap_or_default();
                println!(
                    "● {} :{} -> {} ({} in / {} out, {} active)",
                    app.name,
                    app.port,
                    url,
                    fmt_bytes(t.bytes_in),
                    fmt_bytes(t.bytes_out),
                    t.active_connections
                );
            }
        }
        Cmd::Probe { port, host } => {
            let up = probe::is_listening(port, &host, Duration::from_secs(1)).await;
            println!("{host}:{port} {}", if up { "listening" } else { "closed" });
            if !up {
                std::process::exit(1);
            }
        }
        Cmd::Scan => {
            let mut tasks = Vec::new();
            for &port in daemon::COMMON_PORTS {
                tasks.push(tokio::spawn(async move {
                    (port, probe::is_listening(port, "localhost", Duration::from_millis(600)).await)
                }));
            }
            let saved: std::collections::HashSet<u16> =
                store::load_apps().iter().map(|a| a.port).collect();
            let mut found = Vec::new();
            for t in tasks {
                let (port, up) = t.await?;
                if up {
                    found.push(port);
                }
            }
            found.sort_unstable();
            if found.is_empty() {
                println!("No common dev ports listening.");
            } else {
                for port in found {
                    let tag = if saved.contains(&port) { " (saved)" } else { "" };
                    println!("localhost:{port}{tag}");
                }
            }
        }
        Cmd::RelayStatus { relay } => {
            let cfg = cli::load_config();
            let relay_str = cli::effective_relay(relay, &cfg);
            let (host, port) = bore::parse_relay(&relay_str);
            match probe::measure(&host, port, Duration::from_secs(3)).await {
                Some(d) => println!("{host}:{port} reachable in {}ms", d.as_millis()),
                None => {
                    println!("{host}:{port} unreachable");
                    std::process::exit(1);
                }
            }
        }
        Cmd::Config { relay, auto_reconnect, local_host, secret } => {
            let path = cli::config_path();
            let mut cfg = cli::load_config();
            if let Some(r) = relay {
                cfg.relay = Some(r);
            }
            if let Some(a) = auto_reconnect {
                cfg.auto_reconnect = Some(a);
            }
            if let Some(h) = local_host {
                cfg.local_host = Some(h);
            }
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            std::fs::write(&path, serde_json::to_string_pretty(&cfg)?)?;
            if let Some(s) = secret {
                store::write_secret(&s);
            }
            println!("Wrote {}", path.display());
        }
        Cmd::UpdateCheck => {
            let current = env!("CARGO_PKG_VERSION");
            match update_check().await {
                Ok(Some((ver, url))) => println!("Update available: {ver} ({url}) - you have {current}"),
                Ok(None) => println!("Up to date ({current})."),
                Err(e) => {
                    eprintln!("Update check failed: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
    Ok(())
}

fn fmt_bytes(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    }
}

async fn update_check() -> Result<Option<(String, String)>, AnyErr> {
    // Mirrors App/UpdateChecker.swift: read-only GitHub latest-release check.
    let body = reqwest_like_get("https://api.github.com/repos/9M2PJU/localfly-linux/releases/latest").await?;
    let v: serde_json::Value = serde_json::from_str(&body)?;
    if v.get("draft").and_then(|d| d.as_bool()).unwrap_or(false)
        || v.get("prerelease").and_then(|p| p.as_bool()).unwrap_or(false)
    {
        return Ok(None);
    }
    let tag = v.get("tag_name").and_then(|t| t.as_str()).unwrap_or("").trim_start_matches(['v', 'V']).to_string();
    let url = v.get("html_url").and_then(|u| u.as_str()).unwrap_or("").to_string();
    let current = env!("CARGO_PKG_VERSION");
    if is_newer(&tag, current) {
        Ok(Some((tag, url)))
    } else {
        Ok(None)
    }
}

fn is_newer(candidate: &str, current: &str) -> bool {
    let a: Vec<u64> = candidate.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    let b: Vec<u64> = current.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    for i in 0..a.len().max(b.len()) {
        let l = *a.get(i).unwrap_or(&0);
        let r = *b.get(i).unwrap_or(&0);
        if l != r {
            return l > r;
        }
    }
    false
}

async fn reqwest_like_get(url: &str) -> Result<String, AnyErr> {
    // Minimal HTTPS GET without adding reqwest: use tokio TcpStream + manual TLS? Instead shell to curl.
    // To keep the binary dependency-light, reuse `curl` if present (standard on Linux desktops/servers).
    let out = tokio::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "15", "-H", "Accept: application/vnd.github+json", url])
        .output()
        .await?;
    if !out.status.success() {
        return Err("curl failed".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
