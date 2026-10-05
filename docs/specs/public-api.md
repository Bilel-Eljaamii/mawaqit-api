# Spec: Public API surface

Crate `mawaqit-api` 0.4.0 — everything an embedder can touch. Re-exported
from `src/lib.rs`.

## Cargo Feature Flags

| Feature | Default | Purpose & Targets |
| :--- | :--- | :--- |
| `std` | **Yes** | Standard library runtime (`reqwest`, `tokio`, `std::fs`, `MawaqitClient`, disk snapshots, cache). Used for Linux, Windows, macOS, server, and desktop. Implies `alloc` and `heapless`. |
| `alloc` | No | Enables dynamic data structures (`String`, `Vec`, `BTreeMap`, `ConfData`, `parse_page`) in `#![no_std]` environments with an embedded heap (e.g. ESP32 with `esp-alloc`). |
| `heapless` | No | Enables pure zero-allocation compact calendar lookups (`CompactCalendarView`) and `heapless::String<5>` formatting for bare-metal microcontrollers (e.g. Cortex-M, RISC-V). |

## Entry point (`std`)

```rust
// Requires feature = "std"
let client = MawaqitClient::new();                       // keyless
let client = MawaqitClient::with_base_urls(api, site);   // test seam: custom hosts
let client = client.with_disk_cache(dir);                // opt-in offline layer
```

`MawaqitClient` is `Clone` (cheap: `Arc` internals) and `Default`. All data
methods are `async` and safe to share across tasks. Available only when
`feature = "std"` is enabled.

## Entry point (`no_std` + `heapless` on MCU)

```rust
// Microcontroller with 0 heap allocations
use mawaqit_api::compact::{CompactCalendarView, CompactTime};

static PRAYER_DATA: &[u8] = include_bytes!("prayer_data.bin");

let calendar = CompactCalendarView::from_bytes(PRAYER_DATA).unwrap();
if let Some(today) = calendar.times_for_date(current_date) {
    let fajr_str: heapless::String<5> = today.adhan.fajr.to_hhmm(); // "05:42"
}
```

## `MawaqitClient` methods

| Method | Returns | Semantics |
| --- | --- | --- |
| `search_mosques(&self, word: &str)` | `Result<Vec<Mosque>>` | `GET {api}/2.0/mosque/search?word=…`. Trimmed empty word → `Ok(vec![])` without network. Cached 30 min keyed by lowercased word. HTTP 404 → `MosqueNotFound`; other non-success → `Api`; body must deserialize as `Vec<Mosque>` (one bad element fails the response — no partial results). |
| `conf_data(&self, mosque_id: &str)` | `Result<Arc<ConfData>>` | Fetch (or cache/snapshot-serve) the mosque page's `confData`. 6 h in-memory TTL per slug. |
| `conf_data_dated(&self, mosque_id: &str)` | `Result<(Arc<ConfData>, Option<NaiveDate>)>` | Same, plus `Some(fetched_at)` when the data came from the **disk snapshot**; `None` for memory cache or fresh fetch. |
| `today(&self, mosque_id: &str)` | `Result<TodayTimes>` | `times_for_date` at `Local::now().date_naive()`. |
| `month(&self, mosque_id: &str, month: u32)` | `Result<MonthTimes>` | Adhan times for every valid day of month 1–12. |
| `month_iqama(&self, mosque_id: &str, month: u32)` | `Result<MonthIqamaTimes>` | Resolved iqama for the month (needs both calendars). |
| `invalidate(&self, mosque_id: Option<&str>)` | `()` | Drop one page cache entry, or all of them on `None`. |
| `with_socks_proxy(self, addr: impl Into<String>)` | `Result<Self>` *(0.3.0)* | Chainable Tor opt-in: route all traffic through a SOCKS5 proxy with remote DNS. Address must be `socks5h://host[:port]` (missing port ⇒ 9050); anything else ⇒ `InvalidProxy` before any network use. Raises timeouts to 30 s / 90 s unless `with_timeouts` overrode them. Never enabled by default; never spawns a Tor daemon. |
| `with_timeouts(self, connect: Duration, request: Duration)` | `Self` *(0.3.0)* | Chainable transport-timeout override; composes with `with_socks_proxy` in any order. |

