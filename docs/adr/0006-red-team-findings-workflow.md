# ADR-0006: Red-team findings workflow — secure contracts pinned as ignored tests

- **Status:** Accepted
- **Date:** 2026-10
- **Decides:** How security findings are recorded, tracked and driven to zero.
- **See also:** the [findings ledger](../test-specs/README.md#red-team-findings-ledger),
  [`diagrams/threat-model.md`](../diagrams/threat-model.md).

## Context

The hostile-testing campaign (corpus, hostile HTTP, semantics, mutation
fuzzing, live world tour) regularly surfaces behaviors that are *not*
crashes but are *wrong under attack*: following a cross-origin redirect,
letting a slug escape the namespace, duplicating a day, passing control
characters into the tray tooltip. Issues like these die in commit messages
or TODOs unless they are executable.

## Decision

Every red-team finding is an **executable test with a fixed number** and one
of three states:

1. **Open** — the test is `#[ignore = "RED TEAM FINDING F#: …fix hint…"]`
   and asserts the *secure* contract that does **not** hold yet. `Err` /
   today's behavior would fail it. Run with `cargo test -- --ignored`.
2. **Fixed** — the fix lands and the test is **un-ignored**; it becomes a
   permanent regression anchor asserting the now-real secure contract.
3. **Documented residual** — the risk is consciously accepted; the test is
   un-ignored, renamed to describe today's behavior, and carries a comment
   stating the residual risk and the fix that would close it (e.g. F3).

Rules:

- Numbers are never reused. Gaps in the numbering (F7–F9) mean findings
  that were resolved elsewhere/earlier; the ledger only lists findings
  present in the current tree.
- The ignore message must name the finding number and the fix, so the
  output of `cargo test -- --ignored` doubles as a work queue.
- Fixing a finding = make the ignored test pass, then delete the
  `#[ignore]`. Never rewrite the contract to match the bug.

## Current ledger (as of this writing)

| # | Finding | Suite | State | Test |
| --- | --- | --- | --- | --- |
| F1 | Cross-origin redirects followed | `tst/ct/hostile_http.rs` | **Fixed** (`redirect::Policy::none()` — a 302 surfaces as `Api { status: 302 }`) | `finding_f1_cross_origin_redirect_is_not_followed` |
| F2 | Hostile slugs escape the mosque namespace | `tst/ct/hostile_http.rs` | **Fixed** | `is_valid_slug` + dash-repeat placeholder (`src/client.rs`) |
| F3 | 20 MB cap applied *after* full buffering | `tst/ct/hostile_http.rs` | **Documented residual** | stream via `bytes_stream()` and abort past the cap |
| F4 | Surfaced times not validated `HH:MM` | `tst/ut/semantics.rs` | **Fixed** | whole-day rejection in `daily_from_row` (`src/calendar.rs`) |
| F5 | Duplicate day keys (`"1"`, `"01"`, `"+1"`) | `tst/ut/semantics.rs` | **Fixed** (dedupe by parsed day; canonical decimal key wins) | `finding_f5_duplicate_day_keys_yield_one_day` |
| F6 | Control/bidi chars in display strings | `tst/ut/semantics.rs` | **Fixed** (`scraper::sanitize_text` strips C0/C1 + bidi/isolate from free-text fields) | `finding_f6_display_strings_carry_no_control_or_bidi_characters` |
| F10 | Snapshot round-trip drops wire-tolerated shapes | `tst/fuzz/mutation.rs` | **Fixed** | `conf_to_storage` struct+overlay (`src/disk.rs`) |

Every finding present in the tree is currently fixed; its test runs green
in plain `cargo test` as a permanent regression anchor. Only the live-site
tiers remain `#[ignore]`d. The workflow stays in force for future
findings: an open finding is an `#[ignore]`d red test, and `cargo test --
--ignored` runs exactly the live campaign.

## Consequences

**Positive**

- Findings cannot rot: each is a failing-if-regressed executable, run by
  every `cargo test`.
- The (currently empty) set of open findings would appear as red tests
  under `--ignored` — a to-do list with acceptance criteria baked in.

**Negative / accepted costs**

- `--ignored` now means only "hits the live site"; finding sweeps moved
  into the default run. HIL procedures
  ([`test-specs/hil/live-campaigns.md`](../test-specs/hil/live-campaigns.md))
  spell out what to run when.
- "Documented residual" tests look like contract tests; each carries an
  explicit comment so reviewers do not mistake them for endorsement.
