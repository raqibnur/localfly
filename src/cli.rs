//! CLI surface shared by `localfly` and the shell-completion/man generator.
//! Transport itself lives in [`crate::daemon`]; this module is argument
//! parsing plus config-file access only.

use clap::{Parser, Subcommand};
use std::time::Duration;

/// Flags for the `localflyd` background daemon. It usually needs no
/// arguments: clients auto-spawn it and it idle-exits on its own.
#[derive(Parser)]
#[command(name = "localflyd", version, about = "Localfly Linux tunnel daemon (Unix socket, auto-spawned by the CLI)")]
pub struct DaemonArgs {
    /// Unix socket path (default: $XDG_RUNTIME_DIR/localfly.sock).
    /// Also settable via LOCALFLY_SOCKET.
    #[arg(long)]
    pub socket: Option<std::path::PathBuf>,
}

#[derive(Parser)]
#[command(name = "localfly", version, about = "Your localhost, on the internet (Localfly Linux)")]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
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
    /// Expose a saved app in the daemon (follows status; Ctrl-C detaches, tunnel keeps running).
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
    /// Stop a saved app's tunnel (or --all).
    Off {
        selector: Option<String>,
        #[arg(long, default_value_t = false)]
        all: bool,
    },
    /// One-shot tunnel without saving (foreground; Ctrl-C stops it).
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
    /// Start every app marked start-on-launch in the daemon (foreground follow; SIGTERM stops all).
    Up,
    /// Show live tunnels and their traffic.
    Status {
        #[arg(long, default_value_t = false)]
        json: bool,
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
pub struct GlobalConfig {
    #[serde(default)]
    pub relay: Option<String>,
    #[serde(default)]
    pub auto_reconnect: Option<bool>,
    #[serde(default)]
    pub local_host: Option<String>,
}

pub fn config_path() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("LOCALFLY_CONFIG") {
        if !dir.trim().is_empty() {
            return std::path::PathBuf::from(dir).join("config.json");
        }
    }
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("localfly")
        .join("config.json")
}

pub fn load_config() -> GlobalConfig {
    std::fs::read(config_path())
        .ok()
        .and_then(|d| serde_json::from_slice(&d).ok())
        .unwrap_or_default()
}

/// How long `localfly on` waits for the daemon per request.
pub const DAEMON_TIMEOUT: Duration = Duration::from_secs(20);

pub fn effective_relay(flag: Option<String>, cfg: &GlobalConfig) -> String {
    flag
        .or_else(|| std::env::var("LOCALFLY_RELAY").ok())
        .or_else(|| cfg.relay.clone())
        .unwrap_or_else(|| "bore.pub".to_string())
}

pub fn effective_secret(flag: Option<String>) -> Option<String> {
    flag.or_else(|| {
        let s = crate::store::read_secret();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    })
}

pub fn effective_local_host(flag: Option<String>, cfg: &GlobalConfig) -> String {
    flag.or_else(|| cfg.local_host.clone()).unwrap_or_else(|| "localhost".to_string())
}

pub fn effective_auto_reconnect(no_reconnect: bool, cfg: &GlobalConfig) -> bool {
    if no_reconnect {
        return false;
    }
    cfg.auto_reconnect.unwrap_or(true) // macOS default in AppModel is true
}
