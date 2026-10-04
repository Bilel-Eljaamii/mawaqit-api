# mawaqit-api — every build step and the verify gate, one command each.
#
# Install just:   pacman -S just        (Arch/Manjaro)
#                 cargo install just   (any platform with Rust)
#
# The one command to remember:   just verify

set shell := ["bash", "-cu"]
set positional-arguments

default:
    @just --list

# ---------------------------------------------------------------- build ----

# Debug-build the library and every example
build:
    cargo build --workspace
    cargo build --examples

# Release-build the library
release:
    cargo build --release

# Type-check everything (lib, tests, examples) without producing binaries
check:
    cargo check --all-targets

# ------------------------------------------------------- format and lint ----

# Format all code (nightly rustfmt — rustfmt.toml uses unstable options)
fmt:
    #!/usr/bin/env bash
    set -euo pipefail
    if rustup toolchain list 2>/dev/null | grep -q '^nightly'; then
        cargo +nightly fmt --all
    else
        echo "note: no nightly toolchain; stable rustfmt ignores some rustfmt.toml options" >&2
        cargo fmt --all
    fi

# Format check (no writes; same toolchain choice as `just fmt`)
fmt-check:
    #!/usr/bin/env bash
    set -euo pipefail
    if rustup toolchain list 2>/dev/null | grep -q '^nightly'; then
        cargo +nightly fmt --all --check
    else
        echo "note: no nightly toolchain; stable rustfmt ignores some rustfmt.toml options" >&2
        cargo fmt --all --check
    fi

# Clippy with warnings as errors (respects clippy.toml)
lint:
    cargo clippy --all-targets -- -D warnings

# ----------------------------------------------------------------- test ----

# Offline, deterministic suite (hostile HTTP/semantics/corpus + snapshot)
test:
    cargo test

# Live-site campaign: 100+ real mosques, takes minutes, needs the network
live:
    cargo test --test world_hostile -- --ignored --nocapture

# ------------------------------------------------------------ doc/fuzz ----

# Build the rustdoc documentation
doc:
    cargo doc --no-deps

# Run a fuzz target for SECS seconds (needs cargo-fuzz and nightly)
fuzz target="parse_page" secs="60":
    #!/usr/bin/env bash
    set -euo pipefail
    command -v cargo-fuzz >/dev/null \
        || { echo "missing cargo-fuzz — cargo install cargo-fuzz"; exit 1; }
    rustup toolchain list 2>/dev/null | grep -q nightly \
        || { echo "cargo-fuzz needs nightly — rustup toolchain install nightly"; exit 1; }
    cd fuzz && cargo fuzz run {{target}} -- -max_total_time={{secs}}

# ------------------------------------------------------------ examples ----

# List the available examples
@examples:
    for f in examples/*.rs; do echo "  $(basename "${f%.rs}")"; done

# Run one example, passing it any extra args:
#   just example next_prayer "Grande Mosquée"
@example *args:
    cargo run --example "$1" -- "${@:2}"

# --------------------------------------------------- pre-commit helpers ----

# GitNexus graph change analysis (AGENTS.md requires this before committing)
graph:
    node .gitnexus/run.cjs detect-changes --scope all --repo .

# --------------------------------------------------------- the real gate ----

# THE gate: everything that must pass before a change is done.
# Runs in order and fails fast: format check → clippy → type check →
# offline tests → docs → one network-free example as an end-to-end smoke.
verify: fmt-check lint check test doc
    just example page_scraper
    @echo
    @echo "✔ verify gate passed"

# ------------------------------------------------------------ cleanup ----

# Remove build artifacts (including the examples' export/cache output)
clean:
    cargo clean
    rm -rf times-export times-cache
