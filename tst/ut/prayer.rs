//! Next-event resolution pins (issue #4, P1/P2): `Prayer` identity and
//! `TodayTimes::next_event` — the logic promoted out of the desktop's
//! `prayer_logic.rs`, tray and frontend `computeNextEvent`. The rollover
//! pin is the live defect the promotion fixes: an iqama past midnight
//! belongs to tomorrow, never sorts back onto today.

use chrono::{NaiveDate, NaiveTime};
use mawaqit_api::{
    DailyIqamaInstants, DailyPrayerTimes, TodayTimes,
    prayer::{Prayer, PrayerEventKind},
};

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 6).unwrap()
}

fn adhan(
    fajr: &str,
    shuruq: &str,
    dhuhr: &str,
    asr: &str,
    maghrib: &str,
    isha: &str,
) -> DailyPrayerTimes {
    DailyPrayerTimes {
        fajr: fajr.into(),
        shurouq: shuruq.into(),
        dhuhr: dhuhr.into(),
        asr: asr.into(),
        maghrib: maghrib.into(),
        isha: isha.into(),
    }
}

fn today(adhan: DailyPrayerTimes) -> TodayTimes {
    TodayTimes { date: date(), adhan, iqama: None, iqama_at: None }
}

fn time(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap()
}

/// P1: the five prayers, stable keys, round-trip parse, fixed order.
#[test]
fn p1_prayer_identity_is_stable() {
    assert_eq!(Prayer::ALL.len(), 5);
    for p in Prayer::ALL {
        assert_eq!(Prayer::parse(p.key()), Some(p));
        assert_eq!(p.display_name().to_lowercase(), p.key());
    }
    assert_eq!(
        Prayer::ALL.map(|p| p.key()),
        ["fajr", "dhuhr", "asr", "maghrib", "isha"]
    );
    assert_eq!(Prayer::parse("shuruq"), None, "shuruq is not a prayer");
}

/// P1: every state key is stable and derived from the prayer key —
/// config/UI identity the desktop and the TUI share.
#[test]
fn p1_state_keys_cover_every_prayer_and_kind() {
    for p in Prayer::ALL {
        assert_eq!(
            PrayerEventKind::Adhan(p).state_key(),
            format!("{}/adhan", p.key())
        );
        assert_eq!(
            PrayerEventKind::Iqama(p).state_key(),
            format!("{}/iqama", p.key())
        );
    }
    assert_eq!(PrayerEventKind::Shuruq.state_key(), "shuruq");
}

/// P1: labels and prayer() cover every kind (the doctest exercises these
/// too, but doctests never feed the coverage profdata).
#[test]
fn p1_labels_and_prayer_cover_every_kind() {
    for p in Prayer::ALL {
        let adhan = PrayerEventKind::Adhan(p);
        assert_eq!(adhan.label(), format!("{} adhan", p.display_name()));
        assert_eq!(adhan.prayer(), Some(p));
        let iqama = PrayerEventKind::Iqama(p);
        assert_eq!(iqama.label(), format!("{} iqama", p.display_name()));
        assert_eq!(iqama.prayer(), Some(p));
    }
    let shuruq = PrayerEventKind::Shuruq;
    assert_eq!(shuruq.label(), "Shurouq");
    assert_eq!(shuruq.prayer(), None);
}

/// P2: the adhan still ahead today wins.
#[test]
fn p2_next_event_picks_the_upcoming_adhan() {
    let t = today(adhan("05:30", "07:07", "13:21", "16:37", "19:24", "21:05"));
    let next = t.next_event(time(10, 0)).unwrap();
    assert_eq!(next.kind, PrayerEventKind::Adhan(Prayer::Dhuhr));
    assert_eq!(next.minutes_remaining, 201);
    assert_eq!(next.at, date().and_hms_opt(13, 21, 0).unwrap());
}

/// P2: shuruq is an event (informational) — between the Fajr adhan and
/// shuruq it is what's next.
#[test]
fn p2_shuruq_is_an_event() {
    let t = today(adhan("05:30", "07:07", "13:21", "16:37", "19:24", "21:05"));
    let next = t.next_event(time(6, 0)).unwrap();
    assert_eq!(next.kind, PrayerEventKind::Shuruq);
    assert_eq!(next.kind.label(), "Shurouq");
    assert_eq!(next.kind.prayer(), None);
    assert_eq!(next.kind.state_key(), "shuruq");
}

