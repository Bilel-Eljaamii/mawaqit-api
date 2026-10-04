//! Shared test helpers, included by every tier binary as `mod common;`.
//!
//! Lives in `common/mod.rs` (not `common.rs`) so cargo never compiles it as
//! its own test target. Helpers unused by a given tier are expected, hence
//! the module-level `allow`.

#![allow(dead_code)]

use mawaqit_api::{ConfData, times_for_date};

/// A hostile-or-valid `ConfData` must stay internally digestible: whatever
/// the parser accepted, the calendar pipeline must hold together on it.
/// Shared by the corpus (ut) and the mutation fuzzer (fuzz).
pub fn dig(conf: &ConfData) {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    let _ = mawaqit_api::month_times(conf, 1);
    let _ = mawaqit_api::month_times(conf, 12);
    let _ = mawaqit_api::month_iqama_times(conf, 7);
    let _ = times_for_date(conf, date);
}

/// Strict display contract: exactly `HH:MM`, hours < 24, minutes < 60.
pub fn valid_hhmm(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5
        && b[2] == b':'
        && b[..2].iter().all(|c| c.is_ascii_digit())
        && b[3..].iter().all(|c| c.is_ascii_digit())
        && s[..2].parse::<u8>().is_ok_and(|h| h < 24)
        && s[3..].parse::<u8>().is_ok_and(|m| m < 60)
}

/// "HH:MM" → minutes since midnight; `None` for anything unparseable.
pub fn to_minutes(s: &str) -> Option<i64> {
    let h: i64 = s[..2].parse().ok()?;
    let m: i64 = s[3..].parse().ok()?;
    Some(h * 60 + m)
}

/// Unique temp dir for a tier's scratch files (`suite` labels the tier).
pub fn temp_dir(suite: &str, name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mawaqit-{suite}-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
