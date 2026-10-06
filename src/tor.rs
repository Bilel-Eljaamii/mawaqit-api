//! Embedded Tor — the `builtin-tor` feature (issue #4, P7; ADR-0012 as
//! amended). The crate embeds the Tor Project's Rust client (Arti) and
//! exposes it to the HTTP stack as a local SOCKS5 listener, so Tor
//! routing needs zero external setup: no system tor, no Tor Browser.
//!
//! Privacy contract (contract-tested, not just documented):
//! - the library **never** enables Tor by itself — the feature only ships the
//!   capability, the caller opts in per [`BuiltinTor::ensure_started`];
//! - bootstrap is non-blocking and **never falls back to a direct connection**
//!   — until Tor is ready, requests through the bridge fail loudly (the user
//!   routed this traffic through Tor on purpose);
//! - hostnames cross the bridge **unresolved** (socks5h semantics) — DNS
//!   happens inside the tunnel;
//! - every connection gets a fresh, isolated Tor circuit.
//!
//! Composition with the client (the desktop's recipe):
//!
//! ```no_run
//! use mawaqit_api::tor::BuiltinTor;
//!
//! # async fn example() -> Result<(), mawaqit_api::MawaqitError> {
//! let tor = BuiltinTor::new("/home/me/.local/share/mawaqit/tor".into());
//! let addr = tor.ensure_started()?; // sub-second: listener binds now,
//!                                   // Arti bootstraps in the background
//! let client = mawaqit_api::MawaqitClient::new()
//!     .with_socks_proxy(format!("socks5h://{addr}"))?;
//! # Ok(())
//! # }
//! ```
//!
//! `ensure_started` must be called within a tokio runtime context (the
//! desktop's command handlers and the TUI's async main both are); outside
//! one it returns [`MawaqitError::Tor`] cleanly instead of panicking.
//!
//! Default-off: without the feature this module does not exist and the
//! crate's dependency tree is Arti-free.

use std::{
    future::Future,
    net::SocketAddr,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex},
};

use arti_client::TorClient;

use crate::error::{BOUNDED_DIAGNOSTIC, MawaqitError, bounded};

/// Preferred loopback port for the internal SOCKS5 listener. Chosen to
/// avoid the system tor (9050) and Tor Browser (9150); if taken, the OS
/// assigns a free port instead.
pub const BUILTIN_TOR_PORT: u16 = 9058;

/// How [`BuiltinTor::ensure_started_with`] connects: [`StartMode::Auto`]
/// bootstraps onto the Tor network in the background;
/// [`StartMode::Deferred`] binds the listener and constructs the client
/// but never touches the network — the seam tests and air-gapped checks
/// use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartMode {
    Auto,
    Deferred,
}

/// Connect-to-Tor-network progress, for logs and UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinTorStatus {
    /// No stack has been started (Tor never enabled this session).
    NotStarted,
    /// The listener is up; the client is still connecting to the Tor
    /// network (first run downloads the directory: can take a minute).
    Bootstrapping,
    /// Connected to the Tor network.
    Ready,
}

/// One embedded Tor stack for the process lifetime: the toggle points the
/// transport at it (or away), and a re-enable is instant — the stack stays
/// warm across off/on toggles.
pub struct BuiltinTor {
    state_dir: PathBuf,
    stack: Mutex<Option<Stack>>,
}

struct Stack {
    socks_addr: SocketAddr,
    bootstrapped: Arc<std::sync::atomic::AtomicBool>,
}

impl BuiltinTor {
    /// A stack rooted at `state_dir` (Arti's persistent directory cache
    /// and client state live in `state_dir/arti-state` and `arti-cache`).
    /// The caller owns the location — point it at the app's config dir so
    /// uninstalling clears Tor's cache too.
    pub fn new(state_dir: PathBuf) -> Self {
        Self { state_dir, stack: Mutex::new(None) }
    }

