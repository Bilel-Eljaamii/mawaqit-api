# Spec: `confData` wire format and extraction

Normative description of `src/scraper.rs`. Entry point:
`parse_page(page_html, mosque_id) -> Result<ConfData>` (pub re-export of
`extract_conf_data`).

## 1. Where the data lives

The public page `https://mawaqit.net/{lang}/{slug}` embeds, in one of its
`<script>` blocks, a JavaScript assignment:

```js
var confData = { …JSON literal… };
```

The literal carries the day's times, the whole-year adhan calendar, the
iqama calendar and mosque metadata.

## 2. Extraction algorithm (`find_conf_data_json`)

For every occurrence of the literal marker `confData` (left to right):

1. Skip whitespace after the marker.
2. Require `=`, skip whitespace. **Anything else ⇒ not an assignment**
   (`confData === undefined`, `confData.foo`, a string mentioning it, …);
   resume scanning after the marker.
3. Require `{` at the cursor; run the **balanced-literal scan**
   (`balanced_json`):

   ```mermaid
   stateDiagram-v2
       [*] --> Outside
       Outside --> Outside : any byte except { or "
       Outside --> Depth1 : {
       Depth1 --> DepthN : {  (depth + 1)
       DepthN --> DepthDec : }  (depth − 1; if 0 ⇒ DONE)
       any --> InString : "
       InString --> InString : any byte
       InString --> InEscape : backslash
       InEscape --> InString : next byte (consumed)
       InString --> Outside : "  (string closed)
       Depth1 --> [*] : } at depth 0 ⇒ literal complete
   ```

   - Inside a string, `{` `}` `;` newlines are content.
   - A `\` consumes the next byte (escaped quote stays in-string).
   - The literal ends at the `}` that returns depth to 0.
   - Input that never closes (unbalanced) ⇒ this mention is abandoned and
     the outer scan continues with the next `confData` marker.
4. The **first** mention that yields a balanced literal wins.

The extracted text is parsed with `serde_json::from_str::<Value>` — which
enforces serde_json's **128-level recursion limit**; deeper input is a
`Parse` error, never a stack overflow (fuzz-pinned).

## 3. Field-by-field construction and tolerance

| confData key | Model field | Tolerance rule |
| --- | --- | --- |
| `times` | `times: Vec<String>` | array of strings, non-strings filtered out; **< 5 entries ⇒ hard error** (`Parse`); 6 entries ⇒ `imsak_mode = true`; 7+ accepted as-is |
| `calendar` | `calendar: RawCalendar` | strict `Vec<BTreeMap<String, Vec<String>>>`; any deviation ⇒ empty ⇒ `NoCalendar` at extraction |
| `iqamaCalendar` | `iqama_calendar: Option<RawCalendar>` | same shape; **any** malformed entry (e.g. a `null` in a row) ⇒ whole calendar becomes `None` (adhan still works) |
| `name`, `image`, `jumua`, `jumua2`, `shuruq` | `Option<String>` | non-string ⇒ `None` (collapse, never error); strings sanitized (see §4) |
| `displayingSabahImsak` | — (inferred) | the flag is *not read*; imsak mode is inferred from `times.len() == 6` alone (single-source rule, pinned by `ut/semantics.rs::imsak_mode_is_decided_by_times_count_alone`) |
| `announcements` | `Vec<Announcement>` | per-element: entries that fail to deserialize are dropped; parse continues |
| *(everything else)* | `raw: Value` | kept verbatim (`#[serde(flatten)]`) — no data loss |

### Hard errors (the only two)

| Condition | Error |
| --- | --- |
| No `confData = {…}` assignment with a balanced literal on the page | `ConfDataNotFound(slug)` |
| Extracted literal is not JSON, or `times` < 5 strings | `Parse("confData: …")` / `Parse("confData.times has N entries…")` |
| `calendar` missing/unshapeable | `NoCalendar` |

## 4. Content sanitation: minimal, and only for free text

`scraper::sanitize_text` strips **C0/C1 control characters and
bidi/isolate controls** (U+202A–202E, U+2066–2069, U+200E/200F) from the
free-text display fields — `name`, `image`, `jumua`, `jumua2`, `shuruq`,
and announcement `title`/`content`/`image`/`video` (finding **F6**,
fixed: U+202E in a mosque name visually reverses the window title and
tray tooltip; C0 controls corrupt logs). Deliberate limits:

- **Markup is not touched**: XSS strings pass through verbatim —
  neutralization is the render layer's job (pinned by
  `ct/hostile_http.rs::conf_page_hostile_xss_content_parses_structurally`).
- **Time strings are never sanitized**: they are pinned strict-`HH:MM` at
  the calendar layer instead ([ADR-0010](../adr/0010-display-time-contract.md)) —
  stripping could mint a valid `HH:MM` out of hostile bytes where
  rejection is the correct outcome.

## 5. Reference minimal page

```html
<html><script>var confData = {
  "times": ["05:27","06:37","13:21","16:37","19:24","20:51"],
  "shuruq": "06:37",
  "calendar": [{"1": ["05:27","06:37","07:07","13:21","16:37","19:24","20:51"]}],
  "iqamaCalendar": [{"1": ["+10","+10","+10","+5","+10"]}],
  "name": "Hostile Mosque"
};</script></html>
```

(the `times` array above has 6 entries ⇒ imsak mode; a normal mosque ships
5. Calendar rows are specified in
[`calendar-resolution.md`](calendar-resolution.md#row-layouts-rawcalendar).)

## 6. Why not a DOM parser

See [ADR-0002](../adr/0002-one-page-one-year.md). The scanner
needs no HTML well-formedness, no extra dependency, and is byte-identical
to the fuzzed path (`fuzz/fuzz_targets/parse_page.rs`).
