//! Disk-snapshot contract tests (moved out of `src/disk.rs`): a successful
//! fetch must refresh the snapshot, a failed fetch must serve it, and a
//! hostile snapshot file must degrade to a plain error — never a panic.
//! Real temp files and a mock server: component territory.

use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use mawaqit_api::{ConfData, MawaqitClient, disk};
use serde_json::json;

use crate::common::temp_dir;

// ---------------------------------------------------------------- mock server

/// Serve one canned response for every request (raw TCP, like the hostile
/// HTTP suite). Requests are counted.
fn spawn_mock(response: String) -> (String, thread::JoinHandle<()>) {
    let listener =
        TcpListener::bind("127.0.0.1:0").expect("mock binds an ephemeral port");
    let base = format!("http://{}", listener.local_addr().expect("local addr"));
    let handle = thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let head = String::from_utf8_lossy(&buf);
            if !head.starts_with("GET ") {
                continue;
            }
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    (base, handle)
}

fn ok_html(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// A minimal valid mosque page: 6 daily times + a one-month calendar.
fn mosque_page() -> String {
    let conf = r#"{
        "times": ["05:00", "06:30", "12:00", "15:30", "18:00", "19:30"],
        "calendar": [{"1": ["05:00","06:30","12:00","15:30","18:00","19:30"]}],
        "name": "Snapshot Test Mosque"
    }"#;
    ok_html(&format!("<html><script>var confData = {conf};</script></html>"))
}

/// A base URL on port 1 — connection refused instantly.
const DEAD_BASE: &str = "http://127.0.0.1:1";

const SLUG: &str = "grande-mosquee-de-paris";

// ------------------------------------------------------------------- tests

#[tokio::test]
async fn successful_fetch_writes_a_snapshot_and_serves_memory_afterwards() {
    let dir = temp_dir("ct", "store");
    let (base, server) = spawn_mock(mosque_page());
    let client = MawaqitClient::with_base_urls(base.clone(), base.clone())
        .with_disk_cache(dir.clone());

    let (conf, as_of) =
        client.conf_data_dated(SLUG).await.expect("first fetch");
    assert_eq!(conf.name.as_deref(), Some("Snapshot Test Mosque"));
    assert_eq!(as_of, None, "a live fetch is not 'from disk'");

    // The snapshot landed on disk with today's date.
    let (fetched_at, stored) = disk::load(&dir, SLUG).expect("snapshot stored");
    assert_eq!(fetched_at, chrono::Local::now().date_naive());
    assert_eq!(stored.name.as_deref(), Some("Snapshot Test Mosque"));

    // Second call is the in-memory cache: still not 'from disk'.
    let (_, as_of) = client.conf_data_dated(SLUG).await.expect("second fetch");
    assert_eq!(as_of, None);
    drop(server);
}

#[tokio::test]
async fn failed_fetch_falls_back_to_the_snapshot() {
    let dir = temp_dir("ct", "fallback");
    // Seed the snapshot through the public API (a previous online session).
    let (base, _server) = spawn_mock(mosque_page());
    let online = MawaqitClient::with_base_urls(base.clone(), base)
        .with_disk_cache(dir.clone());
    online.conf_data(SLUG).await.expect("seed fetch");
    drop(_server);

    // Now the network is gone; the snapshot must serve.
    let offline = MawaqitClient::with_base_urls(
        DEAD_BASE.to_string(),
        DEAD_BASE.to_string(),
    )
    .with_disk_cache(dir);
    let (conf, as_of) =
        offline.conf_data_dated(SLUG).await.expect("offline fallback");
    assert_eq!(conf.name.as_deref(), Some("Snapshot Test Mosque"));
    assert!(as_of.is_some(), "served-from-snapshot must carry its date");

    // And derived data paths work offline too.
    let month = offline.month(SLUG, 1).await.expect("offline month");
    assert!(!month.days.is_empty());
}

#[tokio::test]
async fn offline_without_a_snapshot_is_an_error() {
    let dir = temp_dir("ct", "empty");
    let offline = MawaqitClient::with_base_urls(
        DEAD_BASE.to_string(),
        DEAD_BASE.to_string(),
    )
    .with_disk_cache(dir);
    assert!(offline.conf_data_dated(SLUG).await.is_err());
}

#[tokio::test]
async fn hostile_snapshot_file_degrades_to_an_error() {
    let dir = temp_dir("ct", "hostile");
    std::fs::write(disk::snapshot_path(&dir, SLUG), "{\"version\":1,\"mos")
        .unwrap();
    let offline = MawaqitClient::with_base_urls(
        DEAD_BASE.to_string(),
        DEAD_BASE.to_string(),
    )
    .with_disk_cache(dir);
    assert!(
        offline.conf_data_dated(SLUG).await.is_err(),
        "a truncated snapshot must not serve, and must not panic"
    );
}

#[tokio::test]
async fn without_disk_cache_the_client_behaves_as_before() {
    let client = MawaqitClient::with_base_urls(
        DEAD_BASE.to_string(),
        DEAD_BASE.to_string(),
    );
    assert!(
        client.conf_data_dated(SLUG).await.is_err(),
        "no cache, no fallback"
    );
}

/// A default (empty) conf stores and reloads: the storage overlay must
/// tolerate a `raw` that is not an object at all.
#[test]
fn default_conf_roundtrips_through_the_snapshot() {
    let dir = temp_dir("ct", "empty-conf");
    let stored = disk::store(&dir, SLUG, &ConfData::default())
        .expect("an empty conf stores");
    let (fetched_at, conf) = disk::load(&dir, SLUG).expect("reload");
    assert_eq!(fetched_at, stored);
    assert!(conf.times.is_empty());
    assert!(conf.calendar.is_empty());
}

