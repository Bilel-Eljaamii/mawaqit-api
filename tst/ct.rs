//! Component tier (`ct`): one component at a time, exercised through its
//! real I/O boundary. Sockets and files are in, the live site is out —
//! every dependency outside the component under test is a local mock or a
//! temp dir, so this tier is offline, deterministic and CI-safe.
//!
//! - [`hostile_http`] — the client against a raw-TCP mock server that lies:
//!   garbage bodies, wrong statuses, redirects, hostile redirects; plus the
//!   open transport findings (run `--ignored`).
//! - [`disk_cache`] — the offline snapshot layer: store/refresh/fallback
//!   contract with a mock server and a dead base URL.
//!
//! `cargo test --test ct`

mod common;
#[path = "ct/disk_cache.rs"]
mod disk_cache;
#[path = "ct/hostile_http.rs"]
mod hostile_http;
