#[cfg(not(feature = "std"))]
use alloc::{
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec::Vec,
};
#[cfg(feature = "std")]
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A mosque as returned by the keyless search endpoint
/// (`GET /api/2.0/mosque/search?word=...`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mosque {
    /// Stable mosque UUID as published by the search index (string when
    /// the wire carries one). Not used for fetching — the slug is
    /// ([`Mosque::mosque_id`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    /// Numeric mosque id, wire-typed: usually `u64`, but any hostile JSON
    /// value is kept verbatim rather than rejected (ADR-0003 tolerance).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    /// URL slug used to fetch the public page (`/{lang}/{slug}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    /// Mosque-authored name; display fallback after [`Mosque::label`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Search-tuned display label ("Grande Mosquée de Paris") — the first
    /// choice of [`Mosque::display_name`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// City or locality ("Paris"); first half of [`Mosque::place`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locality: Option<String>,
    /// Country name ("France"); second half of [`Mosque::place`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    /// Every unmodeled search-result field, verbatim from the wire.
    /// Search results are not run through the display sanitizer — treat
    /// this (and the free-text fields) as untrusted display data.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Mosque {
    /// Best human-readable name: label, then name, then the slug.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mawaqit_api::Mosque;
    ///
    /// let m: Mosque = serde_json::from_str(
    ///     r#"{"label": "Grande Mosquée de Paris"}"#,
    /// )
    /// .unwrap();
    /// assert_eq!(m.display_name(), "Grande Mosquée de Paris");
    /// ```
    pub fn display_name(&self) -> &str {
        self.label
            .as_deref()
            .or(self.name.as_deref())
            .or(self.slug.as_deref())
            .unwrap_or("?")
    }

    /// The slug identifier used by [`crate::MawaqitClient`] data methods.
    pub fn mosque_id(&self) -> Option<&str> {
        self.slug.as_deref()
    }

    /// Short place description ("locality, country") when available.
    pub fn place(&self) -> Option<String> {
        match (self.locality.as_deref(), self.country.as_deref()) {
            (Some(l), Some(c)) => Some(format!("{l}, {c}")),
            (Some(l), None) => Some(l.to_string()),
            (None, Some(c)) => Some(c.to_string()),
            (None, None) => None,
        }
    }
}

/// The parts of the page's `confData` object the client exposes.
/// Anything else stays accessible through [`ConfData::raw`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfData {
    /// Today's adhan times, [Fajr, Shurouq, Dhuhr, Asr, Maghrib, Isha].
    #[serde(default)]
    pub times: Vec<String>,
    /// Today's sunrise.
    #[serde(default)]
    pub shuruq: Option<String>,
    /// Year calendar: 12 months, each an object day -> [Fajr, Shurouq,
    /// Dhuhr, Asr, Maghrib, Isha] as "HH:MM".
    #[serde(default)]
    pub calendar: RawCalendar,
    /// Iqama calendar: 12 months, each an object day -> 5 values (no
    /// shurouq); values are "HH:MM" or "+N" minutes after the adhan.
    #[serde(default, rename = "iqamaCalendar")]
    pub iqama_calendar: Option<RawCalendar>,
    /// Mosque display name.
    #[serde(default)]
    pub name: Option<String>,
    /// True when the mosque uses the 6-prayer "Sabah Imsak" layout
    /// (`displayingSabahImsak`, common on DİTİB mosques): the first prayer
    /// time is the imsak and is usually displayed as "Imsak", not "Fajr".
    #[serde(default)]
    pub imsak_mode: bool,
    /// Jumu'a times (some mosques have two).
    #[serde(default)]
    pub jumua: Option<String>,
    /// Second Jumu'a time, when the mosque runs two sittings.
    #[serde(default)]
    pub jumua2: Option<String>,
    /// Mosque picture shown as the background on mawaqit.net.
    #[serde(default)]
    pub image: Option<String>,
    /// Announcements configured by the mosque.
    #[serde(default)]
    pub announcements: Vec<Announcement>,
    /// The complete raw confData for anything not modeled above.
    #[serde(flatten)]
    pub raw: serde_json::Value,
}

