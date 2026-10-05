# Test Specifications

Every test suite in this repository has a spec here: what it asserts, what
inputs it uses, how to run it, and what a failure means. Layout mirrors the
code:

```text
test-specs/
├── README.md            this file — pyramid, run matrix, findings ledger
├── ut/                  unit tier (tst/ut.rs)      — pure, no I/O
│   ├── cache.md         in-process TTL cache (doc-hidden internal)
│   ├── calendar.md      calendar pipeline through the public API
│   ├── client.md        helpers + SOCKS5 validation via public builders
│   ├── compact.md       the MQTC binary contract (ADR-0013)
│   ├── scraper.md       page extraction via parse_page
│   ├── corpus.md        hostile corpus for parse_page
│   ├── semantics.md     valid-JSON semantic attacks + contract pins
│   └── voices.md        adhan voice catalog + page validation
├── ct/                  component tier (tst/ct.rs) — local mocks only
│   ├── hostile-http.md  client vs lying raw-TCP server
│   ├── disk-cache.md    offline snapshot store/load/fallback contract
│   └── disk.md          the snapshot store itself on real files
├── e2e/                 end-to-end tier (tst/e2e.rs) — live site
│   ├── smoke.md         fast single-mosque live check
│   └── world-tour.md    100+ real mosques, structural invariants
├── fuzz/                fuzz tier (tst/fuzz.rs) + nightly campaign
│   ├── mutation.md      deterministic seed-driven mutation fuzzer
│   └── libfuzzer-campaign.md  cargo-fuzz targets and procedures
└── hil/                 Hostile-in-the-Loop — live campaigns & release gate
    └── live-campaigns.md
```

## The pyramid

| Tier | Binary | I/O | Runs by default | Count (approx.) | Spec |
| --- | --- | --- | --- | --- | --- |
| 1. unit (`ut`) | `tst/ut.rs` | none | yes | 66 (findings F4–F6, C1, M1 pinned green; MQTC contract in [`ut/compact.md`](ut/compact.md)) | [`ut/`](ut/corpus.md) |
| 2. component (`ct`) | `tst/ct.rs` | local TCP + temp dirs | yes | 26 (findings F1–F3, F10 pinned green) | [`ct/`](ct/hostile-http.md) |
| 3. mutation (`fuzz`) | `tst/fuzz.rs` | temp dirs | yes | 6 (finding F10 pinned green; MQTC seed) | [`fuzz/`](fuzz/mutation.md) |
| 4. end-to-end (`e2e`) | `tst/e2e.rs` | **live mawaqit.net** | `#[ignore]`d | 2 (smoke + world tour) | [`e2e/`](e2e/smoke.md) |
| 5. libFuzzer campaign | `fuzz/` (cargo-fuzz) | none | nightly/manual | 2 targets | [`fuzz/libfuzzer-campaign.md`](fuzz/libfuzzer-campaign.md) |

`src/` carries no `#[cfg(test)]` code: every test lives in the pyramid.
Suites moved out of `src/` (cache, calendar, client helpers, scraper,
disk store) run through the public API; `TtlCache` is `#[doc(hidden)]`
public purely for that purpose and stays out of the semver surface.
The private `resolve_timeouts` matrix was the one casualty — documented
behavior, not publicly assertable.

## Run matrix

| Command | What runs | When |
| --- | --- | --- |
| `cargo test` | lib + ut + ct + fuzz (findings included, green) | every commit (part of `just verify`) |
| `cargo test --test ut` (etc.) | one tier | iterating (`just tier <tier>`) |
| `cargo test -- --ignored` | **live tiers only** (world tour + live search smoke) | pre-release / HIL, needs network |
| `just live` | the e2e world tour | pre-release, network, minutes |
| `just fuzz parse_page 60` | one cargo-fuzz target for 60 s | nightly / on parser changes |
| `just coverage` | library (`src/`) coverage — HTML + lcov; test files excluded; held at 100% lines | nightly / on demand |

Shared invariant helpers live in `tst/common/mod.rs`:

- `dig(&ConfData)` — "whatever the parser accepted, the calendar pipeline
  must digest" (runs `month_times`, `month_iqama_times`, `times_for_date`);
  used by ut corpus and the fuzz tier.
