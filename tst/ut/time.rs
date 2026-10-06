//! Core-tier time-math pins (issue #4, P3): `is_due`, `minutes_before` and
//! the notify-before cap, promoted from mawaqit-desktop's
//! `src-tauri/src/application/prayer_logic.rs`. The hostile strings below
//! are attacker-influenced wire values — they must stay inert.

use chrono::NaiveTime;
use mawaqit_api::{MAX_NOTIFY_BEFORE_MIN, is_due, minutes_before};

fn now(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap()
}

/// The one-tick window: due only in the 60 seconds after the target.
#[test]
fn p3_is_due_only_within_the_minute() {
    let at_target = now(13, 0);
    let thirty_s_in = NaiveTime::from_hms_opt(13, 0, 30).unwrap();
    let next_minute = now(13, 1);
    assert!(is_due(at_target, "13:00"));
    assert!(is_due(thirty_s_in, "13:00"));
    assert!(!is_due(next_minute, "13:00"), "a full minute later is not due");
    assert!(!is_due(now(12, 59), "13:00"));
}

/// Hostile wire shapes must be fully inert: never due, never misparsed
/// into a rollover.
#[test]
fn p3_hostile_time_shapes_are_inert() {
    let n = now(13, 0);
    let inert = [
        "25:70",    // out-of-range rollover bait
        "99:99",    // classic garbage
        "+30",      // iqama-style offset in an adhan slot
        "",         // empty
        "١٣:٠٠",    // Arabic-Indic digits
        "13:00:00", // seconds sneak in
        "13:00\n",  // trailing newline (string smuggling from JSON)
    ];
    for t in inert {
        assert!(!is_due(n, t), "{t:?} must never be due");
        assert!(minutes_before(t, 5).is_none(), "{t:?} must stay inert");
    }
}

/// Lenient-but-accurate shapes resolve to their face value — the property
/// that matters when the strings come from the wire.
#[test]
fn p3_lenient_but_accurate_shapes_resolve_to_face_value() {
    let expected = NaiveTime::from_hms_opt(7, 5, 0).unwrap();
    assert_eq!(minutes_before("7:5", 0), Some(expected));
    let expected = NaiveTime::from_hms_opt(13, 0, 0).unwrap();
    assert_eq!(minutes_before(" 13:00", 0), Some(expected));
}

/// Midnight boundaries: 00:00 fires just after midnight, and the last
/// second of the day still catches 23:59.
#[test]
fn p3_day_boundary_alerts_stay_accurate() {
    let just_after = NaiveTime::from_hms_opt(0, 0, 30).unwrap();
    assert!(is_due(just_after, "00:00"));
    assert!(!is_due(just_after, "23:59"));
    let last = NaiveTime::from_hms_opt(23, 59, 59).unwrap();
    assert!(is_due(last, "23:59"));
}

#[test]
fn p3_minutes_before_lands_n_minutes_earlier() {
    let fmt = |t: Option<NaiveTime>| t.map(|t| t.format("%H:%M").to_string());
    assert_eq!(fmt(minutes_before("06:30", 5)).as_deref(), Some("06:25"));
    assert_eq!(fmt(minutes_before("06:30", 1)).as_deref(), Some("06:29"));
    assert_eq!(fmt(minutes_before("06:30", 0)).as_deref(), Some("06:30"));
}

/// A Fajr at 00:05 with a 30-minute heads-up is due at 23:35.
#[test]
fn p3_minutes_before_wraps_across_midnight() {
    let fmt = |t: Option<NaiveTime>| t.map(|t| t.format("%H:%M").to_string());
    assert_eq!(fmt(minutes_before("00:05", 30)).as_deref(), Some("23:35"));
    assert_eq!(fmt(minutes_before("00:00", 1)).as_deref(), Some("23:59"));
}

/// The cap is a compile-time fact and a runtime no-op at the bound: the
/// config is attacker-writable; `u16::MAX` must behave as the cap, never
/// as a 45-day countdown.
#[test]
fn p3_notify_before_cap_is_sane() {
    const _: () = assert!(MAX_NOTIFY_BEFORE_MIN <= 24 * 60);
    assert!(minutes_before("06:30", MAX_NOTIFY_BEFORE_MIN).is_some());
}
