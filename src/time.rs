//! Domain-core time policy: strict `HH:MM` display validation (ADR-0010)
//! and the pure time arithmetic every tier can use — no allocator, no I/O.
//! Part of the domain layer that compiles all the way down to `#![no_std]`
//! bare metal (ADR-0014).

use chrono::NaiveTime;

/// Parse a strict or lenient `HH:MM` wall-clock string. Lenient parses
/// (`7:5`, leading spaces) are fine for internal time math, never for
/// surfaced strings — those go through [`is_displayable_hhmm`].
pub(crate) fn parse_hhmm(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

/// Exactly `HH:MM` with in-range values — the display contract the
/// red-team suite pins (F4). Lenient parses (`7:5`, leading spaces) are
/// fine for internal time math, never for surfaced strings.
#[cfg(any(feature = "std", feature = "alloc"))]
pub(crate) fn is_displayable_hhmm(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5
        && b[2] == b':'
        && b[..2].iter().all(|c| c.is_ascii_digit())
        && b[3..].iter().all(|c| c.is_ascii_digit())
        && s[..2].parse::<u8>().is_ok_and(|h| h < 24)
        && s[3..].parse::<u8>().is_ok_and(|m| m < 60)
}

/// Convenience: minutes between two "HH:MM" times (b-a), handling midnight
/// wrap.
///
/// # Examples
///
/// ```rust
/// use mawaqit_api::minutes_between;
///
/// assert_eq!(minutes_between("05:30", "06:00"), Some(30));
/// // Midnight wrap: 23:30 -> 00:15 is 45 minutes into the next day.
/// assert_eq!(minutes_between("23:30", "00:15"), Some(45));
/// assert_eq!(minutes_between("25:70", "00:15"), None);
/// ```
pub fn minutes_between(a: &str, b: &str) -> Option<i64> {
    let a = parse_hhmm(a)?;
    let b = parse_hhmm(b)?;
    let diff = (b - a).num_minutes();
    Some(if diff < 0 { diff + 24 * 60 } else { diff })
}
