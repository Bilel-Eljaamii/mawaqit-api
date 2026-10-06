//! Consumer-facing prayer identity and next-event resolution (issue #4,
//! P1/P2) — the logic every mawaqit consumer needs, promoted out of the
//! desktop app (`prayer_logic.rs`, the tray, the frontend's
//! `computeNextEvent`) and this crate's own `next_prayer` example.
//!
//! Tiering: the enum, event types and the [`select_next`] rule are core
//! (they compile down to `#![no_std]` bare metal — MQTC firmware resolves
//! its own next prayer through `CompactDayTimes::next_event` on the very
//! same types); the [`TodayTimes::next_event`] implementation is
//! alloc-tier like its receiver.
//!
//! Rollover correctness is the point: events are absolute
//! [`NaiveDateTime`]s built on the C1 instants, so an iqama that rolls
//! past midnight belongs to *tomorrow* — never sorted back onto the
//! wrong day the way string comparisons do.

use chrono::{NaiveDateTime, NaiveTime};

use crate::{
    models::{DailyIqamaInstants, TodayTimes},
    time::parse_hhmm,
};

/// The five prayers that receive an adhan. Shuruq (sunrise) is not a
/// prayer — it appears in events as [`PrayerEventKind::Shuruq`], exactly
/// like the official screens treat it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Prayer {
    Fajr,
    Dhuhr,
    Asr,
    Maghrib,
    Isha,
}

impl Prayer {
    /// The five prayers in their fixed wire order.
    pub const ALL: [Prayer; 5] = [
        Prayer::Fajr,
        Prayer::Dhuhr,
        Prayer::Asr,
        Prayer::Maghrib,
        Prayer::Isha,
    ];

    /// Stable lowercase key (`"fajr"`…) — the config/UI identity every
    /// consumer shares (per-prayer settings keys, tray menus, dedup keys).
    pub const fn key(self) -> &'static str {
        match self {
            Prayer::Fajr => "fajr",
            Prayer::Dhuhr => "dhuhr",
            Prayer::Asr => "asr",
            Prayer::Maghrib => "maghrib",
            Prayer::Isha => "isha",
        }
    }

    /// Human-readable name (`"Fajr"`…), matching the official displays.
    pub const fn display_name(self) -> &'static str {
        match self {
            Prayer::Fajr => "Fajr",
            Prayer::Dhuhr => "Dhuhr",
            Prayer::Asr => "Asr",
            Prayer::Maghrib => "Maghrib",
            Prayer::Isha => "Isha",
        }
    }

    /// Parse a key ([`Self::key`]); `None` for anything else.
    ///
    /// ```
    /// use mawaqit_api::prayer::Prayer;
    ///
    /// assert_eq!(Prayer::parse("maghrib"), Some(Prayer::Maghrib));
    /// assert_eq!(Prayer::parse("Maghrib"), None, "keys are lowercase");
    /// ```
    pub fn parse(key: &str) -> Option<Prayer> {
        Self::ALL.iter().copied().find(|p| p.key() == key)
    }

    const fn adhan_key(self) -> &'static str {
        match self {
            Prayer::Fajr => "fajr/adhan",
            Prayer::Dhuhr => "dhuhr/adhan",
            Prayer::Asr => "asr/adhan",
            Prayer::Maghrib => "maghrib/adhan",
            Prayer::Isha => "isha/adhan",
        }
    }

    const fn adhan_label(self) -> &'static str {
        match self {
            Prayer::Fajr => "Fajr adhan",
            Prayer::Dhuhr => "Dhuhr adhan",
            Prayer::Asr => "Asr adhan",
            Prayer::Maghrib => "Maghrib adhan",
            Prayer::Isha => "Isha adhan",
        }
    }

    const fn iqama_label(self) -> &'static str {
        match self {
            Prayer::Fajr => "Fajr iqama",
            Prayer::Dhuhr => "Dhuhr iqama",
            Prayer::Asr => "Asr iqama",
            Prayer::Maghrib => "Maghrib iqama",
            Prayer::Isha => "Isha iqama",
        }
    }

    const fn iqama_key(self) -> &'static str {
        match self {
            Prayer::Fajr => "fajr/iqama",
            Prayer::Dhuhr => "dhuhr/iqama",
            Prayer::Asr => "asr/iqama",
            Prayer::Maghrib => "maghrib/iqama",
            Prayer::Isha => "isha/iqama",
        }
    }
}

