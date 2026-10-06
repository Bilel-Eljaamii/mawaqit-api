//! Unit tier (`ut`): the library's pure core — page parsing and calendar
//! semantics — against adversarial input, with no I/O of any kind. No
//! sockets, no files, no clock: everything here is deterministic and runs
//! in milliseconds, so it gates every `cargo test` and every commit.
//!
//! - [`cache`] — the in-process TTL cache: TTL expiry, FIFO cap eviction,
//!   stale-order robustness (via the `#[doc(hidden)]` internal export).
//! - [`calendar`] — row layouts (normal/imsak/no-shuruq), hostile-day rejection
//!   + `dropped` reporting, iqama resolution, rollover instants.
//! - [`client`] — pure helper contracts: `minutes_between`, slug bounds, SOCKS5
//!   proxy validation through the public builders.
//! - [`compact`] — the MQTC codec: roundtrips in both record formats,
//!   CRC/hostile-input rejection (never panic), rollover bitfield, Jumu'ah
//!   header, year boundaries, payload emitters.
//! - [`scraper`] — page → confData extraction on synthetic pages.
//! - [`corpus`] — hostile corpus for `parse_page`: truncations, byte flips,
//!   lookalike assignments, oversized/unicode torture. Nothing may panic.
//! - [`semantics`] — attacks with *valid* JSON that try to lie: imsak-mode
//!   inference, exotic day keys, `+N` rollover, display-field collapse; the
//!   red-team findings are regression-pinned green here.
//! - [`voices`] — the adhan voice catalog: URL building, catalog integrity,
//!   page-to-catalog validation.
//!
//! `cargo test --test ut`

#[path = "ut/cache.rs"]
mod cache;
#[path = "ut/calendar.rs"]
mod calendar;
#[path = "ut/client.rs"]
mod client;
mod common;
#[path = "ut/compact.rs"]
mod compact;
#[path = "ut/corpus.rs"]
mod corpus;
#[path = "ut/prayer.rs"]
mod prayer;
#[path = "ut/scraper.rs"]
mod scraper;
#[path = "ut/semantics.rs"]
mod semantics;
#[path = "ut/time.rs"]
mod time;
#[path = "ut/voices.rs"]
mod voices;