/// P2: with iqama instants, the iqama is its own event.
#[test]
fn p2_iqama_is_an_event_when_resolved() {
    let mut t =
        today(adhan("05:30", "07:07", "13:21", "16:37", "19:24", "21:05"));
    let d = date();
    t.iqama_at = Some(DailyIqamaInstants {
        fajr: d.and_hms_opt(5, 47, 0).unwrap(),
        dhuhr: d.and_hms_opt(13, 35, 0).unwrap(),
        asr: d.and_hms_opt(16, 55, 0).unwrap(),
        maghrib: d.and_hms_opt(19, 40, 0).unwrap(),
        isha: d.and_hms_opt(21, 20, 0).unwrap(),
    });
    let next = t.next_event(time(13, 22)).unwrap();
    assert_eq!(next.kind, PrayerEventKind::Iqama(Prayer::Dhuhr));
    assert_eq!(next.kind.state_key(), "dhuhr/iqama");
}

/// P2 / THE defect pin: an iqama that rolls past midnight (23:50 adhan +
/// 50 min = tomorrow 00:40) is strictly future all evening — the string
/// sort the desktop shipped sorted it back onto today. Rollover-correct
/// instants get this right; this test pins the promotion.
#[test]
fn p2_rollover_iqama_belongs_to_tomorrow() {
    let mut t =
        today(adhan("05:30", "07:07", "13:21", "16:37", "23:50", "23:55"));
    let d = date();
    t.iqama_at = Some(DailyIqamaInstants {
        fajr: d.and_hms_opt(5, 47, 0).unwrap(),
        dhuhr: d.and_hms_opt(13, 35, 0).unwrap(),
        asr: d.and_hms_opt(16, 55, 0).unwrap(),
        maghrib: d.and_hms_opt(23, 59, 0).unwrap(),
        isha: d.succ_opt().unwrap().and_hms_opt(0, 40, 0).unwrap(),
    });
    let next = t.next_event(time(23, 30)).unwrap();
    assert_eq!(next.kind, PrayerEventKind::Adhan(Prayer::Maghrib));
    // After the maghrib adhan (and the 23:59 maghrib iqama), the isha
    // iqama is tomorrow 00:40 — never "today 00:40" (the string-sort
    // defect the desktop shipped would have served exactly that).
    let next =
        t.next_event(NaiveTime::from_hms_opt(23, 59, 30).unwrap()).unwrap();
    assert_eq!(next.kind.state_key(), "isha/iqama");
    assert_eq!(next.kind, PrayerEventKind::Iqama(Prayer::Isha));
    assert_eq!(next.at, d.succ_opt().unwrap().and_hms_opt(0, 40, 0).unwrap());
    assert_eq!(next.minutes_remaining, 40, "23:59:30 -> 00:40 truncates to 40");
}

/// P2: everything today passed → the first adhan of tomorrow.
#[test]
fn p2_all_passed_falls_to_tomorrows_first_adhan() {
    let t = today(adhan("05:30", "07:07", "13:21", "16:37", "19:24", "21:05"));
    let next = t.next_event(time(23, 0)).unwrap();
    assert_eq!(next.kind, PrayerEventKind::Adhan(Prayer::Fajr));
    assert_eq!(
        next.at,
        date().succ_opt().unwrap().and_hms_opt(5, 30, 0).unwrap()
    );
    assert_eq!(next.minutes_remaining, 6 * 60 + 30);
}

/// P2: hostile `HH:MM` fields are inert — skipped, never misparsed; the
/// next parseable prayer answers. All-hostile → `None`, never a guess.
#[test]
fn p2_hostile_times_are_inert() {
    let t = today(adhan("25:70", "07:07", "bogus", "16:37", "19:24", "21:05"));
    let next = t.next_event(time(6, 0)).unwrap();
    assert_eq!(
        next.kind,
        PrayerEventKind::Shuruq,
        "hostile fajr skipped; the still-ahead shuruq answers"
    );
    let next = t.next_event(time(10, 0)).unwrap();
    assert_eq!(
        next.kind,
        PrayerEventKind::Adhan(Prayer::Asr),
        "hostile fajr and dhuhr skipped; the next parseable adhan answers"
    );

    let mut all_hostile = today(adhan("x", "y", "z", "w", "v", "u"));
    all_hostile.date = date();
    assert!(all_hostile.next_event(time(0, 0)).is_none());
}

/// P2: no iqama resolved → adhan-only schedule (plus shuruq), walking the
/// whole day one event at a time.
#[test]
fn p2_no_iqama_adhan_only() {
    let t = today(adhan("05:30", "07:07", "13:21", "16:37", "19:24", "21:05"));
    let mut seen = Vec::new();
    let mut now = NaiveTime::from_hms_opt(5, 0, 0).unwrap();
    while let Some(event) = t.next_event(now) {
        if event.at.date() != date() {
            break; // tomorrow's fajr — the walk is complete (not re-counted)
        }
        let key = event.kind.state_key();
        if seen.last() != Some(&key) {
            seen.push(key);
        }
        now = event.at.time();
        now += chrono::Duration::minutes(1);
    }
    assert_eq!(
        seen,
        [
            "fajr/adhan",
            "shuruq",
            "dhuhr/adhan",
            "asr/adhan",
            "maghrib/adhan",
            "isha/adhan"
        ],
        "the day's event sequence, ending on tomorrow's fajr"
    );
}

