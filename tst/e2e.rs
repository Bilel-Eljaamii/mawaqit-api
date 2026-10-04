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

mod common;
#[path = "e2e/world_tour.rs"]
mod world_tour;