// ------------------------------------------------ round-2 findings (F23/F27)

/// A valid minimal conf object for hand-written envelopes.
fn valid_conf_json() -> serde_json::Value {
    json!({
        "times": ["05:27", "06:37", "13:21", "16:37", "19:24"],
        "calendar": [{"1": ["05:27", "06:37", "07:07", "13:21", "16:37", "19:24"]}],
        "name": "Hand Written Mosque",
    })
}

/// Hand-write an envelope with an explicit `fetched_at` (the store API
/// always stamps today, which is exactly what the staleness pins must not
/// assume).
fn write_envelope(
    dir: &std::path::Path,
    fetched_at: chrono::NaiveDate,
    conf: serde_json::Value,
) {
    let envelope = json!({
        "version": 1,
        "mosque_slug": SLUG,
        "fetched_at": fetched_at.to_string(),
        "conf": conf,
    });
    std::fs::write(disk::snapshot_path(dir, SLUG), envelope.to_string())
        .unwrap();
}

/// FINDING F23a — `load` read the whole file into memory before parsing:
/// the snapshot directory is attacker-writable at the app's privilege, so
/// a 10 GB "snapshot" blows RSS before the first parse. The read is capped
/// at 2 MB (a real page is ~60 KB); an oversized file is "no snapshot".
#[test]
fn finding_f23_oversized_snapshot_file_is_refused() {
    let dir = temp_dir("ct", "f23-cap");
    let mut conf = valid_conf_json();
    conf["junk"] = json!("x".repeat(3 * 1024 * 1024));
    write_envelope(&dir, chrono::Local::now().date_naive(), conf);
    assert!(
        disk::load(&dir, SLUG).is_none(),
        "a file past the read cap is not a snapshot"
    );
}

/// FINDING F23b — the snapshot load deserialized hostile JSON without the
/// shared sanitizer; the offline path now strips exactly what the online
/// page path strips (F6/F21 field set), so a snapshot can never carry what
/// the live path would have rejected.
#[test]
fn finding_f23_snapshot_load_is_sanitized() {
    let dir = temp_dir("ct", "f23-sanitize");
    let mut conf = valid_conf_json();
    conf["name"] = json!("\u{202E}EVIL\u{200B}");
    conf["announcements"] =
        json!([{ "title": "\u{200B}t", "start_date": "\u{202E}d" }]);
    write_envelope(&dir, chrono::Local::now().date_naive(), conf);

    let (_, loaded) = disk::load(&dir, SLUG).expect("a valid envelope loads");
    assert_eq!(loaded.name.as_deref(), Some("EVIL"));
    let ann = &loaded.announcements[0];
    assert_eq!(ann.title.as_deref(), Some("t"));
    assert_eq!(ann.start_date.as_deref(), Some("d"));
}

/// FINDING F23c — the temp file name was fixed (`X.json.tmp`): two writers
/// racing on one slug shared the temp path and one could rename a torn
/// file into place. Temp names are per-writer unique (pid + sequence).
#[test]
fn f23_tmp_names_are_unique_per_writer() {
    let path = disk::snapshot_path(&temp_dir("ct", "f23-tmp"), SLUG);
    let a = disk::tmp_path(&path);
    let b = disk::tmp_path(&path);
    assert_ne!(a, b, "each writer gets its own temp file");
    assert_eq!(a.extension().and_then(|e| e.to_str()), Some("tmp"));
}

/// FINDING F23 (anchor) — a non-UTF8 snapshot file degrades to None, the
/// same contract as any other parse failure.
#[test]
fn f23_non_utf8_snapshot_degrades_to_none() {
    let dir = temp_dir("ct", "f23-utf8");
    std::fs::write(disk::snapshot_path(&dir, SLUG), [0xFF, 0xFE, 0x00])
        .unwrap();
    assert!(disk::load(&dir, SLUG).is_none());
}

/// FINDING F27 — a snapshot had no TTL: a year-old file served "today's"
/// times on the alarm path. Decided contract: ~40-day TTL plus the
/// same-calendar-year rule inside `disk::load` (one page = one year,
/// ADR-0002) — a December snapshot never answers a January date.
#[test]
fn finding_f27_stale_snapshots_are_refused() {
    let dir = temp_dir("ct", "f27");
    let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();

    write_envelope(&dir, today - chrono::Duration::days(40), valid_conf_json());
    assert!(
        disk::load_as_of(&dir, SLUG, today).is_some(),
        "40 days is within the TTL"
    );

    write_envelope(&dir, today - chrono::Duration::days(41), valid_conf_json());
    assert!(
        disk::load_as_of(&dir, SLUG, today).is_none(),
        "41 days is stale — no snapshot"
    );

    // The Dec 31 -> Jan 1 boundary: one day old, wrong calendar year.
    write_envelope(
        &dir,
        chrono::NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
        valid_conf_json(),
    );
    assert!(
        disk::load_as_of(
            &dir,
            SLUG,
            chrono::NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()
        )
        .is_none(),
        "a December snapshot never serves January"
    );
}

/// FINDING F27 (client contract) — offline with only a stale snapshot is
/// an honest error (the real network failure), never year-old times on the
/// alarm path.
#[tokio::test]
async fn finding_f27_offline_stale_snapshot_is_an_error() {
    let dir = temp_dir("ct", "f27-offline");
    let stale = chrono::Local::now().date_naive() - chrono::Duration::days(60);
    write_envelope(&dir, stale, valid_conf_json());

    let offline = MawaqitClient::with_base_urls(
        DEAD_BASE.to_string(),
        DEAD_BASE.to_string(),
    )
    .with_disk_cache(dir);
    assert!(
        offline.conf_data_dated(SLUG).await.is_err(),
        "a stale snapshot must not serve as today's data"
    );
}
