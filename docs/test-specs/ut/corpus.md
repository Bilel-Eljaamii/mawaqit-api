# Test Spec: `ut/corpus.rs` — hostile corpus for the page parser

- **Tier:** unit (`cargo test --test ut`), offline, deterministic, no I/O.
- **Target:** `parse_page` (the exact network entry point) and the calendar
  pipeline (`dig`).
- **Contract:** nothing panics, nothing hangs on bounded input; `Err` is
  always a fine outcome; whatever parses must survive `dig()`.
- **Base fixture:** `valid_page()` — a minimal valid mosque page (6 daily
  times = imsak shape, one-month calendar, `+N` iqama calendar, name).

## Tests

### `truncations_of_valid_page_never_panic`
Every 7th truncation point of the valid page (deterministic stride) is
parsed. Invariant: any prefix — from empty to full — yields `Ok` or `Err`,
never a panic (exercises `balanced_json` against every possible
cut point modulo stride).

### `byte_flips_never_panic`
2000 mutations of the valid page: 1–8 bytes overwritten with pseudorandom
bytes at pseudorandom offsets, driven by xorshift seeded
`0x9E3779B97F4A7C15` (fixed — same space every run). Mutated bytes are
lossily reinterpreted as UTF-8. If a mutant parses, its `ConfData` is
pushed through `dig()`. Invariant: flips may corrupt values (yielding
`Err` downstream) but can never panic the scanner or the pipeline.

### `conf_data_lookalikes_never_confuse_the_parser`
Eight pages where `confData` appears but must not yield data — or must
yield the *right* data:

| Page | Expected |
| --- | --- |
| `if (confData === undefined) load();` | mention, not assignment ⇒ no data |
| `console.log("confData = not json")` | inside a string ⇒ no data |
| `let x = { a: "confData = {" }` | inside a string; brace in string is content ⇒ no data |
| `// var confData = {"broken";` | commented-out assignment (accepted cost, ADR-0002) — may parse or not, must not panic |
| two assignments, first valid JSON `{"a":1}` | **first** balanced literal wins ⇒ parses as a calendar-less conf |
| braces/semicolons inside a name string `"…}; }; <script>"` | string-aware scan keeps the literal intact |
| escaped-quote/backslash torture in a name | escape state machine holds |
| two scripts, last assignment unterminated | first (complete) one wins |

### `hostile_json_structures_never_panic`
A matrix of attacker-shaped confData objects (also driven through
`serde_json::from_value::<ConfData>` to cover both parse paths):

- `calendar` as `null` / string / `[null]` / `{"1": null}` / `{"1": []}` /
  `{"1": ["x","y","z"]}` / `{"1": ["05:27"]}` — unshapeable ⇒ empty ⇒
  `NoCalendar`, never a panic.
- Exotic day keys `0`, `-1`, `1e2`, `4294967296`, Arabic-Indic `٣`.
- Hostile `times` entries: `"5"`, `"05:60"`, `"24:00"`, `"AA:BB"`,
  `"05:2x"`, `null`, `7`, fullwidth `＋5`, Arabic-Indic digit `٠`.
- Hostile iqama values: `"+"`, `"+-"`, `"++"`, `" +5 "`, `"+5.5"`,
  `"+1e9"`, `"+9223372036854775807"`, `"-5"`, `"05:70"`, `"99:99"` —
  each must resolve, fall back, or drop without panicking (the i64::MAX
  clamp is the F-era regression).
- Deep nesting: 5000-deep JSON inside a calendar row through the *page
  path* (serde_json's 128-level limit rejects ⇒ `Err`, no stack overflow);
  a 100-deep structure through `from_value` (no limit there, so it must
  digest).

### `oversized_and_repetitive_pages_stay_bounded`
Three megabyte-scale inputs, all bounded work:
- ~1 MB of `confData ` mentions with no assignment (scanner bails after
  the mentions);
- 1 MB of open braces after a real assignment prefix (unbalanced ⇒ no
  literal);
- 5 MB string value inside a balanced literal (accepted, then dropped or
  kept — either is fine; the point is bounded time/memory).

### `unicode_and_control_characters_never_panic`
NUL, RTL overrides (U+202E/U+202D), zero-widths (U+200B/200D/FEFF), emoji,
CRLF/tabs, C0 controls — each injected (a) as the entire assignment body
and (b) inside the `name` field of a valid conf. No panics; parsed
conf must `dig()`.

## Failure semantics for maintainers

A failure here means the *panic-safety* layer broke. Fix the parser or the
pipeline; never "fix" the corpus. New hostile shapes go here first, then
graduate into the libFuzzer corpus if they reveal a class
([`libfuzzer-campaign.md`](../fuzz/libfuzzer-campaign.md)).
