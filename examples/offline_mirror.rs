//! The offline layer end to end: warm disk snapshots for several mosques
//! concurrently, then cut the network and watch the client keep working.
//!
//! Demonstrates:
//! - `.with_disk_cache(dir)` — the one-builder-call offline mode;
//! - `tokio::spawn` fan-out over a shared, `Clone`-able client;
//! - `conf_data_dated`: `None` = fresh fetch, `Some(date)` = snapshot;
//! - the forced-offline trick: point `with_base_urls` at an unroutable address
//!   — the only way to simulate "network down" without iptables;
//! - `mawaqit_api::disk` for inspecting the snapshot store directly.
//!
//! Usage:
//!   cargo run -p mawaqit-api --example offline_mirror -- [city ...] [cache
//! dir] Example:
//!   cargo run -p mawaqit-api --example offline_mirror -- Paris Berlin
//! ./times-cache

use std::{path::PathBuf, sync::Arc};

use mawaqit_api::{MawaqitClient, disk};

/// Where the offline demo points the client: port 1 refuses instantly, so
/// the fallback path runs in milliseconds instead of hanging on a timeout.
const CUT: &str = "http://127.0.0.1:1";

#[tokio::main]
async fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let dir = match args.iter().position(|a| a.starts_with('.')) {
        Some(i) => PathBuf::from(args.remove(i)),
        None => PathBuf::from("./times-cache"),
    };
    let cities = if args.is_empty() {
        vec!["Paris".to_string(), "Berlin".to_string()]
    } else {
        args
    };

    let online = MawaqitClient::new();

    // 1. Resolve each city to its first mosque that has a page slug.
    let mut targets = Vec::new();
    for city in &cities {
        match online.search_mosques(city).await {
            Ok(mosques) => match mosques.iter().find_map(|m| {
                m.mosque_id().map(|slug| {
                    (m.display_name().to_string(), slug.to_string())
                })
            }) {
                Some((name, slug)) => targets.push((city.clone(), name, slug)),
                None => eprintln!("{city}: no search result carries a slug"),
            },
            Err(e) => eprintln!("{city}: search failed ({e})"),
        }
    }
    if targets.is_empty() {
        eprintln!("nothing to mirror");
        std::process::exit(1);
    }

    // 2. Warm the snapshots concurrently. `MawaqitClient` is `Clone` and the
    //    clone shares one connection pool + memory cache.
    let warmer = Arc::new(online.clone().with_disk_cache(dir.clone()));
    let mut set = tokio::task::JoinSet::new();
    for (city, name, slug) in &targets {
        let client = warmer.clone();
        let (city, name, slug) = (city.clone(), name.clone(), slug.clone());
        set.spawn(async move {
            let data = client.conf_data_dated(&slug).await;
            (city, name, slug, data)
        });
    }
    while let Some(joined) = set.join_next().await {
        let (city, name, slug, data) = joined.expect("warm task panicked");
        match data {
            Ok((conf, _fresh)) => println!(
                "warmed {city:<10} {name} ({slug}) — {} calendar month(s)",
                conf.calendar.len()
            ),
            Err(e) => eprintln!("warmed {city:<10} FAILED: {e}"),
        }
    }

    // 3. Inspect the store on disk — hashed file names, real slug inside.
    println!("\nsnapshot store: {}", dir.display());
    let files: Vec<_> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|f| f.ends_with(".json"))
                .collect()
        })
        .unwrap_or_default();
    println!("  {} snapshot file(s)", files.len());
    let (_, _, first_slug) = &targets[0];
    println!(
        "  {} → hashed as {}",
        first_slug,
        disk::snapshot_path(&dir, first_slug)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    );

    // 4. Cut the network and read back through the offline client.
    let offline =
        MawaqitClient::with_base_urls(CUT.to_string(), CUT.to_string())
            .with_disk_cache(dir.clone());
    println!("\nnetwork cut — serving from snapshots:");
    for (city, name, slug) in &targets {
        match offline.conf_data_dated(slug).await {
            Ok((conf, Some(fetched))) => println!(
                "  {city:<10} {name:<38} snapshot from {fetched}, fajr {}",
                conf.times.first().map(String::as_str).unwrap_or("?")
            ),
            Ok((_, None)) => println!(
                "  {city:<10} {name:<38} UNEXPECTED fresh data — is {CUT} reachable?"
            ),
            Err(e) => println!("  {city:<10} {name:<38} failed: {e}"),
        }
    }

    // 5. Same store, read directly without the client.
    if let Some((fetched, conf)) = disk::load(&dir, first_slug) {
        println!(
            "\ndirect disk::load({first_slug}): fetched {fetched}, {} times today",
            conf.times.len()
        );
    }
}
