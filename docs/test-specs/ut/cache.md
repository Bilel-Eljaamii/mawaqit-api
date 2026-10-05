# Test Spec: `ut/cache.rs` — in-process TTL cache

- **Tier:** unit (`cargo test --test ut`), offline, deterministic, no I/O.
- **Target:** `mawaqit_api::cache::TtlCache` — `#[doc(hidden)]`-public
  purely so these tests live in `tst/` instead of `src/`; not part of the
  public API, not covered by semver.
- **Contract:** entries expire after the TTL; the entry cap evicts FIFO
  over still-present keys; overwrites never evict or grow; eviction is
  robust to stale order entries (review H1).

## Tests

### `stores_and_expires`
Insert, read back, invalidate, read again. Invariant: `get` returns the
stored value until invalidated.

### `zero_ttl_expires_immediately`
`Duration::ZERO` TTL: every `get` after insert is `None`.

### `cap_evicts_the_oldest_entries`
Cap of 2 with three inserts: the first key is evicted, the two newest
survive.

### `overwrite_does_not_evict_or_grow`
Re-inserting an existing key updates the value and must not trigger
eviction of other live entries.

### `eviction_survives_stale_order_entries`
After `invalidate("a")` the key lingers in the insertion-order queue;
subsequent inserts must skip the stale entry and still evict a live one
(the queue is an optimization, never a correctness source).

## Run

```sh
cargo test --test ut cache
```

A failure means the client's memory-bounding contract is broken: a
long-running desktop loop would grow without bound or evict fresh entries.
