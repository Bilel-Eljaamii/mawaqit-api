# ADR-0005: Opt-in disk snapshot layer with hashed-slug files and atomic writes

- **Status:** Accepted
- **Date:** 2026-10, revised after finding F10 and again after red-team
  round 2 (findings F23/F27)
- **Decides:** The offline layer: file layout, envelope, atomicity, and the
  load contract.
- **Related:** [ADR-0003](0003-tolerant-wire-parsing.md) (F10),
  [`specs/offline-snapshots.md`](../specs/offline-snapshots.md).

## Context

The tray app must survive outages: mosques were fetched yesterday, the
network is gone today. Because one page = one year
([ADR-0002](0002-one-page-one-year.md)), a single small file per mosque
serves the Today view, month views and alarms offline.

The snapshot directory is **attacker-writable at the same privilege as the
app config**: it lives next to app state and any local process/malware with
the user's rights can write files there. So `load` must be *total* — a
missing, truncated or hostile file yields "no snapshot", never a panic —
and a hostile *slug* must never control a path.

## Decision

`src/disk.rs`, activated by `MawaqitClient::with_disk_cache(dir)`:

1. **Filename is a hash, not a slug.** `snapshot_path` formats
   `DefaultHasher(slug)` as `{hash:016x}.json`. Path traversal (`../`),
   separators, NULs, 4 MB unicode slugs — none can escape the directory or
   forge another mosque's filename. The real slug travels *inside* the
   envelope and is verified on load.
2. **Versioned envelope** (`SNAPSHOT_VERSION = 1`):
   `{ version, mosque_slug, fetched_at: NaiveDate, conf: Value }`. Load
   rejects wrong `version` or a `mosque_slug` mismatch, so a file can never
   be served for a different mosque, and future format changes ignore old
   files instead of misparsing them.
3. **Storage value = struct serialization overlaid with raw extras**
   (`conf_to_storage`). Naively storing the raw confData would write back
   wire shapes strict serde rejects (finding **F10**: a page that *parses*
   fine would store a snapshot that *never loads*). Naively storing only the
   typed struct would lose unmodeled fields. The fix: serialize the struct
   (nulling its flattened `raw`, which would otherwise duplicate keys),
   then `or_insert` every unmodeled key from `raw`. Modeled keys always win,
   so hostile shapes inside modeled fields can never reach the file.
4. **Atomic writes**: serialize fully in memory → write a temp file →
   `rename` onto `path.json`. A crash mid-write leaves the previous
   snapshot intact and no `.tmp` behind on success. Round 2 (F23): the
   temp name is **per-writer unique** (`disk::tmp_path`, pid + process
   sequence) — the old fixed `X.json.tmp` let two writers racing on one
   slug share the temp path and rename a torn file into place.
5. **Total load**: any failure (missing file, bad JSON, bad date, version
   mismatch, slug mismatch, hostile `conf`) → `None`. Callers treat it as
   "no snapshot". Round 2 (F23) adds two members to that family: a file
   **over the 2 MB read cap** (`MAX_SNAPSHOT_BYTES`; a real snapshot is
   ~60–80 KB) and a **non-UTF8 file** are both "no snapshot" — the read
   is capped before parsing, so a 10 GB planted file costs one bounded
   read, not an RSS blowup. Loaded confs also go through the **shared
   sanitizer** (`sanitize::confdata`, F23) — the offline path strips
   exactly what the online page path strips.
6. **Client integration** (`conf_data_dated`): successful network fetch →
   best-effort `store` (a failed write never breaks an online fetch);
   failed network fetch → `load` and return `(conf, Some(fetched_at))` so
   the caller knows the data is from disk and how stale it is.
7. **Staleness is decided inside `load`** (round 2, F27 — supersedes the
   earlier "no TTL by design" stance below). `load_as_of(dir, slug,
   today)` refuses a snapshot that is more than
   [`SNAPSHOT_MAX_AGE_DAYS`] (40) days old **or** that was fetched in a
   different calendar year than `today` (one page = one year,
   [ADR-0002](0002-one-page-one-year.md): a December snapshot never
   answers a January date). Refusal is honest: offline + stale is "no
   snapshot", and the caller surfaces the real network error instead of
   year-old times on the alarm path. `load` delegates with today's date.

## Consequences

**Positive**

- Offline mode is exactly "what the online path accepted", for messy
  real-world mosques too (F10 regression-pinned in
  `src/disk.rs::messy_wire_shapes_in_raw_never_break_loading` and
  `tst/fuzz/mutation.rs::finding_f10_snapshot_roundtrips_wire_tolerated_shapes`).
- The snapshot layer has no slug-injection surface and no
  snapshot-confusion surface (both fuzz-pinned).

**Negative / accepted costs**

- `DefaultHasher` (SipHash-1-3 with fixed keys) is not collision-resistant
  against a determined attacker who can choose *both* slugs; accepted — an
  attacker who can mint slugs server-side can also just serve hostile
  content for their own slug, and the envelope slug check prevents
  cross-mosque confusion anyway.
- One snapshot per slug, whole-file rewrite per refresh: fine at ~60 KB per
  mosque; not a change-log/history format.
- ~~No TTL on snapshots by design~~ — **revised in round 2 (F27)**: the
  loader now enforces the 40-day / same-year rule itself; `fetched_at` is
  still reported so callers can *display* snapshot age
  (`examples/conf_diff.rs`). A caller that wants different freshness
  semantics uses `load_as_of` with its own clock.

**Alternatives rejected**

- Slug-derived filenames: path traversal and forgery surface, killed by fuzz.
- Storing raw verbatim: F10.
- SQLite store: dependency weight for a flat key→file mapping.
