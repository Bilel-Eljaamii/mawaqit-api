# ADR-0003: Tolerant wire parsing at the boundary, strict display contract inside

- **Status:** Accepted
- **Date:** 2026-09, revised after red-team findings F4/F10
- **Decides:** Where the parser is lenient, where it is strict, and why.
- **Related:** [ADR-0002](0002-one-page-one-year.md),
  [ADR-0010](0010-display-time-contract.md).

## Context

The server (and anything that can stand between us and it — see the threat
model) is free to send garbage. Two opposite failure modes must be avoided:

- **Brittle parsing**: a single odd field (`"name": 12345`, a `null` inside
  an iqama row) fails the whole page → the app shows nothing for a mosque
  that *has* valid times. Real-world mosques do ship odd shapes; the e2e
  world tour proves it.
- **Gullible parsing**: hostile values flow through to the UI ("25:70" as a
  prayer time, control characters in the mosque name) and corrupt display,
  scheduling and logs downstream.

## Decision

A two-layer rule:

**Layer 1 — the scraper/model boundary is lenient per field, strict on
structure.**

- *Optional scalars collapse*: non-string values in display fields
  (`name`, `jumua`, `jumua2`, `image`, `shuruq`) become `None` — never an
  error, never a `"null"` string.
- *Announcements* are collected element-wise: entries that fail
  deserialization are dropped, not fatal.
- *`times` is structural*: fewer than 5 strings is a hard page error
  (`MawaqitError::Parse`); extra entries are kept and used to infer
  imsak mode (`times.len() == 6`, see
  [`specs/calendar-resolution.md`](../specs/calendar-resolution.md)).
- *`calendar` is structural*: unparseable → empty → `MawaqitError::NoCalendar`.
  A `null` *inside* an iqama row is fatal *for that calendar only* (serde
  rejects the whole `iqamaCalendar`), which drops iqama but keeps adhan.
- *Everything unmodeled survives* in `ConfData::raw` (`#[serde(flatten)]`)
  so nothing is silently lost. The raw contract (round-2 review): `raw`
  carries the wire's unmodeled values **as parsed, unsanitized** — it is
  the documented escape hatch for consumers who need a field the client
  does not model, and they own its handling. Sanitization applies to the
  *modeled* free-text surfaces the library itself ships (search results
  are fully sanitized, including their unmodeled extras, because a mosque
  result is pure display metadata — F22).
- *Free-text display fields are sanitized, minimally*: C0/C1 and the
  invisible Format (Cf) family — bidi/isolate controls, zero-width
  characters, the Arabic/shorthand/Egyptian format controls
  (F6 + F21) — are stripped from `name`, `jumua`, `jumua2`, `image`,
  `shuruq` and every announcement text field (including
  `start_date`/`end_date`, F21) at the boundary. Since round 2 the
  character policy lives in one shared module (`src/sanitize.rs`) applied
  at **every** free-text ingress: the page parser, the search endpoint
  (`Vec<Mosque>`, finding **F22** — hostile labels used to reach the
  tray/UI verbatim) and the disk snapshot load (finding **F23**). Content
  is otherwise untouched: XSS strings still pass through verbatim (pinned
  by
  `tst/ct/hostile_http.rs::conf_page_hostile_xss_content_parses_structurally`);
  neutralizing markup is the render layer's job. Time strings are
  deliberately **not** sanitized — stripping could mint a valid `HH:MM`
  out of hostile bytes instead of rejecting the day (F4).

**Layer 2 — the calendar layer is strict about what it *surfaces*.**

- Any adhan row that would surface a non-strict-`HH:MM` value is rejected
  *whole* (the day drops out of the month view; `times_for_date` errors)
  — finding **F4**, fixed, see
  [ADR-0010](0010-display-time-contract.md).
- Iqama entries are either absolute strict `HH:MM`, a `+N` offset resolved
  against the adhan, or garbage that falls back to the adhan time.
- The snapshot layer stores the *collapsed* struct, never raw hostile
  shapes, so what a snapshot serves is what the live path accepted
  (finding **F10**, fixed — see [ADR-0005](0005-disk-snapshot-layer.md)).

## Consequences

- Messy-but-real mosques work; hostile pages can degrade a *day* or an
  *optional field* but cannot fabricate a displayed time or crash a layer.
- "The day errors out instead of showing fabricated times" is the universal
  degradation semantic for rows; it is the same for layout mismatch and
  hostile values, so downstream code has one behavior to reason about.
- Former residuals **F5** (duplicate day keys) and **F6** (control/bidi
  characters in display strings) are fixed and pinned as always-run
  regression tests: F5 dedupes by parsed day in `month_times`/
  `month_iqama_times` with the canonical decimal key winning; F6 strips
  C0/C1 + bidi/isolate characters via `scraper::sanitize_text`.

**Alternatives rejected**

- *Strict serde everywhere*: breaks real mosques (F10's exact failure).
- *Sanitize everything at the parser*: wrong layer — the parser cannot know
  the render context (HTML? tray? notification?), and stripping legitimate
  content (e.g. `<b>` in an announcement) is data loss.
