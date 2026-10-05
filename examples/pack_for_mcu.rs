//! The pre-flash packer (ADR-0013): turns one mosque's confData into an
//! MQTC v1 payload a bare-metal firmware maps from flash — as a `.bin`
//! image, a Rust static array, or a C header.
//!
//! Run it on a workstation or build server (the `std` side of the DDD
//! split); the MCU only ever sees the finished bytes.
//!
//! Usage:
//! ```text
//! cargo run --example pack_for_mcu -- \
//!   --slug grande-mosquee-de-paris \
//!   --scope year --compress none --format rust \
//!   --out mcu_firmware/src/prayer_data.rs
//!
//! pack_for_mcu --file page.html --scope months --months 3 \
//!   --compress delta --format bin --clamp --out prayer_data.bin
//! ```
//!
//! Flags: `--slug <s>|--file <p>`, `--scope <week|months|year>`,
//! `--months <n>` (with `--scope months`, default 3), `--start <YYYY-MM-DD>`
//! (default today), `--compress <none|delta>`, `--format <rust|bin|c>`,
//! `--clamp`, `--out <path>` (default stdout).

use std::path::PathBuf;

use chrono::{Datelike, Days, Local, NaiveDate};
use mawaqit_api::{
    ConfData, MawaqitClient,
    compact::{
        CompactCalendarBuilder, CompactCalendarView, CompactDayInput,
        CompactIqamaInput, ScopeType,
    },
    month_iqama_times, month_times, parse_page,
};

enum Format {
    Bin,
    Rust,
    C,
}

struct Args {
    slug: Option<String>,
    file: Option<PathBuf>,
    scope: ScopeKind,
    months: u32,
    start: Option<NaiveDate>,
    fajr_relative: bool,
    format: Format,
    clamp: bool,
    out: Option<PathBuf>,
}

enum ScopeKind {
    Week,
    Months,
    Year,
}

fn usage() -> String {
    "--slug <slug> | --file <path>   the mosque to package
 --scope <week|months|year>     date scope (default year)
 --months <n>                   month count with --scope months (default 3)
 --start <YYYY-MM-DD>           scope start (default today)
 --compress <none|delta>        direct flash vs fajr-relative records
 --format <rust|bin|c>          output shape (default rust)
 --clamp                        truncate at Dec 31 instead of failing
 --out <path>                   output file (default stdout)"
        .into()
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        slug: None,
        file: None,
        scope: ScopeKind::Year,
        months: 3,
        start: None,
        fajr_relative: false,
        format: Format::Rust,
        clamp: false,
        out: None,
    };
    let mut argv = std::env::args().skip(1);
    while let Some(flag) = argv.next() {
        let mut value = |what: &str| {
            argv.next().ok_or_else(|| format!("{what} needs a value"))
        };
        match flag.as_str() {
            "--help" | "-h" => {
                eprintln!(
                    "pack_for_mcu — MQTC v1 pre-flash packer\n\n{}",
                    usage()
                );
                std::process::exit(0);
            }
            "--slug" => args.slug = Some(value("--slug")?),
            "--file" => args.file = Some(PathBuf::from(value("--file")?)),
            "--scope" => match value("--scope")?.as_str() {
                "week" => args.scope = ScopeKind::Week,
                "months" => args.scope = ScopeKind::Months,
                "year" => args.scope = ScopeKind::Year,
                other => {
                    return Err(format!(
                        "unknown scope {other:?} — use week|months|year \
                         (a custom end date is not supported yet)"
                    ));
                }
            },
            "--months" => {
                args.months = value("--months")?
                    .parse()
                    .map_err(|_| "--months wants a number".to_string())?;
            }
            "--start" => {
                let raw = value("--start")?;
                args.start = Some(
                    NaiveDate::parse_from_str(&raw, "%Y-%m-%d")
                        .map_err(|e| format!("--start {raw:?}: {e}"))?,
                );
            }
            "--compress" => match value("--compress")?.as_str() {
                "none" => args.fajr_relative = false,
                "delta" => args.fajr_relative = true,
                other => {
                    return Err(format!(
                        "unknown compress {other:?} — use none|delta"
                    ));
                }
            },
            "--format" => match value("--format")?.as_str() {
                "rust" => args.format = Format::Rust,
                "bin" => args.format = Format::Bin,
                "c" => args.format = Format::C,
                other => {
                    return Err(format!(
                        "unknown format {other:?} — use rust|bin|c"
                    ));
                }
            },
            "--clamp" => args.clamp = true,
            "--out" => args.out = Some(PathBuf::from(value("--out")?)),
            other => {
                return Err(format!("unknown flag {other:?}\n\n{}", usage()));
            }
        }
    }
    if args.slug.is_some() == args.file.is_some() {
        return Err("pass exactly one of --slug or --file".into());
    }
    Ok(args)
}

