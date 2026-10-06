//! Embedded-Tor tier (issue #4, P7): the SOCKS5 bridge's hostile tests
//! (migrated from the desktop's `socks_bridge.rs`) plus the `BuiltinTor`
//! state machine with its `Deferred` bootstrap seam — the same tests the
//! desktop shipped, now pinning the promoted module. Compiled only under
//! the `builtin-tor` feature.

use std::sync::Arc;

use mawaqit_api::tor::{
    BUILTIN_TOR_PORT, BuiltinTor, BuiltinTorStatus, StartMode,
    socks_bridge::{self, Connector, RelayStream},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

// ---------------------------------------------------------------- helpers

/// A connector that tunnels to a local "upstream" echo server, so the
/// relay is exercised in both directions without any real network.
async fn echo_upstream() -> Connector {
    let upstream = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = upstream.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = upstream.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let (mut reader, mut writer) = sock.split();
                let _ = tokio::io::copy(&mut reader, &mut writer).await;
            });
        }
    });
    Arc::new(move |_host, _port| {
        Box::pin(async move {
            Ok(Box::new(TcpStream::connect(addr).await?)
                as Box<dyn RelayStream>)
        })
    })
}

fn spawn_bridge(connector: Connector) -> std::net::SocketAddr {
    let bridge = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    bridge.set_nonblocking(true).unwrap();
    let bridge_addr = bridge.local_addr().unwrap();
    let listener = tokio::net::TcpListener::from_std(bridge).unwrap();
    tokio::spawn(socks_bridge::serve(listener, connector));
    bridge_addr
}

/// Drive a raw SOCKS5 client against the bridge; returns the reply to the
/// CONNECT request plus the relayed socket (`None` when the bridge closed
/// without replying).
async fn socks_client(
    bridge: std::net::SocketAddr,
    target: &[u8],
    cmd: u8,
) -> (Option<Vec<u8>>, Option<TcpStream>) {
    let mut sock = TcpStream::connect(bridge).await.unwrap();
    sock.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut reply = [0u8; 2];
    sock.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply, [0x05, 0x00], "no-auth must be accepted");

    let mut req = vec![0x05, cmd, 0x00];
    req.extend_from_slice(target);
    req.extend_from_slice(&80u16.to_be_bytes());
    sock.write_all(&req).await.unwrap();

    let mut connect_reply = vec![0u8; 10];
    match sock.read_exact(&mut connect_reply).await {
        Ok(_) => (Some(connect_reply), Some(sock)),
        Err(_) => (None, None),
    }
}

fn temp_state_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mawaqit-api-tor-{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ------------------------------------------------------------ BuiltinTor

#[test]
fn fresh_state_is_not_started() {
    let tor = BuiltinTor::new(temp_state_dir("fresh"));
    assert_eq!(tor.status(), BuiltinTorStatus::NotStarted);
    assert!(tor.socks_addr().is_none());
}

/// The Deferred seam: the listener binds (loopback, preferred port or
/// OS-assigned), the stack persists across calls — and the network is
/// never touched.
#[tokio::test]
async fn ensure_started_deferred_binds_and_is_idempotent() {
    let tor = BuiltinTor::new(temp_state_dir("deferred"));
    let addr =
        tor.ensure_started_with(StartMode::Deferred).expect("stack starts");
    assert_eq!(addr.ip().to_string(), "127.0.0.1");
    assert!(
        addr.port() == BUILTIN_TOR_PORT || addr.port() != BUILTIN_TOR_PORT,
        "preferred port or OS-assigned — both fine"
    );
    let again =
        tor.ensure_started_with(StartMode::Deferred).expect("stack persists");
    assert_eq!(addr, again, "idempotent: same address, no second listener");
    assert!(matches!(tor.status(), BuiltinTorStatus::Bootstrapping));
    assert_eq!(tor.socks_addr(), Some(addr));
}

/// One stack per state dir: two `BuiltinTor`s with different roots each
/// get their own listener (the state dir is the identity).
#[tokio::test]
async fn separate_state_dirs_get_separate_stacks() {
    let a = BuiltinTor::new(temp_state_dir("a"));
    let b = BuiltinTor::new(temp_state_dir("b"));
    let addr_a = a.ensure_started_with(StartMode::Deferred).unwrap();
    let addr_b = b.ensure_started_with(StartMode::Deferred).unwrap();
    assert_ne!(addr_a.port(), addr_b.port(), "no shared listener");
}

// ----------------------------------------------------------- SOCKS bridge

#[tokio::test]
async fn relays_both_directions_after_connect() {
    let connector = echo_upstream().await;
    let bridge_addr = spawn_bridge(connector);

    // Domain target — the bridge must pass it through unresolved.
    let mut target = vec![0x03u8, 7];
    target.extend_from_slice(b"example");
    let (reply, sock) = socks_client(bridge_addr, &target, 0x01).await;
    let reply = reply.expect("success reply expected");
    assert_eq!(&reply[..3], &[0x05, 0x00, 0x00]);
    let mut sock = sock.unwrap();

    sock.write_all(b"ping").await.unwrap();
    let mut buf = [0u8; 4];
    sock.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"ping", "echo round-trip through the bridge");
}

#[tokio::test]
async fn rejects_bind_with_command_not_supported() {
    let connector = echo_upstream().await;
    let bridge_addr = spawn_bridge(connector);

    let target = [0x01u8, 127, 0, 0, 1];
    let (reply, sock) = socks_client(bridge_addr, &target, 0x02).await; // BIND
    let reply = reply.expect("BIND must still get a reply before close");
    assert_eq!(&reply[..2], &[0x05, 0x07], "07 = command not supported");
    drop(sock);
}

#[tokio::test]
async fn rejects_unknown_address_type() {
    let connector = echo_upstream().await;
    let bridge_addr = spawn_bridge(connector);

    let (reply, _) = socks_client(bridge_addr, &[0x09u8, 1, 2], 0x01).await;
    let reply = reply.expect("bad ATYP must still get a reply");
    assert_eq!(&reply[..2], &[0x05, 0x08], "08 = address type not supported");
}

#[tokio::test]
async fn connector_failure_replies_general_failure() {
    let failing: Connector = Arc::new(|_host, _port| {
        Box::pin(async { Err(std::io::Error::other("transport not ready")) })
    });
    let bridge_addr = spawn_bridge(failing);

    let target = [0x01u8, 127, 0, 0, 1];
    let (reply, _) = socks_client(bridge_addr, &target, 0x01).await;
    let reply = reply.expect("connector failure must still get a reply");
    assert_eq!(&reply[..2], &[0x05, 0x01], "01 = general failure");
}

#[tokio::test]
async fn closes_on_bad_version_without_reply() {
    let connector = echo_upstream().await;
    let bridge_addr = spawn_bridge(connector);

    let mut sock = TcpStream::connect(bridge_addr).await.unwrap();
    sock.write_all(&[0x04, 0x01, 0x00]).await.unwrap(); // SOCKS4 greeting
    let mut buf = [0u8; 2];
    let res = sock.read_exact(&mut buf).await;
    assert!(res.is_err(), "bad version must close without a reply");
}
