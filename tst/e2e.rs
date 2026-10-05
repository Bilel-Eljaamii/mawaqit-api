//! End-to-end tier (`e2e`): the whole client against the real mawaqit.net —
//! search, page fetch, parse, calendar resolution, offline snapshot write —
//! exactly the path a user's app takes. Real-world data is the attacker
//! here: layouts differ, slugs die, fields go missing.
//!
//! Network-dependent and slow, so the suite is `#[ignore]`d by default:
//!
//!   cargo test --test e2e -- --ignored --nocapture
//!
//! (or `just live`)

//! - [`smoke`] — the fast single-mosque live check: search → today → month.
//! - [`world_tour`] — 100+ real mosques across five continents, hard-fails on
//!   anything that would break the app.
//!
//! `cargo test --test e2e -- --ignored --nocapture`
//!
//! (or `just live`)

mod common;
#[path = "e2e/smoke.rs"]
mod smoke;
#[path = "e2e/world_tour.rs"]
mod world_tour;
