# Test pyramid

Tiers, what they gate, and where the red-team findings live. Normative
text: [`../test-specs/README.md`](../test-specs/README.md);
decision: [ADR-0007](../adr/0007-four-tier-test-pyramid.md).

```mermaid
flowchart TB
    subgraph gate0["Tier 0 — in-module unit tests (cargo test --lib)"]
        L0["cache expiry · row shapes · envelope round-trips<br/>scraper extraction · minutes_between · ~28 tests"]
    end
    subgraph gate1["Tier 1 — ut: pure hostile parsing (no I/O)"]
        L1["tst/ut: corpus + semantics · 16 tests<br/>findings F4, F5, F6 pinned green"]
    end
    subgraph gate2["Tier 2 — ct: component vs mocks (local TCP + temp dirs)"]
        L2["tst/ct: hostile_http + disk_cache · 18 tests<br/>findings F1, F2 pinned green · F3 residual"]
    end
    subgraph gate3["Tier 3 — fuzz: deterministic mutation (temp dirs)"]
        L3["tst/fuzz: search model · URL builder · snapshot layer · 5 tests<br/>finding F10 pinned green"]
    end
    subgraph hil["Hostile-in-the-Loop (not in cargo test)"]
        H1["e2e world tour — 100+ live mosques<br/>ignore-annotated · just live"]
        H2["libFuzzer campaign — parse_page, conf_pipeline<br/>just fuzz"]
        H3["findings ledger audit — all fixed,<br/>regression tests run in cargo test"]
    end

    COMMIT["every commit"] --> G["just verify:<br/>fmt → clippy -D warnings → check → test → doc → smoke"]
    G --> L0 & L1 & L2 & L3
    RELEASE["release / weekly"] --> H1 & H2 & H3
    H3 --> LEDGER["findings ledger<br/>(test-specs/README.md)"]
```

## What each tier guarantees

| Tier | Guarantee nobody above it re-checks |
| --- | --- |
| 0 | internals behave (private access: cache eviction, envelope round-trip) |
| 1 | hostile bytes/JSON cannot panic, hang, or lie through the parse+calendar path |
| 2 | the client survives a lying *server* and the snapshot layer honors its store/fallback contract |
| 3 | mutation space beyond the corpus is safe, reproducibly, in CI |
| HIL | the real site's real data parses everywhere; fuzzer-class bugs stay found |

## Rule of thumb for new tests

- Pure input → behavior: tier 1 (`tst/ut`).
- Needs a socket/file but not the real site: tier 2 (`tst/ct`).
- New *surface* (public function over attacker-shaped data): seed into
  tier 3 (`tst/fuzz`) and add a corpus case.
- Real-site claim ("all mosques parse"): tier 4 fixture entry, not a unit
  test.
- Security behavior that does not hold yet: `#[ignore]`d finding test
  ([ADR-0006](../adr/0006-red-team-findings-workflow.md)).
