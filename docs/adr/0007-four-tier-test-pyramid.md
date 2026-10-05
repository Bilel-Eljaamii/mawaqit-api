# ADR-0007: Four-tier test pyramid under `tst/`, one binary per tier

- **Status:** Accepted
- **Date:** 2026-09 (initial architecture)
- **Decides:** Where tests live and how they are structured.
- **See also:** [`test-specs/README.md`](../test-specs/README.md),
  [`diagrams/test-pyramid.md`](../diagrams/test-pyramid.md).

## Context

Rust's default layout (`tests/*.rs`, one binary per file) does not scale to
a suite that mixes pure parsing tests, socket-level mocks, live-network
campaigns and mutation fuzzing: file-per-test explodes the number of test
binaries (slow link times), and there is no natural place for shared
helpers without them becoming accidental test targets themselves.

## Decision

Integration tests live in **`tst/`** (outside `tests/`, deliberately) as a
four-tier pyramid. Each tier is **one test binary** declared in
`Cargo.toml` with explicit `[[test]]` entries; its submodules sit in a
same-named directory and are wired with `#[path]`:

```toml
[[test]]
name = "ut"
path = "tst/ut.rs"
```

```text
tst/
├── common/mod.rs      shared helpers (dig, valid_hhmm, to_minutes, temp_dir)
├── ut.rs + ut/        pure parsing/calendar semantics vs hostile input — no I/O
├── ct.rs + ct/        one component vs local mocks (raw-TCP HTTP, temp-dir disk)
├── e2e.rs + e2e/      the live mawaqit.net world tour (#[ignore]d)
└── fuzz.rs + fuzz/    deterministic seed-driven mutation fuzzing
```

- `common/` is a *module*, not a target: `common/mod.rs` (not `common.rs`)
  means cargo never compiles it as its own test binary; every tier includes
  it with `mod common;`. Unused helpers per tier are expected
  (`#![allow(dead_code)]`).
- The `#[path]` wiring (`#[path = "ut/corpus.rs"] mod corpus;`) keeps the
  directory structure without cargo's implicit `tests/` file-mapping rules.
- `src/` carries no `#[cfg(test)]` code (changed 2026-10-05): the former
  tier-0 in-module suites moved into the pyramid — cache, calendar,
  client helpers and scraper into `ut/`, the disk store into `ct/`, the
  live smoke into `e2e/` — re-expressed through the public API.
  `TtlCache` is `#[doc(hidden)]` public purely so its tests live in
  `tst/ut/cache.rs`; it stays out of the semver surface.
- Tier gates: **ut + ct + fuzz are offline and deterministic** and gate
  every commit (`cargo test`, `just verify`). **e2e hits the live site**
  and is `#[ignore]`d (run via `just live`).
- Beyond the in-tree tiers, `fuzz/` hosts a nightly libFuzzer campaign
  (`cargo fuzz`, targets `parse_page` and `conf_pipeline`) that explores
  past the deterministic corpus; `tst/fuzz` keeps the reproducible slice of
  that space in plain `cargo test`.

## Consequences

**Positive**

- Four link units instead of dozens: fast `cargo test`.
- A tier's contract ("no I/O", "mocks only", "live") is enforced by its
  module doc and structure, and documented per tier under
  [`test-specs/`](../test-specs/README.md).
- Shared invariant helpers (`dig` — "whatever the parser accepted, the
  calendar pipeline must digest"; `valid_hhmm` — the strict display
  contract) are written once and reused by ut and fuzz.

**Negative / accepted costs**

- The non-standard `tst/` location and the explicit `[[test]]`/`#[path]`
  wiring are unusual; both are documented here and in `Cargo.toml` comments
  so the next contributor does not "fix" them.
- Adding a tier means touching `Cargo.toml`; that friction is intentional.

**Alternatives rejected**

- `tests/foo.rs` file-per-suite: binary explosion, no shared-helper story.
- All tiers in one binary: `#[ignore]` scopes blur (live vs findings),
  tier-level runs (`just tier ct`) become name-filtering gymnastics.
