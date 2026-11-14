//! Shared fixtures for live-relay tests (real `bore server` on 127.0.0.1:7835).
//!
//! The bore control port is fixed, so every test that needs a relay must
//! hold [`bore_lock()`], which also serializes across test binaries, which
//! cargo runs in parallel.

use localfly_lib::probe;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Exclusive, process-shared lock for the fixed bore control port.
/// Hold the guard for the whole test.
pub fn bore_lock() -> std::fs::File {
        let path = std::env::temp_dir().join("localfly-bore-tests.lock");
    use fs2::FileExt;
    let f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&path)
        .expect("lock file");
    f.lock_exclusive().expect("bore test lock");
    f
}

pub fn bore_bin() -> Option<String> {
    for c in [
        std::env::var("BORE_BIN").ok(),
        Some("/tmp/opencode/bore-test/bin/bore".to_string()),
        std::env::var("HOME").ok().map(|h| format!("{h}/.cargo/bin/bore")),
        Some("/usr/local/bin/bore".to_string()),
        Some("bore".to_string()),
    ]
    .into_iter()
    .flatten()
    {
        if c.contains('/') {
            if std::path::Path::new(&c).exists() {
                return Some(c);
            }
        } else if Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {c}"))
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Some(c);
        }
    }
    None
}

pub struct BoreServer {
    child: Child,
}

impl BoreServer {
    pub async fn launch(secret: Option<&str>) -> Result<Self, String> {
        let bin = bore_bin().ok_or_else(|| "bore binary not found".to_string())?;
        let mut cmd = Command::new(bin);
        cmd.args(["server", "--bind-addr", "127.0.0.1", "--bind-tunnels", "127.0.0.1"]);
        if let Some(s) = secret {
            cmd.args(["--secret", s]);
        }
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
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

    pub fn stop(&mut self) {
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

/// Minimal TCP echo server standing in for a local dev app.
pub struct EchoServer {
    _task: tokio::task::JoinHandle<()>,
    pub port: u16,
}

impl EchoServer {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { break };
                tokio::spawn(async move {
                    let (mut r, mut w) = s.split();
                    let _ = tokio::io::copy(&mut r, &mut w).await;
                });
            }
        });
        Self { _task: task, port }
    }
}

pub async fn round_trip(port: u16, payload: &str) -> String {
    let mut s = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(("127.0.0.1", port)))
        .await
        .expect("connect")
        .expect("tcp");
    s.write_all(payload.as_bytes()).await.unwrap();
    let mut buf = vec![0u8; 4096];
    let n = tokio::time::timeout(Duration::from_secs(5), s.read(&mut buf)).await.unwrap().unwrap();
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

pub async fn eventually_within<F, Fut>(secs: f64, mut f: F) -> bool
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

/// Scratch dir (socket + config) cleaned up on drop. No extra crates needed.
pub struct TempDir {
    pub path: std::path::PathBuf,
}

static TEMPDIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl TempDir {
    pub fn new(prefix: &str) -> Self {
        let n = TEMPDIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("tempdir");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
