# Test Spec: `ut/semantics.rs` — semantic attacks and contract pins

- **Tier:** unit (`cargo test --test ut`), offline, deterministic.
- **Target:** `parse_page` → full calendar pipeline. Everything goes
  through `parse_page` — the same entry network data takes; never a direct
  serde deserialize.
- **Contract:** garbage must not *lie* either. Valid JSON used to corrupt
  behavior is the attack class here; the panic-safety layer is
  [`corpus.md`](corpus.md).
- **Helper:** `conf(json!)` builds a page around a JSON value and parses
  it; `the_date()` is 2026-01-01 (the fixture defines month 1 day 1).

## Always-run contract pins (green)

### `imsak_mode_is_decided_by_times_count_alone`
`times` of length 5/6/7 ⇒ `imsak_mode` false/true/false. The calendar row
shape is never consulted. Plus: fewer than 5 time strings ⇒ the page is
**refused outright** (`Parse`). Pins the single-source inference rule and
the structural minimum.

### `iqama_offset_rollover_stays_valid_hhmm`
Adhan near midnight (`23:30`) with `+600` offsets ⇒ resolved iqama rolls
into the next day (`09:30`) and every value is strict `HH:MM`. Next-day
wall clock is inherent to `HH:MM` display; the frontend builds its own
rollover events.

### `exotic_month_keys_are_skipped`
A month whose keys are `١ ٣١ 1e2 4294967296 " 1" "1 " "1.0" 0x1 ٣` ⇒
`month_times().days` is **empty** — exotic keys are skipped, never coerced,
never counted.

### `all_broken_rows_fail_safe`
31 days of 3-value garbage rows ⇒ `month_times` succeeds with **empty
days**; `times_for_date` errors (`NoCalendar`). Degradation, not failure —
and never fabricated times.

### `non_string_display_fields_become_none`
`name: 12345`, `jumua: {…}`, `jumua2: […`, `image: true`, `shuruq: 7.5` ⇒
all collapse to `None`. They never fail the page and never surface as
`"null"`/`"[object]"` strings.

### `layout_confusion_matrix_never_panics`
Explicit 15-cell matrix: `times.len()` ∈ {5, 6, 7} × row shapes {empty, 1
value, normal 6, imsak 7, 6-with-broken-middle `xx:yy`} — every cell must
parse-or-reject without panicking, and the iqama calendar (same rows) too.
Redundant with the corpus on purpose: a regression names its cell.

### `page_url_is_well_formed_for_benign_slugs`
`page_url("https://mawaqit.net", "grande-mosquee-de-paris")` ⇒
`https://mawaqit.net/en/grande-mosquee-de-paris`.

## Findings (see the [ledger](../README.md#red-team-findings-ledger))

### F4 — FIXED, green: `finding_f4_surfaced_times_are_always_valid_hhmm`
The regression anchor for [ADR-0010](../../adr/0010-display-time-contract.md).
Asserts, for a month where day 1 carries `"25:70"` as fajr and day 2 is
valid:
- day 1 **does not resolve** (`times_for_date` ⇒ `Err`);
- day 2 resolves with **every** surfaced time strict `HH:MM`;
- the month view carries exactly day 2;
- a hostile iqama absolute (`"25:70"`) falls back to the adhan value and
  all five resolved iqama times are strict `HH:MM`.

### F5 — FIXED, green: `finding_f5_duplicate_day_keys_yield_one_day`
Month with keys `"1"` (valid row), `"01"`, `"+1"` (attacker rows) ⇒
exactly **one** day 1 entry, and the **canonical decimal key wins** — the
asserted dhuhr is the valid row's `"13:00"`, not an attacker variant.
(`month_times`/`month_iqama_times` dedupe by parsed day; among variants
alone, BTreeMap order decides deterministically.)

### F6 — FIXED, green: `finding_f6_display_strings_carry_no_control_or_bidi_characters`
Mosque name and jumua containing U+202E (RTL override), U+202D, NUL, DEL,
`\n`, `\t` ⇒ no display string carries control or bidi characters:
`scraper::sanitize_text` strips C0/C1 and bidi/isolate controls from
free-text fields at the `ConfData` boundary (they end up in window titles,
tray tooltips, OS notifications; U+202E can visually reverse the tray
state). Time strings are untouched — they are strict-rejected instead
(F4), so stripping can never mint a valid `HH:MM`.

## Maintainer rules

- A green test here is a **contract**: change the behavior ⇒ change the
  spec and the ADR first.
- All findings in this suite (F4, F5, F6) are fixed regression anchors.
  A future open finding starts as `#[ignore = "RED TEAM FINDING F#: …"]`
  asserting the *secure* contract; fix the library until it passes, then
  delete the ignore — never weaken the assertion.
