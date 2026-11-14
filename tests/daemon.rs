//! Daemon protocol tests: real `localflyd` on a temp socket, real `bore server`.
//! Serialized with the bore control-port lock (see common); the socket and
//! config live in a temp dir so the developer's own daemon is untouched.

mod common;

use common::{bore_bin, bore_lock, round_trip, BoreServer, EchoServer, TempDir};
use localfly_lib::daemon::{self, OptionConfig, Request};
use serial_test::serial;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use uuid::Uuid;

struct Daemon {
    child: Child,
    sock: PathBuf,
    _dir: TempDir,
}

impl Daemon {
    async fn spawn() -> Self {
        let dir = TempDir::new("lf-daemon-test");
        let sock = dir.path.join("localfly.sock");
        let mut child = Command::new(env!("CARGO_BIN_EXE_localflyd"))
            .env("LOCALFLY_SOCKET", &sock)
            .env("LOCALFLY_CONFIG", dir.path.as_os_str())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("localflyd binary");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match daemon::call_at(&sock, &Request::Ping, Duration::from_millis(300)).await {
                Ok(r) if r.ok => break,
                _ if std::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(100)).await
                }
                _ => {
                    let _ = child.kill();
                    panic!("localflyd did not answer");
                }
            }
        }
        Self { child, sock, _dir: dir }
    }

    async fn call(&self, req: &Request) -> daemon::Response {
        daemon::call_at(&self.sock, req, Duration::from_secs(15)).await.expect("daemon call")
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn test_options() -> OptionConfig {
    OptionConfig {
        relay: "127.0.0.1".to_string(),
        secret: None,
        local_host: "localhost".to_string(),
        auto_reconnect: false,
    }
}

#[tokio::test]
#[serial]
async fn daemon_ping_and_crud() {
    let d = Daemon::spawn().await;
    let r = d.call(&Request::Ping).await;
    assert!(r.ok);
    assert!(r.version.is_some());

    let id = Uuid::new_v4();
    let app = localfly_lib::store::SavedApp {
        id,
        name: "web".to_string(),
        port: 3000,
        start_on_launch: false,
        hue: 0.5,
        public_port: None,
        public_address: None,
    };
    let r = d.call(&Request::Add { app: app.clone() }).await;
    assert!(r.ok, "{r:?}");

    let r = d.call(&Request::List).await;
    assert!(r.ok);
    let apps = r.apps.expect("apps");
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].app.name, "web");
    assert!(apps[0].tunnel.is_none());

    let mut app2 = app.clone();
    app2.start_on_launch = true;
    assert!(d.call(&Request::Update { app: app2 }).await.ok);

    assert!(d.call(&Request::Remove { id }).await.ok);
    let r = d.call(&Request::List).await;
    assert!(r.apps.expect("apps").is_empty());
    // `stop` on unknown id is a no-op success.
    assert!(d.call(&Request::Stop { id: Uuid::new_v4() }).await.ok);
}

#[tokio::test]
#[serial]
async fn daemon_starts_streams_and_stops_tunnel() {
    let _lock = bore_lock();
    let Some(_) = bore_bin() else { eprintln!("skip: bore missing"); return };
    let mut relay = BoreServer::launch(None).await.expect("relay");
    let d = Daemon::spawn().await;
    let echo = EchoServer::start().await;

    let id = Uuid::new_v4();
    let mut app = localfly_lib::store::SavedApp::new("echo".to_string(), echo.port);
    app.id = id;
    assert!(d.call(&Request::Add { app }).await.ok);

    // Subscribe first so we observe the live transition as a stream.
    let mut lines = daemon::subscribe_at(&d.sock, Duration::from_secs(5)).await.expect("subscribe");

    let r = d
        .call(&Request::Start { id, local_port: echo.port, remote_port: 0, options: test_options() })
        .await;
    assert!(r.ok, "{r:?}");
    let info = r.info.expect("tunnel info");
    assert_eq!(info.status, "live");
    let remote = info.remote_port.expect("remote port");

    // The stream carries the same live update.
    use tokio::io::AsyncBufReadExt;
    let mut saw_live = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(500), lines.next_line()).await {
            Ok(Ok(Some(line))) => {
                if line.contains("\"status\":\"live\"") {
                    saw_live = true;
                    break;
                }
            }
            _ => break,
        }
    }
    assert!(saw_live, "subscribe stream carried no live update");

    // Traffic actually flows through the daemon-owned tunnel.
    assert_eq!(round_trip(remote, "via-daemon").await, "via-daemon");

    assert!(d.call(&Request::Stop { id }).await.ok);
    let r = d.call(&Request::List).await;
    assert!(r.apps.expect("apps").iter().all(|a| a.tunnel.is_none()));
    relay.stop();
}

#[tokio::test]
#[serial]
async fn daemon_exits_when_idle() {
    let dir = TempDir::new("lf-daemon-idle");
    let sock = dir.path.join("localfly.sock");
    let mut child = Command::new(env!("CARGO_BIN_EXE_localflyd"))
        .env("LOCALFLY_SOCKET", &sock)
        .env("LOCALFLY_CONFIG", dir.path.as_os_str())
        .env("LOCALFLY_IDLE_EXIT_SECS", "2")
        .env("LOCALFLY_IDLE_CHECK_SECS", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("localflyd binary");
    // It answers while alive...
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if daemon::call_at(&sock, &Request::Ping, Duration::from_millis(300)).await.map(|r| r.ok).unwrap_or(false) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "daemon never answered");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // ...then exits on its own once idle.
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                assert!(status.success());
                break;
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            _ => {
                let _ = child.kill();
                panic!("daemon did not idle-exit");
            }
        }
    }
    // Socket file is cleaned up.
    assert!(!sock.exists());
}
