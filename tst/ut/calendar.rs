//! Unit tests for the calendar pipeline (moved out of `src/calendar.rs`),
//! re-expressed through the **public** API: `month_times`,
//! `month_iqama_times`, `times_for_date` on synthetic confData values.
//!
//! Covers the row-layout zoo (normal 6-col, Diyanet imsak 7-col, missing
//! shuruq column), the F4 hostile-time contract (now via the `dropped`
//! list, M2), the iqama resolution rules (relative `+N`, absolute, hostile
//! offsets), and the C1 rollover instants.

use chrono::NaiveDate;
use mawaqit_api::{
    ConfData, MawaqitError, month_iqama_times, month_times, times_for_date,
};
use serde_json::{Value, json};

/// Build a ConfData from a raw JSON value (the wire shape).
fn conf(value: Value) -> ConfData {
    serde_json::from_value(value).expect("test builds valid confData")
}

fn month_map(pairs: &[(&str, Vec<&str>)]) -> Value {
    let mut map = serde_json::Map::new();
    for (k, v) in pairs {
        map.insert(
            k.to_string(),
            json!(v.iter().map(|s| s.to_string()).collect::<Vec<_>>()),
        );
    }
    Value::Object(map)
}

fn sample_conf() -> ConfData {
    conf(json!({
        "calendar": [
            month_map(&[("1", vec!["06:30","08:00","13:00","15:30","17:45","19:15"])]),
            month_map(&[("1", vec!["06:20","07:50","12:50","15:10","17:30","19:00"])])
        ],
        "iqamaCalendar": [
            month_map(&[("1", vec!["06:45","+15","13:20","+20","18:00"])])
        ],
        "name": "Test Mosque"
    }))
}

// ------------------------------------------------------ row layout parsing

/// Normal mode via the public path: a 6-column row surfaces as
/// [Fajr, Shuruq, Dhuhr, Asr, Maghrib, Isha].
#[test]
fn parses_normal_mode_row() {
    let r = conf(json!({
        "times": ["06:30", "08:00", "13:00", "15:30", "17:45"],
        "calendar": [
            month_map(&[("1", vec!["06:30","08:00","13:00","15:30","17:45","19:15"])])
        ],
    }));
    let t = &month_times(&r, 1).unwrap().days[0].times;
    assert_eq!(t.fajr, "06:30");
    assert_eq!(t.shurouq, "08:00");
    assert_eq!(t.dhuhr, "13:00");
    assert_eq!(t.isha, "19:15");
}

/// Diyanet imsak mode: 7-column rows, "Sabah" (index 1) is a chip value,
/// the displayed Shurûq is the third column (Güneş).
#[test]
fn parses_imsak_mode_row() {
    let r = conf(json!({
        "times": ["05:27", "06:07", "07:07", "13:21", "16:37", "19:24"],
        "calendar": [
            month_map(&[("1", vec!["05:27","06:37","07:07","13:21","16:37","19:24","20:51"])])
        ],
    }));
    let t = &month_times(&r, 1).unwrap().days[0].times;
    assert_eq!(t.fajr, "05:27"); // displayed as "Imsak"
    assert_eq!(t.shurouq, "07:07"); // Diyanet Güneş
    assert_eq!(t.dhuhr, "13:21");
    assert_eq!(t.asr, "16:37");
    assert_eq!(t.maghrib, "19:24");
    assert_eq!(t.isha, "20:51");
}

/// Rows without a shuruq column take sunrise from the page-level `shuruq`.
#[test]
fn parses_row_without_shuruq_column() {
    let r = conf(json!({
        "times": ["06:09", "13:47", "16:58", "19:45", "21:12"],
        "shuruq": "07:41",
        "calendar": [
            month_map(&[("1", vec!["06:09","13:47","16:58","19:45","21:12"])])
        ],
    }));
    let t = &month_times(&r, 1).unwrap().days[0].times;
    assert_eq!(t.fajr, "06:09");
    assert_eq!(t.shurouq, "07:41");
    assert_eq!(t.dhuhr, "13:47");
    assert_eq!(t.isha, "21:12");
}

// ------------------------------------------------- hostile day rejection

/// Fewer entries than a prayer list: the day is rejected (dropped).
#[test]
fn rejects_short_day() {
    let r = conf(json!({
        "times": ["06:30", "08:00", "13:00", "15:30", "17:45"],
        "calendar": [ month_map(&[("1", vec!["06:30","08:00"])]) ],
    }));
    let m = month_times(&r, 1).unwrap();
    assert!(m.days.is_empty());
    assert_eq!(m.dropped, vec![1]);
}

