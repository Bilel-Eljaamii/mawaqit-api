//! The shared free-text sanitizer: one character policy, applied at every
//! ingress where network- or disk-controlled strings can reach a display
//! surface (ADR-0003). Extracted from the scraper under FINDING F22 — the
//! search endpoint was deserializing `Vec<Mosque>` verbatim — and applied
//! to the disk snapshot load under FINDING F23, so the page parser, the
//! search results and the offline layer all strip the identical character
//! set. Time strings are deliberately never sanitized: they are pinned
//! strict-`HH:MM` at the calendar layer instead (F4), and stripping could
//! mint a valid display time out of hostile bytes where rejection is the
//! correct outcome.

#[cfg(not(feature = "std"))]
use alloc::string::String;

use crate::models::{Announcement, ConfData, Mosque};

/// Strip control characters and the invisible Unicode *Format* (Cf)
/// category from free-text display fields — FINDING F6 and its review
/// follow-up: `is_control()` alone misses zero-width characters (U+200B,
/// U+FEFF), the Arabic marks and the remaining direction/isolate code
/// points, because they are Format, not Control — yet they spoof display
/// strings and dodge search/dedupe just the same.
pub(crate) fn text(s: &str) -> String {
    s.chars().filter(|ch| !is_invisible(*ch)).collect()
}

/// `is_control()` plus the invisible Format (Cf) characters. std exposes no
/// general-category API, so the Cf set is an explicit range table; the
/// non-BMP marks are the ones relevant to mosque/agenda text. FINDING F21:
/// the table must cover every Cf code point a hostile page can reach —
/// gaps (U+0890–0891, U+1BCA0–1BCA3, U+13440–13455) let through the same
/// spoofing characters their table neighbors are stripped for.
fn is_invisible(ch: char) -> bool {
    ch.is_control()
        || matches!(ch,
            '\u{00AD}'                  // soft hyphen
            | '\u{0600}'..='\u{0605}'   // Arabic number signs
            | '\u{061C}'                // Arabic letter mark
            | '\u{06DD}' | '\u{070F}' | '\u{08E2}'
            | '\u{0890}'..='\u{0891}'   // Arabic pound/piastre marks (F21)
            | '\u{180E}'                // Mongolian vowel separator
            | '\u{200B}'..='\u{200F}'   // zero-width + LRM/RLM
            | '\u{202A}'..='\u{202E}'   // bidi embedding/overrides
            | '\u{2060}'..='\u{206F}'   // invisible operators + isolates
            | '\u{FEFF}'                // BOM / zero-width no-break space
            | '\u{FFF9}'..='\u{FFFB}'   // interlinear annotation anchors
            | '\u{110BD}' | '\u{110CD}' // Kaithi number signs
            | '\u{13430}'..='\u{13455}' // Egyptian format controls, incl. the Unicode 15 extension (F21)
            | '\u{1BCA0}'..='\u{1BCA3}' // shorthand format controls (F21)
            | '\u{1D173}'..='\u{1D17A}' // musical symbol control
            | '\u{E0001}' | '\u{E0020}'..='\u{E007F}' // variation tags
        )
}

/// Sanitize every free-text string of one search result (FINDING F22): the
/// modeled fields, the string-valued `id`, and every string anywhere inside
/// the flattened `extra` map — a mosque result is pure display metadata, so
/// unlike [`confdata`] there is no time-string carve-out and the recursion
/// is total.
pub(crate) fn mosque(m: &mut Mosque) {
    m.uuid = m.uuid.take().map(|s| text(&s));
    m.slug = m.slug.take().map(|s| text(&s));
    m.name = m.name.take().map(|s| text(&s));
    m.label = m.label.take().map(|s| text(&s));
    m.locality = m.locality.take().map(|s| text(&s));
    m.country = m.country.take().map(|s| text(&s));
    if let Some(id) = &mut m.id {
        sanitize_value(id);
    }
    for value in m.extra.values_mut() {
        sanitize_value(value);
    }
}

/// Sanitize the exact free-text field set the page extractor pins
/// (F6/F21): the display scalars and every announcement text — including
/// `start_date`/`end_date` (F21). Applied where a `ConfData` materializes
/// from hostile bytes without going through the scraper: the disk snapshot
/// load (F23). Time strings and calendar rows are untouched (F4).
pub(crate) fn confdata(c: &mut ConfData) {
    c.name = c.name.take().map(|s| text(&s));
    c.jumua = c.jumua.take().map(|s| text(&s));
    c.jumua2 = c.jumua2.take().map(|s| text(&s));
    c.image = c.image.take().map(|s| text(&s));
    c.shuruq = c.shuruq.take().map(|s| text(&s));
    for ann in &mut c.announcements {
        announcement(ann);
    }
}

/// One announcement's free-text fields (F6/F21).
fn announcement(a: &mut Announcement) {
    a.title = a.title.take().map(|s| text(&s));
    a.content = a.content.take().map(|s| text(&s));
    a.image = a.image.take().map(|s| text(&s));
    a.video = a.video.take().map(|s| text(&s));
    a.start_date = a.start_date.take().map(|s| text(&s));
    a.end_date = a.end_date.take().map(|s| text(&s));
    for value in a.extra.values_mut() {
        sanitize_value(value);
    }
}

/// Strip invisible characters from every string reachable in a JSON value.
fn sanitize_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(s) => {
            let cleaned = text(s);
            *s = cleaned;
        }
        serde_json::Value::Array(items) => {
            for item in items {
                sanitize_value(item);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values_mut() {
                sanitize_value(item);
            }
        }
        _ => {}
    }
}