/// What kind of event a [`PrayerEvent`] is: an adhan, the iqama that
/// follows it, or the informational shuruq (sunrise — no adhan, no iqama).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrayerEventKind {
    Adhan(Prayer),
    Iqama(Prayer),
    Shuruq,
}

impl PrayerEventKind {
    /// The prayer taking part, `None` for shuruq.
    pub const fn prayer(&self) -> Option<Prayer> {
        match self {
            PrayerEventKind::Adhan(p) | PrayerEventKind::Iqama(p) => Some(*p),
            PrayerEventKind::Shuruq => None,
        }
    }

    /// Human-readable label: `"Fajr adhan"`, `"Fajr iqama"`, `"Shurouq"`.
    ///
    /// ```
    /// use mawaqit_api::prayer::{Prayer, PrayerEventKind};
    ///
    /// assert_eq!(PrayerEventKind::Adhan(Prayer::Fajr).label(), "Fajr adhan");
    /// assert_eq!(PrayerEventKind::Iqama(Prayer::Isha).label(), "Isha iqama");
    /// assert_eq!(PrayerEventKind::Shuruq.label(), "Shurouq");
    /// ```
    pub const fn label(&self) -> &'static str {
        match self {
            PrayerEventKind::Adhan(p) => p.adhan_label(),
            PrayerEventKind::Iqama(p) => p.iqama_label(),
            PrayerEventKind::Shuruq => "Shurouq",
        }
    }

    /// Stable identity key for dedup/persisted state: `"fajr/adhan"`,
    /// `"fajr/iqama"`, `"shuruq"`.
    pub const fn state_key(&self) -> &'static str {
        match self {
            PrayerEventKind::Adhan(p) => p.adhan_key(),
            PrayerEventKind::Iqama(p) => p.iqama_key(),
            PrayerEventKind::Shuruq => "shuruq",
        }
    }

    /// Tie-break rank for equal instants: the adhan precedes the iqama it
    /// announces, shuruq is informational.
    pub(crate) const fn rank(&self) -> u8 {
        match self {
            PrayerEventKind::Adhan(_) => 0,
            PrayerEventKind::Iqama(_) => 1,
            PrayerEventKind::Shuruq => 2,
        }
    }
}

/// One upcoming prayer event: what it is, the absolute instant (rollover
/// already applied — an iqama past midnight sits on its real day), and
/// how many minutes from `now` it fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrayerEvent {
    pub kind: PrayerEventKind,
    pub at: NaiveDateTime,
    pub minutes_remaining: i64,
}

/// The shared selection rule (P2 and P2b use the very same one, so the
/// desktop and MCU semantics cannot drift): the earliest candidate
/// strictly after `now`, ties broken by kind rank. Callers pass today's
/// candidates plus tomorrow's first events — the tomorrow fallback falls
/// out of the same rule. `None` when nothing is ahead.
pub(crate) fn select_next<I>(
    candidates: I,
    now: NaiveDateTime,
) -> Option<PrayerEvent>
where
    I: Iterator<Item = (NaiveDateTime, PrayerEventKind)>,
{
    candidates
        .filter(|(at, _)| *at > now)
        .min_by_key(|(at, kind)| (*at, kind.rank()))
        .map(|(at, kind)| PrayerEvent {
            kind,
            at,
            minutes_remaining: (at - now).num_minutes(),
        })
}

