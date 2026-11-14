//! One bore client session: control connection + one proxied pair per visitor.
//! Port of LocalflyCore/BoreClient.swift from Network.framework to tokio.

use crate::bore::{self, FrameReader, ServerMessage};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use uuid::Uuid;

pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(6);
/// Server heartbeat is every 500ms; silence this long means the relay is gone.
pub const SILENCE_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TunnelStats {
    /// Bytes received from visitors and forwarded to the local app.
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub active_connections: u64,
    pub total_connections: u64,
}

#[derive(Debug, Clone)]
pub enum Event {
    Live { remote_port: u16 },
    Failed(String),
    Stats(TunnelStats),
}

#[derive(Debug, Clone)]
pub struct Config {
    pub local_host: String,
    pub local_port: u16,
    pub relay_host: String,
    pub relay_port: u16,
    pub secret: Option<String>,
    /// Public port asked of the relay; 0 lets the relay pick.
    pub requested_port: u16,
}

#[derive(Debug, Default)]
struct SharedStats {
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
    active: AtomicU64,
    total: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    AwaitingChallenge,
    AwaitingHello,
    Live,
}

pub async fn run(config: Config, tx: mpsc::UnboundedSender<Event>) {
    let stats = Arc::new(SharedStats::default());
    // Stats ticker (500ms like Swift's tick()).
    let stats_tx = tx.clone();
    let stats_shared = stats.clone();
    let stats_task = tokio::spawn(async move {
        let mut reported = TunnelStats::default();
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        loop {
            interval.tick().await;
            let cur = TunnelStats {
                bytes_in: stats_shared.bytes_in.load(Ordering::Relaxed),
                bytes_out: stats_shared.bytes_out.load(Ordering::Relaxed),
                active_connections: stats_shared.active.load(Ordering::Relaxed),
                total_connections: stats_shared.total.load(Ordering::Relaxed),
            };
            if cur != reported {
                reported = cur;
                if stats_tx.send(Event::Stats(cur)).is_err() {
                    break;
                }
            }
        }
    });

    let secret = config.secret.clone().filter(|s| !s.is_empty());
    let addr = format!("{}:{}", config.relay_host, config.relay_port);
    let started = Instant::now();
    let stream = match tokio::time::timeout(HANDSHAKE_TIMEOUT, TcpStream::connect(&addr)).await {
        Ok(Ok(s)) => s,
        _ => {
            let _ = tx.send(Event::Failed(format!("Couldn't reach relay {}", config.relay_host)));
            stats_task.abort();
            return;
        }
    };

    let (mut rh, mut wh) = stream.into_split();
    let mut phase = if secret.is_some() { Phase::AwaitingChallenge } else { Phase::AwaitingHello };

    if secret.is_none() {
        if wh.write_all(&bore::hello(config.requested_port)).await.is_err() {
            let _ = tx.send(Event::Failed("Relay closed the connection during setup".to_string()));
            stats_task.abort();
            return;
        }
    }

    let mut reader = FrameReader::new();
    let mut buf = vec![0u8; 4096];
    let mut last_message = Instant::now();
    let mut live_port: Option<u16> = None;

    loop {
        let deadline = if live_port.is_none() {
            HANDSHAKE_TIMEOUT.checked_sub(started.elapsed()).unwrap_or(Duration::ZERO)
        } else {
            SILENCE_TIMEOUT.checked_sub(last_message.elapsed()).unwrap_or(Duration::ZERO)
        };
        if deadline.is_zero() {
            let msg = if live_port.is_some() {
                format!("Lost contact with relay {}", config.relay_host)
            } else if phase == Phase::AwaitingChallenge {
                "Relay didn't ask for a secret. Clear it in Relay settings.".to_string()
            } else {
                format!("Timed out connecting to relay {}", config.relay_host)
            };
            let _ = tx.send(Event::Failed(msg));
            break;
        }
        match tokio::time::timeout(deadline, rh.read(&mut buf)).await {
            Err(_) => {
                let msg = if live_port.is_some() {
                    format!("Lost contact with relay {}", config.relay_host)
                } else if phase == Phase::AwaitingChallenge {
                    "Relay didn't ask for a secret. Clear it in Relay settings.".to_string()
                } else {
                    format!("Timed out connecting to relay {}", config.relay_host)
                };
                let _ = tx.send(Event::Failed(msg));
                break;
            }
            Ok(Err(_)) => {
                let _ = tx.send(Event::Failed(
                    if live_port.is_some() {
                        "Relay connection dropped".to_string()
                    } else {
                        "Relay closed the connection during setup".to_string()
                    },
                ));
                break;
            }
            Ok(Ok(0)) => {
                let _ = tx.send(Event::Failed(if live_port.is_some() {
                    "Relay closed the tunnel".to_string()
                } else {
                    "Relay closed the connection during setup".to_string()
                }));
                break;
            }
            Ok(Ok(n)) => {
                last_message = Instant::now();
                let frames = match reader.push(&buf[..n]) {
                    Ok(f) => f,
                    Err(e) => {
                        let _ = tx.send(Event::Failed(e.to_string()));
                        break;
                    }
                };
                let mut fatal = false;
                for frame in frames {
                    match bore::decode(&frame) {
                        Err(e) => {
                            let _ = tx.send(Event::Failed(e.to_string()));
                            fatal = true;
                            break;
                        }
                        Ok(msg) => match (phase, live_port, msg) {
                            (Phase::AwaitingChallenge, None, ServerMessage::Challenge(c)) => {
                                if let Some(s) = &secret {
                                    let tag = bore::answer(&c, s);
                                    if wh.write_all(&bore::authenticate(&tag)).await.is_err()
                                        || wh.write_all(&bore::hello(config.requested_port)).await.is_err()
                                    {
                                        let _ = tx.send(Event::Failed(
                                            "Relay closed the connection during setup".to_string(),
                                        ));
                                        fatal = true;
                                        break;
                                    }
                                    phase = Phase::AwaitingHello;
                                }
                            }
                            (Phase::AwaitingChallenge, None, ServerMessage::Error(m)) => {
                                let _ = tx.send(Event::Failed(format!("Relay refused the tunnel: {m}")));
                                fatal = true;
                                break;
                            }
                            (Phase::AwaitingChallenge, _, _) => {
                                let _ = tx.send(Event::Failed(
                                    "A secret is set, but this relay doesn't use one".to_string(),
                                ));
                                fatal = true;
                                break;
                            }
                            (Phase::AwaitingHello, None, ServerMessage::Hello(port)) => {
                                live_port = Some(port);
                                phase = Phase::Live;
                                let _ = tx.send(Event::Live { remote_port: port });
                            }
                            (Phase::AwaitingHello, None, ServerMessage::Challenge(_)) => {
                                let _ = tx.send(Event::Failed("This relay requires a secret".to_string()));
                                fatal = true;
                                break;
                            }
                            (Phase::AwaitingHello, None, ServerMessage::Error(m)) => {
                                let _ = tx.send(Event::Failed(format!("Relay refused the tunnel: {m}")));
                                fatal = true;
                                break;
                            }
                            (Phase::AwaitingHello, None, _) => {
                                let _ = tx.send(Event::Failed("Relay sent an unexpected reply".to_string()));
                                fatal = true;
                                break;
                            }
                            (Phase::Live, Some(_), ServerMessage::Connection(id)) => {
                                let cfg = config.clone();
                                let st = stats.clone();
                                tokio::spawn(async move {
                                    open_proxy(cfg, id, st).await;
                                });
                            }
                            (Phase::Live, Some(_), _) => { /* heartbeat: silence timer already reset */ }
                            _ => {}
                        },
                    }
                }
                if fatal {
                    break;
                }
            }
        }
    }
    stats_task.abort();
}

