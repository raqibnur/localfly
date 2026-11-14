//! Live-relay tests, mirroring LocalflyCore TunnelManagerTests.swift.
//! Run: BORE_BIN=/tmp/opencode/bore-test/bin/bore cargo test --test integration -- --test-threads=1
//! Serial: the bore control port (7835) is fixed.

use localfly_lib::manager::{TunnelManager, TunnelOptions};
use localfly_lib::probe;
use std::process::{Child, Command};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use serial_test::serial;
use uuid::Uuid;

fn bore_bin() -> Option<String> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(v) = std::env::var("BORE_BIN") {
        candidates.push(v);
    }
    candidates.push("/tmp/opencode/bore-test/bin/bore".to_string());
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(format!("{home}/.cargo/bin/bore"));
    }
    candidates.push("bore".to_string());
    candidates.into_iter().find(|c| {
        if c.contains('/') {
            std::path::Path::new(c).exists()
        } else {
            Command::new("sh").arg("-c").arg(format!("command -v {c}")).output().map(|o| o.status.success()).unwrap_or(false)
        }
    })
}

struct BoreServer {
    child: Child,
}

impl BoreServer {
    async fn launch(secret: Option<&str>) -> anyhow::Result<Self, String> {
        let bin = bore_bin().ok_or_else(|| "bore binary not found".to_string())?;
        // Previous test's server may still be releasing the fixed control port.
        let free_deadline = std::time::Instant::now() + Duration::from_secs(5);
        while probe::is_listening(7835, "127.0.0.1", Duration::from_millis(200)).await {
            if std::time::Instant::now() > free_deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let mut cmd = Command::new(bin);
        cmd.args(["server", "--bind-addr", "127.0.0.1", "--bind-tunnels", "127.0.0.1"]);
        if let Some(s) = secret {
            cmd.args(["--secret", s]);
        }
        cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        let child = cmd.spawn().map_err(|e| e.to_string())?;
        let mut srv = Self { child };
        // Wait for control port.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if probe::is_listening(7835, "127.0.0.1", Duration::from_millis(300)).await {
                return Ok(srv);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        srv.stop();
        Err("bore server did not start".to_string())
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Give the kernel a moment to release the fixed control port before
        // the next test binds it; avoids flaky "address in use" handovers.
        std::thread::sleep(Duration::from_millis(300));
    }
}

impl Drop for BoreServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct EchoServer {
    _task: tokio::task::JoinHandle<()>,
    port: u16,
}

impl EchoServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { break };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 65536];
                    loop {
                        match s.read(&mut buf).await {
                            Ok(0) => break,
                            Ok(n) => {
                                if s.write_all(&buf[..n]).await.is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                });
            }
        });
        Self { _task: task, port }
    }
}

async fn round_trip(port: u16, payload: &str) -> String {
    let mut s = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(("127.0.0.1", port)))
        .await
        .expect("connect")
        .expect("tcp");
    s.write_all(payload.as_bytes()).await.unwrap();
    let mut buf = vec![0u8; 4096];
    let n = tokio::time::timeout(Duration::from_secs(5), s.read(&mut buf)).await.unwrap().unwrap();
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

async fn eventually_within<F, Fut>(secs: f64, mut f: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = std::time::Instant::now() + Duration::from_secs_f64(secs);
    while std::time::Instant::now() < deadline {
        if f().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

#[tokio::test]
#[serial]
async fn start_proxies_traffic_and_stop_closes_it() {
    let Some(_) = bore_bin() else { eprintln!("skip: bore missing"); return };
    let mut server = BoreServer::launch(None).await.expect("relay");
    let echo = EchoServer::start().await;
    let mgr = TunnelManager::new();
    let info = mgr.start(Uuid::new_v4(), echo.port, 0, TunnelOptions { relay: "127.0.0.1".into(), ..Default::default() }).await;
    assert_eq!(info.status, localfly_lib::manager::TunnelStatus::Live, "expected live, got {:?}", info.status);
    let remote = info.remote_port.expect("remote port");
    assert_eq!(round_trip(remote, "ping over the tunnel").await, "ping over the tunnel");
    mgr.stop(info.id).await;
    assert!(mgr.list().await.is_empty());
    assert!(eventually_within(1.5, || async {
        !probe::is_listening(remote, "127.0.0.1", Duration::from_millis(300)).await
    }).await);
    server.stop();
}

#[tokio::test]
#[serial]
async fn authenticates_with_secret() {
    let Some(_) = bore_bin() else { eprintln!("skip: bore missing"); return };
    let mut server = BoreServer::launch(Some("hunter2")).await.expect("relay");
    let echo = EchoServer::start().await;
    let mgr = TunnelManager::new();
    let good = mgr
        .start(
            Uuid::new_v4(),
            echo.port,
            0,
            TunnelOptions { relay: "127.0.0.1".into(), secret: Some("hunter2".into()), ..Default::default() },
        )
        .await;
    assert_eq!(good.status, localfly_lib::manager::TunnelStatus::Live);
    assert_eq!(round_trip(good.remote_port.unwrap(), "authed").await, "authed");

    let missing = mgr
        .start(Uuid::new_v4(), echo.port, 0, TunnelOptions { relay: "127.0.0.1".into(), ..Default::default() })
        .await;
    assert_eq!(missing.status, localfly_lib::manager::TunnelStatus::Error("This relay requires a secret".into()));

    let wrong = mgr
        .start(
            Uuid::new_v4(),
            echo.port,
            0,
            TunnelOptions { relay: "127.0.0.1".into(), secret: Some("nope".into()), ..Default::default() },
        )
        .await;
    assert!(matches!(wrong.status, localfly_lib::manager::TunnelStatus::Error(_)), "wrong secret went {:?}", wrong.status);
    mgr.stop_all().await;
    server.stop();
}

#[tokio::test]
#[serial]
async fn requests_fixed_public_port() {
    let Some(_) = bore_bin() else { eprintln!("skip: bore missing"); return };
    let mut server = BoreServer::launch(None).await.expect("relay");
    let echo = EchoServer::start().await;
    let mgr = TunnelManager::new();
    let fixed = mgr
        .start(Uuid::new_v4(), echo.port, 40123, TunnelOptions { relay: "127.0.0.1".into(), ..Default::default() })
        .await;
    assert_eq!(fixed.status, localfly_lib::manager::TunnelStatus::Live);
    assert_eq!(fixed.remote_port, Some(40123));
    assert_eq!(round_trip(40123, "fixed").await, "fixed");

    let clash = mgr
        .start(Uuid::new_v4(), echo.port, 40123, TunnelOptions { relay: "127.0.0.1".into(), ..Default::default() })
        .await;
    match clash.status {
        localfly_lib::manager::TunnelStatus::Error(ref reason) => assert!(reason.starts_with("Relay refused the tunnel"), "{reason}"),
        ref s => panic!("expected error, got {s:?}"),
    }
    mgr.stop_all().await;
    server.stop();
}

#[tokio::test]
#[serial]
async fn reports_unreachable_relay() {
    let mgr = TunnelManager::new();
    let info = mgr
        .start(Uuid::new_v4(), 3000, 0, TunnelOptions { relay: "127.0.0.1:1".into(), ..Default::default() })
        .await;
    match info.status {
        localfly_lib::manager::TunnelStatus::Error(ref reason) => assert!(reason.contains("Couldn't reach relay"), "{reason}"),
        ref s => panic!("expected error, got {s:?}"),
    }
}
