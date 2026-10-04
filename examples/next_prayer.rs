//! Countdown to the next prayer of the day: "Maghrib adhan in 23 min".
//!
//! Walks the client's full happy path — keyword search → mosque slug →
//! today's adhan + iqama — and does the "HH:MM" arithmetic apps need:
//! minutes-from-now per prayer (day order, so a past prayer never wins) and
//! the adhan→iqama delay via [`minutes_between`].
//!
//! Prayer times are local to the mosque; the countdown uses this machine's
//! clock, so run it in the mosque's timezone or accept the skew.
//!
//! Usage:
//!   cargo run -p mawaqit-api --example next_prayer -- [search word] [index]
//! Example:
//!   cargo run -p mawaqit-api --example next_prayer -- "Grande Mosquée" 0

use chrono::Local;
use mawaqit_api::{
    DailyIqamaTimes, DailyPrayerTimes, MawaqitClient, minutes_between,
};

fn adhan_rows(t: &DailyPrayerTimes) -> [(&'static str, &str); 5] {
    [
        ("Fajr", t.fajr.as_str()),
        ("Dhuhr", t.dhuhr.as_str()),
        ("Asr", t.asr.as_str()),
        ("Maghrib", t.maghrib.as_str()),
        ("Isha", t.isha.as_str()),
    ]
}

fn iqama_for<'a>(
    iq: Option<&'a DailyIqamaTimes>,
    name: &str,
) -> Option<&'a str> {
    let iq = iq?;
    Some(match name {
        "Fajr" => iq.fajr.as_str(),
        "Dhuhr" => iq.dhuhr.as_str(),
        "Asr" => iq.asr.as_str(),
        "Maghrib" => iq.maghrib.as_str(),
        _ => iq.isha.as_str(),
    })
}

/// "HH:MM" → minutes since midnight; `None` for hostile values.
fn hhmm_minutes(s: &str) -> Option<i64> {
    let (h, m) = s.split_once(':')?;
    let h = h.trim().parse::<i64>().ok()?;
    let m = m.trim().parse::<i64>().ok()?;
    Some(h * 60 + m)
}

/// One schedule row: prayer name, adhan, iqama, and when it fires relative
/// to `now` in minutes (`None` = already passed today).
struct Row {
    name: &'static str,
    adhan: String,
    iqama: Option<String>,
    in_minutes: Option<i64>,
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let word = args.next().unwrap_or_else(|| "Paris".to_string());
    let pick: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);

    let client = MawaqitClient::new();

    let mosques = match client.search_mosques(&word).await {
        Ok(mosques) if !mosques.is_empty() => mosques,
        Ok(_) => {
            eprintln!("no mosque matches {word:?}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("search failed: {e}");
            std::process::exit(1);
        }
    };

    println!("{word:?} matched {} mosque(s):", mosques.len());
    for (i, mosque) in mosques.iter().take(5).enumerate() {
        let place = mosque.place().unwrap_or_else(|| "?".to_string());
        println!("  [{i}] {} — {place}", mosque.display_name());
    }

    let mosque = &mosques[pick.min(mosques.len() - 1)];
    let Some(slug) = mosque.mosque_id() else {
        eprintln!("result {pick} carries no page slug — pick another index");
        std::process::exit(1);
    };
    println!("\n→ {} ({slug})", mosque.display_name());

    let today = match client.today(slug).await {
        Ok(today) => today,
        Err(e) => {
            eprintln!("fetch failed: {e}");
            std::process::exit(1);
        }
    };

    let now = Local::now().format("%H:%M").to_string();
    let now_min = hhmm_minutes(&now).unwrap_or(0);

    let mut rows: Vec<Row> = Vec::new();
    for (name, adhan) in adhan_rows(&today.adhan) {
        let iqama = iqama_for(today.iqama.as_ref(), name).map(str::to_string);
        let adhan_min = hhmm_minutes(adhan);
        // Day order resolves "next": a prayer at or before `now` already
        // rang today; everything after it is still ahead (even past noon).
        let in_minutes = adhan_min.map(|min| {
            if min > now_min { min - now_min } else { 1440 + min - now_min }
        });
        rows.push(Row { name, adhan: adhan.to_string(), iqama, in_minutes });
    }

    let next_idx = rows.iter().position(|r| r.in_minutes.is_some());
    println!("\nschedule for {} (viewer clock {now}):", today.date);
    for (i, row) in rows.iter().enumerate() {
        let delay = match (&row.iqama, row.adhan.as_str()) {
            (Some(iq), adhan) => minutes_between(adhan, iq)
                .map(|d| format!(" (iqama {iq}, +{d} min)"))
                .unwrap_or_else(|| format!(" (iqama {iq})")),
            (None, _) => String::new(),
        };
        let countdown = match row.in_minutes {
            None => "passed".to_string(),
            Some(min) => format!("in {min} min"),
        };
        let marker = if Some(i) == next_idx { "  ← next" } else { "" };
        println!(
            "  {:<7} adhan {}{delay}  {countdown}{marker}",
            row.name, row.adhan
        );
    }

    if today.iqama.is_none() {
        println!("\n(this mosque publishes no iqama calendar)");
    }
}
