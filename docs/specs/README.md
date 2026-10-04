# Specifications

Normative descriptions of what the library does today. Specs are updated in
the same change as the code they describe; the ADRs explain *why* the
behavior is what it is.

| Spec | Scope | Implemented by |
| --- | --- | --- |
| [`public-api.md`](public-api.md) | Every public type, method, function, error variant | `src/lib.rs` (re-exports), `src/client.rs`, `src/models.rs`, `src/error.rs`, `src/calendar.rs` |
| [`confdata-wire-format.md`](confdata-wire-format.md) | The `confData` object: extraction algorithm, field-by-field tolerance rules | `src/scraper.rs` |
| [`calendar-resolution.md`](calendar-resolution.md) | Row layouts, imsak mode, day keys, iqama resolution, the display contract | `src/calendar.rs` |
| [`offline-snapshots.md`](offline-snapshots.md) | Snapshot envelope, filenames, atomicity, load contract | `src/disk.rs` |
| [`transport-and-caching.md`](transport-and-caching.md) | Endpoints, headers, timeouts, caps, TTL caches, fallback flow | `src/client.rs`, `src/cache.rs` |

## Reading order

1. [`transport-and-caching.md`](transport-and-caching.md) — what goes on the wire.
2. [`confdata-wire-format.md`](confdata-wire-format.md) — what comes back and how it is extracted.
3. [`calendar-resolution.md`](calendar-resolution.md) — how raw rows become typed times.
4. [`public-api.md`](public-api.md) — the surface an embedder sees.
5. [`offline-snapshots.md`](offline-snapshots.md) — what happens when the wire is gone.

## Key invariants (cross-spec)

- **Nothing panics on hostile input.** Every parse/IO failure is `Err` or
  `None`. Pinned by the whole hostile-test pyramid.
- **Surfaced times are strict `HH:MM`** (hours < 24, minutes < 60) — see
  [`calendar-resolution.md`](calendar-resolution.md#display-contract) and
  [ADR-0010](../adr/0010-display-time-contract.md).
- **Hostile slugs never leave the mosque namespace** — see
  [ADR-0008](../adr/0008-slug-validation.md).
- **A snapshot serves exactly what the live path accepted** — see
  [`offline-snapshots.md`](offline-snapshots.md).