/// Strict `HH:MM` → minutes from midnight.
fn hhmm_minutes(s: &str) -> Option<u16> {
    let (h, m) = s.trim().split_once(':')?;
    let h: u16 = h.parse().ok()?;
    let m: u16 = m.parse().ok()?;
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// The rollover rule: the library resolves `+N` offsets into the
/// wall-clock `HH:MM` of the instant's day, so an iqama that lands before
/// its adhan belongs to the *next* calendar day (C1).
fn iqama_entry(
    iqama: Option<&str>,
    adhan_min: u16,
) -> Result<Option<CompactIqamaInput>, String> {
    match iqama.and_then(hhmm_minutes) {
        None => Ok(None),
        Some(mins) => Ok(Some(CompactIqamaInput {
            minutes: mins,
            rollover: mins < adhan_min,
        })),
    }
}

async fn load_conf(
    args: &Args,
) -> Result<ConfData, Box<dyn std::error::Error>> {
    if let Some(file) = &args.file {
        let html = std::fs::read_to_string(file)?;
        let slug = args.slug.clone().unwrap_or_else(|| "file".into());
        return Ok(parse_page(&html, &slug)?);
    }
    let slug = args.slug.as_deref().expect("checked in parse_args");
    let client = MawaqitClient::new();
    Ok(client.conf_data(slug).await?.as_ref().clone())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    let conf = load_conf(&args).await?;

    let start = args.start.unwrap_or_else(|| Local::now().date_naive());
    let end = match args.scope {
        ScopeKind::Week => start + Days::new(6),
        ScopeKind::Months => {
            // Through the end of the (n-1)-th month after the start month.
            let target_month0 = start.month0() as i32 + args.months as i32 - 1;
            let year = start.year() + target_month0 / 12;
            let month = target_month0 % 12 + 1;
            let next_year = if month == 12 { year + 1 } else { year };
            let next_month = if month == 12 { 1 } else { month + 1 };
            let next_first =
                NaiveDate::from_ymd_opt(next_year, next_month as u32, 1)
                    .ok_or("month arithmetic out of range")?;
            next_first - Days::new(1)
        }
        ScopeKind::Year => NaiveDate::from_ymd_opt(start.year(), 12, 31)
            .ok_or("year-end out of range")?,
    };

    // Mawaqit publishes the current calendar year only — the spec's
    // fail-fast / clamp contract.
    let year_end = NaiveDate::from_ymd_opt(start.year(), 12, 31)
        .expect("Dec 31 of a valid year");
    let end = if end > year_end {
        if !args.clamp {
            return Err(format!(
                "Error: requested scope extends past {year_end}. Upstream \
                 mawaqit only publishes data through the current calendar \
                 year. Use --clamp to package up to Dec 31."
            )
            .into());
        }
        eprintln!("warning: scope clamped to {year_end}");
        year_end
    } else {
        end
    };

    let scope_type = match args.scope {
        ScopeKind::Week => ScopeType::Week,
        ScopeKind::Months => ScopeType::Months,
        ScopeKind::Year => ScopeType::Year,
    };
    let mut builder =
        CompactCalendarBuilder::new(start, scope_type, args.fajr_relative)
            .with_imsak_mode(conf.imsak_mode)
            .with_jumuah(
                conf.jumua.as_deref().and_then(hhmm_minutes),
                conf.jumua2.as_deref().and_then(hhmm_minutes),
            );

    // Per-month resolution through the tested calendar pipeline (F4/F5
    // semantics included): malformed days surface no times and must abort
    // the pack — a consecutive-range format cannot carry gaps.
    let mut date = start;
    while date <= end {
        let month = month_times(&conf, date.month())?;
        let iqama_month = month_iqama_times(&conf, date.month()).ok();
        let Some(day) = month.days.iter().find(|d| d.day == date.day()) else {
            return Err(format!(
                "{date} is missing from the mosque's calendar (dropped as \
                 malformed or never published) — a consecutive MQTC range \
                 cannot carry gaps; narrow the scope"
            )
            .into());
        };
        let iqama_day = iqama_month
            .as_ref()
            .and_then(|m| m.days.iter().find(|d| d.day == date.day()));
        let iq = iqama_day.map(|d| &d.times);
        let adhan = [
            hhmm_minutes(&day.times.fajr).ok_or("fajr not HH:MM")?,
            hhmm_minutes(&day.times.shurouq).ok_or("shurouq not HH:MM")?,
            hhmm_minutes(&day.times.dhuhr).ok_or("dhuhr not HH:MM")?,
            hhmm_minutes(&day.times.asr).ok_or("asr not HH:MM")?,
            hhmm_minutes(&day.times.maghrib).ok_or("maghrib not HH:MM")?,
            hhmm_minutes(&day.times.isha).ok_or("isha not HH:MM")?,
        ];
        builder.push_day(CompactDayInput {
            adhan,
            iqama: [
                iqama_entry(iq.map(|t| t.fajr.as_str()), adhan[0])?,
                iqama_entry(iq.map(|t| t.dhuhr.as_str()), adhan[2])?,
                iqama_entry(iq.map(|t| t.asr.as_str()), adhan[3])?,
                iqama_entry(iq.map(|t| t.maghrib.as_str()), adhan[4])?,
                iqama_entry(iq.map(|t| t.isha.as_str()), adhan[5])?,
            ],
            day_flags: 0,
        });
        date = date + Days::new(1);
    }

    // The packer refuses to emit a payload it cannot load back — the CRC
    // and layout are proven before anything is written.
    let bytes = builder.to_bytes()?;
    CompactCalendarView::from_bytes(&bytes)?;

    let payload = match args.format {
        Format::Bin => bytes.clone(),
        Format::Rust => builder.to_rust_code("PRAYER_DATA")?.into_bytes(),
        Format::C => builder.to_c_header("PRAYER_DATA")?.into_bytes(),
    };

    match &args.out {
        Some(path) => {
            // Atomic-ish write: temp file + rename, like the snapshot layer.
            let tmp = path.with_extension("tmp");
            std::fs::write(&tmp, &payload)?;
            std::fs::rename(&tmp, path)?;
            eprintln!(
                "wrote {} ({} days, {} bytes, {}) → {}",
                path.display(),
                builder.len(),
                bytes.len(),
                if args.fajr_relative { "fajr-relative" } else { "direct" },
                path.display(),
            );
        }
        None => {
            use std::io::Write;
            std::io::stdout().write_all(&payload)?;
            eprintln!(
                "// {} days, {} bytes, {} — written to stdout",
                builder.len(),
                bytes.len(),
                if args.fajr_relative { "fajr-relative" } else { "direct" },
            );
        }
    }
    Ok(())
}
