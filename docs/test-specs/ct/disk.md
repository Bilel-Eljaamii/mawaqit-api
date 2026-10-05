# Test Spec: `ct/disk.rs` — snapshot store on real files

- **Tier:** component (`cargo test --test ct`), offline; real temp dirs.
- **Target:** the `mawaqit_api::disk` module directly (`store`, `load`,
  `snapshot_path`) — moved out of `src/disk.rs` unchanged. (The
  client-level snapshot contract lives in `disk-cache.md`; this spec covers
  the store itself.)
- **Contract:** store→load round-trips faithfully, hostile files and slug
  mismatches degrade to `None`, hostile slugs stay inside the directory,
  atomic writes leave no temp files behind.

## Tests

### `store_then_load_roundtrips`
A stored conf reloads with the same fetch date and values.

### `roundtrip_survives_a_populated_raw_object`
The scraped conf carries the whole original JSON in `raw`; naive
serialization would duplicate calendar/times/name and the file could never
be read back (F10 regression guard).

### `missing_file_is_no_snapshot`
Absent file → `None`.

### `hostile_file_contents_degrade_to_none`
Empty, not-JSON, wrong kind, truncated envelope, bad date, future version —
every hostile file degrades to "no snapshot", never a panic.

### `messy_wire_shapes_in_raw_never_break_loading`
F10: wire shapes strict serde rejects (nulls in iqama rows, numeric names)
collapse at the scraper; the stored file carries the collapsed values and
reloads.

### `snapshot_never_serves_a_different_mosque`
Slug mismatch in the envelope → `None`.

### `hostile_slugs_stay_inside_the_directory`
`../../etc/passwd`, backslashes, `a/b/c`, empty, NUL/RTL, 100 KB — every
slug hashes to a `.json` file whose parent is the snapshot dir.

### `atomic_write_leaves_no_tmp_behind`
After store, the directory holds exactly one `.json` file and no `.tmp`.

## Run

```sh
cargo test --test ct disk::
```

A failure means the offline layer can serve wrong or hostile data, or that
a snapshot can escape its directory.
