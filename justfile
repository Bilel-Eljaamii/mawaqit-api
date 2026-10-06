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

# Type-check the whole feature matrix, including the no_std MCU targets
# (ADR-0013/0014). `rustup target add thumbv7em-none-eabihf
# riscv32imc-unknown-none-elf` once; then this is cheap. This is the gate
# that keeps the no_std docs honest — default-features-only checks cannot
# see a broken `heapless`/`alloc` tier.
targets-mcu:
    #!/usr/bin/env bash
    set -euo pipefail
    for target in thumbv7em-none-eabihf riscv32imc-unknown-none-elf; do
        rustup target list --installed | grep -q "^$target\$" \
            || { echo "missing target $target — rustup target add $target"; exit 1; }
        cargo check --target "$target" --no-default-features --features heapless
        cargo check --target "$target" --no-default-features --features alloc
    done
    cargo check --no-default-features --features heapless
    cargo check --no-default-features --features alloc
    echo "feature matrix clean: {thumbv7em, riscv32imc, host} x {std, alloc, heapless}"

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

# Library coverage gate: src/ held at 100% line coverage — the recipe
# FAILS if any executable src/ line is not executed. Runs each offline
# tier binary separately and checks per-binary line coverage with
# llvm-cov show, then unions the results (a src line is at 100% when ANY
# tier binary executed it). llvm-cov's own multi-binary report/export
# aggregate miscounts functions that exist in several test binaries (each
# embeds a copy of the lib), so its summary cannot be used directly; the
# per-binary show view is exact. The #[ignore]d live tier never executes
# and is excluded; test files are not part of the measured surface.
# Prints a per-file terminal report built from the same union the badge
# and the gate use; part of `just verify`.
coverage:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v cargo-llvm-cov >/dev/null \
        || { echo "missing cargo-llvm-cov — cargo install cargo-llvm-cov"; exit 1; }
    rustup component list --installed 2>/dev/null | grep -q llvm-tools \
        || { echo "missing llvm-tools — rustup component add llvm-tools-preview"; exit 1; }
    LLCOV="$(find ~/.rustup/toolchains -name llvm-cov -path '*bin*' 2>/dev/null | head -1)"
    [ -n "$LLCOV" ] || { echo "llvm-cov binary not found — rustup component add llvm-tools-preview"; exit 1; }
    LPROFDATA="$(find ~/.rustup/toolchains -name llvm-profdata -path '*bin*' 2>/dev/null | head -1)"
    [ -n "$LPROFDATA" ] || { echo "llvm-profdata binary not found"; exit 1; }
    rm -rf target/coverage
    mkdir -p target/coverage
    TOTAL=0
    for tier in ut ct fuzz voices; do
        rm -f target/llvm-cov-target/*.profraw target/llvm-cov-target/*.profdata
        cargo llvm-cov --no-report --test "$tier"
        bin=$(ls -t target/llvm-cov-target/debug/deps/"$tier"-* 2>/dev/null | grep -v '\.d$' | head -1)
        [ -n "$bin" ] || { echo "missing test binary for tier: $tier"; exit 1; }
        "$LPROFDATA" merge -o "target/coverage/prof-$tier.profdata" \
            target/llvm-cov-target/*.profraw
        # Per-tier uncovered executable lines (llvm-cov show is the one
        # renderer whose per-line counts are exact for this layout). Files
        # with no coverage mapping (declarative modules like error.rs/
        # lib.rs — no instrumented functions) render empty and are skipped.
        rm -f "target/coverage/uncovered-$tier.txt"
        for f in src/*.rs; do
            # Mapping-less files (declarative modules) can make llvm-cov
            # show exit 1 with no output — tolerated, then skipped.
            out="$($LLCOV show -instr-profile "target/coverage/prof-$tier.profdata" \
                "$bin" "$f" 2>/dev/null || true)"
            [ -n "$out" ] || { echo "  no coverage mapping: $f (skipped)"; continue; }
            # llvm-cov show pads every column with spaces and humanizes
            # counts ("1.17k"), so the matchers must tolerate both: a line
            # is executable when the count column holds any digit, and
            # uncovered when it holds a bare zero.
            printf '%s\n' "$out" | awk -F'|' -v f="$f" \
                'NF >= 3 && $1 ~ /^[ ]*[0-9]+$/ && $2 ~ /^[ ]*0[ ]*$/ { print f ":" $1 + 0 }' \
                >> "target/coverage/uncovered-$tier.txt"
            # Total executable lines come from the first tier only — the
            # tiers share one lib build, so the mapping is identical; the
            # union below subtracts from this same surface.
            if [ "$tier" = ut ]; then
                n=$(printf '%s\n' "$out" | awk -F'|' \
                    'NF >= 3 && $1 ~ /^[ ]*[0-9]+$/ && $2 ~ /^[ ]*[0-9]/ { n++ } END { print n + 0 }')
                TOTAL=$((TOTAL + n))
                # The terminal report's per-file surface: the ut mapping
                # (the same definition as TOTAL).
                printf '%d %s\n' "$n" "$f" >> target/coverage/perfile-ut.txt
            fi
        done
    done
    # The union of uncovered lines: a src line is uncovered only when NO
    # tier binary executed it — i.e. its file:line appears in every tier's
    # zero list. Drives the badge, the terminal report and the gate.
    cat target/coverage/uncovered-*.txt | sort | uniq -c \
        | awk -v tiers=4 '$1 == tiers { print $2 }' \
        > target/coverage/missed-union.txt
    missed_n=$(wc -l < target/coverage/missed-union.txt)
    awk -F: '{ print $1 }' target/coverage/missed-union.txt | sort | uniq -c \
        > target/coverage/missed-perfile.txt
    # The badge: the real percentage behind the gate, written before the
    # gate check so a failing run still records the honest number.
    covered=$((TOTAL - missed_n))
    pct=$((TOTAL > 0 ? covered * 100 / TOTAL : 0))
    color=brightgreen
    [ "$pct" -eq 100 ] || color=orange
    mkdir -p target/coverage/badges
    printf '{"schemaVersion":1,"label":"line coverage","message":"%d%%","color":"%s"}\n' \
        "$pct" "$color" > target/coverage/badges/coverage.json
    # Terminal report: per-file line coverage over the same union the
    # badge and the gate use (total lines = ut mapping surface; missed =
    # lines zero in every tier binary).
    echo
    echo "  src/ line coverage — union of the ut/ct/fuzz/voices tier binaries:"
    printf '  %-24s %8s %8s %7s %10s\n' FILE LINES COVERED MISSED COVERAGE
    tl=0; tc=0
    while read -r n f; do
        m=$(awk -v f="$f" '$2 == f { print $1; exit }' target/coverage/missed-perfile.txt)
        m=${m:-0}
        c=$((n - m))
        tl=$((tl + n)); tc=$((tc + c))
        p=$(awk -v c="$c" -v n="$n" 'BEGIN { printf "%.2f", (n > 0 ? c * 100 / n : 0) }')
        printf '  %-24s %8d %8d %7d %9s%%\n' "$f" "$n" "$c" "$m" "$p"
    done < <(sort -k2 target/coverage/perfile-ut.txt)
    pp=$(awk -v c="$tc" -v n="$tl" 'BEGIN { printf "%.2f", (n > 0 ? c * 100 / n : 0) }')
    printf '  %-24s %8d %8d %7d %9s%%\n' TOTAL "$tl" "$tc" "$missed_n" "$pp"
    # Union gate: the report above is informational; the exit code is the
    # gate.
    if [ "$missed_n" -gt 0 ]; then
        echo "FAILED: src/ is not at 100% line coverage:"
        sed 's/^/  /' target/coverage/missed-union.txt
        exit 1
    fi
    rm -f target/coverage/uncovered-*.txt target/coverage/missed-union.txt \
        target/coverage/missed-perfile.txt target/coverage/perfile-ut.txt
    for tier in ut ct fuzz voices; do
        $LLCOV show -instr-profile "target/coverage/prof-$tier.profdata" \
            $(ls -t target/llvm-cov-target/debug/deps/"$tier"-* | grep -v '\.d$' | head -1) \
            --format=html --output-dir "target/coverage/html-$tier" \
            --ignore-filename-regex '[^/]+/(tst|tests)/' >/dev/null
    done
    echo
    echo "✔ src/ line coverage: 100% (lines uncovered in every tier binary: 0)"
    echo "HTML reports: target/coverage/html-<tier>/index.html"
    echo "Badge JSON:   target/coverage/badges/coverage.json"

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
# no_std feature matrix → offline tests → the coverage gate (100% of
# src/ lines, per-file report printed to the terminal) → docs → one
# network-free example as an end-to-end smoke.
verify: fmt-check lint check targets-mcu test coverage doc
    just example page_scraper
    @echo
    @echo "✔ verify gate passed"

# ------------------------------------------------------------ cleanup ----

# Remove build artifacts (including the examples' export/cache output)
clean:
    cargo clean
    rm -rf times-export times-cache
