# Test Spec: `fuzz/mutation.rs` — deterministic seed-driven mutation fuzzer

- **Tier:** fuzz (`cargo test --test fuzz`), offline, CI-safe, fast.
- **Contract:** the hostile-data contract, stated as fuzz invariants —
  nothing panics; everything hostile degrades to `Err`/`None` or a
  well-formed value; whatever parses must survive `dig()`.
- **Method:** known-valid seeds, deterministic mutations, fixed xorshift
  seed `0x9E3779B97F4A7C15` — every run explores the same space, so a
  failure is reproducible by seed alone. This tier covers the surfaces the
  [ut corpus](../ut/corpus.md) does not reach: the search-response model,
  the URL builder, and the snapshot layer.

## Mutation engine

`Rng` — xorshift64 (seeded `| 1` so it never stalls at 0).

`mutate(rng, data)` — exactly one operator per mutant:

| # | Operator | Detail |
| --- | --- | --- |
| 0 | truncate | cut at a random point (0..=len) |
| 1 | byte flips | 1..=8 random bytes overwritten |
| 2 | interesting splice | insert 1..16 bytes from `INTERESTING` at a random point |
| 3 | duplicate slice | copy a random slice to another random position |

`INTERESTING` bytes — the ones that break parsers: JSON structure
(`" \ { } [ ] : ,`), signs and dots (`+ - .`), path metacharacters
(`/ ? # @`), digits and space, newline/tab, NUL, DEL, UTF-8 lead bytes
(`0x80 0xC3 0xE2 0xAC 0xFF`).

`for_each_mutation(seeds, iters_per_seed, f)` drives one shared RNG across
all seeds (2500 iterations per seed in the tests below).

## Seeds

- `SEARCH_SEED` — a realistic search response exercising every field shape:
  full entry, sparse entries (`{"slug":"a"}`), odd-but-legal types
  (`"id":{"deep":[1,2]}`), unicode slug, `".."` as a name.
- Snapshot seed — built by parsing a rich page through `parse_page` and
  storing it via `disk::store`, then reading the **exact bytes on disk** —
  so mutations start from what production actually writes.

## Tests

### `fuzz_search_response_parsing_never_panics`
2500 mutations of `SEARCH_SEED` deserialized as `Vec<Mosque>`; for every
mosque that parses: `display_name()`, `place()`, `mosque_id()` must be
total; a parsed slug is fed to `page_url` (totality only — confinement is
ct's F2 contract) and `minutes_between(slug, "13:00")` (must return
`Option`, never panic).

### `fuzz_disk_snapshot_load_never_panics`
2500 mutations of the snapshot bytes written to the real snapshot path,
then `disk::load`:
- `Some` ⇒ `fetched_at.year() == 2026` and `dig(&conf)` — a loaded
  snapshot is structurally alive, always.
- `None` is a fine outcome.
Afterwards the pristine seed is rewritten and must load — the fuzz run
itself must not have corrupted the layer.

### `finding_f10_snapshot_roundtrips_wire_tolerated_shapes` — FIXED, green
The regression anchor for the F10 fix ([ADR-0005](../../adr/0005-disk-snapshot-layer.md)):
a page whose live parse tolerates a `null` iqama entry and a numeric name
must store a snapshot that **loads back** with the collapsed values (5
times, tolerant name), not fail offline.

### `fuzz_disk_store_load_roundtrip_under_hostile_slugs`
Seven hostile slugs (empty, `../etc/passwd`, `a/b/c`, NUL+RTL+BOM, 🕌×100,
100 000 × `x`, plus the benign one) × 400 conf mutations each:
- `snapshot_path` stays inside `dir` with extension `json` — **asserted
  every iteration**;
- `store` ⇒ `Some(date)`; `load` ⇒ same date and a `dig()`-able conf, or
  `None` — never a foreign snapshot.

### `fuzz_search_and_snapshot_coexist_bounded`
Resource-exhaustion shape: a legal-JSON search entry with a 1 MB name
(must parse; `display_name().len() == 1_000_000`); a `ConfData` whose raw
carries a 2 MB string — `store`/`load` either handle it or degrade to
`None`, never hang or panic.

## Maintainer rules

- A failure here is reproducible: the RNG is fixed — rerun `cargo test
  --test fuzz` and the same mutant fails; minimize the mutant by hand into
  a ut-corpus case, then fix.
- New surfaces (a new public function taking attacker-shaped data) get a
  fuzz test here with a seed derived from its real output.
