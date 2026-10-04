# ADR-0004: In-process TTL cache instead of an external cache

- **Status:** Accepted
- **Date:** 2026-09 (initial architecture)
- **Decides:** How repeated requests are served without re-fetching.

## Context

`confData` carries the whole year, so refetching a page per query is pure
waste; the desktop app shares one client across a tray loop and dashboard
views, so cache hits must be cheap and contention-safe. The reference
mawaqit-api implementation uses Redis for this layer.

## Decision

`src/cache.rs` provides `TtlCache<V>`: a `Mutex<HashMap<String,
(Instant, V)>>` with one fixed TTL per instance. The client holds two
instances:

| Cache | Key | Value | TTL | Filled by |
| --- | --- | --- | --- | --- |
| `pages` | mosque slug | `Arc<ConfData>` | 6 h | `fetch_conf_data` |
| `searches` | lowercased search word | `Vec<Mosque>` | 30 min | `search_mosques` |

Details that are contract, not accident:

- Values are stored as `Arc` where large (`ConfData`) so a cache hit clones
  a pointer, not the year calendar.
- Expiry is lazy: `get` checks `created.elapsed() >= ttl` and removes the
  entry on sight. A stale entry costs nothing until someone asks for it.
- Search keys are lowercased, so `"Paris"` and `"paris"` share an entry.
- An empty/whitespace search word returns `Ok(vec![])` without touching the
  cache or the network.
- `MawaqitClient::invalidate(Option<&str>)` drops one page entry or clears
  all of them (used for drift detection — see `examples/conf_diff.rs`).
- The mutex is poisoned-proof by policy: a panicked holder yields `None` /
  no-op rather than propagating poison.

## Consequences

**Positive**

- No external dependency, no deployment footprint — a library, not a
  service. The desktop app gets caching for free.
- `Arc<ConfData>` makes the 6 h page cache effectively free on reads.

**Negative / accepted costs**

- No persistence across process restarts (that is what the disk snapshot
  layer, [ADR-0005](0005-disk-snapshot-layer.md), is for).
- No size bound: one year calendar per mosque is ~tens of KB; the threat is
  bounded by how many *distinct slugs* a process touches, not by attacker
  input directly (a hostile server cannot grow the cache without distinct
  slugs, and each entry is capped by the 20 MB response cap —
  [ADR-0009](0009-bounded-transport.md)).
- No TTL jitter/stampede protection: concurrent callers may both fetch.
  Accepted for a single-app client.
- `Instant`-based TTLs are wall-clock-insensitive to system sleep — a laptop
  that sleeps 12 h still serves a "6 h" page from cache. Accepted: prayer
  calendars are stable; the next successful fetch refreshes.

**Alternatives rejected**

- *Redis / external cache*: wrong process boundary for a library; the
  reference implementation's choice, not ours.
- *LRU with size bound*: complexity for a threat model that does not need
  it; revisit if an embedding hosts thousands of slugs per process.
