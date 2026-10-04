//! A tour of every [`MawaqitError`] variant — each one triggered for real —
//! plus a typed retry policy an app can copy.
//!
//! Two error families need opposite handling:
//! - **transient** (`Http`, `Api` with a 5xx): retry with backoff;
//! - **permanent** (`MosqueNotFound`, `ConfDataNotFound`, `Parse`,
//!   `InvalidMonth`, `NoCalendar`): retrying identical input is wasted work.
//!
//! Variants are demonstrated offline where possible (`parse_page` and the
//! pure calendar functions need no network); the two network-only ones
//! (a real 404 and a connection refusal) hit the live site and a dead
//! mirror respectively.
//!
//! Usage:
//!   cargo run -p mawaqit-api --example error_recovery

use std::time::Duration;

use mawaqit_api::{
    ConfData, MawaqitClient, MawaqitError, TodayTimes, month_times, parse_page,
};

/// Human verdict per variant.
fn describe(err: &MawaqitError) -> &'static str {
    match err {
        MawaqitError::Http(_) => {
            "transport failure (DNS/connect/TLS/timeout) — retryable"
        }
        MawaqitError::Api { status, .. } if (500..600).contains(status) => {
            "server-side 5xx — retryable"
        }
        MawaqitError::Api { .. } => "site answered non-success — inspect URL",
        MawaqitError::MosqueNotFound(_) => "unknown slug — permanent",
        MawaqitError::ConfDataNotFound(_) => {
            "page layout changed, no confData — permanent until fixed"
        }
        MawaqitError::InvalidMonth(_) => "programmer error — permanent",
        MawaqitError::NoCalendar => "mosque publishes no usable calendar",
        MawaqitError::Parse(_) => "malformed payload — permanent",
    }
}

fn retryable(err: &MawaqitError) -> bool {
    matches!(
        err,
        MawaqitError::Http(_) | MawaqitError::Api { status: 500..=599, .. }
    )
}

/// Retry loop with exponential backoff for the transient family only.
async fn today_with_retry(
    client: &MawaqitClient,
    slug: &str,
    attempts: u32,
) -> Result<TodayTimes, MawaqitError> {
    let mut delay = Duration::from_millis(150);
    let mut remaining = attempts;
    loop {
        match client.today(slug).await {
            Ok(today) => return Ok(today),
            Err(e) if retryable(&e) && remaining > 1 => {
                remaining -= 1;
                eprintln!("  transient: {e} — backing off {delay:?}");
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
            Err(e) => return Err(e),
        }
    }
}

fn show(label: &str, err: &MawaqitError) {
    println!("{label}\n  → {err}\n  → {}", describe(err));
}

#[tokio::main]
async fn main() {
    // ---- permanent, pure: no network needed -------------------------------
    let empty = ConfData::default();
    if let Err(e) = month_times(&empty, 13) {
        show("\n[1] month_times(month = 13)", &e);
    }
    if let Err(e) = month_times(&empty, 5) {
        show("\n[2] month_times on a mosque with no calendar", &e);
    }
    if let Err(e) = parse_page("<html><body>no script here</body></html>", "x")
    {
        show("\n[3] parse_page on a page without confData", &e);
    }
    if let Err(e) =
        parse_page("<script>var confData = {times: not json};</script>", "x")
    {
        show("\n[4] parse_page on malformed confData", &e);
    }

    // ---- permanent, network: a slug that 404s ------------------------------
    let client = MawaqitClient::new();
    match client.conf_data("this-mosque-does-not-exist-anywhere").await {
        Err(e) => show("\n[5] conf_data on a nonexistent slug (live 404)", &e),
        Ok(_) => println!("\n[5] unexpected success"),
    }

    // ---- transient: connection refused, retried with backoff --------------
    println!("\n[6] retrying a dead mirror 3 times (exponential backoff):");
    let dead = MawaqitClient::with_base_urls(
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    );
    match today_with_retry(&dead, "grande-mosquee-de-paris", 3).await {
        Err(e) => show("  gave up after 3 attempts", &e),
        Ok(today) => println!("  recovered: fajr {}", today.adhan.fajr),
    }

    // ---- the API-status family (documented; needs a hostile server) -------
    println!(
        "\n[7] Api {{ status, url }} fires on non-404 failures — see\n    tst/ct/hostile_http.rs, which drives it against a local mock."
    );
}
