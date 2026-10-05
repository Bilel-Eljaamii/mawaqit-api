//! The slug/URL hardening matrix — what keeps a hostile string from
//! reaching the network verbatim or escaping the snapshot directory.
//!
//! A mosque "id" is untrusted input: it arrives from a search response, a
//! URL parameter, or a hand-typed config. The client's defense-in-depth:
//!
//! 1. [`is_valid_slug`] accepts only `a-z0-9` with single hyphens, at most 128
//!    bytes — anything else (`../`, `?`, `#`, Unicode, whitespace, giant blobs)
//!    is rejected;
//! 2. rejected slugs never reach the wire: [`MawaqitClient`] substitutes a
//!    deterministic placeholder slug of the same length, which can only 404
//!    into `MosqueNotFound`;
//! 3. snapshot files are keyed by a **hash** of the slug
//!    ([`disk::snapshot_path`]), so no slug can traverse out of the cache
//!    directory or forge another mosque's file.
//!
//! Run offline by default; `--live` also fires one hostile slug at the real
//! site to show the 404 → typed error path.
//!
//! Usage:
//!   cargo run -p mawaqit-api --example slug_hardening -- [--live]

use std::path::PathBuf;

use mawaqit_api::{MawaqitClient, MawaqitError, disk, is_valid_slug, page_url};

const SITE: &str = "https://mawaqit.net";
const HYPOTHETICAL_CACHE: &str = "/var/cache/mawaqit";

/// The placeholder rule from `MawaqitClient::fetch_conf_data`: an invalid
/// slug is replaced by `"-"` repeated to the same length (clamped 4..=64),
/// so requests stay deterministic and land in the mosque namespace only.
fn effective_slug(input: &str) -> String {
    if is_valid_slug(input) {
        input.to_string()
    } else {
        "-".repeat(input.len().clamp(4, 64))
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}

fn main() {
    let live = std::env::args().any(|a| a == "--live");

    println!("── slug decision matrix");
    println!(
        "{:<26} {:<7} URL that would actually be requested",
        "input", "valid?"
    );
    println!("{}", "-".repeat(96));
    let cases: Vec<String> = vec![
        "grande-mosquee-de-paris".into(),
        "../etc/passwd".into(),
        "..\\..\\windows\\win.ini".into(),
        "paris?utm_source=x".into(),
        "paris#admin".into(),
        "%2e%2e%2fpasswd".into(),
        "Grande-Mosquée-de-Paris".into(),
        "double--hyphen".into(),
        "-leading-hyphen".into(),
        "trailing-hyphen-".into(),
        "has spaces".into(),
        String::new(),
        "المسجد-النور".into(),
        "x".repeat(100_000),
    ];
    for case in &cases {
        let eff = effective_slug(case);
        println!(
            "{:<26} {:<7} {}",
            truncate(case, 24),
            is_valid_slug(case),
            truncate(&page_url(SITE, &eff), 62),
        );
    }

    println!("\n── snapshot path traversal (hash-keyed files)");
    let dir = PathBuf::from(HYPOTHETICAL_CACHE);
    for slug in ["grande-mosquee-de-paris", "../../etc/passwd", "a/b/c"] {
        let path = disk::snapshot_path(&dir, slug);
        let inside = path.parent() == Some(dir.as_path());
        println!(
            "  {:<26} → {:<22} stays inside dir: {}",
            truncate(slug, 24),
            path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            inside
        );
        assert!(inside, "a slug escaped the snapshot directory: {path:?}");
        assert_eq!(path.extension().and_then(|e| e.to_str()), Some("json"));
    }

    if live {
        println!("\n── live confirmation (hits mawaqit.net)");
        let client = MawaqitClient::new();
        for slug in ["../../etc/passwd", "grande-mosquee-de-paris"] {
            match fetch_conf_blocking(&client, slug) {
                Ok(_) => {
                    println!("  {slug:?}: fetched (unexpected for hostile)")
                }
                Err(MawaqitError::MosqueNotFound(s)) => {
                    println!("  {slug:?} → MosqueNotFound({s:?}) — defused")
                }
                Err(e) => println!("  {slug:?} → {e}"),
            }
        }
    } else {
        println!(
            "\n(live check skipped — rerun with --live to see a hostile slug\n \
             404 into MosqueNotFound against the real site)"
        );
    }
}

/// Small helper so the live block stays sync-simple: one page fetch.
fn fetch_conf_blocking(
    client: &MawaqitClient,
    slug: &str,
) -> Result<(), MawaqitError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(async { client.conf_data(slug).await.map(|_| ()) })
}
