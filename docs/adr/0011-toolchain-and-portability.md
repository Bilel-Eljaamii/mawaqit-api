# ADR-0011: Toolchain and portability — edition 2024, rustls, nightly rustfmt, the verify gate

- **Status:** Accepted
- **Date:** 2026-09 (initial architecture)
- **Decides:** Compiler/tooling choices and the definition of "done".

## Context

The library must build for Linux (the author's platform), Windows MSVC
(for mawaqit-desktop), and cross-compile from Linux to MSVC without a C
toolchain. Formatting must be deterministic across contributors; linting
must be a gate, not a suggestion. `just` is the single task-runner
surface.

## Decision

| Choice | Value | Why |
| --- | --- | --- |
| Rust edition | 2024 (`Cargo.toml`) | current stable; let-chains used in `client.rs`/`calendar.rs` |
| MSRV | 1.89.0 (`clippy.toml`) | matches the edition and deps in use |
| TLS | `reqwest` 0.13 with default **rustls** (no OpenSSL) | pure-Rust TLS → `x86_64-pc-windows-msvc` cross-compiles from Linux as-is; pinned in a `Cargo.toml` comment so nobody "simplifies" it back to native-tls |
| Dependencies | `chrono`, `reqwest`, `serde`(+json), `thiserror`, `tokio` — nothing else in prod deps | no HTML parser, no Redis client, no mock framework in prod: mocks are hand-rolled raw TCP in `tst/ct` so the *test* code stays honest about the wire |
| Formatting | nightly `rustfmt` with unstable options (`rustfmt.toml`: `imports_granularity = "Crate"`, `group_imports`, `wrap_comments`, max_width 80) | deterministic imports and comment wrapping; `just fmt`/`fmt-check` fall back to stable with a warning when nightly is absent |
| Lint gate | `cargo clippy --all-targets -- -D warnings` (`just lint`), clippy.toml: cognitive complexity 20, ≤ 8 args, `Tauri` in doc-valid-idents | warnings are errors; thresholds tuned so the hostile suites stay readable |
| **The gate** | `just verify` = `fmt-check` → `lint` → `check --all-targets` → `test` (ut+ct+fuzz) → `doc` → one offline example smoke (`page_scraper`) | the completion bar for *any* change; fails fast in that order |
| Pre-commit | `just graph` (GitNexus `detect-changes`) required before committing per AGENTS.md | structural review of the diff before it lands |

Examples are part of the gate (`cargo build --examples`, `check
--all-targets`), and `examples/page_scraper.rs` + `examples/slug_hardening.rs`
are the offline pair that runs in CI/smoke without network
(`page_scraper` is the verify-gate smoke).

## Consequences

**Positive**

- One command (`just verify`) is the whole review checklist; CI and
  humans run the same thing.
- Cross-compilation is a property of the dependency graph, not a CI
  workaround.

**Negative / accepted costs**

- Nightly-only rustfmt options mean stable-only environments produce
  slightly different formatting; `fmt-check` still passes (unstable
  options are simply ignored), so the gate does not break.
- Hand-rolled TCP mocks are more code than `wiremock`; in exchange they
  can lie at the byte level (truncated bodies, lying Content-Length,
  raw redirects) which a friendly mock framework discourages.

**Alternatives rejected**

- native-tls/OpenSSL: breaks the MSVC cross-compile story.
- CI-only formatting (no local gate): format drift lands in review
  instead of being fixed before push.
