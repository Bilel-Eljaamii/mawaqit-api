// `str::to_string` lives in the prelude under std only; the alloc tier
// (no_std) needs the trait in scope for `bounded`.
#[cfg(all(feature = "alloc", not(feature = "std")))]
use alloc::string::ToString;

use thiserror::Error;

#[cfg(feature = "heapless")]
use crate::compact::CompactError;

/// Primary error type for all mawaqit-api operations.
#[derive(Debug, Error)]
pub enum MawaqitError {
    #[cfg(feature = "std")]
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[cfg(any(feature = "std", feature = "alloc"))]
    #[error("mosque not found: {0}")]
    MosqueNotFound(alloc::string::String),

    #[cfg(not(any(feature = "std", feature = "alloc")))]
    #[error("mosque not found")]
    MosqueNotFound,

    #[cfg(any(feature = "std", feature = "alloc"))]
    #[error(
        "confData not found in the page of {0}: the page layout may have changed"
    )]
    ConfDataNotFound(alloc::string::String),

    #[cfg(not(any(feature = "std", feature = "alloc")))]
    #[error("confData not found")]
    ConfDataNotFound,

    #[error("month must be between 1 and 12, got {0}")]
    InvalidMonth(u32),

    #[error("no calendar data for this mosque")]
    NoCalendar,

    #[error("calendar day {0} was rejected as malformed and surfaces no times")]
    InvalidDay(u32),

    #[cfg(any(feature = "std", feature = "alloc"))]
    #[error("unexpected response (HTTP {status}) from {url}")]
    Api { status: u16, url: alloc::string::String },

    #[cfg(not(any(feature = "std", feature = "alloc")))]
    #[error("unexpected API response (HTTP {status})")]
    Api { status: u16 },

    #[cfg(any(feature = "std", feature = "alloc"))]
    #[error("invalid SOCKS5 proxy address: {0}")]
    InvalidProxy(alloc::string::String),

    #[cfg(not(any(feature = "std", feature = "alloc")))]
    #[error("invalid SOCKS5 proxy address")]
    InvalidProxy,

    #[cfg(any(feature = "std", feature = "alloc"))]
    #[error("invalid adhan voice: {0}")]
    InvalidVoice(alloc::string::String),

    #[cfg(not(any(feature = "std", feature = "alloc")))]
    #[error("invalid adhan voice")]
    InvalidVoice,

    #[cfg(any(feature = "std", feature = "alloc"))]
    #[error("malformed payload: {0}")]
    Parse(alloc::string::String),

    #[cfg(not(any(feature = "std", feature = "alloc")))]
    #[error("malformed payload")]
    Parse,

    /// F29: a search word beyond the 128-byte wire bound is refused before
    /// it can become a giant cache key or a giant request URL.
    #[cfg(feature = "std")]
    #[error("search word exceeds the 128 byte limit")]
    SearchWordTooLong,

    #[cfg(feature = "heapless")]
    #[error("compact binary error: {0:?}")]
    Compact(CompactError),
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
