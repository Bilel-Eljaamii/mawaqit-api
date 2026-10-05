# mawaqit-api

[![CI](https://github.com/Bilel-Eljaamii/mawaqit-api/actions/workflows/ci.yml/badge.svg)](https://github.com/Bilel-Eljaamii/mawaqit-api/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/mawaqit-api)](https://crates.io/crates/mawaqit-api)
[![docs.rs](https://img.shields.io/docsrs/mawaqit-api)](https://docs.rs/mawaqit-api)
[![license](https://img.shields.io/github/license/Bilel-Eljaamii/mawaqit-api)](LICENSE)
[![coverage](https://img.shields.io/badge/coverage-100%25%20lines-brightgreen)](docs/test-specs/README.md)
[![no_std](https://img.shields.io/badge/no__std-alloc%20%7C%20heapless-blue)](docs/adr/0013-no-std-and-mcu-support.md)

Keyless Rust client for [mawaqit.net](https://mawaqit.net) prayer times — no
account, no API key, nothing personal stored or sent. Built for
[mawaqit-desktop](https://github.com/Bilel-Eljaamii/mawaqit-desktop) and for
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
- **`no_std` embedded tier**: the pure core compiles for bare-metal MCUs;
  prayer calendars pack to a CRC-sealed 4-byte-aligned binary that firmware
  queries from flash with zero heap (see
  [Embedded / `no_std`](#embedded--no_std-mcu)).

## Usage

```rust
use mawaqit_api::MawaqitClient;

#[tokio::main]
async fn main() -> Result<(), mawaqit_api::MawaqitError> {
    let client = MawaqitClient::new();

    let mosques = client.search_mosques("Paris").await?;
    // Search results may lack a page slug — don't index-and-unwrap.
    let slug = mosques
        .iter()
        .find_map(|m| m.mosque_id())
        .expect("search returned no slug — pick another result")
        .to_string();

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

Route all traffic through Tor (or any SOCKS5 proxy) with one more builder
call — remote DNS (`socks5h://`) is enforced, wrong schemes are rejected
at construction, and timeouts rise for slow circuits automatically. The
library never enables it by default and never starts a Tor daemon:

```rust
let client = MawaqitClient::new()
    // system tor; Tor Browser users pass 9150
    .with_socks_proxy("socks5h://127.0.0.1:9050")?;
```

## Examples

Twelve runnable programs under [`examples/`](examples/) — search, calendars,
offline mirroring, concurrency, error recovery, security hardening. All
compile with the library; the two marked *offline* need no network.

| Example | What it demonstrates |
| --- | --- |
| [`next_prayer`](examples/next_prayer.rs) | search → today's times → countdown to the next prayer |
| [`year_export`](examples/year_export.rs) | one fetch, whole-year adhan + iqama export to JSON/CSV |
| [`offline_mirror`](examples/offline_mirror.rs) | disk snapshots warmed concurrently, read back with the network cut |
| [`world_dashboard`](examples/world_dashboard.rs) | fan-out over cities, who prays next across the board |
| [`iqama_audit`](examples/iqama_audit.rs) | relative (`+N`) vs absolute iqama config, delay statistics |
| [`week_alarm_plan`](examples/week_alarm_plan.rs) | future-date lookups, 7-day alarm schedule with lead offsets |
| [`search_rank`](examples/search_rank.rs) | multi-term search, dedupe, naive relevance ranking |
| [`error_recovery`](examples/error_recovery.rs) | every `MawaqitError` variant + retry with backoff |
| [`conf_diff`](examples/conf_diff.rs) | cache invalidation, drift detection, snapshot staleness |
| [`jumuah_announcements`](examples/jumuah_announcements.rs) | Jumu'a times, announcement date windows, raw extras |
| [`page_scraper`](examples/page_scraper.rs) | *offline* — HTML → `ConfData` pipeline on synthetic pages |
| [`slug_hardening`](examples/slug_hardening.rs) | *offline* — hostile-slug and snapshot-path defense matrix |

Run any of them (args optional, each has a usage header):

```sh
just example next_prayer "Grande Mosquée de Paris"
just example year_export grande-mosquee-de-paris csv
# or plain cargo:
cargo run -p mawaqit-api --example next_prayer -- Paris
```

## Task runner

All build steps live in the [`justfile`](justfile) — install `just` with
`pacman -S just` (or `cargo install just`), then:

| Command | What it does |
| --- | --- |
| `just verify` | **the gate**: fmt check → clippy (`-D warnings`) → type check → offline tests → docs → offline example smoke |
| `just build` / `just release` | debug / release build, examples included |
| `just fmt` | format (nightly rustfmt; the config uses unstable options) |
| `just lint` | clippy with warnings as errors |
| `just test` | offline, deterministic suite (hostile HTTP/semantics/corpus, snapshots) |
| `just live` | the `--ignored` live-site campaign (100+ real mosques) |
| `just coverage` | coverage report for the offline suite (HTML + lcov via `cargo-llvm-cov`) |
| `just doc` | rustdoc |
| `just fuzz [target] [secs]` | cargo-fuzz wrapper for the `fuzz/` targets |
| `just example <name>` | run one example |
| `just graph` | GitNexus graph change analysis (required before committing) |
| `just clean` | remove build artifacts |

## Adhan voices

The mosque-screen adhan recordings are served keylessly from Mawaqit's CDN.
The catalog (Makkah, Madinah, Al-Aqsa/Qods, Algeria, Egypt — plus Fajr
variants) and the downloader live here:

```rust
use mawaqit_api::{ADHAN_VOICES, adhan_voice_url, download_voice};

for voice in &ADHAN_VOICES {
    println!("{} -> {}", voice.name, adhan_voice_url(voice.id).unwrap());
}

let path = download_voice(&client, "adhan-quds", &cache_dir).await?;
```

Downloads are capped (8 MB), atomic, cached (a second call is a no-op), and
routed through the client's transport — a proxy (Tor) applies. The mosque's
own choice is available via `voice_id_from_conf(&conf_data)`.

## Embedded / `no_std` (MCU)

The domain core compiles for bare metal — ESP32, RP2040, STM32, RISC-V
([ADR-0013](docs/adr/0013-no-std-and-mcu-support.md)). Three Cargo feature
tiers in one crate:

| Feature | Tier | What you get |
| --- | --- | --- |
| `std` (default) | desktop/server/CLI | everything: HTTP client, snapshots, voice downloads, the MQTC packer |
| `alloc` | no_std + heap | dynamic `ConfData`, `parse_page`, calendar resolution |
| `heapless` | no_std, zero-alloc | `CompactCalendarView` — prayer times queried straight from memory-mapped flash, 0 bytes of heap |

The MCU path: pack a mosque's calendar **before** flashing (workstation,
std side), then query it in O(1) on the device:

```bash
cargo run --example pack_for_mcu -- --slug grande-mosquee-de-paris \
  --scope year --compress delta --format rust --out firmware/src/prayer_data.rs
```

```rust,ignore
use mawaqit_api::compact::CompactCalendarView;

static PRAYER_DATA: &[u8] = include_bytes!("prayer_data.bin");

let calendar = CompactCalendarView::from_bytes(PRAYER_DATA)?; // CRC-checked
if let Some(today) = calendar.times_for_date(rtc.today()) {
    display.show(today.adhan.fajr.to_hhmm()); // heapless::String<5>, 0 alloc
}
```

Binary layout, record formats, and integrity guarantees:
[`docs/specs/compact-binary-and-mcu.md`](docs/specs/compact-binary-and-mcu.md).
The feature matrix ({host, thumbv7em, riscv32} × {std, alloc, heapless}) is
part of `just verify` and CI.

## Tests

Integration tests live in `tst/` as a four-tier pyramid — one binary per
tier, submodules in the same-named directory, shared helpers in `common/`:

| Tier | Binary | What it covers |
| --- | --- | --- |
| unit | `tst/ut.rs` + `tst/ut/` | pure parse/calendar semantics against hostile input — no I/O |
| component | `tst/ct.rs` + `tst/ct/` | client vs mock HTTP server, disk snapshot layer — offline |
| e2e | `tst/e2e.rs` + `tst/e2e/` | live-site world tour over 100+ real mosques |
| fuzz | `tst/fuzz.rs` + `tst/fuzz/` | deterministic seed-driven mutation fuzzing |

```sh
just test          # ut + ct + fuzz — offline, deterministic
just tier ct       # one tier: ut | ct | fuzz | e2e
just live          # the e2e world tour (#[ignore]d by default, needs network)
just fuzz          # the nightly libFuzzer campaign in fuzz/
```

The red-team findings are all fixed and pinned as always-run regression
tests (F1 redirects, F2 hostile slugs, F4 invalid times, F5 duplicate day
keys, F6 control characters). Only the live-site tiers stay `#[ignore]`d —
`just live` runs them.

## Changelog

Notable changes per release live in [`CHANGELOG.md`](CHANGELOG.md)
(Keep a Changelog format; semver).

## License

MIT
