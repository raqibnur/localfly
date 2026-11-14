//! `localflyd` wire protocol: newline-delimited JSON over a Unix socket.
//!
//! One request per connection, one JSON line back — except `subscribe`,
//! which streams one JSON line per tunnel update until the client hangs up.
//! The socket lives at `$XDG_RUNTIME_DIR/localfly.sock` (same-UID only).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

use crate::manager::{TunnelInfo, TunnelStatus};
use crate::store::SavedApp;

/// Where the daemon listens. Python GUI mirrors this logic.
/// `LOCALFLY_SOCKET` overrides (tests, multi-user boxes).
pub fn socket_path() -> PathBuf {
    if let Ok(custom) = std::env::var("LOCALFLY_SOCKET") {
        if !custom.trim().is_empty() {
            return PathBuf::from(custom);
        }
    }
    if let Ok(rt) = std::env::var("XDG_RUNTIME_DIR") {
        if !rt.trim().is_empty() {
            return PathBuf::from(rt).join("localfly.sock");
        }
    }
    dirs::runtime_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("localfly.sock")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Ping,
    List,
    Start { id: Uuid, local_port: u16, remote_port: u16, options: OptionConfig },
    Stop { id: Uuid },
    StopAll,
    Up,
    Add { app: SavedApp },
    Update { app: SavedApp },
    Remove { id: Uuid },
    /// Stream {"info": TunnelDto} lines until the client disconnects.
    Subscribe,
}

/// Per-start tunnel options (mirrors `TunnelOptions`; secret travels over the
/// same-UID local socket only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionConfig {
    pub relay: String,
    pub secret: Option<String>,
    pub local_host: String,
    pub auto_reconnect: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    #[serde(default)]
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<TunnelDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apps: Option<Vec<AppState>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<Vec<Uuid>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl Response {
    pub fn ok() -> Self {
        Self { ok: true, error: None, info: None, apps: None, started: None, version: None }
    }

    pub fn err(reason: impl Into<String>) -> Self {
        Self { ok: false, error: Some(reason.into()), info: None, apps: None, started: None, version: None }
    }
}

/// One saved app plus its live state (if any) and port health.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub app: SavedApp,
    pub listening: Option<bool>,
    pub tunnel: Option<TunnelDto>,
}

/// JSON-safe view of `TunnelInfo` (`Instant` doesn't serialize).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelDto {
    pub id: Uuid,
    pub local_port: u16,
    pub relay_host: String,
    pub remote_port: Option<u16>,
    pub status: String,
    pub reason: Option<String>,
    pub live_since_epoch: Option<u64>,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub active_connections: u64,
    pub total_connections: u64,
    pub retry_attempt: u32,
}

impl TunnelDto {
    pub fn web_url(&self) -> Option<String> {
        self.remote_port.map(|p| format!("http://{}:{p}", self.relay_host))
    }
}

impl From<&TunnelInfo> for TunnelDto {
    fn from(info: &TunnelInfo) -> Self {
        let (status, reason) = match &info.status {
            TunnelStatus::Connecting => ("connecting".to_string(), None),
            TunnelStatus::Live => ("live".to_string(), None),
            TunnelStatus::Error(e) => ("error".to_string(), Some(e.clone())),
            TunnelStatus::Stopped => ("stopped".to_string(), None),
        };
        Self {
            id: info.id,
            local_port: info.local_port,
            relay_host: info.relay_host.clone(),
            remote_port: info.remote_port,
            status,
            reason,
            live_since_epoch: info.live_since.map(|t| {
                // epoch = now - elapsed
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs().saturating_sub(t.elapsed().as_secs()))
                    .unwrap_or(0)
            }),
            bytes_in: info.stats.bytes_in,
            bytes_out: info.stats.bytes_out,
            active_connections: info.stats.active_connections,
            total_connections: info.stats.total_connections,
            retry_attempt: info.retry_attempt,
        }
    }
}

/// Saved-app registry shared by daemon tasks; persists to disk on change.
pub struct AppRegistry {
    inner: tokio::sync::Mutex<Vec<SavedApp>>,
}

impl AppRegistry {
    pub fn load() -> Self {
        Self { inner: tokio::sync::Mutex::new(crate::store::load_apps()) }
    }

    pub async fn snapshot(&self) -> Vec<SavedApp> {
        self.inner.lock().await.clone()
    }

    pub async fn add(&self, app: SavedApp) {
        let mut apps = self.inner.lock().await;
        apps.retain(|a| a.id != app.id);
        apps.push(app);
        crate::store::save_apps(&apps);
    }

    pub async fn update(&self, app: SavedApp) -> bool {
        let mut apps = self.inner.lock().await;
        if let Some(slot) = apps.iter_mut().find(|a| a.id == app.id) {
            *slot = app;
            crate::store::save_apps(&apps);
            true
        } else {
            false
        }
    }

    pub async fn remove(&self, id: Uuid) -> bool {
        let mut apps = self.inner.lock().await;
        let before = apps.len();
        apps.retain(|a| a.id != id);
        let changed = apps.len() != before;
        if changed {
            crate::store::save_apps(&apps);
        }
        changed
    }

    pub async fn find_by_port_or_id_or_name(&self, sel: &str) -> Option<SavedApp> {
        let apps = self.inner.lock().await;
        find_app(&apps, sel).cloned()
    }
}

