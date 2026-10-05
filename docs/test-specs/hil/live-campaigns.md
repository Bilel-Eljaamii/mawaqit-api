# Test Spec: Hostile-in-the-Loop (HIL) — live campaigns and the release gate

**HIL** in this project means campaigns where the adversary is the *real
outside world* rather than a fixture: the live mawaqit.net site (and its
real-world data quirks), coverage-guided fuzzing beyond the deterministic
corpus, and regression sweeps over the red-team findings. Everything
offline — including the finding regression tests — runs in the loop at
every commit; HIL is what runs when a release is at stake.

## The campaigns

| # | Campaign | Command | Duration | Spec |
| --- | --- | --- | --- | --- |
| 1 | Offline gate | `just verify` | ~minutes | [ADR-0011](../../adr/0011-toolchain-and-portability.md) |
| 2 | Live world tour | `just live` | minutes, network | [`../e2e/world-tour.md`](../e2e/world-tour.md) |
| 3 | libFuzzer campaign | `just fuzz parse_page 300` + `just fuzz conf_pipeline 300` | 2 × 5 min | [`../fuzz/libfuzzer-campaign.md`](../fuzz/libfuzzer-campaign.md) |
| 4 | Live smoke (optional) | `cargo test -p mawaqit-api --lib -- --ignored --nocapture` | seconds, network | `client.rs::live_search_and_calendar` |

`cargo test -- --ignored` now runs **only the live tiers** — all red-team
findings in the tree are fixed and run green in plain `cargo test`
(campaign 1 covers them). The findings ledger
([`../README.md`](../README.md#red-team-findings-ledger)) must stay in
sync: a finding reappearing as `#[ignore]`d means a regression was
re-opened, and the release gate below treats that as a blocker.

## Release gate (run in order; stop at first red)

1. **`just verify`** — fmt, clippy `-D warnings`, type check, offline
   tiers (lib + ut + ct + fuzz, findings included), docs, offline example
   smoke. Must be 100 % green. Live tiers do **not** block here (they are
   `#[ignore]`d).
2. **Ledger check** — every finding in
   [`../README.md`](../README.md#red-team-findings-ledger) is Fixed or
   Documented-residual, and no finding test carries `#[ignore]` (a
   re-ignored finding = an open regression = blocker). A finding silently
   missing from the ledger is a process bug — add it.
3. **Live world tour** — `just live`; requires:
   - zero hard failures;
   - dead slugs ≤ 10 % of the fixture;
   - summary block saved into the release notes (parsed count, iqama
     coverage, anomaly list).
4. **libFuzzer** — 5 min per target, zero crashes; corpus carried forward.
5. **Docs sync** — any behavior change since the last release is reflected
   in `docs/specs/` (specs are updated in the same change as code; this
   step is the audit).

## Escalation rules

| Symptom | Meaning | Action |
| --- | --- | --- |
| World-tour hard failure on one mosque | site layout the parser mishandles | reproduce via `parse_page`, add corpus case, fix parser, re-run tour |
| World-tour hard failures across many mosques | site contract change (template rename, etc.) | halt, inspect a page by hand, update ADR-0002/specs, fix, re-run everything |
| Dead slugs > 10 % | fixture rot or search-index rot | refresh fixture; re-run |
| libFuzzer crash | parser/pipeline panic found beyond the corpus | tmin → corpus case → fix → 10 min clean |
| Finding test red or re-`#[ignore]`d in `cargo test` | regression of a fixed finding | release blocker; fix before anything else |
| `just verify` red | not ready, full stop | no HIL campaigning on a red gate |

## Cadence

- **Every commit:** campaign 1 (it is the commit gate).
- **Weekly:** campaigns 2–4 (site drift and fuzzer discoveries are
  time-dependent, not commit-dependent).
- **Every release:** the full gate above, in order.

## Round-2 HIL record (2026-10-06, issue #2 / findings F21–F30)

| Probe | Result |
| --- | --- |
| Live world tour (`just live`) | run against the fixed tree — see the campaign log in the issue-closing comment |
| Timezone wire-key confirmation (F28) | **confirmed**: `https://mawaqit.net/en/grande-mosquee-de-paris` publishes `"timezone":"Europe/Paris"` — the `ConfData::timezone()` accessor key and IANA-shape validation match the live wire |
| CDN probe (F24) | **confirmed**: `cdn.mawaqit.net/audio/adhan-maquah.mp3` serves `audio/mp3`, 1 512 648 bytes — comfortably inside the 8 MB mid-stream cap |
| Tor/SOCKS5 circuit probe (F29/F30 ingress through a proxy) | **not run** — no local tor daemon on the HIL host; the proxy path remains pinned by the pure validation/composition tests (ADR-0012's accepted approach, manual recipe in its Consequences) |
