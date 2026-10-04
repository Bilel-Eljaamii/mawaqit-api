//! The HTML → `ConfData` pipeline, entirely offline.
//!
//! [`parse_page`] is the pure core of the client: it finds the `confData`
//! JSON literal inside a mosque page and collapses it into the tolerant
//! `ConfData` model. Because it takes any `&str`, you can run the whole
//! parse → resolve → display pipeline with **zero network** — here we
//! synthesize two realistic pages (a normal one and a Diyanet "imsak mode"
//! one), plus hostile pages, and walk them through the library.
//!
//! This is also the pattern for testing your own app without mawaqit.net:
//! generate fixture pages, feed `parse_page`.
//!
//! Usage:
//!   cargo run -p mawaqit-api --example page_scraper -- [YYYY-MM-DD]

use chrono::{Duration, Local, NaiveDate};
use mawaqit_api::parse_page;
use serde_json::{Value, json};

const NORMAL: &str = "grande-mosquee-de-paris";
const IMSAK: &str = "ditib-mosque-example";

/// Synthetic but wire-shaped: 12 months, each day a full calendar row.
fn year_calendar(row: Value) -> Value {
    let mut months = Vec::new();
    for month in 1..=12u32 {
        let mut map = serde_json::Map::new();
        for day in 1..=days_in_month(2026, month) {
            map.insert(day.to_string(), row.clone());
        }
        months.push(Value::Object(map));
    }
    Value::Array(months)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (y2, m2) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    let a = NaiveDate::from_ymd_opt(year, month, 1).expect("valid date");
    let b = NaiveDate::from_ymd_opt(y2, m2, 1).expect("valid date");
    (b - a).num_days() as u32
}

/// Wrap a confData object into a minimal mosque page, exactly the shape
/// mawaqit.net ships: one script assigning `var confData = {...}`.
fn to_page(conf: &Value) -> String {
    format!(
        "<!doctype html><html><head><title>mosque</title>\
         <script>var settings = {{}}</script></head><body>\
         <div id=\"app\"></div>\
         <script>var confData = {conf};</script>\
         </body></html>"
    )
}

fn normal_conf() -> Value {
    json!({
        "times": ["06:09","07:41","13:47","16:58","19:45"],
        "shuruq": "07:41",
        "calendar": year_calendar(
            json!(["07:05","08:44","12:59","14:48","17:08","18:35"])
        ),
        "iqamaCalendar": year_calendar(
            json!(["07:15","+8","+8","+0","+8"])
        ),
        "name": "GRANDE MOSQUÉE DE PARIS",
        "jumua": "13:50",
        "jumua2": "14:30",
        "announcements": [
            {"id": 1, "title": "Portes ouvertes",
             "start_date": "2026-10-01", "end_date": "2026-10-31"}
        ],
        "calcMethod": "UOIF 12°"
    })
}

/// Diyanet layout: 7-column rows `[İmsak, Sabah, Shurûq, Dhuhr, …]` and a
/// 6-entry `times` (which is what flips `imsak_mode` on).
fn imsak_conf() -> Value {
    json!({
        "times": ["05:27","06:07","07:07","13:21","16:37","19:24"],
        "calendar": year_calendar(
            json!(["05:27","06:37","07:07","13:21","16:37","19:24","20:51"])
        ),
        "iqamaCalendar": year_calendar(
            json!(["+15","+15","+15","+10","+15"])
        ),
        "name": "DİTİB Merkez Camii",
        "jumua": "13:30"
    })
}

fn show(slug: &str, conf_value: &Value, date: NaiveDate) {
    let page = to_page(conf_value);
    let conf = parse_page(&page, slug)
        .unwrap_or_else(|e| panic!("{slug} fixture must parse: {e}"));
    println!("── {slug}");
    println!(
        "  name {} | imsak_mode {} | jumua {:?}/{:?}",
        conf.name.as_deref().unwrap_or("?"),
        conf.imsak_mode,
        conf.jumua.as_deref(),
        conf.jumua2.as_deref()
    );
    println!(
        "  announcements: {} | unmodeled calcMethod: {:?}",
        conf.announcements.len(),
        conf.raw.get("calcMethod")
    );

    match mawaqit_api::times_for_date(&conf, date) {
        Ok(day) => {
            let iq = day.iqama.as_ref();
            println!(
                "  {date}: adhan {} … isha {}",
                day.adhan.fajr, day.adhan.isha
            );
            println!(
                "  iqama: {}",
                iq.map(|i| format!(
                    "fajr {} → isha {} (all resolved to absolute)",
                    i.fajr, i.isha
                ))
                .unwrap_or_else(|| "none".to_string())
            );
        }
        Err(e) => println!("  {date}: {e}"),
    }
    println!();
}

fn main() {
    let date = std::env::args()
        .nth(1)
        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
        .unwrap_or_else(|| Local::now().date_naive());
    println!("page_scraper — offline demo for {date}\n");

    show(NORMAL, &normal_conf(), date);
    show(IMSAK, &imsak_conf(), date);

    // Hostile pages must error, not panic — the red-team contract.
    println!("── hostile pages");
    for (label, page) in [
        ("no confData at all", "<html><p>hello</p></html>".to_string()),
        (
            "unterminated JSON literal",
            "<script>var confData = {\"times\": [\"06:09\",".to_string(),
        ),
        (
            "confData mention, no assignment",
            "<script>if (confData === undefined) reload();</script>"
                .to_string(),
        ),
    ] {
        match parse_page(&page, "hostile") {
            Err(e) => println!("  {label}: {e}"),
            Ok(_) => println!("  {label}: UNEXPECTED success"),
        }
    }

    // Bonus: a future date any number of days out still resolves — the year
    // calendar covers everything.
    let far = date + Duration::days(200);
    let conf = parse_page(&to_page(&normal_conf()), NORMAL).expect("parse");
    match mawaqit_api::times_for_date(&conf, far) {
        Ok(day) => println!(
            "\n{far} ({} days out): maghrib {}",
            (far - date).num_days(),
            day.adhan.maghrib
        ),
        Err(e) => println!("\n{far}: {e}"),
    }
}