impl ConfData {
    /// The mosque's IANA timezone designator as published by the page
    /// (`timezone` in confData), shape-validated — FINDING F28 / ADR-0015.
    /// `iqama_at` instants are mosque-local wall clock; this accessor is
    /// the only sanctioned zone source for interpreting them. `None` when
    /// the field is absent, not a string, or malformed (empty, over 64
    /// bytes, absolute path, `..` segment, or characters outside the IANA
    /// identifier alphabet) — a hostile value is indistinguishable from
    /// "the mosque publishes no zone".
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mawaqit_api::ConfData;
    ///
    /// let conf: ConfData = serde_json::from_str(
    ///     r#"{"timezone": "Europe/Paris"}"#,
    /// )
    /// .unwrap();
    /// assert_eq!(conf.timezone(), Some("Europe/Paris"));
    ///
    /// let hostile: ConfData =
    ///     serde_json::from_str(r#"{"timezone": "../etc"}"#).unwrap();
    /// assert_eq!(hostile.timezone(), None);
    /// ```
    pub fn timezone(&self) -> Option<&str> {
        let tz = self.raw.get("timezone")?.as_str()?;
        is_plausible_timezone(tz).then_some(tz)
    }
}

/// IANA-identifier shape check for [`ConfData::timezone`]: letters,
/// digits and `_`, `.`, `/`, `-`, `+`, up to 64 bytes, never empty, never
/// an absolute path, never a `..` segment. Deliberately not a tz-database
/// lookup — validation here only bounds what reaches a caller's zone
/// resolver (F29's bounded-echo rule would otherwise apply to a 4 MB
/// "timezone").
fn is_plausible_timezone(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && !s.starts_with('/')
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '.' | '/' | '-' | '+')
        })
        && !s.split('/').any(|seg| seg == "..")
}

/// One mosque announcement: the banner text and media a mosque publishes
/// on its page. Free-text fields arrive sanitized (F6); the dates are
/// wire strings the mosque formatted itself — parse defensively.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Announcement {
    /// Announcement id, wire-typed (numeric on the live site).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    /// Announcement title, sanitized (F6).
    #[serde(default)]
    pub title: Option<String>,
    /// Announcement body text, sanitized (F6).
    #[serde(default)]
    pub content: Option<String>,
    /// Banner image URL, sanitized (F6). Served from the mosque's own
    /// storage — displaying it is a tracking-vector decision for the app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Announcement video URL, sanitized (F6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<String>,
    /// First day the announcement shows, as the mosque formatted it
    /// (e.g. `"2026-10-05"`); never parsed here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    /// Last day the announcement shows, same format contract as
    /// [`Announcement::start_date`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    /// Every unmodeled announcement field, verbatim from the wire.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Raw API shape: a month is an object keyed by day-of-month ("1".."31"),
/// each day an ordered list of "HH:MM" strings.
pub type RawMonth = BTreeMap<String, Vec<String>>;
/// A year calendar: 12 months (index 0 = January) in [`RawMonth`] shape.
/// Wire-tolerant by construction — hostile keys survive verbatim and are
/// only interpreted by the calendar pipeline (F5 dedupe, F4 rejection).
pub type RawCalendar = Vec<RawMonth>;

/// The six adhan times of one day, in API order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DailyPrayerTimes {
    /// Fajr (dawn) adhan, strict `HH:MM` (ADR-0010).
    pub fajr: String,
    /// Sunrise — not a prayer; displayed as an informational marker.
    pub shurouq: String,
    /// Dhuhr (midday) adhan, strict `HH:MM`.
    pub dhuhr: String,
    /// Asr (afternoon) adhan, strict `HH:MM`.
    pub asr: String,
    /// Maghrib (sunset) adhan, strict `HH:MM`.
    pub maghrib: String,
    /// Isha (night) adhan, strict `HH:MM`.
    pub isha: String,
}

