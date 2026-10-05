//! Hostile semantics: attacks that use *valid* JSON and *valid* structures to
//! corrupt behavior instead of crashing it. The panic-safety layer
//! (`ut/corpus.rs`, `cargo fuzz`) already proves garbage can't break the
//! parser; this suite proves garbage can't lie either.
//!
//! Everything goes through [`mawaqit_api::parse_page`] — the same entry point
//! network data takes — never through a direct serde deserialize, which is
//! not what a hostile response hits.
//!
//! All red-team findings in this suite (F4, F5, F6) are fixed and pinned as
//! always-run regression tests: each one failed against the vulnerable
//! client, then turned green with the fix.

use chrono::Datelike;
use mawaqit_api::{ConfData, month_times, page_url, parse_page};
use serde_json::json;

use crate::common::valid_hhmm;

fn conf(value: serde_json::Value) -> ConfData {
    let page = format!("<html><script>var confData = {value};</script></html>");
    parse_page(&page, "redteam")
        .expect("test builds structurally valid confData")
}

/// 2026-01-01 — the calendar fixtures below define day 1 of month 1.
fn the_date() -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()
}

fn valid_row() -> Vec<String> {
    ["06:30", "08:00", "13:00", "15:30", "17:45", "19:15"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

// ------------------------------------------------------------ contract tests

/// Documented inference: imsak mode is decided by `times.len() == 6` alone,
/// at the scraper boundary. The calendar row shape is an independent,
/// inherently ambiguous signal — this pins the single-source rule so it
/// cannot drift silently.
#[test]
fn imsak_mode_is_decided_by_times_count_alone() {
    for (times_len, imsak) in [(5usize, false), (6, true), (7, false)] {
        let c = conf(json!({
            "times": vec!["06:30"; times_len],
            "calendar": [ { "1": valid_row() } ],
        }));
        assert_eq!(c.imsak_mode, imsak, "times.len() == {times_len}");
        // Whatever the mode, the calendar pipeline still works.
        let _ = month_times(&c, the_date().month()).unwrap();
        let _ = mawaqit_api::times_for_date(&c, the_date()).unwrap();
    }

    // Fewer than 5 time strings: the real boundary refuses the page outright.
    let page = format!(
        "<script>var confData = {}; </script>",
        json!({ "times": ["06:30", "07:00"], "calendar": [ { "1": valid_row() } ] })
    );
    assert!(
        parse_page(&page, "redteam").is_err(),
        "short `times` is a hard error"
    );
}

/// "+N" offsets that roll into the next day still resolve to a valid HH:MM
/// string (next-day semantics are inherent to HH:MM display; the frontend
/// handles the rollover via its own event builder).
#[test]
fn iqama_offset_rollover_stays_valid_hhmm() {
    let c = conf(json!({
        "times": ["23:30", "00:00", "00:00", "00:00", "00:00"],
        "calendar": [ { "1": ["23:30","23:45","23:50","23:55","23:58","23:59"] } ],
        "iqamaCalendar": [ { "1": ["+600", "+600", "+600", "+600", "+600"] } ],
    }));
    let today = mawaqit_api::times_for_date(&c, the_date()).unwrap();
    let iq = today.iqama.expect("iqama present");
    for t in [&iq.fajr, &iq.dhuhr, &iq.asr, &iq.maghrib, &iq.isha] {
        assert!(valid_hhmm(t), "{t} is not valid HH:MM");
    }
    assert_eq!(iq.fajr, "09:30"); // 23:30 + 600 min, next-day wall clock
}

/// Day keys are strings from the wire: exotic keys are skipped, never parsed
/// as something else, never counted as days.
#[test]
fn exotic_month_keys_are_skipped() {
    let mut month = std::collections::BTreeMap::new();
    for key in ["١", "٣١", "1e2", "4294967296", " 1", "1 ", "1.0", "0x1", "٣"]
    {
        month.insert(key.to_string(), valid_row());
    }
    let c = conf(json!({
        "times": ["06:30", "08:00", "13:00", "15:30", "17:45"],
        "calendar": [month],
    }));
    let days = month_times(&c, 1).unwrap().days;
    assert!(days.is_empty(), "no exotic key counts as a day, got {days:?}");
}

/// A calendar full of structurally-broken rows fails safe: empty month for
/// `month_times`, hard `NoCalendar` error for `times_for_date`.
#[test]
fn all_broken_rows_fail_safe() {
    let row = vec!["garbage".to_string(); 3];
    let month: std::collections::BTreeMap<String, Vec<String>> =
        (1..=31).map(|d| (d.to_string(), row.clone())).collect();
    let c = conf(json!({
        "times": ["06:30", "08:00", "13:00", "15:30", "17:45"],
        "calendar": [month],
    }));
    assert!(month_times(&c, 1).unwrap().days.is_empty());
    assert!(mawaqit_api::times_for_date(&c, the_date()).is_err());
}

/// Non-string scalars in display fields collapse to None at the scraper
/// boundary; they never fail the page and never surface as "null"/"[object]"
/// strings.
#[test]
fn non_string_display_fields_become_none() {
    let c = conf(json!({
        "times": ["06:30", "08:00", "13:00", "15:30", "17:45"],
        "calendar": [ { "1": valid_row() } ],
        "name": 12345,
        "jumua": { "x": 1 },
        "jumua2": [1, 2],
        "image": true,
        "shuruq": 7.5,
    }));
    assert!(c.name.is_none());
    assert!(c.jumua.is_none());
    assert!(c.jumua2.is_none());
    assert!(c.image.is_none());
    assert!(c.shuruq.is_none());
}

/// Every combination of times-count × calendar-row-shape survives the whole
/// pipeline without panicking (redundant with the fuzz corpus, but stated as
/// an explicit matrix so a regression in one cell is named, not discovered).
#[test]
fn layout_confusion_matrix_never_panics() {
    let rows: Vec<Vec<String>> = vec![
        vec![],                                            // 0
        ["06:30"].iter().map(|s| s.to_string()).collect(), // 1
        valid_row(),                                       // 6 (normal)
        ["05:27", "06:37", "07:07", "13:21", "16:37", "19:24", "20:51"]
            .iter()
            .map(|s| s.to_string())
            .collect(), // 7 (imsak)
        vec![
            "06:30".into(),
            "xx:yy".into(),
            "13:00".into(),
            "15:30".into(),
            "17:45".into(),
            "19:15".into(),
        ], // broken middle
    ];
    for times_len in [5usize, 6, 7] {
        for row in &rows {
            let page = format!(
                "<script>var confData = {}; </script>",
                json!({
                    "times": vec!["06:30"; times_len],
                    "calendar": [ { "1": row } ],
                    "iqamaCalendar": [ { "1": row } ],
                })
            );
            if let Ok(c) = parse_page(&page, "redteam") {
                let _ = month_times(&c, 1);
                let _ = mawaqit_api::month_iqama_times(&c, 1);
                let _ = mawaqit_api::times_for_date(&c, the_date());
            }
        }
    }
}

/// The URL builder keeps benign slugs inside the mosque page namespace.
#[test]
fn page_url_is_well_formed_for_benign_slugs() {
    let url = page_url("https://mawaqit.net", "grande-mosquee-de-paris");
    assert_eq!(url, "https://mawaqit.net/en/grande-mosquee-de-paris");
}

// ------------------------------------------------ findings (regression-pinned)

/// FINDING F4 — surfaced times are never validated. `daily_from_row` passes
/// adhan strings through verbatim; "25:70" at the fajr position reaches the
/// UI, where the frontend's `setHours(25, 70)` silently rolls into another
/// day and the countdown points at a made-up time. FIXED: a row surfacing
/// any non-strict-HH:MM value is rejected whole, so the day drops out of the
/// month (the same "the day errors out instead of showing fabricated times"
/// semantic as layout-mismatched rows). The original probe expected the
/// hostile day to resolve garbage-free — impossible without fabricating a
/// fajr — so it now asserts the day refuses to resolve while its valid
/// neighbor still surfaces.
#[test]
fn finding_f4_surfaced_times_are_always_valid_hhmm() {
    let c = conf(json!({
        "times": ["25:70", "99:99", "ab:cd", "7:5", "+30"],
        "calendar": [ { "1": ["25:70","06:37","13:21","16:37","19:24","20:51"],
                         "2": ["06:30","08:00","13:00","15:30","17:45","19:15"] } ],
    }));

    // The hostile day must not resolve at all.
    assert!(
        mawaqit_api::times_for_date(&c, the_date()).is_err(),
        "a day surfacing an invalid time must not resolve"
    );

    // Its valid neighbor is untouched and surfaces only valid times.
    let day2 = chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
    let today =
        mawaqit_api::times_for_date(&c, day2).expect("valid day resolves");
    for t in [
        today.adhan.fajr.as_str(),
        today.adhan.shurouq.as_str(),
        today.adhan.dhuhr.as_str(),
        today.adhan.asr.as_str(),
        today.adhan.maghrib.as_str(),
        today.adhan.isha.as_str(),
    ] {
        assert!(valid_hhmm(t), "surfaced {t:?} is not a valid HH:MM time");
    }

    // The month view carries exactly the surviving day.
    let days = month_times(&c, 1).unwrap().days;
    assert_eq!(days.iter().map(|d| d.day).collect::<Vec<_>>(), vec![2]);

    // The iqama passthrough cannot smuggle non-display times either: garbage
    // falls back to the adhan value like it always has.
    let hostile_iq = conf(json!({
        "times": ["06:30", "08:00", "13:00", "15:30", "17:45", "19:15"],
        "calendar": [ { "1": ["06:30","08:00","13:00","15:30","17:45","19:15"] } ],
        "iqamaCalendar": [ { "1": ["25:70","06:45","13:20","+20","18:00"] } ],
    }));
    let today =
        mawaqit_api::times_for_date(&hostile_iq, the_date()).expect("resolves");
    let iq = today.iqama.expect("iqama present");
    for t in [
        iq.fajr.as_str(),
        iq.dhuhr.as_str(),
        iq.asr.as_str(),
        iq.maghrib.as_str(),
        iq.isha.as_str(),
    ] {
        assert!(
            valid_hhmm(t),
            "surfaced iqama {t:?} is not a valid HH:MM time"
        );
    }
    assert_eq!(iq.fajr, today.adhan.fajr, "hostile iqama fell back to adhan");
}

/// FINDING F5 — lenient day keys duplicate days. Rust's integer FromStr
/// accepts "01" and "+1", so a hostile month carrying "1", "01" and "+1"
/// yields three entries for day 1; `times_for_date` then silently picks
/// whichever sorts first (BTreeMap order, not data quality).
/// FIXED: `month_times` deduplicates by parsed day, and the canonical
/// decimal key always wins over its variants.
#[test]
fn finding_f5_duplicate_day_keys_yield_one_day() {
    let mut month = std::collections::BTreeMap::new();
    month.insert("1".to_string(), valid_row());
    month.insert("01".to_string(), vec!["00:00".to_string(); 6]); // attacker's
    // row
    month.insert("+1".to_string(), vec!["12:34".to_string(); 6]); // attacker's
    // row
    let c = conf(json!({
        "times": ["06:30", "08:00", "13:00", "15:30", "17:45"],
        "calendar": [month],
    }));
    let days = month_times(&c, 1).unwrap().days;
    assert_eq!(days.len(), 1, "day 1 appears {} times", days.len());
    // The canonical row wins: "1" carries valid_row, not the attacker's.
    assert_eq!(days[0].times.dhuhr, "13:00", "canonical key must win");
}

/// FINDING F6 — control and bidi-override characters flow verbatim into
/// strings that end up in the window title, the tray tooltip and OS
/// notifications (`Mawaqit: <name>`). U+202E can visually reverse that text
/// (spoofed tray state), C0 controls corrupt terminal logs.
/// FIXED: C0/C1 and bidi/isolate controls are stripped from free-text
/// display fields at the `ConfData` boundary (`scraper::sanitize_text`).
#[test]
fn finding_f6_display_strings_carry_no_control_or_bidi_characters() {
    let evil = "\u{202E}esreveR\u{202D}\u{0000}\u{007F}\n\t";
    let c = conf(json!({
        "times": ["06:30", "08:00", "13:00", "15:30", "17:45"],
        "calendar": [ { "1": valid_row() } ],
        "name": evil,
        "jumua": evil,
    }));
    let today = mawaqit_api::times_for_date(&c, the_date()).unwrap();
    let strings: Vec<&str> = [
        c.name.as_deref(),
        c.jumua.as_deref(),
        Some(today.adhan.fajr.as_str()),
    ]
    .into_iter()
    .flatten()
    .collect();
    for s in strings {
        assert!(
            !s.chars().any(|ch| ch.is_control() || matches!(ch,
                '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}')),
            "display string carries hostile control/bidi characters: {s:?}"
        );
    }
}

/// FINDING F6 (review follow-up) — `is_control()` alone misses the invisible
/// Format (Cf) characters: zero-width space/joiner, BOM, soft hyphen and the
/// unassigned-invisible U+2065 are not Control, yet they spoof display
/// strings and dodge search/dedupe just like the bidi overrides do. The
/// sanitizer strips the whole Cf family.
#[test]
fn finding_f6b_invisible_format_characters_are_stripped() {
    let evil = "Mas\u{200B}jid\u{FEFF}Al\u{00AD}Noor\u{2065}X";
    let c = conf(json!({
        "times": ["06:30", "08:00", "13:00", "15:30", "17:45"],
        "calendar": [ { "1": valid_row() } ],
        "name": evil,
    }));
    assert_eq!(
        c.name.as_deref(),
        Some("MasjidAlNoorX"),
        "invisible format characters must not survive the boundary"
    );
}

/// FINDING F21 — the Cf range table in `is_invisible` has gaps: the Arabic
/// number marks U+0890–0891 (Unicode 14), the shorthand format controls
/// U+1BCA0–1BCA3 and the Egyptian format controls U+13440–13455 (Unicode 15)
/// are all Category Cf — invisible, spoofing and search-dodging exactly like
/// the entries the table already carries. The same review caught the
/// announcement `start_date`/`end_date` fields: free-text wire fields like
/// their sanitized siblings `title`/`content`, yet passed through verbatim.
#[test]
fn finding_f21_full_cf_table_and_announcement_dates_are_sanitized() {
    let evil = "A\u{0890}B\u{0891}C\u{1BCA0}D\u{1BCA3}E\u{13440}F\u{13455}G";
    let c = conf(json!({
        "times": ["06:30", "08:00", "13:00", "15:30", "17:45"],
        "calendar": [ { "1": valid_row() } ],
        "name": evil,
        "announcements": [
            { "title": "\u{200B}t", "start_date": evil, "end_date": "\u{202E}x" }
        ],
    }));
    assert_eq!(
        c.name.as_deref(),
        Some("ABCDEFG"),
        "every Category Cf character must be stripped, whatever table entry covers it"
    );
    let ann = &c.announcements[0];
    assert_eq!(
        ann.start_date.as_deref(),
        Some("ABCDEFG"),
        "announcement start_date is free text and must be sanitized"
    );
    assert_eq!(ann.end_date.as_deref(), Some("x"));
}

/// FINDING F26 — `find_conf_data_json` re-scanned after every failed
/// candidate: a failed balanced scan had consumed to EOF, yet the loop
/// advanced one marker and scanned again, so k unbalanced `confData = {`
/// markers cost O(k·n) CPU on one request (the reqwest timeout does not
/// cover parsing). The tightened contract: once a candidate's balanced
/// scan runs past the end of input, the scan *ends* — a balanced literal
/// nested inside a broken object is not an assignment the page made.
#[test]
fn finding_f26_a_failed_candidate_ends_the_scan() {
    // The first candidate is unbalanced to EOF; a *balanced* literal sits
    // inside it. The old loop found the nested one; the page itself is
    // broken JavaScript — there is no real confData assignment.
    let good = r#"{"times":["05:27","06:37","13:21","16:37","19:24"],"calendar":[{"1":["05:27","06:37","07:07","13:21","16:37","19:24"]}]}"#;
    let page = format!(
        r#"<script>var confData = {{ junk, oops: confData = {good}; </script>"#
    );
    assert!(
        parse_page(&page, "f26").is_err(),
        "a balanced literal nested in a failed candidate is not the page's confData"
    );
}

/// FINDING F28 / ADR-0015 — `iqama_at` instants are mosque-local wall
/// clock; the zone must come from the page through the validated
/// [`ConfData::timezone`] accessor, never from a guess. A hostile,
/// malformed or absent designator is "no zone published", never a value.
#[test]
fn finding_f28_timezone_accessor_validates_the_wire_designator() {
    let base = json!({
        "times": ["06:30", "08:00", "13:00", "15:30", "17:45"],
        "calendar": [ { "1": valid_row() } ],
    });
    let mut good = base.clone();
    good["timezone"] = json!("Europe/Paris");
    assert_eq!(conf(good).timezone(), Some("Europe/Paris"));

    let hostile = [
        json!(""),                     // empty
        json!("../.."),                // traversal
        json!("/etc/passwd"),          // absolute path
        json!("Europe/Paris\nEVIL"),   // control character
        json!("E\u{200B}urope/Paris"), // invisible character
        json!(42),                     // not a string
        json!("A".repeat(65)),         // over the 64-byte bound
        json!("Etc/../Pass"),          // `..` segment
    ];
    for bad in hostile {
        let mut page = base.clone();
        page["timezone"] = bad.clone();
        assert_eq!(
            conf(page).timezone(),
            None,
            "hostile designator {bad:?} must be None"
        );
    }

    // Absent field: the mosque publishes no zone.
    assert_eq!(conf(base).timezone(), None);
}

/// FINDING F29a — `++5` rode the lenient sign parse to "+5": one
/// `strip_prefix('+')` left a second `+`, which `i64::from_str` accepts.
/// The strict grammar is one sign then ASCII digits; anything else falls
/// back to the adhan time like any unparseable entry (never a clamped
/// near-miss).
#[test]
fn finding_f29a_double_sign_is_not_an_offset() {
    // Normal mode: 5 times, the calendar row carries the shuruq column.
    let row = ["05:27", "06:37", "07:07", "13:21", "16:37", "19:24"];
    let c = conf(json!({
        "times": ["05:27", "07:07", "13:21", "16:37", "19:24"],
        "calendar": [ { "1": row } ],
        "iqamaCalendar": [ { "1": ["++5", "+-5", "+ 5", "13:45", "+10"] } ],
    }));
    let today = mawaqit_api::times_for_date(&c, the_date()).unwrap();
    let iq = today.iqama.expect("iqama present");
    assert_eq!(
        iq.fajr, "05:27",
        "'++5' must fall back to the adhan, not adhan+5"
    );
    assert_eq!(iq.dhuhr, "07:07", "'+-5' must not resolve to a negative clamp");
    assert_eq!(iq.asr, "13:26", "'+ 5' stays a valid +5 offset (13:21 + 5)");
    assert_eq!(iq.maghrib, "13:45", "absolute times pass through");
    assert_eq!(iq.isha, "19:34", "'+10' still resolves (19:24 + 10)");
}

/// FINDING F29c — payload-bearing error variants interpolated raw hostile
/// strings into `Display` output (logs, tray toasts, terminals): a hostile
/// mosque id, search word or wire value could be megabytes of control
/// characters. Every payload is now sanitized and truncated at
/// construction, so `Display` is bounded no matter the input.
#[test]
fn finding_f29c_error_payloads_are_bounded_and_sanitized() {
    let hostile = format!("{}\u{202E}", "x".repeat(4096));
    let err =
        parse_page("<html>no confData here</html>", &hostile).unwrap_err();
    let rendered = err.to_string();
    assert!(
        rendered.len() < 4096,
        "error display must be bounded, got {} bytes",
        rendered.len()
    );
    assert!(
        !rendered.chars().any(char::is_control),
        "error display must carry no control characters: {rendered:?}"
    );

    // A multi-byte hostile string exercises the char-boundary walk: the
    // 128-byte cut lands inside an 'é' and must back up, not panic or
    // split a code point.
    let multibyte = "€".repeat(200); // 3-byte chars: byte 128 is mid-char
    let err =
        parse_page("<html>no confData here</html>", &multibyte).unwrap_err();
    let rendered = err.to_string();
    assert!(rendered.len() < 4096, "bounded, got {}", rendered.len());
    assert!(std::str::from_utf8(rendered.as_bytes()).is_ok());
}
