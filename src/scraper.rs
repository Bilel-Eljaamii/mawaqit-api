#[cfg(not(feature = "std"))]
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

use serde_json::Value;

use crate::{
    error::{MawaqitError, Result},
    models::{Announcement, ConfData, RawCalendar},
};

/// Strip control characters and the invisible Unicode *Format* (Cf)
/// category from free-text display fields — FINDING F6 and its review
/// follow-up: `is_control()` alone misses zero-width characters (U+200B,
/// U+FEFF), the Arabic marks and the remaining direction/isolate code
/// points, because they are Format, not Control — yet they spoof display
/// strings and dodge search/dedupe just the same. Time strings are
/// deliberately NOT sanitized here: they are pinned strict elsewhere (F4),
/// and stripping could mint a valid "HH:MM" out of hostile bytes instead
/// of rejecting the day.
fn sanitize_text(s: &str) -> String {
    s.chars().filter(|ch| !is_invisible(*ch)).collect()
}

/// `is_control()` plus the invisible Format (Cf) characters. std exposes no
/// general-category API, so the Cf set is an explicit range table (Unicode
/// 15); the non-BMP marks are the ones relevant to mosque/agenda text.
fn is_invisible(ch: char) -> bool {
    ch.is_control()
        || matches!(ch,
            '\u{00AD}'                  // soft hyphen
            | '\u{0600}'..='\u{0605}'   // Arabic number signs
            | '\u{061C}'                // Arabic letter mark
            | '\u{06DD}' | '\u{070F}' | '\u{08E2}'
            | '\u{180E}'                // Mongolian vowel separator
            | '\u{200B}'..='\u{200F}'   // zero-width + LRM/RLM
            | '\u{202A}'..='\u{202E}'   // bidi embedding/overrides
            | '\u{2060}'..='\u{206F}'   // invisible operators + isolates
            | '\u{FEFF}'                // BOM / zero-width no-break space
            | '\u{FFF9}'..='\u{FFFB}'   // interlinear annotation anchors
            | '\u{110BD}' | '\u{110CD}' // Kaithi number signs
            | '\u{13430}'..='\u{1343F}' // Egyptian format controls
            | '\u{1D173}'..='\u{1D17A}' // musical symbol control
            | '\u{E0001}' | '\u{E0020}'..='\u{E007F}' // variation tags
        )
}

/// The wire-tolerant Option extractor for display strings, sanitized.
fn display_string(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().map(sanitize_text)
}

/// Extract the `confData` JavaScript object embedded in a mosque page.
///
/// The public page (`https://mawaqit.net/{lang}/{slug}`) ships its whole
/// configuration — daily times, year calendar, iqama calendar, mosque
/// metadata — as one JSON literal assigned to a `confData` variable.
pub fn extract_conf_data(page_html: &str, mosque_id: &str) -> Result<ConfData> {
    let json_str = find_conf_data_json(page_html)
        .ok_or_else(|| MawaqitError::ConfDataNotFound(mosque_id.to_string()))?;

    let value: Value = serde_json::from_str(json_str)
        .map_err(|e| MawaqitError::Parse(format!("confData: {e}")))?;

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

    let announcements: Vec<Announcement> = value["announcements"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .map(|mut ann: Announcement| {
                    ann.title = ann.title.take().map(|t| sanitize_text(&t));
                    ann.content = ann.content.take().map(|t| sanitize_text(&t));
                    ann.image = ann.image.take().map(|t| sanitize_text(&t));
                    ann.video = ann.video.take().map(|t| sanitize_text(&t));
                    ann
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(ConfData {
        name: display_string(&value, "name"),
        jumua: display_string(&value, "jumua"),
        jumua2: display_string(&value, "jumua2"),
        image: display_string(&value, "image"),
        shuruq: display_string(&value, "shuruq"),
        imsak_mode: times.len() == 6,
        times,
        calendar,
        iqama_calendar,
        announcements,
        raw: value,
    })
}

/// Find the JSON literal assigned to `confData` in any script of the page,
/// scanning from the opening `{` to its balanced closing brace (string- and
/// escape-aware, so `;` or braces inside JSON strings don't break it).
/// Every `confData` mention is tried until one is an actual assignment.
fn find_conf_data_json(html: &str) -> Option<&str> {
    let mut cursor = 0;
    while let Some(offset) = html[cursor..].find("confData") {
        let start = cursor + offset + "confData".len();
        // The assignment operator and whitespace between the marker and the
        // literal; anything else means this is some other `confData` mention.
        let rest = html[start..].trim_start();
        if let Some(rest) = rest.strip_prefix('=') {
            let rest = rest.trim_start();
            if let Some(json) = balanced_json(rest) {
                return Some(json);
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
    serde_json::from_value::<RawCalendar>(value[key].clone())
        .map_err(|e| MawaqitError::Parse(format!("{key}: {e}")))
}
