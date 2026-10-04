//! A worldwide "who prays next" dashboard: for each city, take the first
//! search hit, fetch today's times for all of them concurrently, and report
//! which mosque on the board has the earliest upcoming prayer right now.
//!
//! Demonstrates:
//! - fan-out with `tokio::task::JoinSet` over a shared client (one TLS session
//!   pool, N parallel page fetches);
//! - per-city error isolation: one dead mosque never takes down the board;
//! - cross-mosque comparison of "HH:MM" rows against a single clock reading.
//!
//! Usage:
//!   cargo run -p mawaqit-api --example world_dashboard -- [city ...]

use chrono::{Local, Timelike};
use mawaqit_api::{MawaqitClient, TodayTimes};

const DEFAULT_CITIES: [&str; 6] =
    ["Paris", "London", "Berlin", "Istanbul", "Casablanca", "Jakarta"];

struct Row {
    city: String,
    name: String,
    today: TodayTimes,
}

fn hhmm_minutes(s: &str) -> Option<i64> {
    let (h, m) = s.split_once(':')?;
    Some(h.trim().parse::<i64>().ok()? * 60 + m.trim().parse::<i64>().ok()?)
}

/// (prayer name, time, minutes from `now`, is today) for the next prayer.
fn next_prayer(
    today: &TodayTimes,
    now_min: i64,
) -> Option<(&'static str, String, i64)> {
    let adhan = [
        ("Fajr", &today.adhan.fajr),
        ("Dhuhr", &today.adhan.dhuhr),
        ("Asr", &today.adhan.asr),
        ("Maghrib", &today.adhan.maghrib),
        ("Isha", &today.adhan.isha),
    ];
    for (name, time) in adhan {
        let Some(min) = hhmm_minutes(time) else { continue };
        if min > now_min {
            return Some((name, time.clone(), min - now_min));
        }
    }
    // Every prayer rang: tomorrow's Fajr, via midnight wrap.
    let fajr = hhmm_minutes(&today.adhan.fajr)?;
    Some(("Fajr (tomorrow)", today.adhan.fajr.clone(), 1440 - now_min + fajr))
}

#[tokio::main]
async fn main() {
    let cities: Vec<String> = {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.is_empty() {
            DEFAULT_CITIES.iter().map(|s| s.to_string()).collect()
        } else {
            args
        }
    };

    let client = MawaqitClient::new();
    let mut set = tokio::task::JoinSet::new();

    for city in cities {
        let client = client.clone();
        set.spawn(async move {
            let mosques = client
                .search_mosques(&city)
                .await
                .map_err(|e| e.to_string())?;
            let mosque = mosques
                .iter()
                .find(|m| m.mosque_id().is_some())
                .ok_or_else(|| "no result carries a slug".to_string())?;
            let slug = mosque.mosque_id().unwrap_or_default().to_string();
            let today = client.today(&slug).await.map_err(|e| e.to_string())?;
            Ok::<Row, String>(Row {
                city,
                name: mosque.display_name().to_string(),
                today,
            })
        });
    }

    let mut rows: Vec<Row> = Vec::new();
    while let Some(joined) = set.join_next().await {
        match joined.expect("dashboard task panicked") {
            Ok(row) => rows.push(row),
            Err(e) => eprintln!("  ✗ a city failed: {e}"),
        }
    }
    rows.sort_by(|a, b| a.city.cmp(&b.city));
    if rows.is_empty() {
        eprintln!("no mosque answered");
        std::process::exit(1);
    }

    let now = Local::now();
    let now_min = now.hour() as i64 * 60 + now.minute() as i64;
    println!(
        "\n{:<12} {:<34} {} (local clock)",
        "city",
        "mosque",
        now.format("%H:%M")
    );
    println!("{}", "-".repeat(78));

    let mut board: Vec<(i64, String, String, String)> = Vec::new();
    for row in &rows {
        let next = next_prayer(&row.today, now_min);
        let line = match &next {
            Some((name, time, min)) => {
                format!("{name} {time} — in {min} min")
            }
            None => "no parsable times".to_string(),
        };
        println!("{:<12} {:<34} {}", row.city, row.name, line);
        if let Some((name, time, min)) = next {
            board.push((
                min,
                row.city.clone(),
                row.name.clone(),
                format!("{name} {time}"),
            ));
        }
    }

    board.sort();
    if let Some((min, city, name, what)) = board.first() {
        println!(
            "\nnext prayer on the board: {what} in {min} min — {name} ({city})"
        );
    }
}
