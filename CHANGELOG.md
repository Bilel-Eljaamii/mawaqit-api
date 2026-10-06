# Changelog

All notable changes to `mawaqit-api` are documented in this file.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versioning: [semver](https://semver.org/).

## [Unreleased]

## [0.6.0] - 2026-10-07

### Added

- **Consumer domain promotions (GitHub issue #4)** — the prayer-time
  logic every mawaqit consumer had been re-implementing now lives in the
  crate, so `mawaqit-tui` (and a migrated `mawaqit-desktop`) share one
  tested implementation:
  - `prayer` module (core tier — compiles at `heapless`): `Prayer` (the
    five adhan prayers, stable keys), `PrayerEventKind`, `PrayerEvent`,
    and `TodayTimes::next_event(now)` — the next adhan / iqama / shuruq
    as absolute instants on the C1-rollover-correct `iqama_at`, with
    tomorrow's first adhan as the all-passed fallback. Fixes the live
    defect the desktop shipped: its string-sorted `next_prayer` placed a
    past-midnight iqama on the wrong day.
  - MCU-native counterpart: `CompactDayTimes::next_event(date, now)` and
    `CompactCalendarView::next_event(date, now)` (heapless tier, zero
    alloc, O(1) per day) — MQTC firmware gets next-prayer alarms from
    flash, on the same core selection rule so desktop and firmware
    semantics cannot drift.
  - Core-tier alarm math in `time`: `is_due`, `minutes_before`
    (midnight-wrapping pre-notification instant; returns `NaiveTime` —
    the display string is the caller's), `MAX_NOTIFY_BEFORE_MIN`.
    Contract-preserving port of the desktop's `prayer_logic.rs`
    hostile-time tests.
  - `ConfData::today_view(date) -> Result<TodayView>`: the one-call
    projection (mosque name, jumu'a times, image, imsak mode, resolved
    times, keyed announcements) with stable announcement keys (wire id,
    else FNV-1a-64 of the content).
  - `Announcement::is_active_on(date) -> Option<bool>`: active-window
    test over `start_date`/`end_date` (`%Y-%m-%d`; missing bound = open;
    unparsable bound = `None` — unknown, never a guess).
  - `voices::cached_path(dir, id)`: the catalog-validated
    `dir/{id}.mp3` cache convention (hostile ids cannot path-join out).
  - `tor` module behind the **default-off `builtin-tor` feature**
    (optional `arti-client =0.47.0`): embedded Arti exposed as a local
    SOCKS5 listener — `BuiltinTor::new(state_dir)` + `ensure_started()`
    composed with `with_socks_proxy("socks5h://{addr}")`. Socks5h-only,
    fresh circuit per connection, never a silent direct fallback;
    promoted from the desktop's battle-tested stack. ADR-0012 amended
    ("never embed Arti" → "never in the default build; opt-in only,
    never self-enabled").
- `MawaqitError::Tor(String)` (bounded, F29 rules) behind `builtin-tor`;
  `Prayer`/`PrayerEvent`/`PrayerEventKind` re-exported at the crate root.

### Changed

- The coverage gate is a **5-tier union** (ut/ct/fuzz/voices/tor) — the
  `tor` tier compiles `--features builtin-tor` and needs `libsqlite3`
  where it runs. `PartialEq` added to `Announcement`, `TodayTimes` and
  `DailyIqamaInstants` (additive) for view comparisons.

## [0.5.1] - 2026-10-06

### Added

- `just verify` now runs the coverage gate and prints a per-file terminal
  coverage report (File / Lines / Covered / Missed / Coverage, built from
  the same per-binary union the badge and the 100% gate use — no llvm-cov
  aggregate undercounting). The CI and release `verify` jobs install
  `llvm-tools` + `cargo-llvm-cov` accordingly.
- A CodeQL badge joins the README badge row (the analysis now runs as its
  own workflow — see Changed).

### Changed

- CodeQL moved off GitHub's default setup (which scanned every push and
  PR) to an explicit advanced workflow running **nightly only** —
  cron 01:00 UTC — plus a manual dispatch button, covering `rust` and
  `actions`. Code scanning belongs to the nightly HIL cadence, not to
  every push.

## [0.5.0] - 2026-10-06

### Fixed

- **Red-team round 2, network/ingress slate — F21–F30** (GitHub issue #2;
  ledger in `docs/test-specs/README.md`; the issue's draft F11–F20
  renumbered because F11–F16 are taken by the MQTC round). All ten
  findings fixed class-wide, each pinned by a green regression test:
  - **F21** — the Cf sanitizer table now covers U+0890–0891,
    U+1BCA0–1BCA3 and U+13440–13455, and announcement
    `start_date`/`end_date` are sanitized like their siblings.
  - **F22** — the search ingress (`Vec<Mosque>`) runs through the new
    shared `sanitize` module (modeled fields, the string `id`, and every
    string inside unmodeled extras); hostile labels no longer reach the
    tray/UI verbatim.
  - **F23** — the disk snapshot load caps the read at 2 MB (a planted
    10 GB file costs one bounded read, not an RSS blowup), applies the
    shared sanitizer exactly like the page path, and writes through
    per-writer unique temp names (pid + sequence) so racing writers can
    never rename a torn file into place.
  - **F24** — voice downloads stream with the 8 MB cap enforced per
    chunk (a hostile CDN without `Content-Length` is cut off mid-stream
    instead of buffered to the request timeout), a cached file over the
    cap is replaced instead of trusted forever, and temp names are
    per-writer unique.
  - **F25** — `TtlCache`'s insertion-order queue compacts once it passes
    twice the entry cap: TTL churn can no longer grow it without bound.
  - **F26** — `find_conf_data_json` ends the scan at the first failed
    `{` candidate (its scan already consumed to EOF and every later
    mention lives inside the broken object), making extraction linear in
    the page size; mentions that are not assignments still resume the
    search.
  - **F27** — staleness is decided inside `disk::load` (40-day TTL plus
    the same-calendar-year rule — a December snapshot never answers a
    January date); offline + stale is an honest network error, never
    year-old times on the alarm path. `disk::load_as_of` exposes the
    testable core.
  - **F28** — new ADR-0015 fixes the timezone contract: `iqama_at` is
    mosque-local wall clock, the sanctioned zone source is the validated
    `ConfData::timezone()` accessor, DST edge handling is the caller's
    (raise, then earlier-offset) — the library never picks an offset.
  - **F29** — pins bundle: `++5`/`+-5` are no longer parsed as `+5`
    (strict single-sign grammar, adhan fallback); search words over 128
    bytes are refused with the new `SearchWordTooLong` before any
    request; every payload-bearing error variant sanitizes and truncates
    at construction (128-byte identifiers, 256-byte diagnostics), so
    `Display` can never echo megabytes of hostile control characters;
    the MQTC source emitters validate `const_name` against
    `[A-Za-z_][A-Za-z0-9_]*` (≤ 64 bytes) and reject with the new
    `CompactError::InvalidConstName` instead of injecting code into the
    firmware build.
  - **F30** — the search cache keys on the exact request string:
    lowercased keys collided case-confusable words ("Paris" vs "paris",
    Turkish İ forms) and served one word's cached results to another.
- `just coverage` failed on a fresh checkout (`tee` into a directory
  that the recipe only created later); the `mkdir -p` now precedes the
  report render.

### Added

- The shared free-text sanitizer (`src/sanitize.rs`, private): one
  character policy at every ingress — page parse, search results, disk
  snapshot load (ADR-0003 amendment).
- ADR-0015 (`iqama_at` is mosque-local wall clock; validated
  `ConfData::timezone()`); amendments to ADR-0003 (shared sanitizer, raw
  contract), ADR-0005 (read cap, unique tmp names, in-loader staleness)
  and ADR-0012 (environment-proxy audit note); spec sync across
  transport-and-caching, offline-snapshots, confdata-wire-format,
  calendar-resolution, public-api and the MQTC spec.
- `just coverage` — a hard coverage gate: one instrumented test run
  (`cargo-llvm-cov`), HTML + lcov reports rendered over `src/` only (test
  files excluded), and a **100%-line-coverage requirement** — the recipe
  fails if a single `src/` line is uncovered. The summary is rendered by
  raw `llvm-cov` with a fixed object order, working around llvm-cov's
  aggregate undercounting for functions that exist in several test
  binaries.

### Fixed

- The last unexercised library paths are now covered by tests:
  `today()`/`month_iqama()` over a full-year mock, `invalidate(Some(slug))`
  cache isolation, empty-search-word short-circuit, `Default`/`Debug`
  wiring, the 5-column-row-without-shuruq rejection, iqama day-key dedupe,
  announcement `content`/`image`/`video` sanitization, variation-tag
  character stripping, empty-`raw` snapshot round-trip, and the voice
  downloader's fault paths (uncreatable destination, blocked tmp path,
  destination-as-directory, oversized `Content-Length`, missing
  `Content-Length` over the cap, empty body, transport failure, truncated
  body). Two defensive branches that could never execute were restructured
  into equivalent always-executed forms (cache mutex-poison guard, voice
  default-port arm, snapshot parent handling).

## [0.4.2] - 2026-10-05

### Added

- The `no_std`/MCU architecture ADR-0013 promised, implemented
  (ADR-0014): `src/compact.rs` — the `MQTC` v1 binary codec with a
  zero-alloc `CompactCalendarView` (O(1) flash reads, CRC-32-IEEE, C1
  rollover bitfield, Jumu'ah header, direct + fajr-relative records) and
  the alloc-side `CompactCalendarBuilder` emitting `.bin` / `.rs` / `.h`.
- `examples/pack_for_mcu` — the pre-flash packer: `--slug`/`--file`,
  `--scope week|months|year`, `--compress none|delta`,
  `--format rust|bin|c`, `--clamp` (Dec 31 rule), and a refuse-to-emit
  contract (every payload passes `from_bytes` before it is written).
- Domain-core modules `src/time.rs` and `src/slug.rs`: `is_valid_slug`
  and `minutes_between` now reach the `heapless` tier as ADR-0013
  promised.
- `just targets-mcu` — the feature matrix {thumbv7em-none-eabihf,
  riscv32imc-unknown-none-elf, host} × {std, alloc, heapless} is
  type-checked inside `just verify`; GitHub Actions runs the gate on
  push/PR (the repo had no CI).
- 13-test MQTC suite in `tst/ut/compact.rs` + a mutation-fuzz seed;
  test spec at `docs/test-specs/ut/compact.md`.
- All 8 architecture diagrams converted from Mermaid-in-Markdown to
  PlantUML sources (`docs/diagrams/*.puml`), plus three new MCU/MQTC
  diagrams (ADR-0014).

### Fixed

- Every non-default feature combination failed to compile (missing
  `src/compact.rs`, ungated `std` imports, std-only collections) —
  invisible to the default-features-only gate. All tiers now compile on
  host and both embedded targets.
- `std` now enables the `heapless` feature per ADR-0013's matrix (was
  `dep:heapless`, leaving the packer without the compact API).
- The spec's 12-byte delta layout could not encode real calendars
  (dhuhr − shurouq exceeds one byte at every latitude); replaced by the
  20-byte fajr-relative layout, with per-format pack-time rejection
  (`DeltaOverflow`, `IqamaOffsetOverflow`, `TimeOutOfRange`,
  `TooManyDays`).
- The builder validated both record formats for every day — direct-only
  days were rejected when packing direct. Encoders are per-format now.

## [0.4.1] - 2026-10-05

### Added

- `no_std` groundwork: `std` / `alloc` / `heapless` feature split with
  `default = ["std"]`; the network client stays behind `std` and all four
  test targets declare `required-features = ["std"]`. Design direction in
  ADR-0013 and the no_std MCU specs (compact binary with CRC-32, C1
  rollover bitfield, Jumuah support).
- crates.io metadata: `repository`, `homepage`, `readme`, `keywords`,
  `categories`, and an expanded crate description.

## [0.4.0] - 2026-10-05

### Added

- Adhan voice catalog (`ADHAN_VOICES`, `adhan_voice_url`,
  `voice_id_from_conf`) with a capped (8 MB), atomic, cache-aware CDN
  downloader (`download_voice`) routed through the client's transport —
  a configured proxy (Tor) applies.

## [0.3.0] - 2026-10-05

### Added

- **Tor routing, opt-in**: `MawaqitClient::with_socks_proxy("socks5h://host[:port]")`
  routes all traffic through a SOCKS5 proxy with remote DNS. Plain
  `socks5://` and http(s) addresses are rejected
  (`MawaqitError::InvalidProxy`) because DNS resolution outside the proxy
  defeats the purpose; a missing port defaults to 9050 (system tor; Tor
  Browser users pass 9150). The library never enables a proxy by default
  and never spawns or bundles a Tor daemon.
- `MawaqitClient::with_timeouts(connect, request)`; with a proxy set the
  timeouts default to 30 s connect / 90 s request (Tor circuits are slow).

### Fixed

- Hostile-review findings C1, H1–H3, M1–M5, L1–L2: cross-midnight iqama
  rollover resolved into absolute times, with malformed days surfaced as a
  typed error instead of dropped silently; TTL cache bounded (FIFO
  eviction); response cap enforced while streaming (1 MB, replacing the
  fully-buffered 20 MB); invisible Unicode (Cf) characters stripped from
  display fields; slug length capped; data-quality fixture, snapshot
  durability (fsync / Windows AV-lock), export filename, and doc
  boilerplate corrections.

### Changed

- `reqwest` gains the `socks` feature (still pure rustls; the Windows
  MSVC cross-compile is unaffected).

## [0.2.0] - 2026-10-04

### Added

- Keyless client for mawaqit.net: keyword mosque search
  (`GET /api/2.0/mosque/search`), and — from one page fetch per mosque —
  the whole-year adhan calendar, the iqama calendar with `+N` offsets
  resolved to absolute `HH:MM`, mosque metadata and announcements (strict
  `HH:MM` display contract; normal and Diyanet "Sabah İmsak" layouts).
- Opt-in offline layer: disk snapshots keyed by hashed slug, versioned
  envelope, atomic writes, automatic network fallback, and
  `conf_data_dated` reporting snapshot staleness.
- Hostile test suite run by plain `cargo test`: adversarial corpus and
  semantics suites, a lying-server HTTP component suite, deterministic
  mutation fuzzing — plus cargo-fuzz targets and a `#[ignore]`d live
  world tour over 100+ real mosques (`just live`).
- Red-team findings F1 (redirects), F2 (hostile slugs), F4 (surfaced
  times), F5 (duplicate day keys), F6 (control characters), F10 (snapshot
  round-trip) fixed and pinned as always-run regression tests.
- Four-tier test pyramid under `tst/` (one binary per tier), 13 runnable
  examples, the `just verify` gate (fmt → clippy → type check → tests →
  docs → offline example smoke), and the `docs/` tree: ADRs 0001–0011,
  functional specs, per-suite test specs, and architecture diagrams.
