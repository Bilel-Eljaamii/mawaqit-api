//! Component tier (`ct`): one component at a time, exercised through its
//! real I/O boundary. Sockets and files are in, the live site is out —
//! every dependency outside the component under test is a local mock or a
//! temp dir, so this tier is offline, deterministic and CI-safe.
//!
//! - [`hostile_http`] — the client against a raw-TCP mock server that lies:
//!   garbage bodies, wrong statuses, redirects, hostile redirects; the red-team
//!   transport findings are regression-pinned green here.
//! - [`disk_cache`] — the offline snapshot layer through the client:
//!   store/refresh/fallback contract with a mock server and a dead base URL.
//! - [`disk`] — the snapshot store itself: round-trips, hostile-file
//!   degradation, slug-confinement (moved out of `src/disk.rs`).
//!
//! `cargo test --test ct`

mod common;
#[path = "ct/disk.rs"]
mod disk;
#[path = "ct/disk_cache.rs"]
mod disk_cache;
#[path = "ct/hostile_http.rs"]
mod hostile_http;
