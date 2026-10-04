# ADR-0010: Strict display contract for surfaced times (F4, fixed)

- **Status:** Accepted (finding F4 fixed)
- **Date:** 2026-10
- **Decides:** What any time string leaving the library may look like.
- **Fixes:** [F4](../test-specs/README.md#red-team-findings-ledger) —
  "surfaced times are never validated".
- **Tests:** `tst/ut/semantics.rs::finding_f4_surfaced_times_are_always_valid_hhmm`,
  `src/calendar.rs` unit tests (`rejects_days_surfacing_invalid_times`,
  `iqama_passthrough_cannot_smuggle_non_display_times`).

## Context

Internally, time math is lenient: `chrono::NaiveTime::parse_from_str`
accepts `"7:5"` and leading spaces. But surfaced strings end up in UIs,
alarms and countdowns. The original bug: an adhan row carrying `"25:70"`
passed through verbatim; the frontend's `setHours(25, 70)` silently rolled
into another day and the countdown pointed at a fabricated time. Lenient
parses are fine for internal math, never for surfaced strings.

## Decision

One predicate defines the display contract — `is_displayable_hhmm`:

> exactly 5 bytes, `:` at index 2, all-digit halves, hours < 24,
> minutes < 60.

Contract rules built on it:

1. **Adhan rows are all-or-nothing.** `daily_from_row` checks every field
   it would surface (fajr, shurouq, dhuhr, asr, maghrib, isha); one
   invalid value rejects the *whole row*. `month_times` then drops the day
   from the month view; `times_for_date` returns `Err`. A hostile day
   never surfaces, and its valid neighbors are untouched.
2. **Iqama passthrough cannot smuggle.** `resolve_iqama` accepts an
   absolute value *only* if it is displayable; `"7:5"`, `"25:70"`, `"99:99"`
   fall back to the adhan time — the same treatment as garbage.
3. **`+N` offsets are clamped** to `[0, 1440]` minutes (i64 parse, no
   overflow — the `TimeDelta::minutes(i64::MAX)` panic found by the red
   team is impossible) and the result is formatted back through `HH:MM`.
4. **`valid_hhmm` in `tst/common`** is the test-side twin of the predicate,
   so every tier asserts the same contract (e2e world tour checks every
   surfaced time of every real mosque with it).

## Consequences

**Positive**

- Downstream code can `setHours`/parse any surfaced string without
  defensive validation. The contract is asserted from unit tier to live
  campaign.
- Degradation is uniform: a bad day vanishes rather than lying — the same
  semantic as layout-mismatched rows
  ([ADR-0003](0003-tolerant-wire-parsing.md)).

**Negative / accepted costs**

- A single hostile value in one column sacrifices six good ones (the row).
  Chosen deliberately: silently substituting a "fixed" fajr would fabricate
  religious times — the worst possible failure for this domain.
- Sub-minute precision and `H:MM` formats (if a mosque ever publishes
  them) are dropped/rejected rather than normalized into display.

**Alternatives rejected**

- Reject only the offending field, keep the rest: produces rows with a
  `None` fajr — every consumer now needs partial-row logic for a shape the
  domain never legitimately produces.
- Clamp bad values into range: fabricated times again.