## Free functions

| Function | Signature | Notes |
| --- | --- | --- |
| `parse_page` | `(page_html: &str, mosque_id: &str) -> Result<ConfData>` | Re-export of `scraper::extract_conf_data`; the exact entry network data takes — exposed for tests/fuzzing. |
| `page_url` | `(site_base: &str, mosque_id: &str) -> String` | `format!("{site}/en/{mosque_id}")`. Pure; used to assert hostile-slug handling. |
| `is_valid_slug` | `(slug: &str) -> bool` | `^[a-z0-9]+(-[a-z0-9]+)*$`-equivalent: lowercase ASCII alnum, single hyphens between segments, no leading/trailing hyphen. |
| `month_times` | `(&ConfData, month: u32) -> Result<MonthTimes>` | Days sorted ascending; unparseable/invalid days silently dropped; non-numeric day keys skipped. |
| `month_iqama_times` | `(&ConfData, month: u32) -> Result<MonthIqamaTimes>` | Joins the iqama month against the adhan month by day; days without an adhan row are skipped. |
| `times_for_date` | `(&ConfData, NaiveDate) -> Result<TodayTimes>` | Exact day lookup in the month; missing day → `NoCalendar`; iqama attached only when that day resolves in the iqama calendar. |
| `minutes_between` | `(a: &str, b: &str) -> Option<i64>` | Minutes from `a` to `b` (b−a), midnight wrap handled (`23:30`→`00:10` = 40). `None` if either is not `HH:MM`. |

`mawaqit_api::disk` is public by design (tests, tooling, examples): `store`,
`load`, `snapshot_path` — see [`offline-snapshots.md`](offline-snapshots.md).

## Models (`src/models.rs`)

### `Mosque` — one search result

All fields optional; unknown keys preserved in `extra` (`#[serde(flatten)]`).

| Field | Type | Meaning |
| --- | --- | --- |
| `uuid`, `id` | `Option<String>` / `Option<Value>` | upstream identity (id may be any JSON type on the wire) |
| `slug` | `Option<String>` | **the** identifier for all data methods (`mosque_id()`) |
| `name`, `label` | `Option<String>` | display names |
| `locality`, `country` | `Option<String>` | place |

Accessors (total, never panic): `display_name()` = label → name → slug →
`"?"`; `mosque_id()` = slug; `place()` = `"locality, country"` pieces as
available.

### `ConfData` — the parsed page payload

| Field | Type | Meaning |
| --- | --- | --- |
| `times` | `Vec<String>` | today's times; ≥ 5 entries required at parse time; **len 6 ⇒ `imsak_mode = true`** |
| `shuruq` | `Option<String>` | page-level sunrise (fallback for 5-column calendar rows) |
| `calendar` | `RawCalendar` | year adhan calendar: 12 × (day-string → row) |
| `iqama_calendar` | `Option<RawCalendar>` | `iqamaCalendar` on the wire; values `"HH:MM"` or `"+N"` |
| `name`, `image`, `jumua`, `jumua2` | `Option<String>` | metadata (non-strings collapse to `None`) |
| `imsak_mode` | `bool` | Diyanet "Sabah İmsak" layout flag (`displayingSabahImsak`) |
| `announcements` | `Vec<Announcement>` | entries that fail to parse are dropped |
| `raw` | `serde_json::Value` | the complete original object — every unmodeled field survives here |

### Time rows

- `DailyPrayerTimes` — `{ fajr, shurouq, dhuhr, asr, maghrib, isha }`, all
  strict `HH:MM` (**display contract**, ADR-0010).
- `DailyIqamaTimes` — `{ fajr, dhuhr, asr, maghrib, isha }`, resolved
  absolute `HH:MM`.
- `TodayTimes` — `{ date, adhan, iqama: Option<DailyIqamaTimes> }`.
- `MonthTimes` / `MonthIqamaTimes` — `{ month: 1..=12, days: Vec<DayTimes/DayIqamaTimes> }`
  with `DayTimes { day, times }`, sorted by day.
