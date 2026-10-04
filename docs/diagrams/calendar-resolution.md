# Calendar resolution

Row-shape decision tree and iqama resolution. Normative text:
[`../specs/calendar-resolution.md`](../specs/calendar-resolution.md);
contract: [ADR-0010](../adr/0010-display-time-contract.md).

## Row-shape decision tree (`build_daily_times`)

```mermaid
flowchart TD
    R["calendar row (Vec<String>)"] --> S{"len ≥ 6 AND row[1] parses as HH:MM?"}
    S -- "yes: has shuruq column" --> R2["remove index 1 → prayers"]
    R2 --> L{"prayers.len()"}
    L -- "6 (imsak/Diyanet)" --> I["fajr=İmsak · shurouq=Güneş<br/>dhuhr,asr,maghrib,isha = rest"]
    L -- "5 (normal)" --> N["fajr=prayers[0] · shurouq=row[1]<br/>dhuhr,asr,maghrib,isha = rest"]
    L -- "other" --> E1["Err(Parse)"]
    S -- "no: plain list" --> P{"row.len()"}
    P -- 6 --> P6["positional fajr,shurouq,…,isha"]
    P -- 5 --> P5{"page-level shuruq present?"}
    P5 -- yes --> P5y["5 prayers + shuruq from page"]
    P5 -- no --> E2["Err(Parse: no shuruq)"]
    P -- "other" --> E3["Err(Parse)"]
    I --> V
    N --> V
    P6 --> V
    P5y --> V
    V{"all 6 surfaced fields strict HH:MM?"}
    V -- "yes (F4 contract)" --> OK["DailyPrayerTimes"]
    V -- no --> VETO["Err — day rejected whole<br/>(dropped from month view / times_for_date ⇒ Err)"]
```

`imsak_mode` (the `ConfData` flag) is **not** decided here — it is
`times.len() == 6`, decided once at the scraper.

## Iqama resolution (`resolve_iqama`)

```mermaid
flowchart TD
    A["raw entry (trimmed)"] --> B{"starts with '+'?"}
    B -- yes --> C{"rest parses as i64?"}
    C -- no --> FB["fallback: adhan time"]
    C -- yes --> CL["clamp N to 0..=1440"]
    CL --> AP{"adhan parses?"}
    AP -- yes --> ADD["format(adhan + N min)<br/>next-day wall clock is valid"]
    AP -- no --> FB
    B -- no --> D{"strict HH:MM?"}
    D -- yes --> PT["passthrough (trimmed)"]
    D -- no --> FB
```

Hostile-value outcomes (all pinned by tests): `+9223372036854775807` ⇒
clamped +1440 ⇒ next-day wall clock; `+abc`, `+`, garbage ⇒ adhan;
`"7:5"`, `"25:70"` (parses but not displayable) ⇒ adhan — passthrough
cannot smuggle non-display times.

## Month/date extraction

```mermaid
flowchart LR
    subgraph MT["month_times(conf, m)"]
        M1["1..=12? else InvalidMonth"] --> M2["calendar[m-1]? else NoCalendar"]
        M2 --> M3["per day: parse key as u32<br/>(exotic keys skipped)"]
        M3 --> M3b["dedupe: canonical decimal key wins<br/>(F5)"]
        M3b --> M4["daily_from_row<br/>failures silently dropped"]
        M4 --> M5["sort by day"]
    end
    subgraph MI["month_iqama_times(conf, m)"]
        Q1["iqama_calendar? else NoCalendar"] --> Q2["adhan month needed (+N expansion)"]
        Q2 --> Q3["join by day; days without adhan row skipped"]
        Q3 --> Q4["sort by day"]
    end
    subgraph TD2["times_for_date(conf, date)"]
        T1["adhan: month path, find day<br/>missing ⇒ NoCalendar"] --> T2["iqama: best-effort<br/>any failure ⇒ None (adhan still surfaces)"]
    end
```

Duplicate day keys (`"1"`, `"01"`, `"+1"`) are deduplicated in step M3
(finding **F5**, fixed): one entry per parsed day, the canonical decimal
key always winning over its variants — pinned by
`finding_f5_duplicate_day_keys_yield_one_day`.