    /// Start the stack if not running and return the local SOCKS5 address
    /// the transport should use (`socks5h://<here>`). Sub-second: binding
    /// the listener is synchronous, Arti construction and the network
    /// bootstrap happen on a background task. Idempotent — an existing
    /// stack is reused. Must be called within a tokio runtime context.
    pub fn ensure_started(&self) -> Result<SocketAddr, MawaqitError> {
        self.ensure_started_with(StartMode::Auto)
    }

    /// [`Self::ensure_started`] with the bootstrap behavior injectable
    /// ([`StartMode::Deferred`] never touches the network — the seam
    /// tests and air-gapped checks use).
    pub fn ensure_started_with(
        &self,
        mode: StartMode,
    ) -> Result<SocketAddr, MawaqitError> {
        let mut guard = self.stack.lock().expect("builtin-tor lock poisoned");
        if let Some(stack) = guard.as_ref() {
            return Ok(stack.socks_addr);
        }

        // Bind synchronously (std, no runtime needed) so the caller gets
        // a real address before the background task has done anything: a
        // socks client connecting early just waits in the accept backlog
        // until the bridge serves.
        let std_listener =
            std::net::TcpListener::bind((host(), BUILTIN_TOR_PORT))
                .or_else(|_| std::net::TcpListener::bind((host(), 0)))
                .map_err(|e| {
                    MawaqitError::Tor(bounded(
                        &format!(
                            "could not bind the built-in Tor listener: {e}"
                        ),
                        BOUNDED_DIAGNOSTIC,
                    ))
                })?;
        let socks_addr = std_listener.local_addr().map_err(|e| {
            MawaqitError::Tor(bounded(
                &format!("built-in Tor listener has no address: {e}"),
                BOUNDED_DIAGNOSTIC,
            ))
        })?;
        std_listener.set_nonblocking(true).map_err(|e| {
            MawaqitError::Tor(bounded(
                &format!("built-in Tor listener: {e}"),
                BOUNDED_DIAGNOSTIC,
            ))
        })?;

        let bootstrapped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&bootstrapped);
        let state_dir = Arc::new(self.state_dir.clone());
        tokio::spawn(async move {
            // Runtime context: now the std listener can join the reactor.
            let listener = match tokio::net::TcpListener::from_std(std_listener)
            {
                Ok(l) => l,
                Err(e) => {
                    eprintln!(
                        "built-in tor stopped: could not register listener: {e}"
                    );
                    return;
                }
            };
            if let Err(e) =
                run_stack(listener, (*state_dir).clone(), flag, mode).await
            {
                // The stack is dead; the listener is closed. Remote
                // requests fail until the next enable/restart — loud, not
                // silent, and never a fallback to direct.
                eprintln!("built-in tor stopped: {e}");
            }
        });

        if mode == StartMode::Auto {
            eprintln!(
                "built-in tor: listener on {socks_addr}, connecting to the Tor network in the background"
            );
        }
        *guard = Some(Stack { socks_addr, bootstrapped });
        Ok(socks_addr)
    }

    /// Where the transport should point for built-in mode, without
    /// starting anything. `None` when no stack is running.
    pub fn socks_addr(&self) -> Option<SocketAddr> {
        self.stack
            .lock()
            .expect("builtin-tor lock poisoned")
            .as_ref()
            .map(|s| s.socks_addr)
    }

    /// Connect-to-Tor-network progress.
    pub fn status(&self) -> BuiltinTorStatus {
        let guard = self.stack.lock().expect("builtin-tor lock poisoned");
        match guard.as_ref() {
            None => BuiltinTorStatus::NotStarted,
            Some(s)
                if s.bootstrapped
                    .load(std::sync::atomic::Ordering::Relaxed) =>
            {
                BuiltinTorStatus::Ready
            }
            Some(_) => BuiltinTorStatus::Bootstrapping,
        }
    }
}

