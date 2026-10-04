# Error taxonomy

Which failure produces which `MawaqitError`. Normative text:
[`../specs/public-api.md`](../specs/public-api.md#error-taxonomy-srcerrorrs).

```mermaid
flowchart TD
    REQ["request"] --> NET["transport"]
    NET -->|connect refused / reset| HTTP["Http(reqwest)"]
    NET -->|30s timeout| HTTP
    NET -->|redirect loop (≤10)| HTTP
    NET -->|non-UTF-8 body| PARSE["Parse"]
    NET -->|"body > 20 MiB"| PARSE

    REQ --> ST["status check (before body read)"]
    ST -->|404| MNF["MosqueNotFound"]
    ST -->|"other !2xx — incl. 3xx:<br/>redirects never followed (F1)"| API["Api{status, url}"]

    REQ --> BODY["body handling"]
    BODY -->|search: not a JSON array<br/>one bad element| PARSE
    BODY -->|page: no 'confData =' assignment<br/>or unbalanced literal| CDNF["ConfDataNotFound"]
    BODY -->|page: literal not JSON| PARSE
    BODY -->|"times < 5 strings"| PARSE

    REQ --> CAL["calendar extraction"]
    CAL -->|"calendar missing/empty"| NC["NoCalendar"]
    CAL -->|"month ∉ 1..=12"| IM["InvalidMonth(u32)"]
    CAL -->|"row would surface non-HH:MM<br/>(F4: day rejected whole)"| PARSE
    CAL -->|"row shape ≠ 5/6 (+/− shuruq col)"| PARSE
    CAL -->|"today's day missing"| NC
```

## Semantic groupings

| Group | Variants | Retry? |
| --- | --- | --- |
| Transient | `Http`, `Api` (5xx/429) | yes — backoff (`examples/error_recovery.rs`) |
| Terminal | `MosqueNotFound`, `InvalidMonth` | no — caller error |
| Contract drift / hostile | `ConfDataNotFound`, `Parse`, `NoCalendar` | no — do not hammer; the snapshot fallback engages for `conf_data` |

## Fallback interaction

`conf_data`/`conf_data_dated` wrap the whole fetch: **any** error variant
from the diagram triggers the snapshot fallback when a disk cache is
configured — the original error only surfaces if no snapshot loads
([`request-flow.md`](request-flow.md)). `search_mosques` has no fallback
(searches are cheap and user-initiated).
