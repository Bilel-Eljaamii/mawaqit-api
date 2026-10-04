# Test Spec: `ct/disk_cache.rs` — the offline snapshot contract

- **Tier:** component (`cargo test --test ct`), offline, deterministic.
- **Target:** `MawaqitClient` + `mawaqit_api::disk` through their real
  boundaries: a raw-TCP mock (can be killed mid-test) and real temp dirs.
- **Contract:** a successful fetch refreshes the snapshot; a failed fetch
  serves it; a hostile snapshot file degrades to a plain error — never a
  panic.

## Fixtures

- `spawn_mock(response)` — raw TCP server serving one canned response to
  every request; dropping its handle kills it.
- `mosque_page()` — minimal valid page (6 daily times, one-month
  calendar).
- `DEAD_BASE = http://127.0.0.1:1` — connection refused instantly (the
  "network is gone" simulation).
- `SLUG = "grande-mosquee-de-paris"`.
- Snapshots are seeded **through the public API** (online client fetch),
  never by hand-writing files — so every test exercises the real
  store→load path.

## Tests

### `successful_fetch_writes_a_snapshot_and_serves_memory_afterwards`
1. Online fetch ⇒ `(conf, as_of = None)` — a live fetch is never "from
   disk".
2. `disk::load` directly ⇒ snapshot exists with today's `fetched_at` and
   the right name.
3. Second fetch ⇒ in-memory cache (still `as_of = None`), server dropped
   to prove no hidden second request is needed.

### `failed_fetch_falls_back_to_the_snapshot`
1. Seed with an online client, then kill the server.
2. A fresh client with the same `dir` but `DEAD_BASE` fetches ⇒ succeeds
   from snapshot, `as_of.is_some()`.
3. Derived paths work offline too: `offline.month(SLUG, 1)` ⇒ non-empty.

### `offline_without_a_snapshot_is_an_error`
Empty dir + dead base ⇒ `Err`. No snapshot, no fallback, no fabrication.

### `hostile_snapshot_file_degrades_to_an_error`
Snapshot file replaced with a truncated envelope (`{"version":1,"mos`) +
dead base ⇒ `Err`. The total-load contract at the client level.

### `without_disk_cache_the_client_behaves_as_before`
No `with_disk_cache` + dead base ⇒ `Err` — the offline layer is opt-in and
its absence changes nothing else.

## Cross-references

- Envelope format, hashed filenames, atomicity:
  [`../../specs/offline-snapshots.md`](../../specs/offline-snapshots.md).
- Unit-tier hostile-file matrix and slug-confinement matrix:
  `src/disk.rs` `#[cfg(test)]` (round-trips, hostile contents list, slug
  traversal, `.tmp` residue).
- Mutation-level snapshot fuzzing:
  [`../fuzz/mutation.md`](../fuzz/mutation.md).
