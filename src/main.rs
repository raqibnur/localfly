use clap::{Parser, Subcommand};
use localfly_lib::manager::{TunnelManager, TunnelOptions, TunnelStatus};
use localfly_lib::{bore, probe, store};
use std::time::Duration;
use uuid::Uuid;

const COMMON_PORTS: &[u16] = &[3000, 3001, 3333, 4000, 4200, 4321, 5173, 5174, 8000, 8080, 8081, 8888, 1313];

#[derive(Parser)]
#[command(name = "localfly", version, about = "Your localhost, on the internet (Linux port of Localfly)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Save an app by its local port.
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        port: u16,
        #[arg(long)]
        public_port: Option<u16>,
        #[arg(long)]
        public_address: Option<String>,
        #[arg(long, default_value_t = false)]
        start_on_launch: bool,
    },
    /// List saved apps with live status dots.
    List {
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Remove a saved app (by id prefix, name, or port).
    Rm {
        selector: String,
    },
    /// Edit a saved app.
    Edit {
        selector: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        public_port: Option<u16>,
        #[arg(long)]
        no_public_port: bool,
        #[arg(long)]
        public_address: Option<String>,
        #[arg(long)]
        no_public_address: bool,
        #[arg(long)]
        start_on_launch: Option<bool>,
    },
    /// Expose a saved app (foreground; Ctrl-C stops). Refuses empty ports like macOS.
    On {
        selector: String,
        #[arg(long)]
        relay: Option<String>,
        #[arg(long)]
        secret: Option<String>,
        #[arg(long)]
        local_host: Option<String>,
        #[arg(long, default_value_t = false)]
        no_reconnect: bool,
    },
    /// One-shot tunnel without saving (foreground).
    Expose {
        port: u16,
        #[arg(long)]
        remote_port: Option<u16>,
        #[arg(long)]
        relay: Option<String>,
        #[arg(long)]
        secret: Option<String>,
        #[arg(long)]
        local_host: Option<String>,
        #[arg(long, default_value_t = false)]
        no_reconnect: bool,
    },
    /// Start every app marked start-on-launch (foreground; Ctrl-C stops all).
    Up {
        #[arg(long)]
        relay: Option<String>,
        #[arg(long)]
        secret: Option<String>,
        #[arg(long)]
        local_host: Option<String>,
    },
    /// Check whether something listens on a port.
    Probe {
        port: u16,
        #[arg(long, default_value = "localhost")]
        host: String,
    },
    /// Scan localhost for common dev-server ports.
    Scan,
    /// Measure relay control-port latency.
    RelayStatus {
        #[arg(long)]
        relay: Option<String>,
    },
    /// Persist defaults (mirrors macOS Settings: relay, auto-reconnect, local host).
    Config {
        #[arg(long)]
        relay: Option<String>,
        #[arg(long)]
        auto_reconnect: Option<bool>,
        #[arg(long)]
        local_host: Option<String>,
        #[arg(long)]
        secret: Option<String>,
    },
    /// Check GitHub releases for a newer version (read-only, like UpdateChecker).
    UpdateCheck,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct GlobalConfig {
    #[serde(default)]
    relay: Option<String>,
    #[serde(default)]
    auto_reconnect: Option<bool>,
    #[serde(default)]
    local_host: Option<String>,
}

fn config_path() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("LOCALFLY_CONFIG") {
        if !dir.trim().is_empty() {
            return std::path::PathBuf::from(dir).join("config.json");
        }
    }
    dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from(".")).join("localfly").join("config.json")
}

fn load_config() -> GlobalConfig {
    std::fs::read(config_path())
        .ok()
        .and_then(|d| serde_json::from_slice(&d).ok())
        .unwrap_or_default()
}

fn effective_relay(flag: Option<String>, cfg: &GlobalConfig) -> String {
    flag
        .or_else(|| std::env::var("LOCALFLY_RELAY").ok())
        .or_else(|| cfg.relay.clone())
        .unwrap_or_else(|| "bore.pub".to_string())
}

