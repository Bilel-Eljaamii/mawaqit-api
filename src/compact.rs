//! Placeholder for the `heapless`-gated compact representation (no_std MCU
//! work, ADR-0013). It exists so `rustfmt` and the module tree resolve
//! while `pub mod compact` is declared in `lib.rs` behind
//! `#[cfg(feature = "heapless")]`; the implementation lands with that
//! feature's series.
