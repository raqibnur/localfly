//! Live-relay tests, mirroring LocalflyCore TunnelManagerTests.swift.
//! Serialized with the bore control-port lock (see common); the control
//! port (7835) is fixed across all test binaries.

mod common;

use common::{bore_bin, bore_lock, eventually_within, round_trip, BoreServer, EchoServer};
use localfly_lib::manager::{TunnelManager, TunnelOptions};
use localfly_lib::probe;
use serial_test::serial;
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn start_proxies_traffic_and_stop_closes_it() {
    let _lock = bore_lock();
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
    let _lock = bore_lock();
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
    let _lock = bore_lock();
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
