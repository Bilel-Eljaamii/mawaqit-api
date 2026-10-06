// `str::to_string` lives in the prelude under std only; the alloc tier
// (no_std) needs the trait in scope for `bounded`.
#[cfg(all(feature = "alloc", not(feature = "std")))]
use alloc::string::ToString;

use thiserror::Error;

#[cfg(feature = "heapless")]
use crate::compact::CompactError;

/// Primary error type for all mawaqit-api operations.
///
/// Every variant is a *degradation*, never a fabrication (ADR-0003): when
/// an operation fails, no hostile value has surfaced as prayer data. All
/// payload-bearing variants carry sanitized, length-bounded strings (F29)
/// so `Display` output is safe for logs and tray toasts.
#[derive(Debug, Error)]
pub enum MawaqitError {
    /// Transport failure — DNS, connect, TLS or timeout, before any HTTP
    /// status existed. The root cause is in the reqwest error's `source`
    /// chain, not its `Display`.
    #[cfg(feature = "std")]
    #[error("HTTP request failed: {0}")]
    Http(
        /// The underlying reqwest error — walk its `source` chain for the
        /// transport-level cause.
        #[from]
        reqwest::Error,
    ),

    /// The mosque page 404'd under its (possibly defused) slug — no such
    /// mosque, or the slug left the mosque namespace and was fetched as a
    /// placeholder (F2).
    #[cfg(any(feature = "std", feature = "alloc"))]
    #[error("mosque not found: {0}")]
    MosqueNotFound(
        /// The slug that was not found — sanitized and bounded (F29).
        alloc::string::String,
    ),

    /// `no_std` form of [`MawaqitError::MosqueNotFound`].
    #[cfg(not(any(feature = "std", feature = "alloc")))]
    #[error("mosque not found")]
    MosqueNotFound,

    /// The page fetched fine but carried no parseable `confData` literal —
    /// a layout change upstream, or a page that only looks like a mosque
    /// page.
    #[cfg(any(feature = "std", feature = "alloc"))]
    #[error(
        "confData not found in the page of {0}: the page layout may have changed"
    )]
    ConfDataNotFound(
        /// The slug of the page that carried no confData — sanitized and
        /// bounded (F29).
        alloc::string::String,
    ),

    /// `no_std` form of [`MawaqitError::ConfDataNotFound`].
    #[cfg(not(any(feature = "std", feature = "alloc")))]
    #[error("confData not found")]
    ConfDataNotFound,

    /// A month outside `1..=12` was requested; nothing was fetched or
    /// parsed.
    #[error("month must be between 1 and 12, got {0}")]
    InvalidMonth(
        /// The out-of-range month that was requested.
        u32,
    ),

    /// The mosque publishes no calendar at all (or the requested month/day
    /// is genuinely absent from it) — the honest "no data" answer.
    #[error("no calendar data for this mosque")]
    NoCalendar,

    /// The day exists on the wire but was rejected as malformed (F4) and
    /// surfaces no times — the degraded-but-honest counterpart of
    /// [`MawaqitError::NoCalendar`].
    #[error("calendar day {0} was rejected as malformed and surfaces no times")]
    InvalidDay(
        /// The day of month the wire carried but the pipeline rejected.
        u32,
    ),

    /// An HTTP response outside 2xx (redirects are never followed — F1 —
    /// so a 3xx surfaces here). `url` is the request URL that failed.
    #[cfg(any(feature = "std", feature = "alloc"))]
    #[error("unexpected response (HTTP {status}) from {url}")]
    Api {
        /// The HTTP status code the server answered with.
        status: u16,
        /// The request URL that produced it.
        url: alloc::string::String,
    },

    /// `no_std` form of [`MawaqitError::Api`].
    #[cfg(not(any(feature = "std", feature = "alloc")))]
    #[error("unexpected API response (HTTP {status})")]
    Api { status: u16 },

    /// [`crate::MawaqitClient::with_socks_proxy`] refused the address —
    /// scheme not `socks5h`, or a path/query/fragment on a proxy address
    /// (ADR-0012). `0` echoes the (bounded) input.
    #[cfg(any(feature = "std", feature = "alloc"))]
    #[error("invalid SOCKS5 proxy address: {0}")]
    InvalidProxy(
        /// The rejected proxy address — sanitized and bounded (F29).
        alloc::string::String,
    ),

    /// `no_std` form of [`MawaqitError::InvalidProxy`].
    #[cfg(not(any(feature = "std", feature = "alloc")))]
    #[error("invalid SOCKS5 proxy address")]
    InvalidProxy,

    /// A voice id is not in the static catalog, or a downloaded/cached
    /// voice file violates the size contract. `0` echoes the (bounded) id
    /// or diagnostic.
    #[cfg(any(feature = "std", feature = "alloc"))]
    #[error("invalid adhan voice: {0}")]
    InvalidVoice(
        /// The rejected voice id, or the size/diagnostic text — sanitized
        /// and bounded (F29).
        alloc::string::String,
    ),

    /// `no_std` form of [`MawaqitError::InvalidVoice`].
    #[cfg(not(any(feature = "std", feature = "alloc")))]
    #[error("invalid adhan voice")]
    InvalidVoice,

    /// A wire payload was malformed (HTML, JSON, UTF-8) or violated a
    /// transport bound (response over the streaming cap). `0` carries a
    /// bounded diagnostic, never the raw payload.
    #[cfg(any(feature = "std", feature = "alloc"))]
    #[error("malformed payload: {0}")]
    Parse(
        /// A bounded diagnostic describing what was malformed — never the
        /// raw payload itself.
        alloc::string::String,
    ),

    /// `no_std` form of [`MawaqitError::Parse`].
    #[cfg(not(any(feature = "std", feature = "alloc")))]
    #[error("malformed payload")]
    Parse,

    /// F29: a search word beyond the 128-byte wire bound is refused before
    /// it can become a giant cache key or a giant request URL.
    #[cfg(feature = "std")]
    #[error("search word exceeds the 128 byte limit")]
    SearchWordTooLong,

    /// A packed MQTC payload or packer input failed its contract
    /// (`#![no_std]` firmware path); see [`CompactError`].
    #[cfg(feature = "heapless")]
    #[error("compact binary error: {0:?}")]
    Compact(
        /// The MQTC codec failure — see [`CompactError`].
        CompactError,
    ),
}

/// Convenience alias for `core::result::Result<T, MawaqitError>`.
pub type Result<T> = core::result::Result<T, MawaqitError>;

/// Payload bound for variants that echo an *identifier* (a search word, a
/// mosque slug, a voice id, a proxy address) — FINDING F29.
#[cfg(any(feature = "std", feature = "alloc"))]
pub(crate) const BOUNDED_ID: usize = 128;
/// Payload bound for variants that echo a *diagnostic* (a serde message, a
/// rejected wire value) — FINDING F29.
#[cfg(any(feature = "std", feature = "alloc"))]
pub(crate) const BOUNDED_DIAGNOSTIC: usize = 256;

/// FINDING F29: payload-bearing error variants never carry raw hostile
/// strings — network words, slugs, ids and wire values are sanitized
/// (invisible characters stripped) and truncated at construction, so
/// `Display` output (logs, tray toasts, terminals) stays bounded no matter
/// the input.
#[cfg(any(feature = "std", feature = "alloc"))]
pub(crate) fn bounded(s: &str, max: usize) -> alloc::string::String {
    let clean = crate::sanitize::text(s);
    if clean.len() <= max {
        return clean;
    }
    let mut end = max;
    while !clean.is_char_boundary(end) {
        end -= 1;
    }
    clean[..end].to_string()
}
