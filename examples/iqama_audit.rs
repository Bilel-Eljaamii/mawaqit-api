//! Iqama configuration audit for one month.
//!
//! Mawaqit mosques configure iqama two ways: an absolute "HH:MM" or a
//! relative "+N" minutes after the adhan. The client resolves both to
//! absolute times — this example goes one level deeper and cross-references
//! the *raw* calendar against the resolved one to answer the questions a
//! mosque admin would ask:
//!
//! - which prayers are relative vs absolute, per prayer of the day;
//! - the configured delay distribution (min / average / max minutes after the
//!   adhan);
//! - anomaly days: delays that exceed 45 minutes (or wrap negative).
//!
//! Usage:
//!   cargo run -p mawaqit-api --example iqama_audit -- [slug] [month]
//! Example:
//!   cargo run -p mawaqit-api --example iqama_audit -- grande-mosquee-de-paris
//! 10

use chrono::{Datelike, Local};
use mawaqit_api::{MawaqitClient, month_iqama_times, month_times};

const PRAYERS: [&str; 5] = ["Fajr", "Dhuhr", "Asr", "Maghrib", "Isha"];

fn hhmm_minutes(s: &str) -> Option<i64> {
    let (h, m) = s.split_once(':')?;
    Some(h.trim().parse::<i64>().ok()? * 60 + m.trim().parse::<i64>().ok()?)
}

/// Minutes from adhan to iqama on the same day (0..1440 wrap-safe).
fn delay_minutes(adhan: &str, iqama: &str) -> Option<i64> {
    Some((hhmm_minutes(iqama)? - hhmm_minutes(adhan)? + 1440) % 1440)
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let slug =
        args.next().unwrap_or_else(|| "grande-mosquee-de-paris".to_string());
    let month = args
        .next()
        .and_then(|m| m.parse::<u32>().ok())
        .unwrap_or_else(|| Local::now().month());

    let client = MawaqitClient::new();
    let conf = client.conf_data(&slug).await.unwrap_or_else(|e| {
        eprintln!("fetch failed: {e}");
        std::process::exit(1);
    });
    println!(
        "{} ({slug}) — month {month}",
        conf.name.as_deref().unwrap_or("?")
    );

    let Some(iqama_calendar) = &conf.iqama_calendar else {
        println!("this mosque publishes no iqama calendar — nothing to audit");
        return;
    };
    let Some(raw_month) = iqama_calendar.get((month - 1) as usize) else {
        println!("no raw iqama rows for month {month}");
        return;
    };

    let adhan = month_times(&conf, month).unwrap_or_else(|e| {
        eprintln!("adhan calendar unusable: {e}");
        std::process::exit(1);
    });
    let resolved = month_iqama_times(&conf, month).unwrap_or_else(|e| {
        eprintln!("iqama resolution failed: {e}");
        std::process::exit(1);
    });

    // Per prayer: bucket every day by how its iqama is configured.
    let mut stats = [[0i64; 3]; 5]; // [count, relative, absolute]
    let mut delays: [Vec<i64>; 5] = Default::default();
    let mut anomalies: Vec<(u32, &str, i64)> = Vec::new();

    for day in &resolved.days {
        let Some(raw) = raw_month.get(&day.day.to_string()) else { continue };
        let adhan_day = adhan
            .days
            .iter()
            .find(|d| d.day == day.day)
            .map(|d| &d.times)
            .expect("resolved day implies adhan day");
        let resolved_times = [
            (&day.times.fajr, &adhan_day.fajr),
            (&day.times.dhuhr, &adhan_day.dhuhr),
            (&day.times.asr, &adhan_day.asr),
            (&day.times.maghrib, &adhan_day.maghrib),
            (&day.times.isha, &adhan_day.isha),
        ];
        for (i, (iqama_t, adhan_t)) in resolved_times.iter().enumerate() {
            let Some(delay) = delay_minutes(adhan_t, iqama_t) else {
                continue;
            };
            stats[i][0] += 1;
            match raw.get(i).map(String::as_str) {
                Some(v) if v.trim().starts_with('+') => stats[i][1] += 1,
                Some(_) => stats[i][2] += 1,
                None => continue,
            }
            delays[i].push(delay);
            if delay > 45 {
                anomalies.push((day.day, PRAYERS[i], delay));
            }
        }
    }

    println!(
        "\n{:<8} {:>5} {:>9} {:>9} {:>7} {:>7} {:>7}",
        "prayer", "days", "relative", "absolute", "min", "avg", "max"
    );
    for (i, name) in PRAYERS.iter().enumerate() {
        let ds = &delays[i];
        let (min, avg, max) = if ds.is_empty() {
            ("-".to_string(), "-".to_string(), "-".to_string())
        } else {
            (
                ds.iter().min().unwrap_or(&0).to_string(),
                format!(
                    "{:.1}",
                    ds.iter().sum::<i64>() as f64 / ds.len() as f64
                ),
                ds.iter().max().unwrap_or(&0).to_string(),
            )
        };
        println!(
            "{:<8} {:>5} {:>9} {:>9} {:>7} {:>7} {:>7}",
            name, stats[i][0], stats[i][1], stats[i][2], min, avg, max
        );
    }

    if anomalies.is_empty() {
        println!("\nno anomalies (all delays within 45 minutes)");
    } else {
        println!("\nanomalies (delay > 45 min):");
        for (day, prayer, delay) in anomalies.iter().take(10) {
            println!("  day {day}: {prayer} fires {delay} min after adhan");
        }
    }
}
