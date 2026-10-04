//! Cache invalidation and change detection across refetches.
//!
//! The client memoizes each mosque's confData for 6 hours — right for a
//! tray app, wrong when you want to *watch* for changes. This example shows
//! the levers around the cache:
//!
//! - `invalidate(Some(slug))` / `invalidate(None)` force the next call back to
//!   the network;
//! - a fingerprint of "today's row" plus every month's shape makes drift
//!   visible between two fetches;
//! - `conf_data_dated` on a forced-offline client reports the snapshot's fetch
//!   date — the staleness signal for the offline layer.
//!
//! Usage:
//!   cargo run -p mawaqit-api --example conf_diff -- [slug] [cache dir]

use std::path::PathBuf;

use chrono::Local;
use mawaqit_api::{ConfData, MawaqitClient};

const CUT: &str = "http://127.0.0.1:1";

/// Fingerprint: today's adhan row + the shape of all 12 months + identity.
fn fingerprint(conf: &ConfData) -> String {
    let today = Local::now().date_naive();
    let today_row = mawaqit_api::times_for_date(conf, today)
        .map(|t| {
            [
                t.adhan.fajr.as_str(),
                t.adhan.shurouq.as_str(),
                t.adhan.dhuhr.as_str(),
                t.adhan.asr.as_str(),
                t.adhan.maghrib.as_str(),
                t.adhan.isha.as_str(),
            ]
            .join(" ")
        })
        .unwrap_or_else(|_| "(no row for today)".to_string());
    let months: Vec<String> = conf
        .calendar
        .iter()
        .enumerate()
        .map(|(i, m)| format!("{}:{}", i + 1, m.len()))
        .collect();
    format!(
        "name={:?} jumua={:?} today=[{}] months=[{}]",
        conf.name,
        conf.jumua,
        today_row,
        months.join(" ")
    )
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let slug =
        args.next().unwrap_or_else(|| "grande-mosquee-de-paris".to_string());
    let dir = PathBuf::from(
        args.next().unwrap_or_else(|| "./times-cache".to_string()),
    );

    let client = MawaqitClient::new().with_disk_cache(dir.clone());

    // Fetch #1 — fresh from the network (source date = None).
    let (first, source) =
        client.conf_data_dated(&slug).await.unwrap_or_else(|e| {
            eprintln!("first fetch failed: {e}");
            std::process::exit(1);
        });
    println!("#1 fresh fetch: from-snapshot={source:?}");
    let fp1 = fingerprint(&first);
    println!("  {fp1}");

    // Fetch #2 — still cached: identical fingerprint, zero requests.
    let (_, source2) =
        client.conf_data_dated(&slug).await.unwrap_or_else(|e| {
            eprintln!("cached fetch failed: {e}");
            std::process::exit(1);
        });
    println!(
        "\n#2 without invalidate: from-snapshot={source2:?} (memory cache hit)"
    );

    // Fetch #3 — drop the cache entry, force the network again.
    client.invalidate(Some(&slug));
    let (second, source3) =
        client.conf_data_dated(&slug).await.unwrap_or_else(|e| {
            eprintln!("refetch failed: {e}");
            std::process::exit(1);
        });
    let fp2 = fingerprint(&second);
    println!("#3 after invalidate(Some(slug)): from-snapshot={source3:?}");
    println!("  {fp2}");

    if fp1 == fp2 {
        println!("\nno drift between the two network fetches (expected)");
    } else {
        println!("\nDRIFT DETECTED — the mosque changed something live:");
        println!("  before: {fp1}");
        println!("  after:  {fp2}");
    }

    // Drop everything cached, then read through the offline layer to age
    // the snapshot.
    client.invalidate(None);
    let offline =
        MawaqitClient::with_base_urls(CUT.to_string(), CUT.to_string())
            .with_disk_cache(dir);
    match offline.conf_data_dated(&slug).await {
        Ok((_, Some(fetched))) => {
            let age = (Local::now().date_naive() - fetched).num_days();
            println!(
                "\nsnapshot for {slug}: fetched {fetched} ({age} day(s) old)"
            );
        }
        Ok((_, None)) => {
            println!("\nunexpected: offline client got fresh data")
        }
        Err(e) => println!("\nno snapshot to fall back to: {e}"),
    }
}