/// The five iqama times of one day (no iqama for shurouq), already resolved
/// to absolute "HH:MM" times (relative "+N" entries are applied to the adhan).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DailyIqamaTimes {
    /// Fajr iqama, strict `HH:MM`; equals the adhan when the mosque
    /// publishes none.
    pub fajr: String,
    /// Dhuhr iqama, strict `HH:MM`.
    pub dhuhr: String,
    /// Asr iqama, strict `HH:MM`.
    pub asr: String,
    /// Maghrib iqama, strict `HH:MM`.
    pub maghrib: String,
    /// Isha iqama, strict `HH:MM`.
    pub isha: String,
}

/// Absolute naive datetimes for the five iqama prayers of one day, in API
/// order. These carry what the display strings cannot (C1): a "+600"
/// offset after a 23:30 adhan lands on the *next* day, and alarms or
/// calendars must fire at the instant, not at the rolled wall clock.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DailyIqamaInstants {
    /// Fajr iqama instant: midnight of the display day plus the resolved
    /// minutes, so rollover already lands on the next day (C1). Mosque-
    /// local wall clock — interpret with [`ConfData::timezone`].
    pub fajr: chrono::NaiveDateTime,
    /// Dhuhr iqama instant, same contract as [`DailyIqamaInstants::fajr`].
    pub dhuhr: chrono::NaiveDateTime,
    /// Asr iqama instant, same contract as [`DailyIqamaInstants::fajr`].
    pub asr: chrono::NaiveDateTime,
    /// Maghrib iqama instant, same contract as [`DailyIqamaInstants::fajr`].
    pub maghrib: chrono::NaiveDateTime,
    /// Isha iqama instant, same contract as [`DailyIqamaInstants::fajr`].
    pub isha: chrono::NaiveDateTime,
}

/// Adhan + resolved iqama times for one calendar day.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TodayTimes {
    /// The calendar day these times belong to (the caller's local date
    /// for [`crate::MawaqitClient::today`], the queried date otherwise).
    pub date: chrono::NaiveDate,
    /// The day's six adhan times.
    pub adhan: DailyPrayerTimes,
    /// Resolved iqama display times; `None` when the mosque publishes no
    /// usable iqama for this day (absent calendar or hostile row).
    pub iqama: Option<DailyIqamaTimes>,
    /// Absolute instants for the iqama prayers — the rollover-correct
    /// counterpart of `iqama` (C1). `None` when the mosque publishes no
    /// usable iqama for this day.
    pub iqama_at: Option<DailyIqamaInstants>,
}

/// One calendar day's adhan times, as surfaced inside a [`MonthTimes`].
#[derive(Debug, Clone, Serialize)]
pub struct DayTimes {
    /// Day of month (1-31), deduplicated to the canonical decimal key (F5).
    pub day: u32,
    /// The day's six adhan times.
    pub times: DailyPrayerTimes,
}

/// Adhan times for every day of a month ([`crate::month_times`]).
#[derive(Debug, Clone, Serialize)]
pub struct MonthTimes {
    /// 1-12
    pub month: u32,
    /// One entry per usable day, ordered by day.
    pub days: Vec<DayTimes>,
    /// Days present in the wire calendar but rejected as malformed (F4) —
    /// they surface no times. Empty for a clean month (M2).
    pub dropped: Vec<u32>,
}

/// One calendar day's resolved iqama times, as surfaced inside a
/// [`MonthIqamaTimes`].
#[derive(Debug, Clone, Serialize)]
pub struct DayIqamaTimes {
    /// Day of month (1-31), deduplicated to the canonical decimal key (F5).
    pub day: u32,
    /// The day's five resolved iqama times.
    pub times: DailyIqamaTimes,
}

/// Resolved iqama times for every day of a month
/// ([`crate::month_iqama_times`]).
#[derive(Debug, Clone, Serialize)]
pub struct MonthIqamaTimes {
    /// 1-12
    pub month: u32,
    /// One entry per usable day, ordered by day.
    pub days: Vec<DayIqamaTimes>,
    /// Same contract as [`MonthTimes::dropped`] (M2).
    pub dropped: Vec<u32>,
}
