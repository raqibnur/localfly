//! `localflyd`: owns every tunnel and the saved-app registry.
//!
//! Clients (the `localfly` CLI, the GUI) talk to it over a same-UID Unix
//! socket with one JSON request per connection. The daemon exits on its own
//! after 10 idle minutes with no live tunnels (clients re-spawn it).

use localfly_lib::daemon::{
    self, AppRegistry, Request, Response, TunnelDto,
};
use localfly_lib::manager::{TunnelManager, TunnelOptions};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use uuid::Uuid;

const IDLE_EXIT_AFTER: Duration = Duration::from_secs(600);
const IDLE_CHECK_EVERY: Duration = Duration::from_secs(30);

fn idle_tuning() -> (Duration, Duration) {
    let secs = std::env::var("LOCALFLY_IDLE_EXIT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(IDLE_EXIT_AFTER);
    let every = std::env::var("LOCALFLY_IDLE_CHECK_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(IDLE_CHECK_EVERY);
    (every, secs)
}

fn now_epoch() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

struct State {
    manager: TunnelManager,
    apps: AppRegistry,
    last_activity: AtomicU64,
    subscribers: AtomicU64,
}

impl State {
    fn touch(&self) {
        self.last_activity.store(now_epoch(), Ordering::Relaxed);
    }
}

#[tokio::main]
async fn main() {
    let path = daemon::socket_path();
    // Another daemon already running? Defer to it (used by auto-spawn races).
    if is_alive(&path).await {
        return;
    }
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("localflyd: cannot bind {}: {e}", path.display());
            std::process::exit(1);
        }
    };
    // Same-UID private socket.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700));
    }

    let state = Arc::new(State {
        manager: TunnelManager::new(),
        apps: AppRegistry::load(),
        last_activity: AtomicU64::new(now_epoch()),
        subscribers: AtomicU64::new(0),
    });

    // Idle auto-exit so a forgotten daemon doesn't linger past logout.
    let (check_every, exit_after) = idle_tuning();
    {
        let state = state.clone();
        let path = path.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(check_every).await;
                let idle_for = now_epoch().saturating_sub(state.last_activity.load(Ordering::Relaxed));
                let busy = state.subscribers.load(Ordering::Relaxed) > 0
                    || manager_busy(&state.manager).await;
                if !busy && Duration::from_secs(idle_for) >= exit_after {
                    state.manager.stop_all().await;
                    let _ = std::fs::remove_file(&path);
                    std::process::exit(0);
                }
            }
        });
    }

    // SIGTERM/SIGINT: stop everything, remove the socket, exit.
    {
        let state = state.clone();
        let path = path.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};
            let mut term = signal(SignalKind::terminate()).ok();
            let mut int = signal(SignalKind::interrupt()).ok();
            match (term.as_mut(), int.as_mut()) {
                (Some(t), Some(i)) => {
                    tokio::select! {
                        _ = t.recv() => {},
                        _ = i.recv() => {},
                    }
                }
                (Some(t), None) => {
                    let _ = t.recv().await;
                }
                (None, Some(i)) => {
                    let _ = i.recv().await;
                }
                (None, None) => std::future::pending::<()>().await,
            }
            state.manager.stop_all().await;
            let _ = std::fs::remove_file(&path);
            std::process::exit(0);
        });
    }

    loop {
        let Ok((stream, _)) = listener.accept().await else { break };
        let state = state.clone();
        tokio::spawn(async move { handle(stream, state).await });
    }
}

async fn manager_busy(manager: &TunnelManager) -> bool {
    manager.list().await.iter().any(|i| i.status.is_active())
}

async fn is_alive(path: &std::path::Path) -> bool {
    match tokio::time::timeout(Duration::from_millis(300), UnixStream::connect(path)).await {
        Ok(Ok(mut s)) => {
            let req = serde_json::to_string(&Request::Ping).unwrap_or_default() + "\n";
            if s.write_all(req.as_bytes()).await.is_err() {
                return false;
            }
            let mut reader = BufReader::new(&mut s);
            let mut line = String::new();
            if reader.read_line(&mut line).await.is_err() {
                return false;
            }
            serde_json::from_str::<Response>(&line).map(|r| r.ok).unwrap_or(false)
        }
        _ => false,
    }
}

async fn read_request(stream: &mut UnixStream) -> Option<Request> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let n = match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        Ok(Ok(n)) => n,
        _ => return None,
    };
    if n == 0 {
        return None;
    }
    serde_json::from_str(line.trim()).ok()
}

async fn write_line(stream: &mut UnixStream, value: &impl serde::Serialize) -> bool {
    let mut s = serde_json::to_string(value).unwrap_or_else(|_| "{\"ok\":false}".into());
    s.push('\n');
    stream.write_all(s.as_bytes()).await.is_ok()
}

