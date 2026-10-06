//! # mawaqit-api
//!
//! Keyless Rust client for [mawaqit.net](https://mawaqit.net) prayer times —
//! no account required.
//!
//! - Mosque search: public endpoint `GET /api/2.0/mosque/search?word=…`
//! - Prayer data: the public mosque page `https://mawaqit.net/{lang}/{slug}`
//!   embeds a `confData` object with the daily times, the year calendar, the
//!   iqama calendar and the mosque metadata; one fetch serves every method.
//!
//! ```no_run
//! # async fn example() -> Result<(), mawaqit_api::MawaqitError> {
//! let client = mawaqit_api::MawaqitClient::new();
//!
//! let mosques = client.search_mosques("Paris").await?;
//! // Search results may lack a page slug — don't index-and-unwrap.
//! let slug = mosques.iter().find_map(|m| m.mosque_id())
//!     .expect("search returned no slug — pick another result")
//!     .to_string();
//!
//! let today = client.today(&slug).await?;
//! println!("{} — Fajr at {} (iqama {})", mosques[0].display_name(),
//!     today.adhan.fajr,
//!     today.iqama.as_ref().map(|i| i.fajr.as_str()).unwrap_or("?"));
//! # Ok(())
//! # }
//! ```
//!
//! Iqama entries of the form `"+15"` (minutes after the adhan) are resolved
//! to absolute `HH:MM` times automatically; [`today`](MawaqitClient::today)
//! also reports `iqama_at`, the rollover-correct instants — a "+600" after
//! a 23:30 adhan belongs to the *next* day.

#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(feature = "alloc")]
extern crate alloc;

/// Internal in-process TTL cache. `#[doc(hidden)]`-public only so its unit
/// tests live in `tst/ut/cache.rs`; not part of the public API, not covered
/// by semver.
#[doc(hidden)]
#[cfg(feature = "std")]
pub mod cache;
#[cfg(any(feature = "std", feature = "alloc"))]
mod calendar;
#[cfg(feature = "std")]
mod client;
#[cfg(feature = "heapless")]
pub mod compact;
#[cfg(feature = "std")]
pub mod disk;
mod error;
#[cfg(any(feature = "std", feature = "alloc"))]
mod models;
/// Consumer-facing prayer identity + next-event resolution (issue #4) —
/// core types usable at every tier down to bare metal.
pub mod prayer;
/// The shared free-text sanitizer (ADR-0003) — one character policy at
/// every ingress: page parse, search results, disk snapshot load.
#[cfg(any(feature = "std", feature = "alloc"))]
mod sanitize;
#[cfg(any(feature = "std", feature = "alloc"))]
mod scraper;
/// Domain-core slug policy (ADR-0014) — pure, compiles in `core`.
mod slug;
/// Domain-core time policy (ADR-0014) — pure, compiles in `core`.
mod time;
/// Embedded Tor (the opt-in `builtin-tor` feature): a local SOCKS5
/// listener over an embedded Arti client — ADR-0012 as amended.
#[cfg(feature = "builtin-tor")]
pub mod tor;
pub mod voices;

#[cfg(any(feature = "std", feature = "alloc"))]
pub use calendar::{month_iqama_times, month_times, times_for_date};
#[cfg(feature = "std")]
pub use client::MawaqitClient;
#[cfg(feature = "heapless")]
pub use compact::*;
pub use error::{MawaqitError, Result};
#[cfg(any(feature = "std", feature = "alloc"))]
pub use models::{
    Announcement, ConfData, DailyIqamaInstants, DailyIqamaTimes,
    DailyPrayerTimes, DayIqamaTimes, DayTimes, MonthIqamaTimes, MonthTimes,
    Mosque, RawCalendar, RawMonth, TodayTimes,
};
pub use prayer::{Prayer, PrayerEvent, PrayerEventKind};
/// Parse a mosque page's HTML into its [`ConfData`] — exposed for tests
/// and fuzzing; [`MawaqitClient::conf_data`] is the network-backed
/// wrapper.
#[cfg(any(feature = "std", feature = "alloc"))]
pub use scraper::extract_conf_data as parse_page;
pub use slug::is_valid_slug;
#[cfg(any(feature = "std", feature = "alloc"))]
pub use slug::page_url;
pub use time::{
    MAX_NOTIFY_BEFORE_MIN, is_due, minutes_before, minutes_between,
};
#[cfg(feature = "builtin-tor")]
pub use tor::{BUILTIN_TOR_PORT, BuiltinTor, BuiltinTorStatus, StartMode};
#[cfg(feature = "std")]
pub use voices::download_voice;
pub use voices::{ADHAN_VOICES, AdhanVoice};
#[cfg(any(feature = "std", feature = "alloc"))]
pub use voices::{adhan_voice_url, voice_id_from_conf};
