//! Disk snapshot of a mosque's `ConfData` — the offline layer.
//!
//! One fetched mosque page carries the whole year (adhan calendar, iqama
//! calendar, metadata), so a single snapshot file per mosque serves the
//! Today view, the tray alarms and the month view with no network. The
//! snapshot directory lives next to the app config and is exactly as
//! attacker-writable as the config file: loading must be total — missing,
//! truncated or hostile files yield `None`, never a panic.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::models::ConfData;

/// Bump when the envelope layout changes; older versions are ignored.
const SNAPSHOT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct SnapshotEnvelope {
    version: u32,
    /// The slug this snapshot belongs to — verified on load so a file can
    /// never be served for a different mosque.
    mosque_slug: String,
    fetched_at: NaiveDate,
    /// The original confData object (`ConfData::raw`). Storing the raw
    /// object — not the struct — avoids the duplicate-key corruption
    /// `#[serde(flatten)]` produces on serialize (calendar/times/name would
    /// appear twice, and a flattened struct refuses to parse that back).
    conf: serde_json::Value,
}

/// The JSON stored in the envelope: the tolerant struct's serialization,
/// overlaid with the unmodeled extras from `raw`. Storing the struct (not
/// the raw object) is what makes messy mosques work (FINDING F10): the
/// scraper tolerates wire shapes strict serde rejects (nulls inside iqama
/// rows, numeric names) — serializing raw verbatim would write those shapes
/// back and the snapshot could never load. Modeled keys always win over raw
/// keys, so hostile shapes inside them can never reach the file.
fn conf_to_storage(conf: &ConfData) -> Option<serde_json::Value> {
    let mut stripped = conf.clone();
    stripped.raw = serde_json::Value::Null;
    let mut value = serde_json::to_value(&stripped).ok()?;
    if let (
        serde_json::Value::Object(base),
        serde_json::Value::Object(extras),
    ) = (&mut value, &conf.raw)
    {
        for (key, extra) in extras {
            base.entry(key.clone()).or_insert_with(|| extra.clone());
        }
    }
    Some(value)
}

/// The snapshot file for `slug` inside `dir`. The name is derived from a
/// hash of the slug, so a hostile slug (`../`, `?`, `#`, 4 MB of Unicode)
/// can never escape the directory or forge another mosque's file; the real
/// slug travels inside the envelope.
pub fn snapshot_path(dir: &Path, slug: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    slug.hash(&mut hasher);
    dir.join(format!("{:016x}.json", hasher.finish()))
}

/// Load the snapshot for `slug`, or `None` when absent, stale-schema, or
/// hostile (any parse failure is "no snapshot" — same contract as the app
/// config).
pub fn load(dir: &Path, slug: &str) -> Option<(NaiveDate, ConfData)> {
    let content = std::fs::read_to_string(snapshot_path(dir, slug)).ok()?;
    let envelope: SnapshotEnvelope = serde_json::from_str(&content).ok()?;
    if envelope.version != SNAPSHOT_VERSION || envelope.mosque_slug != slug {
        return None;
    }
    let conf = serde_json::from_value(envelope.conf).ok()?;
    Some((envelope.fetched_at, conf))
}

/// Atomically store a snapshot for `slug` (write to a temp file, then
/// rename). IO and serialization failures are swallowed: the snapshot is an
/// optimization, a failed write must never break an online fetch. Returns
/// the recorded fetch date on success.
pub fn store(dir: &Path, slug: &str, conf: &ConfData) -> Option<NaiveDate> {
    let fetched_at = Local::now().date_naive();
    let envelope = SnapshotEnvelope {
        version: SNAPSHOT_VERSION,
        mosque_slug: slug.to_string(),
        fetched_at,
        conf: conf_to_storage(conf)?,
    };
    let json = serde_json::to_string(&envelope).ok()?;
    let path = snapshot_path(dir, slug);
    // Always Some: snapshot_path joins the directory with a file name.
    let parent = path.parent()?;
    std::fs::create_dir_all(parent).ok()?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).ok()?;
    // Durability before the atomic swap (review L1): without fsync a power
    // cut can silently lose the just-stored snapshot. The directory entry
    // itself stays un-fsynced — a residual gap accepted for a cache.
    if let Ok(f) = std::fs::File::open(&tmp) {
        let _ = f.sync_all();
    }
    std::fs::rename(&tmp, &path).ok()?;
    Some(fetched_at)
}
