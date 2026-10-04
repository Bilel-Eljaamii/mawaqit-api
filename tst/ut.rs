//! Unit tier (`ut`): the library's pure core — page parsing and calendar
//! semantics — against adversarial input, with no I/O of any kind. No
//! sockets, no files, no clock: everything here is deterministic and runs
//! in milliseconds, so it gates every `cargo test` and every commit.
//!
//! - [`corpus`] — hostile corpus for `parse_page`: truncations, byte flips,
//!   lookalike assignments, oversized/unicode torture. Nothing may panic.
//! - [`semantics`] — attacks with *valid* JSON that try to lie: imsak-mode
//!   inference, exotic day keys, `+N` rollover, display-field collapse;
//!   includes the open red-team findings (run `--ignored`).
//!
//! `cargo test --test ut`

mod common;
#[path = "ut/corpus.rs"]
mod corpus;
#[path = "ut/semantics.rs"]
mod semantics;