/// F4: a hostile value anywhere in the row rejects the whole day —
/// month_times drops it and reports it (M2) instead of surfacing
/// fabricated times.
#[test]
fn rejects_days_surfacing_invalid_times() {
    for bad in ["25:70", "99:99", "ab:cd", "7:5", "+30", "24:00"] {
        let mut row =
            vec!["06:30", "08:00", "13:00", "15:30", "17:45", "19:15"];
        row[0] = bad;
        let r = conf(json!({
            "times": ["06:30", "08:00", "13:00", "15:30", "17:45"],
            "calendar": [ month_map(&[("1", row)]) ],
        }));
        let m = month_times(&r, 1).unwrap();
        assert!(m.days.is_empty(), "row with fajr {bad:?} must be rejected");
        assert_eq!(m.dropped, vec![1], "hostile day must be reported");
    }
}

// ------------------------------------------------------ iqama resolution

/// The iqama passthrough cannot smuggle non-display times: "7:5" parses
/// internally (chrono is lenient) but falls back to the adhan value like
/// garbage does. Exercised through the public resolution via a month.
#[test]
fn iqama_passthrough_cannot_smuggle_non_display_times() {
    let r = conf(json!({
        "times": ["06:30", "08:00", "13:00", "15:30", "17:45", "19:15"],
        "calendar": [
            month_map(&[("1", vec!["06:30","08:00","13:00","15:30","17:45","19:15"])])
        ],
        "iqamaCalendar": [ month_map(&[("1", vec!["7:5","25:70","06:45","+x","!"])]) ],
    }));
    let iq = &month_iqama_times(&r, 1).unwrap().days[0].times;
    assert_eq!(iq.fajr, "06:30", "lenient 7:5 falls back to adhan");
    assert_eq!(iq.dhuhr, "13:00", "hostile 25:70 falls back to adhan");
    assert_eq!(iq.asr, "06:45", "absolute passthrough kept");
}

/// Relative "+N" expands against the adhan; absolute "HH:MM" stays as-is.
#[test]
fn resolves_relative_and_absolute_iqama() {
    let r = conf(json!({
        "times": ["06:30", "08:00", "13:00", "15:30", "17:45", "19:15"],
        "calendar": [
            month_map(&[("1", vec!["06:30","08:00","13:00","15:30","17:45","19:15"])])
        ],
        "iqamaCalendar": [
            month_map(&[("1", vec!["06:45","+15","13:20","+20","18:00"])])
        ],
    }));
    let iq = &month_iqama_times(&r, 1).unwrap().days[0].times;
    assert_eq!(iq.fajr, "06:45");
    assert_eq!(iq.dhuhr, "13:15"); // +15 after 13:00
    assert_eq!(iq.asr, "13:20"); // absolute value kept
    assert_eq!(iq.maghrib, "18:05"); // 17:45 + 20 minutes
    assert_eq!(iq.isha, "18:00");
}

/// Any unparseable iqama entry falls back to the adhan time.
#[test]
fn invalid_iqama_falls_back_to_adhan() {
    let r = conf(json!({
        "times": ["06:30", "08:00", "13:00", "15:30", "17:45", "19:15"],
        "calendar": [
            month_map(&[("1", vec!["06:30","08:00","13:00","15:30","17:45","19:15"])])
        ],
        "iqamaCalendar": [ month_map(&[("1", vec!["garbage","+abc","+abc","garbage","+abc"])]) ],
    }));
    let adhan = &month_times(&r, 1).unwrap().days[0].times;
    let iq = &month_iqama_times(&r, 1).unwrap().days[0].times;
    assert_eq!(iq.fajr, adhan.fajr);
    assert_eq!(iq.dhuhr, adhan.dhuhr);
    assert_eq!(iq.maghrib, adhan.maghrib);
}

/// Hostile iqama offsets never panic and clamp to one day (red-team
/// finding: TimeDelta::minutes(i64::MAX) used to panic). The clamped
/// +24 h rollover shows up as the same wall clock, next day.
#[test]
fn hostile_iqama_offsets_do_not_panic() {
    let adhan_row = vec!["06:30", "08:00", "13:00", "15:30", "17:45", "19:15"];
    let r = conf(json!({
        "times": ["06:30", "08:00", "13:00", "15:30", "17:45", "19:15"],
        "calendar": [ month_map(&[("1", adhan_row)]) ],
        "iqamaCalendar": [
            month_map(&[("1", vec!["+9223372036854775807", "+999999999999999", "+0", "+1440", "++5"])])
        ],
    }));
    let day = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    let today = times_for_date(&r, day).unwrap();
    let iq = today.iqama.expect("iqama present");
    // clamped to +24h -> next-day 17:45 (same wall clock, rolled day)
    assert_eq!(iq.maghrib, "17:45");
    let at = today.iqama_at.expect("instants");
    assert_eq!(
        at.maghrib,
        day.succ_opt().unwrap().and_hms_opt(17, 45, 0).unwrap()
    );
    // one '+' is stripped and "+5" parses: 19:15 + 5 = 19:20
    assert_eq!(iq.isha, "19:20");
}

