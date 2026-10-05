//! Build a 7-day prayer-alarm plan from a single page fetch.
//!
//! The whole week lives in one confData document, so the client fetches
//! once and the rest is pure local computation: [`times_for_date`] walks
//! the embedded year calendar for any date. Each alarm fires `lead` minutes
//! before the iqama (apps usually wake users before the iqama, not the
//! adhan) or before the adhan when the mosque publishes no iqama.
//!
//! The review's C1 finding in practice: alarms must fire at the iqama
//! **instant** (`TodayTimes.iqama_at`), not at the display string. A
//! "+600" offset after a 23:30 adhan belongs to the *next* day — a plan
//! built from the rolled "HH:MM" alone would fire 24 hours early. Days
//! where the fire time crosses midnight are marked in the output.
//!
//! Usage:
//!   cargo run -p mawaqit-api --example week_alarm_plan -- [slug] [days]
//! [lead-min] Example:
//!   cargo run -p mawaqit-api --example week_alarm_plan --
//! grande-mosquee-de-paris 7 10

use chrono::{Duration, Local, NaiveTime};
use mawaqit_api::{MawaqitClient, times_for_date};

const PRAYERS: [&str; 5] = ["Fajr", "Dhuhr", "Asr", "Maghrib", "Isha"];

fn parse_hhmm(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let slug =
        args.next().unwrap_or_else(|| "grande-mosquee-de-paris".to_string());
    let days: u32 =
        args.next().and_then(|d| d.parse().ok()).unwrap_or(7).clamp(1, 14);
    let lead: i64 = args.next().and_then(|l| l.parse().ok()).unwrap_or(10);

    let client = MawaqitClient::new();
    // ONE fetch serves the whole plan: the year calendar is embedded in the
    // page's confData.
    let conf = client.conf_data(&slug).await.unwrap_or_else(|e| {
        eprintln!("fetch failed: {e}");
        std::process::exit(1);
    });
    println!(
        "alarm plan for {} — {days} day(s), fire {lead} min early\n",
        conf.name.as_deref().unwrap_or(&slug)
    );

    let today = Local::now().date_naive();
    let mut planned = 0usize;
    let mut adhan_fallback = 0usize;
    let mut skipped = Vec::new();

    for offset in 0..days {
        let date = today + Duration::days(offset as i64);
        let Ok(day) = times_for_date(&conf, date) else {
            // A dropped (hostile) day errors as InvalidDay, an absent day
            // as NoCalendar — either way the plan just skips it.
            skipped.push(date.to_string());
            continue;
        };

        let adhan = [
            ("Fajr", day.adhan.fajr.as_str()),
            ("Dhuhr", day.adhan.dhuhr.as_str()),
            ("Asr", day.adhan.asr.as_str()),
            ("Maghrib", day.adhan.maghrib.as_str()),
            ("Isha", day.adhan.isha.as_str()),
        ];
        let iqama_instants = day.iqama_at.as_ref();
        let iqama_display = day.iqama.as_ref();
        for (i, (name, adhan_time)) in adhan.iter().enumerate() {
            let Some(base) = parse_hhmm(adhan_time) else { continue };
            let (basis, basis_time, fire) = match iqama_instants.map(|at| {
                match i {
                    0 => at.fajr,
                    1 => at.dhuhr,
                    2 => at.asr,
                    3 => at.maghrib,
                    _ => at.isha,
                }
            }) {
                Some(at) => {
                    let display = iqama_display
                        .map(|iq| match i {
                            0 => iq.fajr.as_str(),
                            1 => iq.dhuhr.as_str(),
                            2 => iq.asr.as_str(),
                            3 => iq.maghrib.as_str(),
                            _ => iq.isha.as_str(),
                        })
                        .unwrap_or_default();
                    ("iqama", display.to_string(), at - Duration::minutes(lead))
                }
                None => {
                    adhan_fallback += 1;
                    (
                        "adhan",
                        (*adhan_time).to_string(),
                        date.and_time(base) - Duration::minutes(lead),
                    )
                }
            };
            planned += 1;
            // The fire instant may land on the next day (rollover) — say
            // so instead of silently planning the alarm a day early.
            let next_day = if fire.date() != date {
                format!(" → {}", fire.format("%Y-%m-%d"))
            } else {
                String::new()
            };
            println!(
                "  {}  {:<7} fire {}{next_day} ({basis} {basis_time})",
                date.format("%a %Y-%m-%d"),
                name,
                fire.format("%H:%M"),
            );
        }
        println!();
    }

    println!("planned {planned} alarm(s) across {days} day(s): {PRAYERS:?}");
    if adhan_fallback > 0 {
        println!("{adhan_fallback} alarm(s) fell back to the adhan (no iqama)");
    }
    if !skipped.is_empty() {
        println!("days the calendar could not cover: {}", skipped.join(", "));
    }
}
