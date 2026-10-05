//! Live smoke test (moved out of `src/client.rs`): a single search +
//! today-times round trip against the real site — the fast first check
//! that the keyless path still works before running the full world tour.
//!
//! `cargo test --test e2e -- --ignored --nocapture` (or `just live`).

use mawaqit_api::MawaqitClient;

#[tokio::test]
#[ignore = "hits the live mawaqit.net site"]
async fn live_search_and_calendar() {
    let client = MawaqitClient::new();
    let mosques = client.search_mosques("Paris").await.expect("search");
    println!("search hits: {}", mosques.len());
    assert!(!mosques.is_empty(), "expected at least one mosque");

    let slug = mosques
        .iter()
        .find_map(|m| m.mosque_id())
        .expect("a mosque with a slug")
        .to_string();
    println!("mosque: {} ({slug})", mosques[0].display_name());

    let today = client.today(&slug).await.expect("today");
    println!(
        "fajr={} shurouq={} isha={}",
        today.adhan.fajr, today.adhan.shurouq, today.adhan.isha
    );
    if let Some(iq) = &today.iqama {
        println!("iqama: {:?}", iq);
    }

    let month = client.month(&slug, 1).await.expect("month");
    assert!(!month.days.is_empty());
}