/// Mirrors the CLI's selector: id prefix, exact port, or case-insensitive name.
pub fn find_app<'a>(apps: &'a [SavedApp], selector: &str) -> Option<&'a SavedApp> {
    let sel = selector.trim();
    if let Ok(port) = sel.trim_start_matches(':').parse::<u16>() {
        if let Some(a) = apps.iter().find(|a| a.port == port) {
            return Some(a);
        }
    }
    if let Some(a) = apps.iter().find(|a| a.id.to_string().starts_with(sel)) {
        return Some(a);
    }
    apps.iter().find(|a| a.name.eq_ignore_ascii_case(sel))
}

/// Probes saved + common dev ports concurrently (macOS `pollPorts` parity).
pub async fn probe_all(ports: &[u16], host: &str) -> HashMap<u16, bool> {
    use std::time::Duration;
    let mut tasks = Vec::new();
    for &port in ports {
        let host = host.to_string();
        tasks.push(tokio::spawn(async move {
            (port, crate::probe::is_listening(port, &host, Duration::from_millis(600)).await)
        }));
    }
    let mut out = HashMap::new();
    for t in tasks {
        if let Ok((port, up)) = t.await {
            out.insert(port, up);
        }
    }
    out
}

pub const COMMON_PORTS: &[u16] = &[3000, 3001, 3333, 4000, 4200, 4321, 5173, 5174, 8000, 8080, 8081, 8888, 1313];

#[cfg(unix)]
fn daemon_binary() -> Option<std::path::PathBuf> {
    // Same dir as this executable (cargo install / system package layout)...
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let cand = dir.join("localflyd");
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    // ...or on PATH.
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join("localflyd"))
            .find(|p| p.is_file())
    })
}

/// One-shot JSON request. `LOCALFLY_SOCKET` overrides the path (tests).
pub async fn call(req: &Request, timeout: Duration) -> Result<Response, String> {
    call_at(&socket_path(), req, timeout).await
}

/// One-shot request to an explicit socket path (tests, multi-user boxes).
pub async fn call_at(sock: &std::path::Path, req: &Request, timeout: Duration) -> Result<Response, String> {
    use tokio::io::AsyncWriteExt;
    let mut stream = tokio::time::timeout(timeout, tokio::net::UnixStream::connect(sock))
        .await
        .map_err(|_| "daemon unreachable".to_string())?
        .map_err(|e| format!("daemon unreachable: {e}"))?;
    let mut payload =
        serde_json::to_string(req).map_err(|e| format!("encode request: {e}"))?;
    payload.push('\n');
    stream.write_all(payload.as_bytes()).await.map_err(|e| format!("daemon write: {e}"))?;
    let mut reader = tokio::io::BufReader::new(&mut stream);
    let mut line = String::new();
    use tokio::io::AsyncBufReadExt;
    tokio::time::timeout(timeout, reader.read_line(&mut line))
        .await
        .map_err(|_| "daemon timed out".to_string())?
        .map_err(|e| format!("daemon read: {e}"))?;
    serde_json::from_str(line.trim()).map_err(|e| format!("bad daemon reply: {e}"))
}

/// Lines of `{"info": ...}` until the server closes (subscribe mode).
pub async fn subscribe(
    timeout: Duration,
) -> Result<tokio::io::Lines<tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>>, String> {
    subscribe_at(&socket_path(), timeout).await
}

/// Subscribe on an explicit socket path.
pub async fn subscribe_at(
    sock: &std::path::Path,
    timeout: Duration,
) -> Result<tokio::io::Lines<tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>>, String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let stream = tokio::time::timeout(timeout, tokio::net::UnixStream::connect(sock))
        .await
        .map_err(|_| "daemon unreachable".to_string())?
        .map_err(|e| format!("daemon unreachable: {e}"))?;
    let (rh, mut wh) = stream.into_split();
    let mut payload = serde_json::to_string(&Request::Subscribe).map_err(|e| e.to_string())?;
    payload.push('\n');
    wh.write_all(payload.as_bytes()).await.map_err(|e| format!("daemon write: {e}"))?;
    drop(wh);
    Ok(tokio::io::BufReader::new(rh).lines())
}

/// Connect to the daemon, spawning a detached one if none answers.
pub async fn ensure_daemon() -> Result<(), String> {
    if ping().await.is_ok() {
        return Ok(());
    }
    let bin = daemon_binary().ok_or_else(|| "localflyd not found (reinstall localfly)".to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;
        let mut cmd = std::process::Command::new(bin);
        // New process group: survives terminal close and Ctrl-C in clients.
        cmd.process_group(0);
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        cmd.spawn().map_err(|e| format!("cannot start localflyd: {e}"))?;
    }
    #[cfg(not(unix))]
    {
        return Err("localflyd auto-spawn unsupported here".to_string());
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if ping().await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("localflyd did not come up".to_string())
}

async fn ping() -> Result<Response, String> {
    let r = call(&Request::Ping, Duration::from_secs(2)).await?;
    if r.ok {
        Ok(r)
    } else {
        Err("ping failed".to_string())
    }
}

impl OptionConfig {
    /// Convert to the engine's options type.
    pub fn into_tunnel_options(self) -> crate::manager::TunnelOptions {
        crate::manager::TunnelOptions {
            relay: self.relay,
            secret: self.secret,
            local_host: self.local_host,
            auto_reconnect: self.auto_reconnect,
        }
    }
}
