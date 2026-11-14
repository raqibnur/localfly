//! Tunnel lifecycle, ported from LocalflyCore/TunnelManager.swift.

use crate::bore::parse_relay;
use crate::client::{self, Event};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, broadcast, oneshot};
use uuid::Uuid;

pub type TunnelId = Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelStatus {
    Connecting,
    Live,
    Error(String),
    Stopped,
}

impl TunnelStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Connecting | Self::Live)
    }
}

#[derive(Debug, Clone)]
pub struct TunnelOptions {
    pub relay: String,
    pub secret: Option<String>,
    pub local_host: String,
    pub auto_reconnect: bool,
}

impl Default for TunnelOptions {
    fn default() -> Self {
        Self {
            relay: "bore.pub".to_string(),
            secret: None,
            local_host: "localhost".to_string(),
            auto_reconnect: false,
        }
    }
}

impl TunnelOptions {
    pub fn relay_endpoint(&self) -> (String, u16) {
        parse_relay(&self.relay)
    }
}

#[derive(Debug, Clone)]
pub struct TunnelInfo {
    pub id: TunnelId,
    pub local_port: u16,
    pub relay_host: String,
    pub remote_port: Option<u16>,
    pub status: TunnelStatus,
    pub live_since: Option<Instant>,
    pub stats: client::TunnelStats,
    /// Reconnect attempts since the tunnel was last live.
    pub retry_attempt: u32,
}

impl TunnelInfo {
    /// `relay:remote_port`, the address visitors use.
    pub fn public_url(&self) -> Option<String> {
        self.remote_port.map(|p| format!("{}:{p}", self.relay_host))
    }
    pub fn web_url(&self) -> Option<String> {
        self.public_url().map(|u| format!("http://{u}"))
    }
}

struct TunnelState {
    info: TunnelInfo,
    task: tokio::task::JoinHandle<()>,
}

struct Inner {
    tunnels: HashMap<TunnelId, TunnelState>,
}

#[derive(Clone)]
pub struct TunnelManager {
    inner: Arc<Mutex<Inner>>,
    updates: broadcast::Sender<TunnelInfo>,
}

