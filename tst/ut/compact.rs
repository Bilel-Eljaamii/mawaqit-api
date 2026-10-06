//! Unit tests for the MQTC compact binary codec (ADR-0013), through the
//! public API: roundtrips in both record formats, CRC/hostile-input
//! rejection (nothing may panic — global invariant #1), the C1 rollover
//! bitfield, Jumu'ah header fields, year-end boundaries, and the payload
//! emitters.

use chrono::{NaiveDate, NaiveTime};
use mawaqit_api::{
    compact::{
        CompactCalendarBuilder, CompactCalendarView, CompactDayInput,
        CompactError, CompactIqamaInput, ScopeType,
    },
    prayer::{Prayer, PrayerEventKind},
};

/// Deterministic xorshift64* — no rng dependency in the offline suite.
struct Xorshift(u64);

impl Xorshift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn byte_flip_index(&mut self, len: usize) -> (usize, u8) {
        (self.next() as usize % len, 1u8 << (self.next() % 8))
    }
}

const DAY: [u16; 6] = [330, 450, 780, 990, 1140, 1260];

/// Per-day jitter: keeps the drift deterministic and in range even for a
/// 365-day payload.
fn drift(i: u32) -> u16 {
    (i % 3) as u16
}

fn iqama(mins: [(u16, bool); 5]) -> [Option<CompactIqamaInput>; 5] {
    mins.map(|(minutes, rollover)| {
        Some(CompactIqamaInput { minutes, rollover })
    })
}

fn full_day() -> CompactDayInput {
    CompactDayInput {
        adhan: DAY,
        iqama: iqama([
            (345, false),
            (795, false),
            (1005, false),
            (1155, false),
            (10, true), // isha iqama past midnight — C1 rollover
        ]),
        day_flags: 0,
    }
}

fn builder(
    start: NaiveDate,
    days: usize,
    fajr_relative: bool,
) -> CompactCalendarBuilder {
    let mut b =
        CompactCalendarBuilder::new(start, ScopeType::Year, fajr_relative);
    for i in 0..days {
        let mut day = full_day();
        // Jitter each day deterministically so day-index mixups cannot
        // pass a roundtrip by accident.
        let d = drift(i as u32);
        for slot in day.adhan.iter_mut() {
            *slot += d;
        }
        for entry in day.iqama.iter_mut().flatten() {
            entry.minutes += d;
        }
        b.push_day(day);
    }
    b
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("test date")
}

#[test]
fn direct_roundtrip_preserves_every_field() {
    let start = date(2026, 10, 5);
    let bytes = builder(start, 3, false).to_bytes().unwrap();
    let view = CompactCalendarView::from_bytes(&bytes).unwrap();

    assert_eq!(view.day_count(), 3);
    assert!(!view.is_fajr_relative());
    assert_eq!(view.start_date().unwrap(), start);
    assert_eq!(view.end_date().unwrap(), date(2026, 10, 7));

    for i in 0..3u32 {
        let day =
            view.times_for_date(start + chrono::Days::new(i as u64)).unwrap();
        for (slot, expected) in
            ["fajr", "shurouq", "dhuhr", "asr", "maghrib", "isha"]
                .iter()
                .zip(DAY)
        {
            let got = match *slot {
                "fajr" => day.adhan.fajr,
                "shurouq" => day.adhan.shurouq,
                "dhuhr" => day.adhan.dhuhr,
                "asr" => day.adhan.asr,
                "maghrib" => day.adhan.maghrib,
                _ => day.adhan.isha,
            };
            assert_eq!(
                got.minutes_from_midnight(),
                expected + drift(i),
                "{slot} day {i}"
            );
            assert!(got.hours() < 24 && got.minutes() < 60, "{slot} day {i}");
        }
        let iqama = day.iqama.expect("iqama packed");
        assert!(iqama.fajr.is_valid());
        assert!(!iqama.fajr.is_rollover());
        // The isha iqama at 00:1x must carry the C1 rollover bit.
        assert!(iqama.isha.is_rollover());
        assert_eq!(iqama.isha.minutes_from_midnight(), 10 + drift(i));
        assert_eq!(
            iqama.isha.to_hhmm().as_str(),
            format!("00:{:02}", 10 + drift(i))
        );
    }
}

