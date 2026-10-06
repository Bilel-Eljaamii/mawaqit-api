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

/// Sanity cap for notification lead times that arrive from attacker-writable
/// config files (issue #4, P3): a `u16::MAX` "notify me before Fajr" must
/// behave as this cap, never as a 45-day countdown.
///
/// # Examples
///
/// ```rust
/// use mawaqit_api::MAX_NOTIFY_BEFORE_MIN;
///
/// assert_eq!(MAX_NOTIFY_BEFORE_MIN, 120);
/// ```
pub const MAX_NOTIFY_BEFORE_MIN: u16 = 120;

/// True when `now` falls within the first minute after `hhmm` — the alert
/// window a minute-tick loop (60 s cadence) uses so every prayer fires
/// exactly once. Hostile input is inert: an unparsable `hhmm` is never due,
/// never a misparse.
///
/// # Examples
///
/// ```rust
/// use chrono::NaiveTime;
/// use mawaqit_api::is_due;
///
/// let now = NaiveTime::from_hms_opt(13, 0, 30).unwrap();
/// assert!(is_due(now, "13:00"));
/// assert!(!is_due(now, "12:59"));
/// // Hostile strings never fire.
/// assert!(!is_due(now, "25:70"));
/// ```
pub fn is_due(now: NaiveTime, hhmm: &str) -> bool {
    match NaiveTime::parse_from_str(hhmm, "%H:%M") {
        Ok(t) => {
            let elapsed = (now - t).num_seconds();
            (0..60).contains(&elapsed)
        }
        Err(_) => false,
    }
}

/// The instant `minutes` before `hhmm` — the pre-notification target.
/// Wraps across midnight (a 00:05 prayer with a 30-minute heads-up alerts
/// at 23:35). Hostile input yields `None` — never a misparse.
///
/// Core tier: returns a [`NaiveTime`], not a display string — formatting is
/// the caller's `.format("%H:%M")`.
///
/// # Examples
///
/// ```rust
/// use mawaqit_api::minutes_before;
///
/// let t = minutes_before("06:30", 5).unwrap();
/// assert_eq!(t.format("%H:%M").to_string(), "06:25");
/// // Midnight wrap: a 00:05 Fajr pre-alerts at 23:35.
/// let t = minutes_before("00:05", 30).unwrap();
/// assert_eq!(t.format("%H:%M").to_string(), "23:35");
/// assert!(minutes_before("25:70", 5).is_none());
/// ```
pub fn minutes_before(hhmm: &str, minutes: u16) -> Option<NaiveTime> {
    let t = NaiveTime::parse_from_str(hhmm, "%H:%M").ok()?;
    Some(t - chrono::Duration::minutes(i64::from(minutes)))
}
