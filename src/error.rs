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

    #[cfg(feature = "heapless")]
    #[error("compact binary error: {0:?}")]
    Compact(CompactError),
}

/// Convenience alias for `core::result::Result<T, MawaqitError>`.
pub type Result<T> = core::result::Result<T, MawaqitError>;
