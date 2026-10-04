# ADR-0002: One page, one year — scrape `confData` with a balanced-brace scanner

- **Status:** Accepted
- **Date:** 2026-09 (initial architecture)
- **Decides:** How prayer data is extracted from a mosque page.
- **Implements:** [ADR-0001](0001-keyless-acquisition.md).

## Context

Every data method (`today`, `month`, `month_iqama`, …) needs the mosque's
`confData` object. Options considered:

1. **Per-day REST calls** against the private API — rejected in ADR-0001;
   also 365 requests per mosque-year versus one.
2. **Run the page through a DOM parser** (`scraper`/`html5ever` crates) and
   locate the script node — heavyweight dependency, still needs JSON
   extraction from the script text, and couples us to HTML well-formedness
   (real pages are not always well-formed).
3. **Locate the `confData` assignment textually and extract the JSON literal
   with a hand-rolled balanced-brace scan** — no new dependencies, works on
   any bytes containing the assignment, independent of HTML structure.

## Decision

`src/scraper.rs::extract_conf_data` implements option 3:

1. Scan the page for each occurrence of the literal `confData`.
2. After the marker, allow whitespace, then require `=` followed by
   whitespace — anything else is a *mention* (`if (confData === undefined)`,
   a log line, a comment) and is skipped; scanning resumes after the marker.
3. From the first `{`, extract the balanced `{…}` literal with a
   string-aware state machine: inside a JSON string, `{`/`}` are content and
   `\` escapes the next byte; outside them they nest/unnest depth. The
   literal ends at the brace that returns depth to zero. Unbalanced input
   simply never closes and that mention is skipped.
4. Parse the extracted literal with `serde_json::from_str` (which carries
   serde_json's 128-level recursion limit — a deliberate guardrail, see the
   corpus suite) and build `ConfData` field-by-field with tolerance rules
   ([ADR-0003](0003-tolerant-wire-parsing.md)).

Steps 1–2 mean a hostile page cannot *smuggle* data through mentions: only
an actual `confData = {…}` assignment is consumed, and the first one that
yields a balanced literal wins.

## Consequences

**Positive**

- No HTML parser dependency; the scanner operates on any `&str`.
- Robust against `;`, braces, quotes and newlines *inside* JSON strings
  (pinned by `tst/ut/corpus.rs::conf_data_lookalikes_never_confuse_the_parser`).
- The same entry point (`parse_page`, the re-exported
  `extract_conf_data`) is what tests and fuzz targets exercise — the
  network path and the fuzz path are byte-identical.

**Negative / accepted costs**

- A page that assigns `confData` *twice* gives the first balanced literal;
  the site's own template wins as long as it is first.
- Comments (`// var confData = {`) are not tokenized away: a commented-out
  assignment *would* parse. Accepted — it never happens in the wild and the
  corpus pins the realistic mention shapes.
- Deep JSON (>128 nesting) is rejected by serde_json rather than crashing —
  documented and fuzz-pinned in `tst/ut/corpus.rs::hostile_json_structures_never_panic`.

**Alternatives rejected**

- DOM-based extraction (option 2): dependency weight and well-formedness
  coupling for no robustness gain over the textual scan.
- Regex for the literal: cannot express balanced nesting.
