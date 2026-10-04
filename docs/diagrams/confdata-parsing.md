# confData parsing pipeline

From raw page bytes to a typed `ConfData`. Normative text:
[`../specs/confdata-wire-format.md`](../specs/confdata-wire-format.md);
decision: [ADR-0002](../adr/0002-one-page-one-year.md).

## Pipeline

```mermaid
flowchart TD
    A["page HTML (≤ 20 MiB, UTF-8)"] --> B["scan for 'confData' marker"]
    B --> C{"next non-ws char is '='?"}
    C -- "no (mention: ===, .foo, log…)" --> B
    C -- yes --> D{"starts with '{'?"}
    D -- no --> B
    D -- yes --> E["balanced-brace scan<br/>(string- & escape-aware)"]
    E -- "unbalanced ⇒ abandon mention" --> B
    E -- "balanced literal" --> F["serde_json::from_str<br/>(128-level depth limit)"]
    F -- "invalid JSON" --> X["Err(Parse)"]
    F -- "Value" --> G["build ConfData field-by-field"]
    G --> H{"times ≥ 5 strings?"}
    H -- no --> X
    H -- yes --> I{"calendar shape ok?"}
    I -- no --> Y["Err(NoCalendar)"]
    I -- yes --> J["imsak_mode = (times.len() == 6)<br/>collapse non-string display fields to None<br/>drop unparseable announcements<br/>keep raw verbatim"]
    J --> K["Ok(ConfData)"]
```

Notes:

- Mentions are skipped, not fatal — a page can discuss `confData` in code
  and still parse.
- The **first** mention that yields a balanced literal wins.
- The only two hard errors out of the scraper are `ConfDataNotFound` and
  `Parse`; a bad `calendar` surfaces as `NoCalendar`.

## Brace-scanner state machine

```mermaid
stateDiagram-v2
    [*] --> Outside
    Outside --> Outside : other byte
    Outside --> Depth1 : {
    Depth1 --> Deeper : {
    Deeper --> Deeper : {
    Deeper --> LessDeep : }
    LessDeep --> Deeper : {
    LessDeep --> LessDeep : }
    LessDeep --> Done : } at depth 1 ⇒ return literal
    Outside --> InString : "
    InString --> InString : printable / { } ; \n
    InString --> Escape : backslash
    Escape --> InString : any byte (consumed)
    InString --> Outside : " (string closed)
    note right of InString : braces/semicolons inside strings are content —<br/>the lookalike corpus pins this
```

Depth accounting is suspended inside strings and after `\`; the literal
ends at the `}` returning depth to zero. No closing brace ⇒ `None` ⇒ the
mention is abandoned (bounded work, no backtracking).