fn effective_secret(flag: Option<String>) -> Option<String> {
    flag.or_else(|| {
        let s = store::read_secret();
        if s.is_empty() { None } else { Some(s) }
    })
}

fn effective_local_host(flag: Option<String>, cfg: &GlobalConfig) -> String {
    flag.or_else(|| cfg.local_host.clone()).unwrap_or_else(|| "localhost".to_string())
}

fn effective_auto_reconnect(no_reconnect: bool, cfg: &GlobalConfig) -> bool {
    if no_reconnect {
        return false;
    }
    cfg.auto_reconnect.unwrap_or(true) // macOS default in AppModel is true
}

fn find_app(apps: &[store::SavedApp], selector: &str) -> Option<usize> {
    let sel = selector.trim();
    if let Ok(port) = sel.trim_start_matches(':').parse::<u16>() {
        if let Some(i) = apps.iter().position(|a| a.port == port) {
            return Some(i);
        }
    }
    if let Some(i) = apps.iter().position(|a| a.id.to_string().starts_with(sel)) {
        return Some(i);
    }
    apps.iter().position(|a| a.name.eq_ignore_ascii_case(sel))
}

async fn run_tunnel_foreground(
    app_name: &str,
    local_port: u16,
    requested: u16,
    options: TunnelOptions,
    public_address: Option<String>,
) -> Result<(), AnyErr> {
    if !probe::is_listening(local_port, &options.local_host, Duration::from_millis(1000)).await {
        eprintln!("Nothing running on :{local_port} — start your dev server first.");
        std::process::exit(2);
    }
    let mgr = TunnelManager::new();
    let id = Uuid::new_v4();
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
    tokio::signal::ctrl_c().await.ok();
    mgr.stop(id).await;
    printer.abort();
    Ok(())
}

type AnyErr = Box<dyn std::error::Error + Send + Sync>;