impl TodayTimes {
    /// The next prayer event strictly after `now` (wall clock of the
    /// mosque, same frame as the displayed times): the adhan events, the
    /// shuruq, and — when the iqama resolved for this day — the
    /// rollover-correct iqama instants. When everything today has passed,
    /// the first adhan of tomorrow answers. Hostile `HH:MM` values are
    /// inert (skipped), never misparsed; `None` only when nothing at all
    /// resolves.
    ///
    /// ```
    /// use chrono::NaiveTime;
    /// use mawaqit_api::{DailyPrayerTimes, TodayTimes};
    ///
    /// let today = TodayTimes {
    ///     date: chrono::NaiveDate::from_ymd_opt(2026, 10, 6).unwrap(),
    ///     adhan: DailyPrayerTimes {
    ///         fajr: "05:30".into(),
    ///         shurouq: "07:07".into(),
    ///         dhuhr: "13:21".into(),
    ///         asr: "16:37".into(),
    ///         maghrib: "19:24".into(),
    ///         isha: "21:05".into(),
    ///     },
    ///     iqama: None,
    ///     iqama_at: None,
    /// };
    /// let next = today
    ///     .next_event(NaiveTime::from_hms_opt(10, 0, 0).unwrap())
    ///     .unwrap();
    /// assert_eq!(next.kind.label(), "Dhuhr adhan");
    /// assert_eq!(next.minutes_remaining, 201);
    /// ```
    pub fn next_event(&self, now: NaiveTime) -> Option<PrayerEvent> {
        let now_dt = self.date.and_time(now);
        let mut candidates = Vec::new();

        // Today's adhan + shuruq from the display strings (defensive:
        // TodayTimes is a public struct — hand-built fields are not
        // guaranteed displayable, so hostile values are skipped, never
        // misparsed).
        let rows: [(&str, Prayer, bool); 6] = [
            (self.adhan.fajr.as_str(), Prayer::Fajr, false),
            (self.adhan.shurouq.as_str(), Prayer::Fajr, true),
            (self.adhan.dhuhr.as_str(), Prayer::Dhuhr, false),
            (self.adhan.asr.as_str(), Prayer::Asr, false),
            (self.adhan.maghrib.as_str(), Prayer::Maghrib, false),
            (self.adhan.isha.as_str(), Prayer::Isha, false),
        ];
        for (hhmm, prayer, shuruq) in rows {
            let kind = if shuruq {
                PrayerEventKind::Shuruq
            } else {
                PrayerEventKind::Adhan(prayer)
            };
            if let Some(at) = parse_hhmm(hhmm).map(|t| self.date.and_time(t)) {
                candidates.push((at, kind));
            }
        }

        // Iqama instants already carry C1 rollover — an event past
        // midnight is tomorrow's and stays strictly future all day.
        if let Some(iqama) = &self.iqama_at {
            for prayer in Prayer::ALL {
                candidates.push((
                    iqama.instant_of(prayer),
                    PrayerEventKind::Iqama(prayer),
                ));
            }
        }

        // Everything passed: the first adhan of tomorrow answers. (Total:
        // `checked_add_days` yields no candidate at NaiveDate::MAX — there
        // is no tomorrow to fall back to, which is `None`, never a panic.)
        candidates.extend(
            self.date.checked_add_days(chrono::Days::new(1)).and_then(
                |tomorrow| {
                    let first = Prayer::ALL
                        .iter()
                        .filter_map(|p| {
                            let hhmm = match p {
                                Prayer::Fajr => &self.adhan.fajr,
                                Prayer::Dhuhr => &self.adhan.dhuhr,
                                Prayer::Asr => &self.adhan.asr,
                                Prayer::Maghrib => &self.adhan.maghrib,
                                Prayer::Isha => &self.adhan.isha,
                            };
                            parse_hhmm(hhmm).map(|t| (t, *p))
                        })
                        .min_by_key(|(t, _)| *t);
                    first.map(|(t, prayer)| {
                        (tomorrow.and_time(t), PrayerEventKind::Adhan(prayer))
                    })
                },
            ),
        );

        select_next(candidates.into_iter(), now_dt)
    }
}

impl DailyIqamaInstants {
    /// The iqama instant for one prayer (C1 rollover included).
    ///
    /// ```
    /// use chrono::NaiveDate;
    /// use mawaqit_api::{DailyIqamaInstants, prayer::Prayer};
    ///
    /// let day = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
    /// let instants = DailyIqamaInstants {
    ///     fajr: day.and_hms_opt(5, 47, 0).unwrap(),
    ///     dhuhr: day.and_hms_opt(13, 35, 0).unwrap(),
    ///     asr: day.and_hms_opt(16, 55, 0).unwrap(),
    ///     maghrib: day.and_hms_opt(19, 40, 0).unwrap(),
    ///     isha: day.and_hms_opt(21, 5, 0).unwrap(),
    /// };
    /// assert_eq!(instants.instant_of(Prayer::Maghrib), day.and_hms_opt(19, 40, 0).unwrap());
    /// ```
    pub fn instant_of(&self, prayer: Prayer) -> NaiveDateTime {
        match prayer {
            Prayer::Fajr => self.fajr,
            Prayer::Dhuhr => self.dhuhr,
            Prayer::Asr => self.asr,
            Prayer::Maghrib => self.maghrib,
            Prayer::Isha => self.isha,
        }
    }
}
