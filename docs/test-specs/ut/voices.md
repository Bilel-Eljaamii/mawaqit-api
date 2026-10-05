# Test Spec: `ut/voices.rs` — adhan voice catalog

- **Tier:** unit (`cargo test --test ut`), offline, deterministic, no I/O.
- **Target:** the `voices` module's pure surface — `ADHAN_VOICES`,
  `adhan_voice_url`, `voice_id_from_conf` (moved out of `src/voices.rs`).
- **Contract:** URLs are built only from the static catalog; the catalog
  is duplicate-free; a mosque page's `adhanVoice` field validates against
  the catalog and never reaches the URL builder raw.

## Tests

### `urls_are_built_only_from_the_catalog`
Catalog ids produce `https://cdn.mawaqit.net/audio/{id}.mp3`; hostile ids
(empty, path escape, omitted-by-product id, query/fragment, wrong case)
return `None` — a caller-provided string can never shape the URL.

### `catalog_has_no_duplicates`
All 8 entries carry distinct ids (a duplicate would make the
`adhanVoice` mapping ambiguous).

### `voice_id_from_conf_validates_against_the_catalog`
A page whose `adhanVoice` is a catalog id maps to it; `null` (the
default) and unknown ids return `None`; the value is validated, never
passed through.

## Run

```sh
cargo test --test ut voices
```

A failure means the CDN URL contract broke (a hostile id could shape a
URL) or the catalog integrity drifted from the official apps' list.
