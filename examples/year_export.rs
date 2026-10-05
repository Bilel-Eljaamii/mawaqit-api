//! Export a mosque's whole-year calendar — adhan + resolved iqama for all
//! 12 months — to a JSON or CSV file.
//!
//! The teaching point: **one page fetch carries the whole year**. We call
//! `conf_data` once and expand every month locally with the pure functions
//! [`month_times`] and [`month_iqama_times`] — no per-month network round
//! trips. Months the mosque does not publish are skipped with a warning
//! instead of failing the export.
//!
//! Usage:
//!   cargo run -p mawaqit-api --example year_export -- <slug> [json|csv]
//! [out-dir] [year] Example:
//!   cargo run -p mawaqit-api --example year_export -- grande-mosquee-de-paris
//!
//! confData carries no year (review L2): the export names and stamps the
//! year you pass (default: the current one) so December and next-December
//! exports cannot be confused.

use std::path::PathBuf;

use chrono::{Datelike, Local};
use mawaqit_api::{
    ConfData, MawaqitClient, MonthIqamaTimes, MonthTimes, month_iqama_times,
    month_times,
};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let slug =
        args.next().unwrap_or_else(|| "grande-mosquee-de-paris".to_string());
    let format =
        args.next().unwrap_or_else(|| "json".to_string()).to_lowercase();
    let out_dir = PathBuf::from(
        args.next().unwrap_or_else(|| "times-export".to_string()),
    );
    let calendar_year: i32 = args
        .next()
        .and_then(|y| y.parse().ok())
        .unwrap_or_else(|| Local::now().year());

    if format != "json" && format != "csv" {
        eprintln!("format must be `json` or `csv`, got {format:?}");
        std::process::exit(2);
    }

    let client = MawaqitClient::new();
    let conf = client.conf_data(&slug).await.unwrap_or_else(|e| {
        eprintln!("fetch failed: {e}");
        std::process::exit(1);
    });

    let year: Vec<(MonthTimes, Option<MonthIqamaTimes>)> = (1..=12)
        .filter_map(|month| match month_times(&conf, month) {
            Ok(adhan) => {
                let iqama = month_iqama_times(&conf, month).ok();
                Some((adhan, iqama))
            }
            Err(e) => {
                eprintln!("month {month}: skipped ({e})");
                None
            }
        })
        .collect();

    let days: usize = year.iter().map(|(m, _)| m.days.len()).sum();
    println!(
        "{}: {} month(s), {} day(s), imsak mode: {}",
        conf.name.as_deref().unwrap_or(&slug),
        year.len(),
        days,
        conf.imsak_mode
    );

    std::fs::create_dir_all(&out_dir).expect("create output dir");
    let path = out_dir.join(format!("{slug}-{calendar_year}.{format}"));
    let written = match format.as_str() {
        "json" => write_json(&path, &slug, calendar_year, &conf, &year),
        _ => {
            write_csv(&path, &year).map_err(Box::<dyn std::error::Error>::from)
        }
    };
    match written {
        Ok(()) => println!("wrote {}", path.display()),
        Err(e) => {
            eprintln!("export failed: {e}");
            std::process::exit(1);
        }
    }
}

type Year = Vec<(MonthTimes, Option<MonthIqamaTimes>)>;

fn write_json(
    path: &PathBuf,
    slug: &str,
    calendar_year: i32,
    conf: &ConfData,
    year: &Year,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut months = serde_json::Map::new();
    for (adhan, iqama) in year {
        let mut days = serde_json::Map::new();
        for day in &adhan.days {
            let iq = iqama
                .as_ref()
                .and_then(|m| m.days.iter().find(|d| d.day == day.day));
            days.insert(
                day.day.to_string(),
                serde_json::json!({
                    "adhan": day.times,
                    "iqama": iq.map(|d| d.times.clone()),
                }),
            );
        }
        months.insert(adhan.month.to_string(), days.into());
    }
    let doc = serde_json::json!({
        "slug": slug,
        "mosque": conf.name,
        "imsak_mode": conf.imsak_mode,
        "year": calendar_year,
        "exported_at": Local::now().date_naive(),
        "jumua": [conf.jumua, conf.jumua2],
        "months": months,
    });
    std::fs::write(path, serde_json::to_string_pretty(&doc)?)?;
    Ok(())
}

fn write_csv(path: &PathBuf, year: &Year) -> Result<(), std::io::Error> {
    let mut out = String::from(
        "month,day,fajr,shurouq,dhuhr,asr,maghrib,isha,\
         iqama_fajr,iqama_dhuhr,iqama_asr,iqama_maghrib,iqama_isha\n",
    );
    for (adhan, iqama) in year {
        for day in &adhan.days {
            let iq = iqama
                .as_ref()
                .and_then(|m| m.days.iter().find(|d| d.day == day.day));
            let iq_cols = match iq {
                Some(d) => format!(
                    "{},{},{},{},{}",
                    d.times.fajr,
                    d.times.dhuhr,
                    d.times.asr,
                    d.times.maghrib,
                    d.times.isha
                ),
                None => ",,,,".to_string(),
            };
            let t = &day.times;
            out.push_str(&format!(
                "{},{},{},{},{},{},{},{},{iq_cols}\n",
                adhan.month,
                day.day,
                t.fajr,
                t.shurouq,
                t.dhuhr,
                t.asr,
                t.maghrib,
                t.isha,
            ));
        }
    }
    std::fs::write(path, out)
}