async fn handle(mut stream: UnixStream, state: Arc<State>) {
    let Some(req) = read_request(&mut stream).await else { return };
    state.touch();
    match req {
        Request::Ping => {
            let mut r = Response::ok();
            r.version = Some(env!("CARGO_PKG_VERSION").to_string());
            write_line(&mut stream, &r).await;
        }
        Request::List => {
            write_line(&mut stream, &list_snapshot(&state).await).await;
        }
        Request::Start { id, local_port, remote_port, options } => {
            let info = state
                .manager
                .start(id, local_port, remote_port, options.into_tunnel_options())
                .await;
            let mut r = Response::ok();
            r.info = Some(TunnelDto::from(&info));
            if !matches!(info.status, localfly_lib::manager::TunnelStatus::Error(_)) {
                // ok stays true; errors are reported but not fatal at protocol level
            }
            write_line(&mut stream, &r).await;
        }
        Request::Stop { id } => {
            state.manager.stop(id).await;
            write_line(&mut stream, &Response::ok()).await;
        }
        Request::StopAll => {
            state.manager.stop_all().await;
            write_line(&mut stream, &Response::ok()).await;
        }
        Request::Up => {
            let started = up(&state).await;
            let mut r = Response::ok();
            r.started = Some(started);
            write_line(&mut stream, &r).await;
        }
        Request::Add { app } => {
            state.apps.add(app).await;
            write_line(&mut stream, &Response::ok()).await;
        }
        Request::Update { app } => {
            if state.apps.update(app).await {
                write_line(&mut stream, &Response::ok()).await;
            } else {
                write_line(&mut stream, &Response::err("unknown app id")).await;
            }
        }
        Request::Remove { id } => {
            state.manager.stop(id).await;
            state.apps.remove(id).await;
            write_line(&mut stream, &Response::ok()).await;
        }
        Request::Subscribe => {
            state.subscribers.fetch_add(1, Ordering::Relaxed);
            let mut rx = state.manager.subscribe();
            // Catch the subscriber up, then stream.
            let snap = list_snapshot(&state).await;
            for app in snap.apps.unwrap_or_default() {
                if let Some(info) = app.tunnel {
                    if !write_line(&mut stream, &serde_json::json!({"info": info})).await {
                        state.subscribers.fetch_sub(1, Ordering::Relaxed);
                        return;
                    }
                }
            }
            loop {
                match rx.recv().await {
                    Ok(info) => {
                        state.touch();
                        if !write_line(&mut stream, &serde_json::json!({"info": TunnelDto::from(&info)})).await {
                            break;
                        }
                    }
                    Err(_) => break, // lagged or closed
                }
            }
            state.subscribers.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

async fn list_snapshot(state: &Arc<State>) -> Response {
    let apps = state.apps.snapshot().await;
    let tunnels: std::collections::HashMap<Uuid, TunnelDto> = state
        .manager
        .list()
        .await
        .iter()
        .map(|i| (i.id, TunnelDto::from(i)))
        .collect();
    let mut ports: Vec<u16> = apps.iter().map(|a| a.port).collect();
    ports.extend_from_slice(daemon::COMMON_PORTS);
    ports.sort_unstable();
    ports.dedup();
    let probed = daemon::probe_all(&ports, "localhost").await;
    let states = apps
        .into_iter()
        .map(|app| {
            let listening = probed.get(&app.port).copied();
            let tunnel = tunnels.get(&app.id).cloned();
            daemon::AppState { app, listening, tunnel }
        })
        .collect();
    let mut r = Response::ok();
    r.apps = Some(states);
    r
}

async fn up(state: &Arc<State>) -> Vec<Uuid> {
    let apps = state.apps.snapshot().await;
    let mut ports: Vec<u16> = apps.iter().map(|a| a.port).collect();
    ports.sort_unstable();
    ports.dedup();
    let probed = daemon::probe_all(&ports, "localhost").await;
    let mut started = Vec::new();
    for app in apps.into_iter().filter(|a| a.start_on_launch) {
        if probed.get(&app.port) != Some(&true) {
            continue;
        }
        let mgr = state.manager.clone();
        let id = app.id;
        let port = app.port;
        let remote = app.public_port.unwrap_or(0);
        tokio::spawn(async move {
            let options = TunnelOptions {
                relay: default_relay(),
                secret: default_secret(),
                local_host: default_local_host(),
                auto_reconnect: true,
            };
            mgr.start(id, port, remote, options).await;
        });
        started.push(id);
    }
    started
}

fn default_relay() -> String {
    localfly_lib::store::global_config_value("relay").unwrap_or_else(|| "bore.pub".to_string())
}

fn default_local_host() -> String {
    localfly_lib::store::global_config_value("local_host").unwrap_or_else(|| "localhost".to_string())
}

fn default_secret() -> Option<String> {
    let s = localfly_lib::store::read_secret();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