#[test]
fn fajr_relative_roundtrip_preserves_every_field() {
    let start = date(2026, 10, 5);
    let bytes = builder(start, 3, true).to_bytes().unwrap();
    let view = CompactCalendarView::from_bytes(&bytes).unwrap();
    assert!(view.is_fajr_relative());

    for i in 0..3u32 {
        let day =
            view.times_for_date(start + chrono::Days::new(i as u64)).unwrap();
        assert_eq!(day.adhan.fajr.minutes_from_midnight(), DAY[0] + drift(i));
        assert_eq!(day.adhan.isha.minutes_from_midnight(), DAY[5] + drift(i));
        let iqama = day.iqama.expect("iqama packed");
        // The rollover survives delta encoding: 00:1x after a 21:00+ adhan.
        assert!(iqama.isha.is_rollover());
        assert_eq!(iqama.isha.minutes_from_midnight(), 10 + drift(i));
        // Same-day iqamas stay same-day.
        assert!(!iqama.fajr.is_rollover());
        assert_eq!(iqama.fajr.minutes_from_midnight(), 345 + drift(i));
    }
}

#[test]
fn payload_sizes_match_the_layout_contract() {
    let start = date(2026, 1, 1);
    let direct = builder(start, 365, false).to_bytes().unwrap();
    let fajr_rel = builder(start, 365, true).to_bytes().unwrap();
    // 24-byte header + 24 B/day direct, + 20 B/day fajr-relative.
    assert_eq!(direct.len(), 24 + 365 * 24);
    assert_eq!(fajr_rel.len(), 24 + 365 * 20);
}

#[test]
fn crc_bit_flip_is_rejected_before_any_lookup() {
    let start = date(2026, 10, 5);
    let mut bytes = builder(start, 3, false).to_bytes().unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    match CompactCalendarView::from_bytes(&bytes) {
        Err(CompactError::ChecksumMismatch { .. }) => {}
        other => panic!("expected ChecksumMismatch, got {other:?}"),
    }
}

#[test]
fn truncated_and_tiny_buffers_are_rejected() {
    let start = date(2026, 10, 5);
    let bytes = builder(start, 3, false).to_bytes().unwrap();
    // Shorter than a header.
    for len in 0..24 {
        assert!(
            matches!(
                CompactCalendarView::from_bytes(&bytes[..len]),
                Err(CompactError::BufferTooSmall)
            ),
            "len {len}"
        );
    }
    // Header present, day records cut short (both formats).
    let rel = builder(start, 3, true).to_bytes().unwrap();
    for cut in [24, 47, 70] {
        assert!(matches!(
            CompactCalendarView::from_bytes(&bytes[..cut]),
            Err(CompactError::BufferTooSmall)
        ));
        assert!(matches!(
            CompactCalendarView::from_bytes(&rel[..cut]),
            Err(CompactError::BufferTooSmall)
        ));
    }
}

#[test]
fn bad_magic_and_version_are_rejected() {
    let start = date(2026, 10, 5);
    let bytes = builder(start, 1, false).to_bytes().unwrap();

    let mut bad_magic = bytes.clone();
    bad_magic[0] = b'X';
    assert!(matches!(
        CompactCalendarView::from_bytes(&bad_magic),
        Err(CompactError::InvalidMagic)
    ));

    let mut bad_version = bytes;
    bad_version[4] = 0x02;
    // CRC no longer matches, but magic/version are validated first — the
    // spec's ordered checks make the diagnosis deterministic.
    assert!(matches!(
        CompactCalendarView::from_bytes(&bad_version),
        Err(CompactError::UnsupportedVersion(0x02))
    ));
}

#[test]
fn out_of_bounds_dates_return_none_never_error() {
    let start = date(2026, 10, 5);
    let bytes = builder(start, 3, false).to_bytes().unwrap();
    let view = CompactCalendarView::from_bytes(&bytes).unwrap();

    assert!(view.times_for_date(date(2026, 10, 4)).is_none());
    assert!(view.times_for_date(date(2026, 10, 8)).is_none());
    assert!(view.times_for_date(date(2025, 10, 5)).is_none());
}

