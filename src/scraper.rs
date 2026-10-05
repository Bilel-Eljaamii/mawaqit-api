#[cfg(not(feature = "std"))]
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

use serde_json::Value;

use crate::{
    error::{self, BOUNDED_DIAGNOSTIC, BOUNDED_ID, MawaqitError, Result},
    models::{Announcement, ConfData, RawCalendar},
    sanitize,
};

/// The wire-tolerant Option extractor for display strings. Sanitization is
/// deliberately *not* done here: the assembled [`ConfData`] goes through
/// the shared sanitizer once at the end of extraction, the same pass the
/// disk snapshot load applies (FINDING F23 — one character policy at every
/// ingress).
fn string_field(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().map(str::to_string)
}

/// Extract the `confData` JavaScript object embedded in a mosque page.
///
/// The public page (`https://mawaqit.net/{lang}/{slug}`) ships its whole
/// configuration — daily times, year calendar, iqama calendar, mosque
/// metadata — as one JSON literal assigned to a `confData` variable.
pub fn extract_conf_data(page_html: &str, mosque_id: &str) -> Result<ConfData> {
    let json_str = find_conf_data_json(page_html).ok_or_else(|| {
        MawaqitError::ConfDataNotFound(error::bounded(mosque_id, BOUNDED_ID))
    })?;

    let value: Value = serde_json::from_str(json_str).map_err(|e| {
        MawaqitError::Parse(error::bounded(
            &format!("confData: {e}"),
            BOUNDED_DIAGNOSTIC,
        ))
    })?;

    let times: Vec<String> = value["times"]
        .as_array()
        .map(|a| {
            a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
        })
        .unwrap_or_default();
    if times.len() < 5 {
        return Err(MawaqitError::Parse(format!(
            "confData.times has {} entries, expected 5",
            times.len()
        )));
    }

    let calendar: RawCalendar =
        serde_json::from_value(value["calendar"].clone()).unwrap_or_default();
    if calendar.is_empty() {
        return Err(MawaqitError::NoCalendar);
    }
    let iqama_calendar = parse_calendar(&value, "iqamaCalendar").ok();

    // Announcements are collected element-wise: entries that fail to
    // deserialize are dropped, not fatal (ADR-0003). Their text is
    // sanitized by the shared pass below, together with every other
    // free-text field (F6/F21/F23).
    let announcements: Vec<Announcement> = value["announcements"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default();

    let mut conf = ConfData {
        name: string_field(&value, "name"),
        jumua: string_field(&value, "jumua"),
        jumua2: string_field(&value, "jumua2"),
        image: string_field(&value, "image"),
        shuruq: string_field(&value, "shuruq"),
        imsak_mode: times.len() == 6,
        times,
        calendar,
        iqama_calendar,
        announcements,
        raw: value,
    };
    // The page boundary sanitizes exactly what the disk snapshot load
    // sanitizes — one pass, one character policy (F22/F23).
    sanitize::confdata(&mut conf);
    Ok(conf)
}

/// Find the JSON literal assigned to `confData` in any script of the page,
/// scanning from the opening `{` to its balanced closing brace (string- and
/// escape-aware, so `;` or braces inside JSON strings don't break it).
///
/// FINDING F26: a failed candidate has consumed to EOF — its object never
/// closes. Every later mention lives *inside* that broken object (not an
/// assignment the page made), and the old loop re-scanning for each of them
/// was the O(n²) the red-team round exposed. The scan ends at the first
/// failed candidate: total work stays linear in the page size.
fn find_conf_data_json(html: &str) -> Option<&str> {
    let mut cursor = 0;
    while let Some(offset) = html[cursor..].find("confData") {
        let start = cursor + offset + "confData".len();
        // The assignment operator and whitespace between the marker and the
        // literal; anything else means this is some other `confData` mention
        // (`confData === undefined` eats one `=` here and still counts as a
        // mention — pinned by ut/scraper.rs).
        let rest = html[start..].trim_start();
        if let Some(rest) = rest.strip_prefix('=') {
            let rest = rest.trim_start();
            if rest.starts_with('{') {
                if let Some(json) = balanced_json(rest) {
                    return Some(json);
                }
                // FINDING F26: a real `{` candidate whose scan ran past the
                // end of input — the object never closes, every later
                // mention lives *inside* it (not an assignment the page
                // made), and the old loop re-scanning for each was the
                // O(n²) the red-team round exposed. The scan ends.
                return None;
            }
        }
        cursor = start;
    }
    None
}

/// Length of the balanced `{...}` JSON literal at the start of `s`.
fn balanced_json(s: &str) -> Option<&str> {
    if !s.starts_with('{') {
        return None;
    }
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (i, &b) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_calendar(value: &Value, key: &str) -> Result<RawCalendar> {
    serde_json::from_value::<RawCalendar>(value[key].clone()).map_err(|e| {
        MawaqitError::Parse(error::bounded(
            &format!("{key}: {e}"),
            BOUNDED_DIAGNOSTIC,
        ))
    })
}