/// Start the stack if not running and return the local SOCKS5 address
/// the transport should use. See [`BuiltinTor::ensure_started`].
#[allow(clippy::too_many_lines)]
async fn run_stack(
    listener: tokio::net::TcpListener,
    state_dir: PathBuf,
    bootstrapped: Arc<std::sync::atomic::AtomicBool>,
    mode: StartMode,
) -> Result<(), String> {
    let cache_dir = state_dir.join("arti-cache");
    let state_dir = state_dir.join("arti-state");
    for dir in [&state_dir, &cache_dir] {
        std::fs::create_dir_all(dir).map_err(|e| {
            format!("could not create Tor state dir {}: {e}", dir.display())
        })?;
    }

    let config = arti_client::config::TorClientConfigBuilder::from_directories(
        state_dir, cache_dir,
    )
    .build()
    .map_err(|e| format!("built-in tor config: {e}"))?;

    // Unbootstrapped first: the listener must start accepting now; the
    // network connection continues in the background.
    let client = TorClient::builder()
        .config(config)
        .create_unbootstrapped_async()
        .await
        .map_err(|e| format!("built-in tor client: {e}"))?;

    if mode == StartMode::Auto {
        let client = Arc::clone(&client);
        tokio::spawn(async move {
            // Safe to retry later (per arti docs); a failed bootstrap
            // keeps the bridge up so the next request attempt surfaces a
            // transport error rather than a dead socket.
            match client.bootstrap().await {
                Ok(()) => {
                    bootstrapped
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    eprintln!("built-in tor: connected to the Tor network");
                }
                Err(e) => eprintln!("built-in tor: bootstrap failed: {e}"),
            }
        });
    }

    // Fresh isolation per connection: two requests never share a circuit,
    // so concurrent UI data paths and voice downloads are not linkable.
    let connect = Arc::new(move |host: String, port: u16| {
        let client = client.isolated_client();
        Box::pin(async move {
            let stream = client
                .connect((host.as_str(), port))
                .await
                .map_err(|e| std::io::Error::other(format!("tor: {e}")))?;
            Ok(Box::new(stream) as Box<dyn socks_bridge::RelayStream>)
        })
            as Pin<
                Box<
                    dyn Future<
                            Output = std::io::Result<
                                Box<dyn socks_bridge::RelayStream>,
                            >,
                        > + Send,
                >,
            >
    });

    socks_bridge::serve(listener, connect).await;
    Ok(())
}

fn host() -> std::net::IpAddr {
    use std::net::Ipv4Addr;
    Ipv4Addr::LOCALHOST.into()
}

/// The SOCKS5 CONNECT bridge between a local TCP listener and the
/// embedded transport. Exposed (`pub`) behind the feature so tests and
/// advanced transports (the connector is injectable — anything that
/// opens `(host, port)` streams) can compose with it.
///
/// The protocol surface is deliberately tiny, because the only client
/// that ever talks to this listener is this crate's own reqwest (socks
/// feature) and it binds to loopback only: no auth methods beyond "none",
/// no BIND, no UDP ASSOCIATE. Hostnames are passed through **unresolved**
/// (socks5h semantics): DNS never happens on this side of the bridge, so
/// nothing leaks before the traffic enters the tunnel.
pub mod socks_bridge {
    use std::{future::Future, pin::Pin, sync::Arc};

    use tokio::{
        io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };

    /// A relay-able stream: what a connector hands back for one CONNECT
    /// request. A trait object because the two implementations (Tor
    /// `DataStream`, test `TcpStream`) are unrelated types, and Rust trait
    /// objects cannot combine two non-auto traits directly.
    pub trait RelayStream: AsyncRead + AsyncWrite + Unpin + Send {}
    impl<T: AsyncRead + AsyncWrite + Unpin + Send> RelayStream for T {}

    /// Opens `(hostname, port)` through the transport behind the bridge.
    /// The future must be `Send` — each CONNECT is served on its own task.
    pub type Connector = Arc<
        dyn Fn(
                String,
                u16,
            ) -> Pin<
                Box<
                    dyn Future<Output = std::io::Result<Box<dyn RelayStream>>>
                        + Send,
                >,
            > + Send
            + Sync,
    >;