#[test]
fn year_boundary_days_are_addressable() {
    // Dec 30 2026 + 3 days crosses into 2027: from_yo arithmetic must
    // address Jan 1 without the "current calendar year" assumption.
    let start = date(2026, 12, 30);
    let mut b = CompactCalendarBuilder::new(start, ScopeType::Custom, false);
    b.push_day(full_day());
    b.push_day(full_day());
    b.push_day(full_day());
    let bytes = b.to_bytes().unwrap();
    let view = CompactCalendarView::from_bytes(&bytes).unwrap();

    assert!(view.times_for_date(date(2027, 1, 1)).is_some());
    assert!(view.times_for_date(date(2027, 1, 2)).is_none());
}

#[test]
fn jumuah_header_roundtrips() {
    let start = date(2026, 10, 5);
    let mut b = CompactCalendarBuilder::new(start, ScopeType::Week, false)
        .with_jumuah(Some(750), Some(810));
    b.push_day(full_day());
    let bytes = b.to_bytes().unwrap();
    let view = CompactCalendarView::from_bytes(&bytes).unwrap();

    assert!(view.has_jumua());
    assert_eq!(view.jumua().unwrap().to_hhmm().as_str(), "12:30");
    assert_eq!(view.jumua2().unwrap().to_hhmm().as_str(), "13:30");

    // Absent jumuah stays absent.
    let plain = builder(start, 1, true).to_bytes().unwrap();
    let view = CompactCalendarView::from_bytes(&plain).unwrap();
    assert!(!view.has_jumua());
    assert!(view.jumua().is_none() && view.jumua2().is_none());
}

#[test]
fn invalid_day_records_are_rejected_at_pack_time() {
    let start = date(2026, 10, 5);

    // Adhan minute past 23:59 — no representation anywhere.
    let mut b = CompactCalendarBuilder::new(start, ScopeType::Week, false);
    let mut day = full_day();
    day.adhan[2] = 1500;
    b.push_day(day);
    assert!(matches!(
        b.to_bytes(),
        Err(CompactError::TimeOutOfRange { prayer: 2, minutes: 1500 })
    ));

    // Non-ascending adhan (shurouq before fajr) has no fajr-relative
    // encoding — but the direct format carries it fine.
    let mut b = CompactCalendarBuilder::new(start, ScopeType::Week, true);
    let mut day = full_day();
    day.adhan[1] = day.adhan[0] - 1;
    b.push_day(day);
    assert!(matches!(b.to_bytes(), Err(CompactError::DeltaOverflow { .. })));

    let mut b = CompactCalendarBuilder::new(start, ScopeType::Week, false);
    b.push_day(day);
    assert!(b.to_bytes().is_ok());

    // Iqama offset past the one-byte limit (254 min) — direct format is
    // the fallback.
    let mut b = CompactCalendarBuilder::new(start, ScopeType::Week, true);
    let mut day = full_day();
    day.iqama = iqama([
        (DAY[0] + 300, false),
        (795, false),
        (1005, false),
        (1155, false),
        (10, true),
    ]);
    b.push_day(day);
    assert!(matches!(
        b.to_bytes(),
        Err(CompactError::IqamaOffsetOverflow { .. })
    ));

    let mut b = CompactCalendarBuilder::new(start, ScopeType::Week, false);
    b.push_day(day);
    assert!(b.to_bytes().is_ok());
}

