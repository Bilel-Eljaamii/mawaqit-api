//! Fuzz-adjacent tier (`fuzz`): deterministic, seed-driven mutation fuzzing
//! of the surfaces the corpus tier does not reach — the search-response
//! model, the URL builder and the disk snapshot layer. Fixed xorshift seed,
//! so every run explores the same space; offline and CI-safe.
//!
//! The libFuzzer campaign (`just fuzz`, nightly `cargo fuzz` in `fuzz/`)
//! explores beyond this tier; this one keeps the mutation space reproducible
//! in a plain `cargo test`.
//!
//! `cargo test --test fuzz`

mod common;
#[path = "fuzz/mutation.rs"]
mod mutation;
