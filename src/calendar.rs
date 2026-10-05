#[cfg(not(feature = "std"))]
use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
#[cfg(feature = "std")]
use std::collections::{BTreeMap, BTreeSet};

use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Timelike};

use crate::{
    error::{MawaqitError, Result},
    models::{
        ConfData, DailyIqamaInstants, DailyIqamaTimes, DailyPrayerTimes,
        DayIqamaTimes, DayTimes, MonthIqamaTimes, MonthTimes, RawCalendar,
        TodayTimes,
    },
    time::{is_displayable_hhmm, parse_hhmm},
};

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
    let mut by_day: BTreeMap<u32, DailyPrayerTimes> = BTreeMap::new();
    let mut seen: BTreeSet<u32> = BTreeSet::new();
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
    let kept: BTreeSet<u32> = by_day.keys().copied().collect();
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
type IqamaMonth =
    (BTreeMap<u32, DailyIqamaTimes>, BTreeMap<u32, [i64; 5]>, Vec<u32>);

fn resolved_iqama_month(conf: &ConfData, month: u32) -> Result<IqamaMonth> {
    let iqama_calendar =
        conf.iqama_calendar.as_ref().ok_or(MawaqitError::NoCalendar)?;
    let raw_iqama = raw_month(iqama_calendar, month)?;
    let adhan_month = month_times(conf, month)?;
    let adhan_by_day: BTreeMap<u32, &DailyPrayerTimes> =
        adhan_month.days.iter().map(|d| (d.day, &d.times)).collect();

    let mut times: BTreeMap<u32, DailyIqamaTimes> = BTreeMap::new();
    let mut minutes: BTreeMap<u32, [i64; 5]> = BTreeMap::new();
    let mut seen: BTreeSet<u32> = BTreeSet::new();
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
    let kept: BTreeSet<u32> = times.keys().copied().collect();
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
