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

# All tiers: ut + ct + fuzz (offline) — e2e is #[ignore]d, run `just live`
test:
    cargo test

# One test tier: ut (pure) | ct (mocked I/O) | fuzz (mutation) | e2e (live)
tier tier:
    cargo test --test {{tier}}

# Live-site campaign: 100+ real mosques, takes minutes, needs the network
live:
    cargo test --test e2e -- --ignored --nocapture

# Library coverage gate: HTML + lcov, src/ held at 100% line coverage.
# Runs the same tests `just test` runs (the #[ignore]d live tiers are out;
# test files are excluded from the report). The summary is rendered by raw
# llvm-cov with a fixed object order — cargo-llvm-cov's own aggregate
# miscounts functions that exist in several test binaries (each embeds the
# lib), and the voices binary must lead the object list for the union to
# reflect every tier's execution.
coverage:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v cargo-llvm-cov >/dev/null \
        || { echo "missing cargo-llvm-cov — cargo install cargo-llvm-cov"; exit 1; }
    rustup component list --installed 2>/dev/null | grep -q llvm-tools \
        || { echo "missing llvm-tools — rustup component add llvm-tools-preview"; exit 1; }
    LLCOV="$(find ~/.rustup/toolchains -name llvm-cov -path '*bin*' 2>/dev/null | head -1)"
    [ -n "$LLCOV" ] || { echo "llvm-cov binary not found — rustup component add llvm-tools-preview"; exit 1; }
    # One instrumented run over the offline tiers; the e2e target is skipped
    # (it runs nothing — both its tests are #[ignore]d — and its all-zero
    # function copies would poison the union). Stale profiles from earlier
    # builds must go too: mixing records of different function layouts makes
    # llvm-cov's summary undercount ("mismatched data").
    rm -f target/llvm-cov-target/*.profraw target/llvm-cov-target/*.profdata
    cargo llvm-cov --no-report
    LPROFDATA="$(find ~/.rustup/toolchains -name llvm-profdata -path '*bin*' 2>/dev/null | head -1)"
    "$LPROFDATA" merge -o target/llvm-cov-target/mawaqit-api.profdata \
        target/llvm-cov-target/*.profraw
    # Deterministic render order: the voices binary first (it exercises the
    # async download paths no other tier runs), then the offline tiers.
    OBJS=""
    for tier in voices ut ct fuzz; do
        for b in target/llvm-cov-target/debug/deps/"$tier"-*; do
            case "$b" in *.d) ;; *) OBJS="$OBJS -object $b" ;; esac
        done
    done
    # The gate: src/ line coverage must be exactly 100%.
    $LLCOV report $OBJS -instr-profile target/llvm-cov-target/mawaqit-api.profdata \
        --ignore-filename-regex '[^/]+/(tst|tests)/' | tee target/coverage/summary.txt \
        | awk -F'|' '/^TOTAL/ { missed = $9; gsub(/ /, "", missed); if (missed + 0 > 0) { print "FAILED: " missed " uncovered lines in src/ (see per-file rows above)"; exit 1 } }'
    mkdir -p target/coverage
    $LLCOV show $OBJS -instr-profile target/llvm-cov-target/mawaqit-api.profdata \
        --format=html --output-dir target/coverage/html \
        --ignore-filename-regex '[^/]+/(tst|tests)/' >/dev/null
    $LLCOV export $OBJS -instr-profile target/llvm-cov-target/mawaqit-api.profdata \
        --format=lcov --ignore-filename-regex '[^/]+/(tst|tests)/' \
        > target/coverage/lcov.info
    echo
    echo "✔ src/ line coverage: 100% (summary: target/coverage/summary.txt)"
    echo "HTML report: target/coverage/html/index.html"
    echo "lcov trace:  target/coverage/lcov.info"

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