- Raw wire types: `RawMonth = BTreeMap<String, Vec<String>>`,
  `RawCalendar = Vec<RawMonth>` (index 0 = January).

`TodayTimes`/`DayTimes`/`MonthTimes`/`MonthIqamaTimes`/`DayIqamaTimes` are
`Serialize`-only (computed views); `ConfData`/`Mosque`/`Announcement` are
`Serialize + Deserialize`.

## Compact MCU API (`src/compact.rs`, feature = "heapless")

Zero-allocation types for microcontrollers reading from flash ROM or RAM:

| Type | Purpose | Methods |
| :--- | :--- | :--- |
| `CompactCalendarView<'a>` | Zero-copy borrowing view over binary payload (`MQTC`) | `from_bytes(&'a [u8]) -> Result<Self, CompactError>`, `times_for_date(NaiveDate) -> Option<CompactDayTimes>`, `jumua() -> Option<CompactTime>`, `jumua2() -> Option<CompactTime>`, `day_count() -> usize`, `start_date() -> NaiveDate` |
| `CompactTime(u16)` | Minutes from midnight (`0..=1439`) with rollover and validity flags | `hours() -> u8`, `minutes() -> u8`, `minutes_from_midnight() -> u16`, `is_rollover() -> bool`, `is_valid() -> bool`, `to_hhmm() -> heapless::String<5>` |
| `CompactDayTimes` | Prayer times for one calendar day | `date: NaiveDate`, `adhan: CompactDayAdhan`, `iqama: Option<CompactDayIqama>` |
| `CompactDayAdhan` | 6 adhan times | `fajr`, `shurouq`, `dhuhr`, `asr`, `maghrib`, `isha` |
| `CompactDayIqama` | 5 iqama times | `fajr`, `dhuhr`, `asr`, `maghrib`, `isha` |

Under `feature = "alloc"` / `"std"`, the module also provides:

- `CompactScope`: `Week { start_date }`, `Months { year, start_month, count }`, `Year { year }`, `Custom { start_date, end_date }`.
- `CompactCalendarBuilder`: `from_conf(&ConfData, scope) -> Result<Self, MawaqitError>`, `to_bytes(compress: bool) -> Vec<u8>`, `to_rust_code(var_name) -> String`, `to_c_header(var_name) -> String`.

See [`compact-binary-and-mcu.md`](compact-binary-and-mcu.md) for full binary layout and CLI packer details.

## Error taxonomy (`src/error.rs`)

`type Result<T> = core::result::Result<T, MawaqitError>`.

| Variant | Fields | Produced when |
| --- | --- | --- |
| `Http` | source `reqwest::Error` | *(feature = "std" only)* connect/timeout/redirect-loop/body errors (includes truncated bodies, connection reset) |
| `MosqueNotFound` | slug/word | HTTP 404 from search or page fetch; invalid slug (placeholder-fetched, then 404) |
| `ConfDataNotFound` | slug | page fetched fine but no `confData = {…}` assignment, or unbalanced literal |
| `InvalidMonth` | `u32` | month outside 1–12 |
| `NoCalendar` | — | `calendar` missing/empty, or the requested day absent from it |
| `Api` | `status: u16, url` | any other non-success HTTP status |
| `InvalidProxy` | message *(0.3.0)* | `with_socks_proxy` got an address that is not `socks5h://host[:port]` — wrong scheme (`socks5`/http(s)/none), empty host, path/query/fragment, or unparseable |
| `Compact` | `CompactError` *(0.4.0)* | invalid compact binary header, buffer too small, checksum mismatch (CRC-32), unsupported version, date out of bounds |

Retry guidance (used by `examples/error_recovery.rs`): `Http` and `Api`
(5xx/429) are transient → retry with backoff; `MosqueNotFound`,
`InvalidMonth` and `InvalidProxy` are terminal; `ConfDataNotFound`/`Parse`
mean the site contract changed or the response is hostile — do not hammer.

## Versioning note

`SnapshotEnvelope::version` (disk layer), `MQTC` version (compact binary),
and the wire tolerance rules are the compatibility surfaces; everything
above them (struct fields, accessor behavior) follows semver from 0.2.0.