impl TunnelManager {
    pub fn new() -> Self {
        let (updates, _) = broadcast::channel(256);
        Self { inner: Arc::new(Mutex::new(Inner { tunnels: HashMap::new() })), updates }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TunnelInfo> {
        self.updates.subscribe()
    }

    /// Opens a tunnel and returns once it is live or the first attempt failed.
    /// Pass `id` to key the tunnel; an existing tunnel with that id is replaced.
    /// Pass `remote_port` to ask the relay for that public port.
    pub async fn start(
        &self,
        id: TunnelId,
        local_port: u16,
        remote_port: u16,
        options: TunnelOptions,
    ) -> TunnelInfo {
        self.stop(id).await;
        let (host, _) = options.relay_endpoint();
        let info = TunnelInfo {
            id,
            local_port,
            relay_host: host,
            remote_port: None,
            status: TunnelStatus::Connecting,
            live_since: None,
            stats: client::TunnelStats::default(),
            retry_attempt: 0,
        };
        self.publish(info.clone());
        let (first_tx, first_rx) = oneshot::channel::<TunnelInfo>();
        let mgr = self.clone();
        let task = tokio::spawn(async move {
            run_loop(mgr, id, local_port, remote_port, options, Some(first_tx)).await;
        });
        {
            let mut inner = self.inner.lock().await;
            inner.tunnels.insert(id, TunnelState { info: info.clone(), task });
        }
        // Wait for first live/failed (or stop, which drops the task).
        match first_rx.await {
            Ok(info) => info,
            Err(_) => self.info(id).await.unwrap_or(info),
        }
    }

    pub async fn stop(&self, id: TunnelId) {
        let state = {
            let mut inner = self.inner.lock().await;
            inner.tunnels.remove(&id)
        };
        if let Some(state) = state {
            state.task.abort();
            let mut info = state.info;
            info.status = TunnelStatus::Stopped;
            info.stats.active_connections = 0;
            self.publish(info);
        }
    }

    pub async fn stop_all(&self) {
        let ids: Vec<TunnelId> = {
            let inner = self.inner.lock().await;
            inner.tunnels.keys().copied().collect()
        };
        for id in ids {
            self.stop(id).await;
        }
    }

    pub async fn list(&self) -> Vec<TunnelInfo> {
        let inner = self.inner.lock().await;
        let mut v: Vec<TunnelInfo> = inner.tunnels.values().map(|s| s.info.clone()).collect();
        v.sort_by_key(|i| i.local_port);
        v
    }

    pub async fn info(&self, id: TunnelId) -> Option<TunnelInfo> {
        let inner = self.inner.lock().await;
        inner.tunnels.get(&id).map(|s| s.info.clone())
    }
}

impl Default for TunnelManager {
    fn default() -> Self {
        Self::new()
    }
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_secs_f64((2f64.powi(attempt as i32)).min(30.0))
}

async fn run_loop(
    mgr: TunnelManager,
    id: TunnelId,
    local_port: u16,
    fixed_port: u16,
    options: TunnelOptions,
    mut first: Option<oneshot::Sender<TunnelInfo>>,
) {
    let mut retry_attempt: u32 = 0;
    let mut remote_port: Option<u16> = None;
    loop {
        let requested = if retry_attempt == 0 {
            fixed_port
        } else if fixed_port != 0 {
            fixed_port
        } else if retry_attempt == 1 {
            // First retry tries to keep the old address (Swift parity).
            remote_port.unwrap_or(0)
        } else {
            0
        };
        let (relay_host, relay_port) = options.relay_endpoint();
        let config = client::Config {
            local_host: options.local_host.clone(),
            local_port,
            relay_host: relay_host.clone(),
            relay_port,
            secret: options.secret.clone(),
            requested_port: requested,
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Event>();
        // Inline (not spawned): aborting this loop also drops the control
        // connection, matching Swift stop() teardown. The select below keeps
        // draining rx after the client future finishes so the terminal Failed
        // event is never lost in the race between task-completion and channel.
        let client_fut = client::run(config, tx);
        tokio::pin!(client_fut);
        let mut client_finished = false;
        // Drain events until Failed (client::run returns after Failed).
        loop {
            tokio::select! {
                _ = &mut client_fut, if !client_finished => {
                    client_finished = true;
                    // Fall through to drain any buffered terminal event.
                    // If the channel is already closed with nothing left, recv returns None below.
                }
                ev = rx.recv() => {
                    let Some(ev) = ev else { break };
            match ev {
                Event::Live { remote_port: port } => {
                    remote_port = Some(port);
                    let info = TunnelInfo {
                        id,
                        local_port,
                        relay_host: relay_host.clone(),
                        remote_port: Some(port),
                        status: TunnelStatus::Live,
                        live_since: Some(Instant::now()),
                        stats: client::TunnelStats::default(),
                        retry_attempt: 0,
                    };
                    retry_attempt = 0;
                    mgr.set_info(id, info.clone()).await;
                    if let Some(tx) = first.take() {
                        let _ = tx.send(info);
                    }
                }
                Event::Stats(stats) => {
                    mgr.set_stats(id, stats).await;
                }
                Event::Failed(reason) => {
                    let mut info = mgr.info(id).await.unwrap_or(TunnelInfo {
                        id,
                        local_port,
                        relay_host: relay_host.clone(),
                        remote_port,
                        status: TunnelStatus::Error(reason.clone()),
                        live_since: None,
                        stats: client::TunnelStats::default(),
                        retry_attempt,
                    });
                    info.status = TunnelStatus::Error(reason);
                    info.live_since = None;
                    info.stats.active_connections = 0;
                    mgr.set_info(id, info.clone()).await;
                    if let Some(tx) = first.take() {
                        let _ = tx.send(info);
                    }
                    break;
                }
            }
                }
            }
        }
        if !options.auto_reconnect {
            break;
        }
        // Auto-reconnect: mark connecting and back off (Swift parity).
        {
            let inner = mgr.inner.lock().await;
            if !inner.tunnels.contains_key(&id) {
                break; // stopped while failing
            }
        }
        retry_attempt += 1;
        // Update status to connecting + bump attempt count.
        {
            let mut inner = mgr.inner.lock().await;
            if let Some(state) = inner.tunnels.get_mut(&id) {
                state.info.status = TunnelStatus::Connecting;
                state.info.remote_port = remote_port;
                state.info.retry_attempt = retry_attempt;
                let info = state.info.clone();
                let _ = mgr.updates.send(info);
            } else {
                break;
            }
        }
        // A fixed port is always asked for again (Swift parity — handled via `requested` above).
        tokio::time::sleep(backoff(retry_attempt.saturating_sub(1))).await;
        {
            let inner = mgr.inner.lock().await;
            if !inner.tunnels.contains_key(&id) {
                break;
            }
        }
    }
}

impl TunnelManager {
    fn publish(&self, info: TunnelInfo) {
        let _ = self.updates.send(info);
    }

    async fn set_info(&self, id: TunnelId, info: TunnelInfo) {
        let mut inner = self.inner.lock().await;
        if let Some(state) = inner.tunnels.get_mut(&id) {
            state.info = info.clone();
        } else {
            return;
        }
        let _ = self.updates.send(info);
    }

    async fn set_stats(&self, id: TunnelId, stats: client::TunnelStats) {
        let mut inner = self.inner.lock().await;
        if let Some(state) = inner.tunnels.get_mut(&id) {
            state.info.stats = stats;
            let info = state.info.clone();
            let _ = self.updates.send(info);
        }
    }
}