- `valid_hhmm(&str)` — the strict display contract ([ADR-0010](../adr/0010-display-time-contract.md));
  used by ut semantics and the e2e world tour.
- `to_minutes(&str)`, `temp_dir(suite, name)`.

## What every tier must guarantee (global invariants)

1. **No panics on hostile input** — `Err`/`None` are always fine outcomes.
2. **No hangs on bounded input** — bounded work, no unbounded loops.
3. **Surfaced times are strict `HH:MM`** (ADR-0010).
4. **Hostile slugs never leave the mosque namespace** (ADR-0008).
5. **A snapshot serves exactly what the live path accepted** (ADR-0005/F10).
6. **Degradation, never fabrication** — a bad day vanishes; the library
   never invents a time.

## Red-team findings ledger

Authoritative table (narrative: [ADR-0006](../adr/0006-red-team-findings-workflow.md)).
Numbers are never reused; gaps (F7–F9) are findings resolved elsewhere.
All findings currently in the tree are **fixed** — their tests run green in
every `cargo test` as regression anchors; only live tiers stay `#[ignore]`d.

| # | Finding | Suite | State | Test |
| --- | --- | --- | --- | --- |
| F1 | Cross-origin redirects are followed | ct | **Fixed** (`Policy::none()`) | `finding_f1_cross_origin_redirect_is_not_followed` |
| F2 | Hostile slug escapes the mosque namespace | ct | **Fixed** | `finding_f2_hostile_slug_never_leaves_the_mosque_namespace` |
| F3 | 20 MB cap applied after full buffering | ct | **Documented residual** | `finding_f3_documented_cap_rejects_oversized_response` |
| F4 | Surfaced times not validated `HH:MM` | ut | **Fixed** | `finding_f4_surfaced_times_are_always_valid_hhmm` |
| F5 | Duplicate day keys yield multiple days | ut | **Fixed** (canonical key wins) | `finding_f5_duplicate_day_keys_yield_one_day` |
| F6 | Control/bidi chars reach display strings | ut | **Fixed** (`sanitize_text`) | `finding_f6_display_strings_carry_no_control_or_bidi_characters` |
| F10 | Snapshot drops wire-tolerated shapes | fuzz | **Fixed** | `finding_f10_snapshot_roundtrips_wire_tolerated_shapes` |
| F11 | Fajr-relative records wrap negative deltas (White-Night Isha at 00:00) | ut | **Fixed** (rejects with `DeltaOverflow`; direct format carries the day) | `white_night_isha_before_maghrib_is_rejected_in_fajr_relative_format` |
| F12 | CRC scope ambiguity in the MQTC spec | docs | **Fixed** (spec states the zeroed `0x14..0x18` range explicitly; impl was always correct) | `crc_bit_flip_is_rejected_before_any_lookup` |
| F13 | Crafted CRC-valid MQTC records with impossible minutes / reserved bits reach display as "34:07" | ut | **Fixed** (strict decode: record dropped as `None`, never clamped) | `crafted_crc_valid_records_with_impossible_times_are_dropped` |
| F14 | Fajr-relative format loses the iqama rollover | ut | **Fixed by design** (rollover is implicit: adhan + offset ≥ 1440 ⇒ bit 15) | `fajr_relative_roundtrip_preserves_every_field` |
| F15 | `to_hhmm()` had no bounds contract | docs | **Fixed** (precondition documented; hostile-bits scenario eliminated by F13) | `crafted_crc_valid_records_with_impossible_times_are_dropped` |
| F16 | `start_day_of_year` = 0/367 under-specified | ut | **Fixed** (`1..=366` guard before chrono; spec states it) | `start_day_of_year_bounds_are_rejected_with_valid_crc` |

Round-2 QE review (2026-10-05, tracked in
[GitHub issue #1](https://github.com/Bilel-Eljaamii/mawaqit-api/issues/1)):
F11–F16, all MQTC codec data-representation findings. F13 was the one
live defect — `decode_direct` accepted impossible minutes and reserved
bits from a CRC-valid crafted blob; strict decode now drops the record.

A future finding starts as an `#[ignore]`d red test and graduates to green;
the release-gate procedure is in
[`hil/live-campaigns.md`](hil/live-campaigns.md).