#[test]
fn hostile_mutations_never_panic_and_ok_payloads_stay_queryable() {
    let start = date(2026, 10, 5);
    let bytes = builder(start, 7, false).to_bytes().unwrap();
    let rel_bytes = builder(start, 7, true).to_bytes().unwrap();
    let mut rng = Xorshift(0x4D515443); // "MQTC" as the seed

    for payload in [&bytes, &rel_bytes] {
        for _ in 0..2_000 {
            let mut mutated = payload.clone();
            for _ in 0..1 + rng.next() % 4 {
                let (i, bit) = rng.byte_flip_index(mutated.len());
                mutated[i] ^= bit;
            }
            // Total parsing: Ok or Err, never a panic.
            if let Ok(view) = CompactCalendarView::from_bytes(&mutated) {
                // A payload that loads must answer every in-range day.
                let start = view.start_date().unwrap();
                for i in 0..view.day_count() {
                    let day = view
                        .times_for_date(start + chrono::Days::new(i as u64));
                    assert!(day.is_some(), "loaded payload must serve day {i}");
                    if let Some(day) = day {
                        // Surfaced times satisfy the display contract even
                        // from a mutated-but-valid payload.
                        for t in [
                            day.adhan.fajr,
                            day.adhan.shurouq,
                            day.adhan.dhuhr,
                            day.adhan.asr,
                            day.adhan.maghrib,
                            day.adhan.isha,
                        ] {
                            assert!(t.hours() < 24 && t.minutes() < 60);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn emitters_embed_the_exact_payload() {
    let start = date(2026, 10, 5);
    let b = builder(start, 2, false);
    let _ = b.to_bytes().unwrap();

    let rust_code = b.to_rust_code("PRAYER_DATA").unwrap();
    assert!(rust_code.starts_with("// Generated by mawaqit-api pack_for_mcu"));
    assert!(rust_code.contains("pub static PRAYER_DATA: &[u8] = &["));
    // "MQTC" as decimal bytes — the payload is embedded verbatim.
    assert!(rust_code.contains("77,81,84,67,"));

    let c_header = b.to_c_header("PRAYER_DATA").unwrap();
    assert!(c_header.contains("#define PRAYER_DATA_LEN"));
    assert!(c_header.contains("static const uint8_t PRAYER_DATA["));
    assert!(c_header.contains("0x4D,0x51,0x54,0x43,"));
}

#[test]
fn empty_builder_yields_a_header_only_payload() {
    let start = date(2026, 10, 5);
    let b = CompactCalendarBuilder::new(start, ScopeType::Year, false);
    let bytes = b.to_bytes().unwrap();
    assert_eq!(bytes.len(), 24);
    let view = CompactCalendarView::from_bytes(&bytes).unwrap();
    assert_eq!(view.day_count(), 0);
    assert!(view.times_for_date(start).is_none());
}

// ---------------------------------------------- round-2 QE review pins ----

/// Re-sign a mutated payload: zero the CRC field, recompute the CRC-32 over
/// the whole payload, write it back. The tool for crafting CRC-valid
/// hostile blobs — an attacker who can write flash can recompute the CRC,
/// so post-CRC strictness is what keeps crafted bytes off the display.
fn resign(bytes: &mut [u8]) {
    bytes[0x14..0x18].fill(0);
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(bytes);
    let crc = hasher.finalize();
    bytes[0x14..0x18].copy_from_slice(&crc.to_le_bytes());
}

#[test]
fn white_night_isha_before_maghrib_is_rejected_in_fajr_relative_format() {
    // FINDING F11: high-latitude summer — Mawaqit can wrap Isha to 00:00,
    // putting a "prayer" before Fajr. Fajr-relative offsets have no
    // encoding for that: the packer must reject with a typed error, never
    // wrap. The direct format carries the same day explicitly.
    let start = date(2026, 6, 21);
    let day = CompactDayInput {
        adhan: [165, 255, 792, 1075, 1325, 0], /* 02:45 … 22:05, isha 00:00
                                                * (wrapped) */
        iqama: iqama([
            (180, false),
            (330, false),
            (810, false),
            (1095, false),
            (15, false),
        ]),
        day_flags: 0,
    };

    let mut b = CompactCalendarBuilder::new(start, ScopeType::Week, true);
    b.push_day(day);
    assert!(matches!(
        b.to_bytes(),
        Err(CompactError::DeltaOverflow { prayer: 5, delta: 165 })
    ));

    let mut b = CompactCalendarBuilder::new(start, ScopeType::Week, false);
    b.push_day(day);
    assert!(b.to_bytes().is_ok());
}

#[test]
fn crafted_crc_valid_records_with_impossible_times_are_dropped() {
    // FINDING F13: an attacker who can write flash can also recompute the
    // CRC. Out-of-range minutes, reserved bits 12–13, and rollover-without-
    // VALID mark the record corrupt: the day is dropped (None) — never
    // clamped, never fabricated as "34:07" (ADR-0010).
    let start = date(2026, 10, 5);
    let second = date(2026, 10, 6);
    let day1 = 24 + 24; // second direct record

    // adhan dhuhr = 0xFFFF
    let mut bytes = builder(start, 2, false).to_bytes().unwrap();
    bytes[day1 + 4] = 0xFF;
    bytes[day1 + 5] = 0xFF;
    resign(&mut bytes);
    let view = CompactCalendarView::from_bytes(&bytes).unwrap();
    assert!(view.times_for_date(start).is_some(), "day 1 stays queryable");
    assert!(view.times_for_date(second).is_none(), "corrupt record dropped");

    // adhan dhuhr = exactly 24:00 (1440)
    let mut bytes = builder(start, 2, false).to_bytes().unwrap();
    bytes[day1 + 4] = 0xA0;
    bytes[day1 + 5] = 0x05;
    resign(&mut bytes);
    let view = CompactCalendarView::from_bytes(&bytes).unwrap();
    assert!(view.times_for_date(second).is_none());

    // iqama reserved bit 12 set (undefined bits must be zero)
    let mut bytes = builder(start, 2, false).to_bytes().unwrap();
    bytes[day1 + 0x0D] = 0x10; // 0x1000
    resign(&mut bytes);
    let view = CompactCalendarView::from_bytes(&bytes).unwrap();
    assert!(view.times_for_date(second).is_none());

    // iqama minutes in the 1440..=2047 dead zone (VALID set)
    let mut bytes = builder(start, 2, false).to_bytes().unwrap();
    bytes[day1 + 0x0C] = 0xA0;
    bytes[day1 + 0x0D] = 0x45; // 0x45A0 = VALID | 1440
    resign(&mut bytes);
    let view = CompactCalendarView::from_bytes(&bytes).unwrap();
    assert!(view.times_for_date(second).is_none());

    // rollover bit without VALID — meaningless, dropped
    let mut bytes = builder(start, 2, false).to_bytes().unwrap();
    bytes[day1 + 0x0D] = 0x80; // 0x8000
    resign(&mut bytes);
    let view = CompactCalendarView::from_bytes(&bytes).unwrap();
    assert!(view.times_for_date(second).is_none());

    // fajr-relative: offset ≥ 1440 likewise drops the record
    let mut rel = builder(start, 2, true).to_bytes().unwrap();
    let rel1 = 24 + 20;
    rel[rel1 + 2] = 0xFF;
    rel[rel1 + 3] = 0xFF; // shurouq offset 0xFFFF
    resign(&mut rel);
    let view = CompactCalendarView::from_bytes(&rel).unwrap();
    assert!(view.times_for_date(second).is_none());
}

#[test]
fn start_day_of_year_bounds_are_rejected_with_valid_crc() {
    // FINDING F16: start_day_of_year must be 1..=366 — validated before
    // chrono is called, with the CRC re-signed so the date check is what
    // fires, not the checksum.
    let start = date(2026, 10, 5);
    for doy in [0u16, 367, 0xFFFF] {
        let mut bytes = builder(start, 2, false).to_bytes().unwrap();
        bytes[0x0A..0x0C].copy_from_slice(&doy.to_le_bytes());
        resign(&mut bytes);
        assert!(
            matches!(
                CompactCalendarView::from_bytes(&bytes),
                Err(CompactError::InvalidDate)
            ),
            "doy {doy} must be InvalidDate"
        );
    }
}

// ---- round-2 gate gap: the arms the codec suite never drove -------------
//
// The per-binary coverage union went vacuous for a while (regex vs the
// space-padded llvm-cov show columns), and these arms lost their tests'
// protection without anyone noticing. Back to 100% for real.

/// The scope wire-byte mapping is total and stable: every variant
/// roundtrips through `to_byte`/`from_byte`, and unknown bytes degrade to
/// [`ScopeType::Custom`] — informational, never an error.
#[test]
fn scope_byte_mapping_is_total_and_unknowns_degrade_to_custom() {
    assert_eq!(ScopeType::Week.to_byte(), 0);
    assert_eq!(ScopeType::Months.to_byte(), 1);
    assert_eq!(ScopeType::Year.to_byte(), 2);
    assert_eq!(ScopeType::Custom.to_byte(), 3);
    assert_eq!(ScopeType::from_byte(0), ScopeType::Week);
    assert_eq!(ScopeType::from_byte(1), ScopeType::Months);
    assert_eq!(ScopeType::from_byte(2), ScopeType::Year);
    assert_eq!(ScopeType::from_byte(3), ScopeType::Custom);
    assert_eq!(ScopeType::from_byte(0xFF), ScopeType::Custom);
    assert_eq!(ScopeType::from_byte(42), ScopeType::Custom);
}

/// The header metadata accessors reflect the packed bits: scope byte,
/// iqama presence, imsak mode, Jumu'ah fields and the raw flags byte — on
/// a plain payload and on a fully-flagged one.
#[test]
fn header_flag_accessors_reflect_the_packed_bits() {
    // Plain: HAS_IQAMA set, every other flag off.
    let plain = builder(date(2026, 1, 1), 1, false).to_bytes().unwrap();
    let view = CompactCalendarView::from_bytes(&plain).unwrap();
    assert_eq!(view.scope_type(), ScopeType::Year);
    assert!(view.has_iqama());
    assert!(!view.imsak_mode());
    assert!(!view.has_jumua());
    assert!(!view.is_fajr_relative());
    assert_eq!(view.flags() & 0x02, 0x02);

    // Fully flagged: imsak mode, a first Jumu'ah only, fajr-relative
    // records, Months scope. full_day() encodes in this format: every
    // iqama sits within one byte of its adhan slot.
    let mut b =
        CompactCalendarBuilder::new(date(2026, 2, 1), ScopeType::Months, true)
            .with_imsak_mode(true)
            .with_jumuah(Some(660), None);
    b.push_day(full_day());
    let flagged = b.to_bytes().unwrap();
    let view = CompactCalendarView::from_bytes(&flagged).unwrap();
    assert_eq!(view.scope_type(), ScopeType::Months);
    assert!(view.imsak_mode());
    assert!(view.has_jumua());
    assert!(view.is_fajr_relative());
    assert_eq!(view.jumua().unwrap().to_hhmm().as_str(), "11:00");
    assert_eq!(view.jumua2(), None);
    assert_eq!(view.flags() & 0x0F, 0x0F);
}

/// Builder bookkeeping: `len`/`is_empty` track pushed days.
#[test]
fn builder_len_and_is_empty_track_pushed_days() {
    let mut b =
        CompactCalendarBuilder::new(date(2026, 1, 1), ScopeType::Week, false);
    assert!(b.is_empty());
    assert_eq!(b.len(), 0);
    b.push_day(full_day());
    b.push_day(full_day());
    assert!(!b.is_empty());
    assert_eq!(b.len(), 2);
}

/// A day with no iqama at all packs and decodes in both record formats:
/// direct records leave the VALID bits unset, fajr-relative records carry
/// the 0xFF sentinel — and decode reports `iqama: None`, never a
/// fabricated iqama.
#[test]
fn days_without_iqama_pack_and_decode_in_both_formats() {
    for fajr_relative in [false, true] {
        let mut b = CompactCalendarBuilder::new(
            date(2026, 1, 1),
            ScopeType::Week,
            fajr_relative,
        );
        b.push_day(CompactDayInput {
            adhan: DAY,
            iqama: [None; 5],
            day_flags: 0,
        });
        let bytes = b.to_bytes().unwrap();
        let view = CompactCalendarView::from_bytes(&bytes).unwrap();
        assert!(!view.has_iqama());
        let day = view.times_for_date(date(2026, 1, 1)).unwrap();
        assert!(day.iqama.is_none());
        for (got, want) in [
            day.adhan.fajr,
            day.adhan.shurouq,
            day.adhan.dhuhr,
            day.adhan.asr,
            day.adhan.maghrib,
            day.adhan.isha,
        ]
        .iter()
        .zip(DAY)
        {
            assert_eq!(got.minutes_from_midnight(), want);
        }
    }
}

/// Out-of-range iqama minutes are rejected at pack time in both record
/// formats, naming the offending prayer's adhan slot.
#[test]
fn out_of_range_iqama_minutes_are_rejected_in_both_formats() {
    for fajr_relative in [false, true] {
        let mut b = CompactCalendarBuilder::new(
            date(2026, 1, 1),
            ScopeType::Week,
            fajr_relative,
        );
        b.push_day(CompactDayInput {
            adhan: DAY,
            iqama: iqama([
                (1441, false),
                (795, false),
                (1005, false),
                (1155, false),
                (10, true),
            ]),
            day_flags: 0,
        });
        assert!(
            matches!(
                b.to_bytes(),
                Err(CompactError::TimeOutOfRange { prayer: 0, minutes: 1441 })
            ),
            "fajr_relative={fajr_relative}"
        );
    }
}

/// The u16 day-count ceiling is a pack-time error, not a silent wrap.
#[test]
fn too_many_days_is_rejected_at_pack_time() {
    let mut b =
        CompactCalendarBuilder::new(date(2026, 1, 1), ScopeType::Year, false);
    let day = CompactDayInput { adhan: DAY, iqama: [None; 5], day_flags: 0 };
    // u16::MAX days are representable; one more overflows the wire field.
    for _ in 0..(u16::MAX as usize + 2) {
        b.push_day(day);
    }
    assert_eq!(b.len(), 65_537);
    assert_eq!(b.to_bytes(), Err(CompactError::TooManyDays(65_537)));
}

/// FINDING F29d — the source emitters interpolate `const_name` verbatim
/// into generated Rust/C source; an unvalidated name is code injection
/// into the firmware build (`X"; static EVIL: ...`). Names outside the
/// identifier grammar shared by both languages are rejected before any
/// interpolation.
#[test]
fn finding_f29d_const_name_is_validated_before_interpolation() {
    let payload = builder(date(2026, 10, 5), 2, false);
    let hostile = [
        "X\"; static EVIL: [u8; 0] = [",
        "A\nB",
        "",
        "1abc",
        "a-b",
        "a b",
        // 65 bytes of legal identifier characters is still over the bound.
    ];
    let long_name = "A".repeat(65);
    for name in hostile.into_iter().chain([long_name.as_str()]) {
        assert_eq!(
            payload.to_rust_code(name).unwrap_err(),
            CompactError::InvalidConstName,
            "to_rust_code accepted {name:?}"
        );
        assert_eq!(
            payload.to_c_header(name).unwrap_err(),
            CompactError::InvalidConstName,
            "to_c_header accepted {name:?}"
        );
    }
    let ok = "PRAYER_CALENDAR_V1";
    assert!(payload.to_rust_code(ok).is_ok());
    assert!(payload.to_c_header(ok).is_ok());
}

// ------------------------------------------------- next_event (issue #4, P2b)

/// A two-day payload: day 0 carries per-prayer iqama with a rollover isha
/// (00:20 next day); day 1 is adhan-only. The view borrows the bytes —
/// bind them in the caller.
fn two_day_payload(start: NaiveDate) -> Vec<u8> {
    let mut b = CompactCalendarBuilder::new(start, ScopeType::Week, false);
    b.push_day(CompactDayInput {
        adhan: [330, 427, 781, 997, 1164, 1265],
        iqama: [
            Some(CompactIqamaInput { minutes: 345, rollover: false }),
            Some(CompactIqamaInput { minutes: 795, rollover: false }),
            Some(CompactIqamaInput { minutes: 1010, rollover: false }),
            Some(CompactIqamaInput { minutes: 1180, rollover: false }),
            Some(CompactIqamaInput { minutes: 20, rollover: true }),
        ],
        day_flags: 0,
    });
    b.push_day(CompactDayInput {
        adhan: [331, 428, 782, 998, 1165, 1266],
        iqama: [None, None, None, None, None],
        day_flags: 0,
    });
    b.to_bytes().unwrap()
}

/// P2b: the upcoming adhan answers, with minutes-to-go.
#[test]
fn p2b_compact_next_event_picks_upcoming_adhan() {
    let start = date(2026, 10, 6);
    let bytes = two_day_payload(start);
    let view = CompactCalendarView::from_bytes(&bytes).unwrap();
    let day = view.times_for_date(start).unwrap();
    let next = day
        .next_event(start, NaiveTime::from_hms_opt(10, 0, 0).unwrap())
        .unwrap();
    assert_eq!(next.kind.label(), "Dhuhr adhan");
    assert_eq!(next.at, start.and_hms_opt(13, 1, 0).unwrap());
    assert_eq!(next.minutes_remaining, 181);
}

/// P2b: the rollover bit moves the iqama to the NEXT calendar day — at
/// 23:50 the 00:20 (next-day) isha iqama is what's next, at tomorrow's
/// instant (C1).
#[test]
fn p2b_compact_rollover_iqama_is_a_tomorrow_instant() {
    let start = date(2026, 10, 6);
    let bytes = two_day_payload(start);
    let view = CompactCalendarView::from_bytes(&bytes).unwrap();
    let day = view.times_for_date(start).unwrap();
    let next = day
        .next_event(start, NaiveTime::from_hms_opt(23, 50, 0).unwrap())
        .unwrap();
    assert_eq!(next.kind, PrayerEventKind::Iqama(Prayer::Isha));
    assert_eq!(
        next.at,
        start.succ_opt().unwrap().and_hms_opt(0, 20, 0).unwrap()
    );
}

/// P2b: iqama fields without the VALID bit are inert — the day resolves
/// to adhan-only events.
#[test]
fn p2b_compact_skips_invalid_iqama() {
    let start = date(2026, 10, 6);
    let mut b = CompactCalendarBuilder::new(start, ScopeType::Week, false);
    b.push_day(CompactDayInput {
        adhan: [330, 427, 781, 997, 1164, 1265],
        iqama: [None, None, None, None, None],
        day_flags: 0,
    });
    let bytes = b.to_bytes().unwrap();
    let view = CompactCalendarView::from_bytes(&bytes).unwrap();
    let day = view.times_for_date(start).unwrap();
    let mut now = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
    let mut kinds = Vec::new();
    while let Some(event) = day.next_event(start, now) {
        kinds.push(event.kind.label().to_string());
        now = event.at.time();
        now += chrono::Duration::minutes(1);
    }
    assert_eq!(
        kinds,
        [
            "Fajr adhan",
            "Shurouq",
            "Dhuhr adhan",
            "Asr adhan",
            "Maghrib adhan",
            "Isha adhan"
        ]
    );
}

/// P2b: after the start day's last event, the view falls through to the
/// next day's Fajr adhan.
#[test]
fn p2b_view_next_event_falls_through_to_tomorrow() {
    let start = date(2026, 10, 6);
    let bytes = two_day_payload(start);
    let view = CompactCalendarView::from_bytes(&bytes).unwrap();
    // At day-0 23:00 the rollover isha iqama (tomorrow 00:20) is next —
    // ahead of tomorrow's Fajr adhan.
    let next = view
        .next_event(start, NaiveTime::from_hms_opt(23, 0, 0).unwrap())
        .unwrap();
    assert_eq!(next.kind, PrayerEventKind::Iqama(Prayer::Isha));
    assert_eq!(
        next.at,
        start.succ_opt().unwrap().and_hms_opt(0, 20, 0).unwrap()
    );
    // Once that instant passed, day 1 (adhan-only) answers: its Fajr.
    let next = view
        .next_event(
            start.succ_opt().unwrap(),
            NaiveTime::from_hms_opt(1, 0, 0).unwrap(),
        )
        .unwrap();
    assert_eq!(next.kind, PrayerEventKind::Adhan(Prayer::Fajr));
    assert_eq!(
        next.at,
        start.succ_opt().unwrap().and_hms_opt(5, 31, 0).unwrap()
    );
}

/// P2b: past the scope's end there is nothing ahead — `None`, never a
/// wraparound into day 0.
#[test]
fn p2b_view_next_event_beyond_scope_is_none() {
    let start = date(2026, 10, 6);
    let bytes = two_day_payload(start);
    let view = CompactCalendarView::from_bytes(&bytes).unwrap();
    let last = start.succ_opt().unwrap();
    assert!(
        view.next_event(last, NaiveTime::from_hms_opt(23, 59, 0).unwrap())
            .is_none()
    );
}
