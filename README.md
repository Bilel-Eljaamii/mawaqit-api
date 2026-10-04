# mawaqit-api

Keyless Rust client for [mawaqit.net](https://mawaqit.net) prayer times — no
account, no API key, nothing personal stored or sent. Built for
[mawaqit-desktop](https://github.com/<your-handle>/mawaqit-desktop) and for
anyone else who wants to build their own prayer-times app on the same public
data.

## What you get

- **Mosque search** via the public endpoint (`GET /api/2.0/mosque/search?word=…`).
- **One page, one year**: each mosque's public page embeds a `confData` object
  with today's times, the whole-year adhan calendar, the iqama calendar and
  mosque metadata — one fetch serves every query.
- **Resolved iqama**: `+15`-style relative offsets are expanded to absolute
  `HH:MM` automatically; all known calendar layouts are handled (including
  Diyanet "Sabah İmsak" 7-column rows).
- **Offline snapshots** (opt-in): the client can persist a mosque's confData to
  disk and fall back to it when the network is down — snapshot storage is
  atomic, hostile-file hardened, and keyed by hashed slug.
- **Hostile-tested**: a red-team suite (HTTP garbage, lying JSON, hostile
  slugs, wire-tolerated confData shapes, snapshot fuzzing) runs in normal
  `cargo test`.

## Usage

```rust
use mawaqit_api::MawaqitClient;

#[tokio::main]
async fn main() -> Result<(), mawaqit_api::MawaqitError> {
    let client = MawaqitClient::new();

    let mosques = client.search_mosques("Paris").await?;
    let slug = mosques[0].mosque_id().unwrap().to_string();

    // Adhan + resolved iqama for today.
    let today = client.today(&slug).await?;
    println!("{} — Fajr {}", slug, today.adhan.fajr);

    // Whole-month calendars.
    let month = client.month(&slug, 1).await?;
    println!("{} days in January", month.days.len());
    Ok(())
}
```

Enable the offline layer with one builder call:

```rust
let client = MawaqitClient::new()
    .with_disk_cache(std::path::PathBuf::from("./times-cache"));
```

Failed fetches now fall back to the stored snapshot; successful fetches
refresh it. `conf_data_dated` tells you which one you got.

## Tests

```sh
cargo test
```

The suite is offline and deterministic; the live-site campaign
(`tests/world_hostile.rs`) is `--ignored` by default.

## License

MIT