async fn open_proxy(config: Config, id: Uuid, stats: Arc<SharedStats>) {
    stats.active.fetch_add(1, Ordering::Relaxed);
    stats.total.fetch_add(1, Ordering::Relaxed);
    let done = || {
        stats.active.fetch_sub(1, Ordering::Relaxed);
    };

    let relay_addr = format!("{}:{}", config.relay_host, config.relay_port);
    let remote = match tokio::time::timeout(Duration::from_secs(6), TcpStream::connect(&relay_addr)).await
    {
        Ok(Ok(s)) => s,
        _ => {
            done();
            return;
        }
    };
    let (mut r_read, mut r_write) = remote.into_split();

    if let Some(secret) = config.secret.clone().filter(|s| !s.is_empty()) {
        let mut reader = FrameReader::new();
        let mut buf = vec![0u8; 4097];
        match wait_challenge(&mut r_read, &mut reader, &mut buf).await {
            ProxyAuth::Got(c) => {
                let tag = bore::answer(&c, &secret);
                if r_write.write_all(&bore::authenticate(&tag)).await.is_err() {
                    done();
                    return;
                }
                // bore never pipelines bytes after the challenge; drop any leftover
                // (Swift forwards it to the local app — no-op here).
                let _ = reader.drain();
            }
            ProxyAuth::Failed => {
                done();
                return;
            }
        }
    }
    if r_write.write_all(&bore::accept(&id)).await.is_err() {
        done();
        return;
    }
    let local_addr = format!("{}:{}", config.local_host, config.local_port);
    let local = match tokio::time::timeout(Duration::from_secs(6), TcpStream::connect(&local_addr)).await
    {
        Ok(Ok(s)) => s,
        _ => {
            done();
            return;
        }
    };
    let (mut l_read, mut l_write) = local.into_split();
    let s1 = stats.clone();
    let remote_to_local = async move {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match r_read.read(&mut buf).await {
                Ok(0) => {
                    let _ = l_write.shutdown().await;
                    break;
                }
                Ok(n) => {
                    s1.bytes_in.fetch_add(n as u64, Ordering::Relaxed);
                    if l_write.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    };
    let s2 = stats.clone();
    let local_to_remote = async move {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match l_read.read(&mut buf).await {
                Ok(0) => {
                    let _ = r_write.shutdown().await;
                    break;
                }
                Ok(n) => {
                    s2.bytes_out.fetch_add(n as u64, Ordering::Relaxed);
                    if r_write.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    };
    tokio::join!(remote_to_local, local_to_remote);
    done();
}

enum ProxyAuth {
    Got(Uuid),
    Failed,
}

async fn wait_challenge(
    r_read: &mut tokio::net::tcp::OwnedReadHalf,
    reader: &mut FrameReader,
    buf: &mut [u8],
) -> ProxyAuth {
    let deadline = tokio::time::sleep(Duration::from_secs(6));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => return ProxyAuth::Failed,
            n = r_read.read(buf) => {
                match n {
                    Ok(0) | Err(_) => return ProxyAuth::Failed,
                    Ok(n) => match reader.push(&buf[..n]) {
                        Err(_) => return ProxyAuth::Failed,
                        Ok(frames) => {
                            for f in frames {
                                match bore::decode(&f) {
                                    Ok(ServerMessage::Challenge(c)) => return ProxyAuth::Got(c),
                                    _ => return ProxyAuth::Failed,
                                }
                            }
                        }
                    },
                }
            }
        }
    }
}
