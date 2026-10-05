# Changelog

All notable changes to `mawaqit-api` are documented in this file.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versioning: [semver](https://semver.org/).

## [Unreleased]

### Fixed

- Round-2 QE review findings F11–F16 on the MQTC codec (GitHub issue #1;
  ledger in `docs/test-specs/README.md`). The one live defect — F13:
  `decode_direct` accepted impossible minutes (1440..=2047) and reserved
  bitfield bits 12–13 from a CRC-valid crafted blob, reaching display as
  e.g. "34:07" — is fixed by strict decode: corrupt records drop the day
  as `None`, never clamped or fabricated. F11 (White-Night negative
  delta) and F14 (delta rollover) were already correct by the
  fajr-relative design and are now pinned by tests; F12/F15/F16 tighten
  the spec (explicit CRC zero-range, `to_hhmm` precondition,
  `start_day_of_year` 1..=366).
- `just coverage` failed on a fresh checkout (`tee` into a directory
  that the recipe only created later); the `mkdir -p` now precedes the
  report render.

### Added

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