#[tokio::main]
async fn main() -> Result<(), AnyErr> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Add { name, port, public_port, public_address, start_on_launch } => {
            let mut apps = store::load_apps();
            if apps.iter().any(|a| a.port == port && a.name == name) {
                eprintln!("Already saved.");
                std::process::exit(1);
            }
            let mut app = store::SavedApp::new(name, port);
            app.public_port = public_port;
            app.public_address = public_address;
            app.start_on_launch = start_on_launch;
            apps.push(app.clone());
            store::save_apps(&apps);
            println!("Saved {} on :{} (id {})", app.name, app.port, app.id);
        }
        Cmd::List { json } => {
            let apps = store::load_apps();
            if json {
                println!("{}", serde_json::to_string_pretty(&apps)?);
                return Ok(());
            }
            if apps.is_empty() {
                println!("Nothing saved yet. Add one: localfly add --name web --port 3000");
                return Ok(());
            }
            // Probe all ports concurrently.
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
            let mut apps = store::load_apps();
            match find_app(&apps, &selector) {
                Some(i) => {
                    let removed = apps.remove(i);
                    store::save_apps(&apps);
                    println!("Removed {} (:{})", removed.name, removed.port);
                }
                None => {
                    eprintln!("No app matching '{selector}'.");
                    std::process::exit(1);
                }
            }
        }
        Cmd::Edit { selector, name, port, public_port, no_public_port, public_address, no_public_address, start_on_launch } => {
            let mut apps = store::load_apps();
            match find_app(&apps, &selector) {
                Some(i) => {
                    if let Some(n) = name {
                        apps[i].name = n.clone();
                        apps[i].hue = store::hue_for(&n);
                    }
                    if let Some(p) = port {
                        apps[i].port = p;
                    }
                    if no_public_port {
                        apps[i].public_port = None;
                    } else if let Some(p) = public_port {
                        apps[i].public_port = Some(p);
                    }
                    if no_public_address {
                        apps[i].public_address = None;
                    } else if let Some(a) = public_address {
                        apps[i].public_address = Some(a);
                    }
                    if let Some(s) = start_on_launch {
                        apps[i].start_on_launch = s;
                    }
                    store::save_apps(&apps);
                    println!("Updated {} (:{})", apps[i].name, apps[i].port);
                }
                None => {
                    eprintln!("No app matching '{selector}'.");
                    std::process::exit(1);
                }
            }
        }
        Cmd::On { selector, relay, secret, local_host, no_reconnect } => {
            let apps = store::load_apps();
            let i = find_app(&apps, &selector).unwrap_or_else(|| {
                eprintln!("No app matching '{selector}'.");
                std::process::exit(1);
            });
            let app = apps[i].clone();
            let cfg = load_config();
            let options = TunnelOptions {
                relay: effective_relay(relay, &cfg),
                secret: effective_secret(secret),
                local_host: effective_local_host(local_host, &cfg),
                auto_reconnect: effective_auto_reconnect(no_reconnect, &cfg),
            };
            run_tunnel_foreground(&app.name, app.port, app.public_port.unwrap_or(0), options, app.public_address).await?;
        }
        Cmd::Expose { port, remote_port, relay, secret, local_host, no_reconnect } => {
            let cfg = load_config();
            let options = TunnelOptions {
                relay: effective_relay(relay, &cfg),
                secret: effective_secret(secret),
                local_host: effective_local_host(local_host, &cfg),
                auto_reconnect: effective_auto_reconnect(no_reconnect, &cfg),
            };
            run_tunnel_foreground(&format!(":{port}"), port, remote_port.unwrap_or(0), options, None).await?;
        }
        Cmd::Up { relay, secret, local_host } => {
            let apps = store::load_apps();
            let launch: Vec<_> = apps.into_iter().filter(|a| a.start_on_launch).collect();
            if launch.is_empty() {
                println!("No apps marked start-on-launch. Use: localfly edit <name> --start-on-launch true");
                return Ok(());
            }
            let cfg = load_config();
            let base = TunnelOptions {
                relay: effective_relay(relay, &cfg),
                secret: effective_secret(secret),
                local_host: effective_local_host(local_host, &cfg),
                auto_reconnect: true,
            };
            let mgr = TunnelManager::new();
            let mut rx = mgr.subscribe();
            for app in &launch {
                if !probe::is_listening(app.port, &base.local_host, Duration::from_millis(800)).await {
                    eprintln!("Skipping {}: nothing running on :{}", app.name, app.port);
                    continue;
                }
                let options = base.clone();
                let mgr2 = mgr.clone();
                let app2 = app.clone();
                tokio::spawn(async move {
                    mgr2.start(app2.id, app2.port, app2.public_port.unwrap_or(0), options).await;
                });
            }
            let printer = tokio::spawn(async move {
                while let Ok(info) = rx.recv().await {
                    match &info.status {
                        TunnelStatus::Live => {
                            eprintln!("live :{} -> {}", info.local_port, info.web_url().unwrap_or_default());
                        }
                        TunnelStatus::Error(e) => {
                            eprintln!(":{} error: {e}", info.local_port);
                        }
                        _ => {}
                    }
                }
            });
            println!("Running {} app(s). Ctrl-C stops all.", launch.len());
            tokio::signal::ctrl_c().await.ok();
            mgr.stop_all().await;
            printer.abort();
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
            for &port in COMMON_PORTS {
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
            let cfg = load_config();
            let relay_str = effective_relay(relay, &cfg);
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
            let path = config_path();
            let mut cfg = load_config();
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
                Ok(Some((ver, url))) => println!("Update available: {ver} ({url}) — you have {current}"),
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

async fn update_check() -> Result<Option<(String, String)>, AnyErr> {
    // Mirrors App/UpdateChecker.swift: read-only GitHub latest-release check.
    let body = reqwest_like_get("https://api.github.com/repos/raqibnur/localfly/releases/latest").await?;
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
