# Changelog

All notable changes to `mawaqit-api` are documented in this file.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versioning: [semver](https://semver.org/).

## [Unreleased]

### Added

- `just coverage` — HTML + lcov coverage report for the offline suite via
  `cargo-llvm-cov` (same tests as `just test`; the `#[ignore]`d live tiers
  are out).

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
