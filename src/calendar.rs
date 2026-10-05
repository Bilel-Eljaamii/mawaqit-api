use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Timelike};

use crate::{
    error::{MawaqitError, Result},
    models::{
        ConfData, DailyIqamaInstants, DailyIqamaTimes, DailyPrayerTimes,
        DayIqamaTimes, DayTimes, MonthIqamaTimes, MonthTimes, RawCalendar,
        TodayTimes,
    },
};

pub(crate) fn parse_hhmm(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

fn time_string(t: NaiveTime) -> String {
    t.format("%H:%M").to_string()
}

/// Build one day's times from a calendar day row.
///
/// Calendar rows carry a shuruq (sunrise) column at index 1:
/// `[first_prayer, shuruq, rest…]`. The number of prayers varies by mosque:
/// - normal mode (5 prayers): `[Fajr, Shuruq, Dhuhr, Asr, Maghrib, Isha]`
/// - imsak mode (6 prayers, `displayingSabahImsak`, e.g. DİTİB mosques):
///   `[İmsak, Sabah, Shurûq, Dhuhr, Asr, Maghrib, Isha]` — the displayed Shurûq
///   is the third column (Diyanet Güneş); "Sabah" (index 1) is an extra chip
///   value not used as a prayer time.
///
/// Rows without the shuruq column are treated as plain prayer lists, with
/// sunrise taken from the page-level `shuruq` field.
///
/// Exactly `HH:MM` with in-range values — the display contract the red-team
/// suite pins (F4). Lenient parses (`7:5`, leading spaces) are fine for
/// internal time math, never for surfaced strings.
pub(crate) fn is_displayable_hhmm(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5
        && b[2] == b':'
        && b[..2].iter().all(|c| c.is_ascii_digit())
        && b[3..].iter().all(|c| c.is_ascii_digit())
        && s[..2].parse::<u8>().is_ok_and(|h| h < 24)
        && s[3..].parse::<u8>().is_ok_and(|m| m < 60)
}

pub(crate) fn daily_from_row(
    row: &[String],
    conf_shuruq: Option<&str>,
) -> Result<DailyPrayerTimes> {
    let times = build_daily_times(row, conf_shuruq)?;
    // FINDING F4: a surfaced time must be strict HH:MM. A row carrying a
    // hostile value ("25:70") is rejected whole — month_times drops the day,
    // so the day errors out instead of showing fabricated times (the same
    // semantic as layout-mismatched rows).
    let fields = [
        (&times.fajr, "fajr"),
        (&times.shurouq, "shurouq"),
        (&times.dhuhr, "dhuhr"),
        (&times.asr, "asr"),
        (&times.maghrib, "maghrib"),
        (&times.isha, "isha"),
    ];
    if let Some((bad, name)) =
        fields.iter().find(|(t, _)| !is_displayable_hhmm(t))
    {
        return Err(MawaqitError::Parse(format!(
            "calendar row surfaces invalid {name} time {bad:?} — day rejected"
        )));
    }
    Ok(times)
}

fn build_daily_times(
    row: &[String],
    conf_shuruq: Option<&str>,
) -> Result<DailyPrayerTimes> {
    let has_shuruq_column =
        row.len() >= 6 && row.get(1).is_some_and(|v| parse_hhmm(v).is_some());

    if has_shuruq_column {
        let normal_shurouq = row[1].clone();
        let mut prayers = vec![row[0].clone()];
        prayers.extend_from_slice(&row[2..]);
        return match prayers.len() {
            // imsak mode: [İmsak, Shurûq, Dhuhr, Asr, Maghrib, Isha]
            6 => Ok(DailyPrayerTimes {
                fajr: prayers[0].clone(),
                shurouq: prayers[1].clone(),
                dhuhr: prayers[2].clone(),
                asr: prayers[3].clone(),
                maghrib: prayers[4].clone(),
                isha: prayers[5].clone(),
            }),
            // normal mode: [Fajr, Dhuhr, Asr, Maghrib, Isha]
            5 => Ok(DailyPrayerTimes {
                fajr: prayers[0].clone(),
                shurouq: normal_shurouq,
                dhuhr: prayers[1].clone(),
                asr: prayers[2].clone(),
                maghrib: prayers[3].clone(),
                isha: prayers[4].clone(),
            }),
            n => Err(MawaqitError::Parse(format!(
                "expected 5 or 6 prayer times, got {n}"
            ))),
        };
    }

    // Plain prayer list without a shuruq column.
    match row.len() {
        6 => Ok(DailyPrayerTimes {
            fajr: row[0].clone(),
            shurouq: row[1].clone(),
            dhuhr: row[2].clone(),
            asr: row[3].clone(),
            maghrib: row[4].clone(),
            isha: row[5].clone(),
        }),
        5 => {
            let shurouq = conf_shuruq
                .ok_or_else(|| {
                    MawaqitError::Parse("no shuruq value for day".into())
                })?
                .to_string();
            Ok(DailyPrayerTimes {
                fajr: row[0].clone(),
                shurouq,
                dhuhr: row[1].clone(),
                asr: row[2].clone(),
                maghrib: row[3].clone(),
                isha: row[4].clone(),
            })
        }
        n => Err(MawaqitError::Parse(format!(
            "expected 5 or 6 prayer times, got {n}"
        ))),
    }
}

/// Resolve one raw iqama entry into its display string plus the minutes
/// from midnight of the adhan's calendar day. "+N" offsets can push past
/// 1440 — that excess *is* the rollover: the iqama belongs to the next
/// day (C1), which a display string alone cannot carry. Anything
/// unparseable falls back to the adhan time itself, like the official
/// integrations do; N is clamped to one day so hostile values cannot
/// overflow the time math.
pub(crate) fn resolve_iqama_parts(
    raw: &str,
    adhan: &str,
) -> (String, Option<i64>) {
    let adhan_t = parse_hhmm(adhan);
    let adhan_min = adhan_t.map(|t| t.hour() as i64 * 60 + t.minute() as i64);
    if let Some(mins) = raw.trim().strip_prefix('+')
        && let (Ok(n), Some(t)) = (mins.trim().parse::<i64>(), adhan_t)
        && let Some(delta) = Duration::try_minutes(n.clamp(0, 24 * 60))
    {
        let n = n.clamp(0, 24 * 60);
        return (time_string(t + delta), Some(adhan_min.unwrap_or(0) + n));
    }
    if is_displayable_hhmm(raw.trim()) {
        let t = parse_hhmm(raw.trim()).unwrap_or_default();
        return (
            raw.trim().to_string(),
            Some(t.hour() as i64 * 60 + t.minute() as i64),
        );
    }
    (adhan.trim().to_string(), adhan_min)
}

/// Display-only view of [`resolve_iqama_parts`] — the pinned historical
/// behavior. Test-only: production code uses the full parts (C1).
#[cfg(test)]
pub(crate) fn resolve_iqama(raw: &str, adhan: &str) -> String {
    resolve_iqama_parts(raw, adhan).0
}

pub(crate) fn daily_iqama_parts(
    raw: &[String],
    adhan: &DailyPrayerTimes,
) -> Result<(DailyIqamaTimes, [i64; 5])> {
    if raw.len() < 5 {
        return Err(MawaqitError::Parse(format!(
            "expected 5 iqama times, got {}",
            raw.len()
        )));
    }
    let (fajr, m0) = resolve_iqama_parts(&raw[0], &adhan.fajr);
    let (dhuhr, m1) = resolve_iqama_parts(&raw[1], &adhan.dhuhr);
    let (asr, m2) = resolve_iqama_parts(&raw[2], &adhan.asr);
    let (maghrib, m3) = resolve_iqama_parts(&raw[3], &adhan.maghrib);
    let (isha, m4) = resolve_iqama_parts(&raw[4], &adhan.isha);
    Ok((
        DailyIqamaTimes { fajr, dhuhr, asr, maghrib, isha },
        [
            m0.unwrap_or(0),
            m1.unwrap_or(0),
            m2.unwrap_or(0),
            m3.unwrap_or(0),
            m4.unwrap_or(0),
        ],
    ))
}

/// Display-only view of [`daily_iqama_parts`] — test-only; production
/// code needs the minutes too (C1).
#[cfg(test)]
pub(crate) fn daily_iqama_from(
    raw: &[String],
    adhan: &DailyPrayerTimes,
) -> Result<DailyIqamaTimes> {
    daily_iqama_parts(raw, adhan).map(|(times, _)| times)
}

fn raw_month(
    calendar: &RawCalendar,
    month: u32,
) -> Result<&crate::models::RawMonth> {
    if !(1..=12).contains(&month) {
        return Err(MawaqitError::InvalidMonth(month));
    }
    calendar.get((month - 1) as usize).ok_or(MawaqitError::NoCalendar)
}

/// Adhan times for every day of a month.
///
/// FINDING F5: day keys are wire strings and integer `FromStr` accepts
/// `"01"` and `"+1"`, so a hostile month can carry three rows for one day.
/// Days are deduplicated, and the canonical decimal key (`"1"`) always wins
/// over its variants; among variants alone, BTreeMap order decides
/// deterministically.
pub fn month_times(conf: &ConfData, month: u32) -> Result<MonthTimes> {
    let raw = raw_month(&conf.calendar, month)?;
    let mut by_day: std::collections::BTreeMap<u32, DailyPrayerTimes> =
        std::collections::BTreeMap::new();
    let mut seen: std::collections::BTreeSet<u32> =
        std::collections::BTreeSet::new();
    for (key, values) in raw {
        let Ok(day) = key.parse::<u32>() else {
            continue;
        };
        seen.insert(day);
        if by_day.contains_key(&day) && key.as_str() != day.to_string() {
            continue;
        }
        if let Ok(times) = daily_from_row(values, conf.shuruq.as_deref()) {
            by_day.insert(day, times);
        }
    }
    let kept: std::collections::BTreeSet<u32> =
        by_day.keys().copied().collect();
    // Days that were on the wire but surface no times (F4) are reported,
    // not silently lost (M2).
    let dropped: Vec<u32> = seen.difference(&kept).copied().collect();
    let days = by_day
        .into_iter()
        .map(|(day, times)| DayTimes { day, times })
        .collect();
    Ok(MonthTimes { month, days, dropped })
}

/// Resolved iqama data for one month: display times, minutes from midnight
/// of the adhan's day (rollover included), and the dropped-day list.
type IqamaMonth = (
    std::collections::BTreeMap<u32, DailyIqamaTimes>,
    std::collections::BTreeMap<u32, [i64; 5]>,
    Vec<u32>,
);

fn resolved_iqama_month(conf: &ConfData, month: u32) -> Result<IqamaMonth> {
    let iqama_calendar =
        conf.iqama_calendar.as_ref().ok_or(MawaqitError::NoCalendar)?;
    let raw_iqama = raw_month(iqama_calendar, month)?;
    let adhan_month = month_times(conf, month)?;
    let adhan_by_day: std::collections::HashMap<u32, &DailyPrayerTimes> =
        adhan_month.days.iter().map(|d| (d.day, &d.times)).collect();

    let mut times: std::collections::BTreeMap<u32, DailyIqamaTimes> =
        std::collections::BTreeMap::new();
    let mut minutes: std::collections::BTreeMap<u32, [i64; 5]> =
        std::collections::BTreeMap::new();
    let mut seen: std::collections::BTreeSet<u32> =
        std::collections::BTreeSet::new();
    for (key, values) in raw_iqama {
        let Ok(day) = key.parse::<u32>() else {
            continue;
        };
        seen.insert(day);
        let Some(adhan) = adhan_by_day.get(&day) else {
            continue;
        };
        if times.contains_key(&day) && key.as_str() != day.to_string() {
            continue;
        }
        if let Ok((t, m)) = daily_iqama_parts(values, adhan) {
            times.insert(day, t);
            minutes.insert(day, m);
        }
    }
    let kept: std::collections::BTreeSet<u32> = times.keys().copied().collect();
    let dropped: Vec<u32> = seen.difference(&kept).copied().collect();
    Ok((times, minutes, dropped))
}

/// Resolved iqama times for every day of a month (uses the adhan calendar
/// to expand "+N" entries). Same duplicate-day rule as [`month_times`].
pub fn month_iqama_times(
    conf: &ConfData,
    month: u32,
) -> Result<MonthIqamaTimes> {
    let (times, _minutes, dropped) = resolved_iqama_month(conf, month)?;
    let days = times
        .into_iter()
        .map(|(day, times)| DayIqamaTimes { day, times })
        .collect();
    Ok(MonthIqamaTimes { month, days, dropped })
}

/// Adhan (+ iqama) times for a specific date.
///
/// The iqama instants carry the rollover (C1): a "+600" after a 23:30
/// adhan is reported at *next-day* 09:30, which the display string alone
/// cannot say. A day the calendar rejected as malformed surfaces as
/// [`MawaqitError::InvalidDay`] — never as fabricated times; `NoCalendar`
/// means the day is genuinely absent from the mosque's calendar.
pub fn times_for_date(conf: &ConfData, date: NaiveDate) -> Result<TodayTimes> {
    let month = date.month();
    let day = date.day();
    let adhan_month = month_times(conf, month)?;
    let Some(adhan) =
        adhan_month.days.iter().find(|d| d.day == day).map(|d| d.times.clone())
    else {
        return Err(if adhan_month.dropped.contains(&day) {
            MawaqitError::InvalidDay(day)
        } else {
            MawaqitError::NoCalendar
        });
    };

    let resolved = conf
        .iqama_calendar
        .as_ref()
        .and_then(|_| resolved_iqama_month(conf, month).ok());
    let (iqama, iqama_at) = match resolved {
        Some((times, minutes, _dropped)) => match times.get(&day) {
            Some(times) => {
                let midnight =
                    date.and_hms_opt(0, 0, 0).expect("valid date has midnight");
                let m = minutes.get(&day).copied().unwrap_or([0; 5]);
                (
                    Some(times.clone()),
                    Some(DailyIqamaInstants {
                        fajr: midnight + Duration::minutes(m[0]),
                        dhuhr: midnight + Duration::minutes(m[1]),
                        asr: midnight + Duration::minutes(m[2]),
                        maghrib: midnight + Duration::minutes(m[3]),
                        isha: midnight + Duration::minutes(m[4]),
                    }),
                )
            }
            // Iqama row hostile for this day: degrade to adhan-only.
            None => (None, None),
        },
        None => (None, None),
    };
    Ok(TodayTimes { date, adhan, iqama, iqama_at })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::models::RawMonth;

    fn month_map(pairs: &[(&str, Vec<&str>)]) -> RawMonth {
        pairs
            .iter()
            .map(|(k, v)| {
                (k.to_string(), v.iter().map(|s| s.to_string()).collect())
            })
            .collect()
    }

    fn sample_conf() -> ConfData {
        serde_json::from_value(json!({
            "calendar": [
                month_map(&[("1", vec!["06:30","08:00","13:00","15:30","17:45","19:15"])]),
                month_map(&[("1", vec!["06:20","07:50","12:50","15:10","17:30","19:00"])])
            ],
            "iqamaCalendar": [
                month_map(&[("1", vec!["06:45","+15","13:20","+20","18:00"])])
            ],
            "name": "Test Mosque"
        }))
        .unwrap()
    }

    #[test]
    fn parses_normal_mode_row() {
        // [Fajr, Shuruq, Dhuhr, Asr, Maghrib, Isha]
        let row: Vec<String> =
            ["06:30", "08:00", "13:00", "15:30", "17:45", "19:15"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        let t = daily_from_row(&row, None).unwrap();
        assert_eq!(t.fajr, "06:30");
        assert_eq!(t.shurouq, "08:00");
        assert_eq!(t.dhuhr, "13:00");
        assert_eq!(t.isha, "19:15");
    }

    #[test]
    fn parses_imsak_mode_row() {
        let row: Vec<String> =
            ["05:27", "06:37", "07:07", "13:21", "16:37", "19:24", "20:51"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        let t = daily_from_row(&row, Some("06:37")).unwrap();
        assert_eq!(t.fajr, "05:27"); // displayed as "Imsak"
        assert_eq!(t.shurouq, "07:07"); // Diyanet Güneş
        assert_eq!(t.dhuhr, "13:21");
        assert_eq!(t.asr, "16:37");
        assert_eq!(t.maghrib, "19:24");
        assert_eq!(t.isha, "20:51");
    }

    #[test]
    fn parses_row_without_shuruq_column() {
        let row: Vec<String> = ["06:09", "13:47", "16:58", "19:45", "21:12"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let t = daily_from_row(&row, Some("07:41")).unwrap();
        assert_eq!(t.fajr, "06:09");
        assert_eq!(t.shurouq, "07:41");
        assert_eq!(t.dhuhr, "13:47");
        assert_eq!(t.isha, "21:12");
    }

    #[test]
    fn rejects_short_day() {
        let raw: Vec<String> = vec!["06:30".into(), "08:00".into()];
        assert!(matches!(
            daily_from_row(&raw, None),
            Err(MawaqitError::Parse(_))
        ));
    }

    #[test]
    fn rejects_days_surfacing_invalid_times() {
        // F4: a hostile value anywhere in the row rejects the whole day —
        // month_times then drops it instead of surfacing fabricated times.
        for bad in ["25:70", "99:99", "ab:cd", "7:5", "+30", "24:00"] {
            let mut row: Vec<String> =
                ["06:30", "08:00", "13:00", "15:30", "17:45", "19:15"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
            row[0] = bad.to_string();
            assert!(
                matches!(
                    daily_from_row(&row, None),
                    Err(MawaqitError::Parse(_))
                ),
                "row with fajr {bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn iqama_passthrough_cannot_smuggle_non_display_times() {
        // "7:5" parses internally (chrono is lenient) but must never surface:
        // the passthrough falls back to the adhan value like garbage does.
        assert_eq!(resolve_iqama("7:5", "06:30"), "06:30");
        assert_eq!(resolve_iqama("25:70", "06:30"), "06:30");
        assert_eq!(resolve_iqama("06:45", "06:30"), "06:45");
    }

    #[test]
    fn resolves_relative_and_absolute_iqama() {
        let adhan = DailyPrayerTimes {
            fajr: "06:30".into(),
            shurouq: "08:00".into(),
            dhuhr: "13:00".into(),
            asr: "15:30".into(),
            maghrib: "17:45".into(),
            isha: "19:15".into(),
        };
        let raw: Vec<String> = ["06:45", "+15", "13:20", "+20", "18:00"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let iq = daily_iqama_from(&raw, &adhan).unwrap();
        assert_eq!(iq.fajr, "06:45");
        assert_eq!(iq.dhuhr, "13:15");
        assert_eq!(iq.asr, "13:20"); // absolute value kept
        assert_eq!(iq.maghrib, "18:05"); // 17:45 + 20 minutes
        assert_eq!(iq.isha, "18:00");
    }

    #[test]
    fn invalid_iqama_falls_back_to_adhan() {
        assert_eq!(resolve_iqama("garbage", "17:45"), "17:45");
        assert_eq!(resolve_iqama("+abc", "17:45"), "17:45");
    }

    #[test]
    fn hostile_iqama_offsets_do_not_panic() {
        // Red-team finding: TimeDelta::minutes(i64::MAX) used to panic.
        let big = "+9223372036854775807";
        assert_eq!(resolve_iqama(big, "17:45"), "17:45"); // clamped to +24h ->
        // next-day 17:45
        assert_eq!(resolve_iqama("+999999999999999", "17:45"), "17:45");
        assert_eq!(resolve_iqama("+0", "17:45"), "17:45");
        assert_eq!(resolve_iqama("+1440", "17:45"), "17:45");
        assert_eq!(resolve_iqama("+", "17:45"), "17:45");
        // one '+' is stripped and "+5" parses, so this resolves to +5 minutes
        assert_eq!(resolve_iqama("++5", "17:45"), "17:50");
    }

    #[test]
    fn extracts_month_and_iqama() {
        let r = sample_conf();
        let m = month_times(&r, 1).unwrap();
        assert_eq!(m.days.len(), 1);
        assert_eq!(m.days[0].times.dhuhr, "13:00");

        let mi = month_iqama_times(&r, 1).unwrap();
        assert_eq!(mi.days[0].times.dhuhr, "13:15");
    }

    #[test]
    fn rejects_invalid_month() {
        let r = sample_conf();
        assert!(matches!(
            month_times(&r, 0),
            Err(MawaqitError::InvalidMonth(0))
        ));
        assert!(matches!(
            month_times(&r, 13),
            Err(MawaqitError::InvalidMonth(13))
        ));
    }

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

    #[test]
    fn iqama_rollover_instants_carry_the_next_day() {
        // C1: "+600" after a 23:30 adhan belongs to the NEXT day — the
        // display string shows 09:30 either way, the instant says when.
        let r = serde_json::from_value(json!({
            "times": ["23:30", "00:00", "00:00", "00:00", "00:00"],
            "calendar": [month_map(&[(
                "1",
                vec!["23:30", "23:45", "23:50", "23:55", "23:58", "23:59"],
            )])],
            "iqamaCalendar": [month_map(&[(
                "1",
                vec!["+600", "+600", "+600", "+600", "+600"],
            )])]
        }))
        .unwrap();
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

    #[test]
    fn dropped_days_are_reported_and_error_as_invalid_day() {
        let r = serde_json::from_value(json!({
            "times": ["06:30", "08:00", "13:00", "15:30", "17:45"],
            "calendar": [month_map(&[
                // hostile row: rejected whole (F4) but reported (M2)
                ("1", vec!["25:70", "06:37", "13:21", "16:37", "19:24", "20:51"]),
                ("2", vec!["06:30", "08:00", "13:00", "15:30", "17:45", "19:15"]),
            ])]
        }))
        .unwrap();
        let month = month_times(&r, 1).unwrap();
        assert_eq!(month.days.len(), 1, "only the valid day surfaces");
        assert_eq!(month.dropped, vec![1], "the hostile day is reported");

        let err =
            times_for_date(&r, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap())
                .unwrap_err();
        assert!(matches!(err, MawaqitError::InvalidDay(1)), "{err}");
        // A day that was never on the wire stays NoCalendar.
        let err =
            times_for_date(&r, NaiveDate::from_ymd_opt(2026, 1, 7).unwrap())
                .unwrap_err();
        assert!(matches!(err, MawaqitError::NoCalendar), "{err}");
    }
}