// ------------------------------------------------------ month extraction

#[test]
fn extracts_month_and_iqama() {
    let r = sample_conf();
    let m = month_times(&r, 1).unwrap();
    assert_eq!(m.days.len(), 1);
    assert_eq!(m.days[0].times.dhuhr, "13:00");
    assert!(m.dropped.is_empty());

    let mi = month_iqama_times(&r, 1).unwrap();
    assert_eq!(mi.days[0].times.dhuhr, "13:15");
}

#[test]
fn rejects_invalid_month() {
    let r = sample_conf();
    assert!(matches!(month_times(&r, 0), Err(MawaqitError::InvalidMonth(0))));
    assert!(matches!(month_times(&r, 13), Err(MawaqitError::InvalidMonth(13))));
}

// ------------------------------------------------------------- today view

#[test]
fn finds_today() {
    let r = sample_conf();
    let today =
        times_for_date(&r, NaiveDate::from_ymd_opt(2026, 2, 1).unwrap())
            .unwrap();
    assert_eq!(today.adhan.fajr, "06:20");
    // no iqama calendar entry for February -> iqama is None, adhan still
    // works
    assert!(today.iqama.is_none());

    let first =
        times_for_date(&r, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap())
            .unwrap();
    assert_eq!(first.adhan.fajr, "06:30");
    assert_eq!(first.iqama.unwrap().dhuhr, "13:15");
}

/// C1: "+600" after a 23:30 adhan belongs to the NEXT day — the display
/// string shows 09:30 either way, the instant says when.
#[test]
fn iqama_rollover_instants_carry_the_next_day() {
    let r = conf(json!({
        "times": ["23:30", "00:00", "00:00", "00:00", "00:00"],
        "calendar": [
            month_map(&[("1", vec!["23:30", "23:45", "23:50", "23:55", "23:58", "23:59"])])
        ],
        "iqamaCalendar": [
            month_map(&[("1", vec!["+600", "+600", "+600", "+600", "+600"])])
        ],
    }));
    let today =
        times_for_date(&r, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap())
            .unwrap();
    assert_eq!(today.iqama.as_ref().unwrap().fajr, "09:30");
    let at = today.iqama_at.expect("rollover instants present");
    let next = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
    assert_eq!(at.fajr, next.and_hms_opt(9, 30, 0).unwrap());
    assert_eq!(at.isha, next.and_hms_opt(9, 59, 0).unwrap());
}

#[test]
fn iqama_instants_stay_same_day_for_absolute_and_small_offsets() {
    let day = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    let today = times_for_date(&sample_conf(), day).unwrap();
    let at = today.iqama_at.expect("instants present");
    assert_eq!(at.fajr, day.and_hms_opt(6, 45, 0).unwrap()); // absolute
    assert_eq!(at.dhuhr, day.and_hms_opt(13, 15, 0).unwrap()); // +15
}

/// M2: a day rejected as malformed is reported in `dropped` and errors as
/// `InvalidDay`; a day never on the wire stays `NoCalendar`.
#[test]
fn dropped_days_are_reported_and_error_as_invalid_day() {
    let r = conf(json!({
        "times": ["06:30", "08:00", "13:00", "15:30", "17:45"],
        "calendar": [ month_map(&[
            // hostile row: rejected whole (F4) but reported (M2)
            ("1", vec!["25:70", "06:37", "13:21", "16:37", "19:24", "20:51"]),
            ("2", vec!["06:30", "08:00", "13:00", "15:30", "17:45", "19:15"]),
        ]) ]
    }));
    let month = month_times(&r, 1).unwrap();
    assert_eq!(month.days.len(), 1, "only the valid day surfaces");
    assert_eq!(month.dropped, vec![1], "the hostile day is reported");

    let err = times_for_date(&r, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap())
        .unwrap_err();
    assert!(matches!(err, MawaqitError::InvalidDay(1)), "{err}");
    // A day that was never on the wire stays NoCalendar.
    let err = times_for_date(&r, NaiveDate::from_ymd_opt(2026, 1, 7).unwrap())
        .unwrap_err();
    assert!(matches!(err, MawaqitError::NoCalendar), "{err}");
}