    /// Accept forever, serving each connection on its own task. Accept
    /// errors are logged and retried — a transient listener hiccup must
    /// not take the bridge down.
    pub async fn serve(listener: TcpListener, connect: Connector) {
        loop {
            match listener.accept().await {
                Ok((sock, _peer)) => {
                    let connect = Arc::clone(&connect);
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(sock, connect).await {
                            eprintln!("socks bridge connection ended: {e}");
                        }
                    });
                }
                Err(e) => eprintln!("socks bridge accept failed: {e}"),
            }
        }
    }

    /// Serve one SOCKS5 client connection end to end: handshake, CONNECT
    /// request, relay. Every failure path closes the socket — the client
    /// (our own reqwest) surfaces a connect error, nothing more.
    async fn handle_connection(
        mut sock: TcpStream,
        connect: Connector,
    ) -> Result<(), std::io::Error> {
        let (host, port) = negotiate(&mut sock).await?;
        let mut target = match (connect)(host, port).await {
            Ok(stream) => stream,
            Err(e) => {
                // 01 = general SOCKS server failure (the transport refused
                // or is not ready — e.g. Tor still bootstrapping).
                let _ = sock
                    .write_all(&[0x05, 0x01, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                    .await;
                return Err(std::io::Error::other(format!(
                    "connect failed: {e}"
                )));
            }
        };
        // Success: BND.ADDR/BND.PORT are meaningless for our client,
        // zeroed.
        sock.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await?;
        let _ = tokio::io::copy_bidirectional(&mut sock, &mut target).await;
        Ok(())
    }

    /// SOCKS5 greeting + CONNECT request. Returns the (unresolved) target.
    async fn negotiate(
        sock: &mut TcpStream,
    ) -> Result<(String, u16), std::io::Error> {
        // Greeting: VER NMETHODS METHODS...
        let mut head = [0u8; 2];
        sock.read_exact(&mut head).await?;
        if head[0] != 0x05 {
            return Err(std::io::Error::other(format!(
                "bad socks version {:#x}",
                head[0]
            )));
        }
        let mut methods = vec![0u8; head[1] as usize];
        sock.read_exact(&mut methods).await?;
        if !methods.contains(&0x00) {
            // 0xFF = no acceptable methods.
            let _ = sock.write_all(&[0x05, 0xFF]).await;
            return Err(std::io::Error::other(
                "client offered no no-auth method",
            ));
        }
        sock.write_all(&[0x05, 0x00]).await?;

        // Request: VER CMD RSV ATYP ADDR PORT
        let mut req = [0u8; 4];
        sock.read_exact(&mut req).await?;
        if req[0] != 0x05 {
            return Err(std::io::Error::other(format!(
                "bad socks version {:#x} in request",
                req[0]
            )));
        }
        if req[1] != 0x01 {
            // 07 = command not supported (we only implement CONNECT).
            let _ = sock
                .write_all(&[0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await;
            return Err(std::io::Error::other(format!(
                "unsupported socks command {:#x}",
                req[1]
            )));
        }
        let host = match req[3] {
            0x01 => {
                let mut octets = [0u8; 4];
                sock.read_exact(&mut octets).await?;
                std::net::Ipv4Addr::from(octets).to_string()
            }
            0x03 => {
                let mut len = [0u8; 1];
                sock.read_exact(&mut len).await?;
                let mut name = vec![0u8; len[0] as usize];
                sock.read_exact(&mut name).await?;
                String::from_utf8(name).map_err(|_| {
                    std::io::Error::other("non-utf8 socks hostname")
                })?
            }
            0x04 => {
                let mut octets = [0u8; 16];
                sock.read_exact(&mut octets).await?;
                std::net::Ipv6Addr::from(octets).to_string()
            }
            other => {
                // 08 = address type not supported.
                let _ = sock
                    .write_all(&[0x05, 0x08, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                    .await;
                return Err(std::io::Error::other(format!(
                    "unsupported socks address type {other:#x}"
                )));
            }
        };
        let mut port = [0u8; 2];
        sock.read_exact(&mut port).await?;
        Ok((host, u16::from_be_bytes(port)))
    }
}
