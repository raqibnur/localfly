//! TCP liveness probes, ported from LocalflyCore/PortProbe.swift.

use std::net::ToSocketAddrs;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// Whether something accepts TCP connections on host:port.
pub async fn is_listening(port: u16, host: &str, timeout_dur: Duration) -> bool {
    measure(host, port, timeout_dur).await.is_some()
}

/// TCP connect time to host:port, or None if unreachable within timeout.
pub async fn measure(host: &str, port: u16, timeout_dur: Duration) -> Option<Duration> {
    // Resolve like Swift's NWConnection (tries IPv4/IPv6 in order).
    let addrs: Vec<std::net::SocketAddr> = match (host, port).to_socket_addrs() {
        Ok(it) => it.collect(),
        Err(_) => {
            // Bare IPv6 literal without brackets or unresolvable host.
            return None;
        }
    };
    let started = Instant::now();
    for addr in addrs {
        match timeout(timeout_dur, TcpStream::connect(addr)).await {
            Ok(Ok(_)) => return Some(started.elapsed()),
            _ => continue,
        }
    }
    None
}
