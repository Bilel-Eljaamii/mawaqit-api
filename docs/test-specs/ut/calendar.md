# Test Spec: `ut/calendar.rs` — calendar pipeline through the public API

- **Tier:** unit (`cargo test --test ut`), offline, deterministic, no I/O.
- **Target:** `month_times`, `month_iqama_times`, `times_for_date` on
  synthetic `ConfData` values (moved out of `src/calendar.rs`; re-expressed
  through the public API when tests left `src/`).
- **Contract:** the three row layouts parse per their documented mapping;
  hostile days are rejected whole, reported in `dropped`, and error as
  `InvalidDay` (never fabricated times); `+N` iqama offsets expand against
  the adhan and roll into the next day as *instants* (C1).

## Tests — row layouts

- `parses_normal_mode_row` — 6-column row surfaces as
  [Fajr, Shuruq, Dhuhr, Asr, Maghrib, Isha].
- `parses_imsak_mode_row` — Diyanet 7-column row: "Sabah" (index 1) is a
  chip value; the displayed Shurûq is the third column (Güneş).
- `parses_row_without_shuruq_column` — 5-value rows take sunrise from the
  page-level `shuruq` field.

## Tests — hostile-day rejection (F4 + M2)

- `rejects_short_day` — fewer entries than a prayer list: day dropped,
  reported in `dropped`.
- `rejects_days_surfacing_invalid_times` — `25:70`, `99:99`, `ab:cd`,
  `7:5`, `+30`, `24:00` in any column reject the whole day; the day lands
  in `dropped`, never in `days`.

## Tests — iqama resolution

- `iqama_passthrough_cannot_smuggle_non_display_times` — lenient parses
  ("7:5") and hostile values ("25:70") fall back to the adhan; only strict
  `HH:MM` passes through.
- `resolves_relative_and_absolute_iqama` — `+15`/`+20` expand against the
  adhan; absolute `HH:MM` stays as-is.
- `invalid_iqama_falls_back_to_adhan` — `garbage`/`+abc` resolve to the
  adhan value, like the official integrations.
- `hostile_iqama_offsets_do_not_panic` — `+9223372036854775807` clamps to
  +24 h and shows up as the same wall clock on the *next* day (instants);
  `++5` strips one `+` and resolves to +5 minutes.

## Tests — month and today views

- `extracts_month_and_iqama` — month 1 adhan + resolved iqama from the
  sample conf; `dropped` empty for a clean month.
- `rejects_invalid_month` — month 0 and 13 error as `InvalidMonth`.
- `finds_today` — February (no iqama row) degrades to adhan-only; January
  resolves both.
- `iqama_rollover_instants_carry_the_next_day` — C1: `+600` after a 23:30
  adhan displays `09:30` but the instant is *next-day* 09:30.
- `iqama_instants_stay_same_day_for_absolute_and_small_offsets` — absolute
  and `+15` iqama instants stay on the adhan's calendar day.
- `dropped_days_are_reported_and_error_as_invalid_day` — M2: the hostile
  day errors as `InvalidDay(1)`; a day never on the wire stays
  `NoCalendar`.

## Run

```sh
cargo test --test ut calendar
```

A failure means surfaced times can be fabricated, misattributed across
midnight, or that hostile data is silently dropped.
