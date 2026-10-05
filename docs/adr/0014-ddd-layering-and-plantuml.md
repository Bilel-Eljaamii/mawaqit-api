# ADR-0014: DDD layering for no_std correctness and the PlantUML diagram convention

- **Status:** Accepted
- **Date:** 2026-10-05
- **Decides:** The domain/infrastructure layering that makes ADR-0013's
  feature tiers actually compile, the per-format MQTC encoder rule, and
  the replacement of the Mermaid-in-Markdown diagram convention with
  PlantUML sources.

## Context

ADR-0013 declared the feature-tier architecture (`std` / `alloc` /
`heapless`) but the tree never satisfied it: every non-default feature
combination failed to compile, `src/compact.rs` did not exist, and
`just verify` (default features only) could not see any of it. Turning
the tiers real surfaced two structural questions the ADR had left open,
plus a documentation-convention decision the maintainer settled by
mandate:

1. **Which code must compile in which tier?** Without a layering rule,
   "gate the std stuff" degenerates into per-line `#[cfg]` noise (the
   first attempt had `client.rs` compiling in `core` with five gated
   imports). Domain-Driven Design gives the criterion: the *domain*
   (time semantics, slug policy, wire parsing, calendar resolution, the
   MQTC codec) is the part that must run everywhere — including bare
   metal — while *infrastructure* (HTTP, filesystem, TTL caches, the
   packer CLI) only exists where an OS does.
2. **What validates a record that one format cannot express?** The
   original MQTC spec had a single day-record encoder validating both
   formats; a day only expressible in direct format (shurouq before
   fajr, or an iqama offset past one byte) was rejected even when
   packing direct — and the spec's 12-byte "delta" layout itself could
   not encode real calendars at all (dhuhr − shurouq alone exceeds one
   byte at every latitude).
3. **Diagrams**: the repo's 8 architecture diagrams were Mermaid blocks
   inside `.md` files. The maintainer mandated PlantUML sources.

## Decision

| Area | Decision | Rationale |
| :--- | :--- | :--- |
| **Domain–core** (`core`, always compiled) | `time.rs` (`parse_hhmm`, `is_displayable_hhmm`, `minutes_between`), `slug.rs` (`is_valid_slug`), the voice catalog consts | Pure policy with no allocator; this is what "keep the pure helpers available to no_std" (ADR-0013) concretely means. |
| **Domain–alloc** | `models.rs`, `calendar.rs`, `scraper.rs`; `serde_json` becomes an **optional** dep wired into the `alloc` feature | Wire parsing and calendar resolution are domain policy (ADR-0003) but need collections and JSON; serde_json cannot compile without an allocator at all. |
| **Domain–zero-alloc codec** (`heapless`) | `compact.rs` — view (`CompactCalendarView`, zero-copy, CRC-checked) + builder (`CompactCalendarBuilder`, cfg `alloc`) | The runtime side never allocates; the packer side may. One module, two halves, each cfg-honest. |
| **Infrastructure–std** | `client.rs`, `cache.rs`, `disk.rs`, `download_voice`, `pack_for_mcu` | Gated `#[cfg(feature = "std")]` at module level — no intra-file cfg noise. |
| **Layer rule** | Domain never imports infrastructure; every module compiles inside its dependency tier (`core < alloc < std`); tier membership is asserted by the build matrix | Makes ADR-0013's "OS isolation" row mechanical instead of aspirational. |
| **Per-format encoders** | `encode_day_direct` / `encode_day_fajr_rel` — each validates only its own invariants | A format's limits are its own; the direct format carries rollover and non-ascending adhan explicitly, the fajr-relative format rejects them at pack time with a typed error. |
| **Fajr-relative layout replaces the 12-byte delta** | 20 B/day: base fajr u16 + 5 adhan offsets u16 (minutes after fajr) + 5 iqama offsets u8 (minutes after own adhan, `0xFF` = absent, ≤ 254) | The 12-byte layout was unimplementable (see Context); 20 B keeps the zero-RAM, register-decode property and still shrinks a year to 7 324 B vs 8 784 B direct. |
| **Error taxonomy** | `CompactError` gains `TimeOutOfRange`, `DeltaOverflow`, `IqamaOffsetOverflow`, `TooManyDays`; `DateOutOfBounds` never existed — an out-of-range lookup is `None` | Pack-time rejection is typed and actionable; a lookup miss is not an error (total-parsing invariant). |
| **Diagrams are PlantUML** | `docs/diagrams/*.puml`, one diagram per file; prose lives in `note`/`legend` blocks; the per-topic `.md` diagram files are deleted | Mandated by the maintainer. Trade-off accepted: GitHub does not inline-render PlantUML; the README lists render paths (jar, editor, plantuml.com/kroki). The `.puml` file is the source of truth, any rendering is a view. |
| **Gate extension** | `just targets-mcu` — `{thumbv7em-none-eabihf, riscv32imc-unknown-none-elf, host} × {std, alloc, heapless}` `cargo check` — runs inside `just verify` and CI | A feature tier that is not compiled by the gate does not exist (the entire C-class of the 2026-10-05 review). |

## Consequences

**Positive**

- The no_std story is checkable, not narrated: four commits take the
  tree from "every tier broken" to "5 configurations × 2 targets green,
  pinned by CI".
- `is_valid_slug`/`minutes_between` reach the heapless tier as promised
  in the ADR-0013 plan, instead of being accidentally std-only.
- MQTC rejections are typed per format; hostile packer input cannot
  silently wrap.
- Diagrams render from reviewable, diffable text with the prose riding
  inside the diagram.

**Negative / accepted costs**

- PlantUML needs a renderer (no inline GitHub rendering) — mitigated by
  README render instructions.
- `serde_json` optionality means the alloc feature pulls a dependency
  the std tier used to get implicitly — behavior is identical, the lock
  graph is marginally more complex.
- Two MQTC record layouts must be tested separately (they are — 13 ut
  tests + a mutation-fuzz seed cover both).

**Alternatives rejected**

- **Workspace split (`mawaqit-core` + `mawaqit-client`)**: rejected in
  ADR-0013; the layer rule achieves the same decoupling without repo
  churn.
- **Keeping the 12-byte delta with u16 gaps**: 12 B of adhan alone
  leaves no room for iqama; 17 B pads to 20 B anyway, so the simpler
  fajr-relative layout wins.
- **Mermaid kept alongside PlantUML**: two diagram conventions is one
  more than zero.
