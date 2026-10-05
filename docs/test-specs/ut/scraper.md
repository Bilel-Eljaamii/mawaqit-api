# Test Spec: `ut/scraper.rs` — page extraction via `parse_page`

- **Tier:** unit (`cargo test --test ut`), offline, deterministic, no I/O.
- **Target:** `parse_page` (public alias of the scraper; moved out of
  `src/scraper.rs` unchanged).
- **Contract:** the `confData` JSON literal is found and extracted from
  realistic page shapes; non-assignment mentions never satisfy the
  extractor; a page without confData is a typed error.

## Tests

### `extracts_conf_data_from_page`
A full page (scripts, head, body) yields name, times, shuruq, calendar,
iqama calendar, jumua, announcements.

### `handles_multiline_and_semicolons_in_strings`
Multiline JSON and `;`/`?` inside string values survive the
balanced-brace scan; unmodeled fields stay reachable via `raw`.

### `ignores_conf_data_mentions_that_are_not_assignments`
`if (confData === undefined)` and other mentions do not satisfy the
extractor — only an actual `confData = {...}` assignment parses.

### `missing_conf_data_is_an_error`
No assignment anywhere → `MawaqitError::ConfDataNotFound`.

## Run

```sh
cargo test --test ut scraper
```

A failure means live pages (or hostile lookalikes) parse wrong or the
layout-changed signal is lost.
