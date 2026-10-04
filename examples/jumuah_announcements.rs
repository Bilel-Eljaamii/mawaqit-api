//! Jumu'a times and announcements currently in effect for a mosque.
//!
//! The confData object carries more than times: `jumua`/`jumua2` (some
//! mosques run two khutbas) and an announcements list with optional
//! `start_date`/`end_date` windows. This example classifies each
//! announcement against today — active, scheduled, expired, undated — the
//! filtering a mosque-info screen actually needs.
//!
//! Demonstrates:
//! - optional-field handling for `jumua` / `jumua2`;
//! - date-window parsing on `Announcement` (`%Y-%m-%d`, tolerate absent);
//! - reading unmodeled fields through `ConfData::raw`.
//!
//! Usage:
//!   cargo run -p mawaqit-api --example jumuah_announcements -- [slug]

use chrono::{Local, NaiveDate};
use mawaqit_api::{Announcement, MawaqitClient};

fn parse_date(s: Option<&str>) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s?.trim(), "%Y-%m-%d").ok()
}

fn classify(a: &Announcement, today: NaiveDate) -> String {
    match (
        parse_date(a.start_date.as_deref()),
        parse_date(a.end_date.as_deref()),
    ) {
        (None, None) => "undated — treat as active".to_string(),
        (Some(start), None) if start <= today => {
            format!("active since {start}")
        }
        (Some(start), None) => format!("scheduled from {start}"),
        (None, Some(end)) if end >= today => format!("active until {end}"),
        (None, Some(end)) => format!("expired {end}"),
        (Some(start), Some(end)) if start <= today && today <= end => {
            format!("active {start} → {end}")
        }
        (Some(start), Some(_)) if start > today => {
            format!("scheduled from {start}")
        }
        (Some(_), Some(end)) => format!("expired {end}"),
    }
}

fn one_line(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        format!("{}…", flat.chars().take(max - 1).collect::<String>())
    }
}

#[tokio::main]
async fn main() {
    let slug = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "grande-mosquee-de-paris".to_string());

    let client = MawaqitClient::new();
    let conf = client.conf_data(&slug).await.unwrap_or_else(|e| {
        eprintln!("fetch failed: {e}");
        std::process::exit(1);
    });

    println!("{} ({slug})", conf.name.as_deref().unwrap_or("?"));

    match (conf.jumua.as_deref(), conf.jumua2.as_deref()) {
        (Some(j1), Some(j2)) => println!("Jumu'a: {j1} and {j2} (two khutbas)"),
        (Some(j1), None) => println!("Jumu'a: {j1}"),
        (None, _) => println!("no Jumu'a time published (a musalla?)"),
    }

    let today = Local::now().date_naive();
    println!("\nannouncements as of {today}:");
    if conf.announcements.is_empty() {
        println!("  (none configured)");
    }
    for a in &conf.announcements {
        let title = a.title.as_deref().unwrap_or("(untitled)");
        println!("  • {title} — {}", classify(a, today));
        if let Some(content) = a.content.as_deref() {
            println!("    {}", one_line(content, 110));
        }
        if let Some(video) = a.video.as_deref() {
            println!("    video: {video}");
        }
    }

    // Everything the model does not map lives in `raw` — peek at what else
    // this mosque publishes.
    let extra_keys: Vec<&str> = conf
        .raw
        .as_object()
        .map(|o| o.keys().map(String::as_str).collect())
        .unwrap_or_default();
    let preview: Vec<&str> = extra_keys.iter().take(8).copied().collect();
    println!(
        "\n{} unmodeled confData field(s): {}",
        extra_keys.len(),
        preview.join(", ")
    );
}