// ---------------------------------------------------------------- P4/P5

fn the_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 6).unwrap()
}

/// The fixture calendar defines day 1 of month 1 (January).
fn view_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()
}

/// A page fixture for the Today-view projection.
fn view_conf() -> mawaqit_api::ConfData {
    let page = r#"<html><script>var confData = {"times":["05:27","06:37","13:21","16:37","19:24","20:51"],
        "calendar":[{"1":["05:27","06:37","07:07","13:21","16:37","19:24","20:51"]}],
        "name":"Grande Mosquée","jumua":"13:50","jumua2":"14:30",
        "image":"https://x.test/a.jpg",
        "announcements":[
            {"id":42,"title":"first","start_date":"2026-10-01","end_date":"2026-10-31"},
            {"id":"second","title":"second"},
            {"title":"third"}
        ]};</script></html>"#.to_string();
    mawaqit_api::parse_page(&page, "view").expect("fixture parses")
}

/// P4: one call carries everything the Today screen needs — metadata,
/// the resolved times and keyed announcements.
#[test]
fn p4_today_view_carries_the_full_payload() {
    let conf = view_conf();
    let view = conf.today_view(view_date()).expect("day resolves");
    assert_eq!(view.mosque_name.as_deref(), Some("Grande Mosquée"));
    assert_eq!(view.jumua.as_deref(), Some("13:50"));
    assert_eq!(view.jumua2.as_deref(), Some("14:30"));
    assert_eq!(view.image.as_deref(), Some("https://x.test/a.jpg"));
    assert!(view.imsak_mode, "6 times = imsak mode (single-source rule)");
    assert_eq!(view.times.adhan.dhuhr, "13:21");
    assert_eq!(view.announcements.len(), 3);
    assert_eq!(view.announcements[0].key, "42");
    assert_eq!(view.announcements[1].key, "second");
    assert!(
        view.announcements[2].key.starts_with("hash-"),
        "id-less announcements get a stable content hash"
    );
}

/// P4: keys are stable across re-projections and content-sensitive.
#[test]
fn p4_announcement_keys_are_stable_and_content_sensitive() {
    let a = view_conf().today_view(view_date()).unwrap();
    let b = view_conf().today_view(view_date()).unwrap();
    assert_eq!(
        a.announcements[2].key, b.announcements[2].key,
        "same content → same hash key"
    );
    assert_ne!(a.announcements[0].key, a.announcements[1].key);
}

/// P4: `today_view` surfaces the calendar errors (`NoCalendar`), never a
/// fabricated view.
#[test]
fn p4_today_view_errors_without_a_calendar() {
    let page = r#"<html><script>var confData = {"times":["05:27","06:37","13:21","16:37","19:24"]};</script></html>"#;
    let conf = mawaqit_api::parse_page(page, "empty").unwrap_err();
    let _ = conf; // parse fails outright; a calendar-less conf via default:
    let conf = mawaqit_api::ConfData::default();
    assert!(conf.today_view(the_date()).is_err());
}

/// P5: the active-window contract — closed bounds, open bounds, both
/// missing, and hostile bounds = `None` (unknown, never a guess).
#[test]
fn p5_is_active_on_windows() {
    use mawaqit_api::Announcement;
    let mid = the_date();
    let ann = |start: Option<&str>, end: Option<&str>| Announcement {
        id: None,
        title: None,
        content: None,
        image: None,
        video: None,
        start_date: start.map(str::to_string),
        end_date: end.map(str::to_string),
        extra: Default::default(),
    };

    assert_eq!(
        ann(Some("2026-10-01"), Some("2026-10-31")).is_active_on(mid),
        Some(true)
    );
    assert_eq!(
        ann(Some("2026-10-01"), Some("2026-10-31"))
            .is_active_on(NaiveDate::from_ymd_opt(2026, 11, 1).unwrap()),
        Some(false)
    );
    assert_eq!(ann(Some("2026-10-06"), None).is_active_on(mid), Some(true));
    assert_eq!(ann(None, Some("2026-10-05")).is_active_on(mid), Some(false));
    assert_eq!(
        ann(None, None).is_active_on(mid),
        Some(true),
        "no window = always active"
    );
    assert_eq!(
        ann(Some("soon"), None).is_active_on(mid),
        None,
        "hostile start = unknown"
    );
    assert_eq!(
        ann(None, Some("later")).is_active_on(mid),
        None,
        "hostile end = unknown"
    );
}
