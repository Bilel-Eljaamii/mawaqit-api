//! Multi-term search with cross-query dedupe and naive relevance ranking.
//!
//! Real apps do not trust a single search: the word "Paris" finds the
//! Grande Mosquée, but "mosquée Paris" and "masjid Paris" find different
//! subsets. This example runs several queries against the keyless search
//! endpoint, merges the results into one deduplicated candidate set, scores
//! each candidate by token overlap, and prints a ranked shortlist — the
//! shape of a real "pick your mosque" screen.
//!
//! Demonstrates:
//! - several `search_mosques` calls under one shared in-memory cache (repeating
//!   a term is free for 30 minutes);
//! - the tolerant `Mosque` model: optional `slug`/`uuid`, display fallbacks via
//!   `display_name()`, and the catch-all `extra` map for unmodeled fields;
//! - dedupe keys that survive slug-less results.
//!
//! Usage:
//!   cargo run -p mawaqit-api --example search_rank -- [term ...]
//! Example:
//!   cargo run -p mawaqit-api --example search_rank -- "grande mosquée"
//! "mosquée paris"

use std::collections::BTreeMap;

use mawaqit_api::Mosque;

/// Dedupe key: prefer the page slug, fall back to the name.
fn key_of(m: &Mosque) -> String {
    m.mosque_id()
        .map(str::to_lowercase)
        .or_else(|| m.name.as_deref().map(str::to_lowercase))
        .or_else(|| m.uuid.clone())
        .unwrap_or_else(|| m.display_name().to_lowercase())
}

/// Naive score: query tokens found in the display name outweigh tokens found
/// only in the place ("locality, country").
fn score_of(m: &Mosque, terms: &[String]) -> i64 {
    let name = m.display_name().to_lowercase();
    let place = m.place().unwrap_or_default().to_lowercase();
    terms
        .iter()
        .flat_map(|t| {
            t.to_lowercase()
                .split_whitespace()
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .map(|token| {
            if name.contains(&token) {
                2
            } else if place.contains(&token) {
                1
            } else {
                0
            }
        })
        .sum()
}

#[tokio::main]
async fn main() {
    let terms: Vec<String> = {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.is_empty() {
            vec!["grande mosquée".into(), "mosquée paris".into()]
        } else {
            args
        }
    };

    let client = mawaqit_api::MawaqitClient::new();

    // Merge every query into one candidate set, keyed for dedupe.
    let mut candidates: BTreeMap<String, Mosque> = BTreeMap::new();
    let mut per_term_hits = Vec::new();
    for term in &terms {
        match client.search_mosques(term).await {
            Ok(mosques) => {
                per_term_hits.push((term, mosques.len()));
                for mosque in mosques {
                    candidates.entry(key_of(&mosque)).or_insert(mosque);
                }
            }
            Err(e) => eprintln!("{term:?}: search failed ({e})"),
        }
    }
    for (term, hits) in &per_term_hits {
        println!("{term:?}: {hits} hit(s)");
    }
    println!(
        "\n{} unique candidate(s) after dedupe over {} query/queries\n",
        candidates.len(),
        terms.len()
    );

    let mut ranked: Vec<(&Mosque, i64)> =
        candidates.values().map(|m| (m, score_of(m, &terms))).collect();
    ranked.sort_by(|a, b| {
        b.1.cmp(&a.1).then_with(|| a.0.display_name().cmp(b.0.display_name()))
    });

    println!(
        "{:>4}  {:<38} {:<22} slug / extra fields",
        "score", "name", "place"
    );
    println!("{}", "-".repeat(96));
    for (mosque, score) in ranked.iter().take(15) {
        let extras = {
            let keys: Vec<&str> =
                mosque.extra.keys().take(3).map(String::as_str).collect();
            format!("{} ({})", mosque.extra.len(), keys.join(","))
        };
        println!(
            "{score:>4}  {:<38} {:<22} {} [{extras}]",
            truncate(mosque.display_name(), 38),
            mosque.place().unwrap_or_else(|| "-".into()),
            mosque.mosque_id().unwrap_or("(no slug)"),
        );
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}
