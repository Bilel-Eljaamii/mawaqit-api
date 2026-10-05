# mawaqit-api — Project Documentation

This directory is the written memory of the project: why the architecture
looks the way it does, what exactly the library promises, how every test
suite is specified, and how the pieces fit together visually.

| Folder | Contains | Read it when you want to… |
| --- | --- | --- |
| [`adr/`](adr/) | Architecture Decision Records (ADR-0001 … ADR-0013) | understand *why* a design choice was made and what it cost |
| [`specs/`](specs/) | Functional and format specifications | know *exactly* what the public API, the wire format, the calendar resolution and the snapshot layer do |
| [`test-specs/`](test-specs/) | One spec per test suite, per pyramid tier | know what every test asserts, how to run it, and what a failure means |
| [`diagrams/`](diagrams/) | Mermaid diagrams with prose | see the architecture, request flows, parsing pipeline and threat model at a glance |

## Entry points by task

- **I want to use the library** → [`specs/public-api.md`](specs/public-api.md),
  then the crate docs (`cargo doc --open`) and `examples/`.
- **I want to run on microcontrollers (MCUs) or zero-alloc** →
  [ADR-0013](adr/0013-no-std-and-mcu-support.md),
  [`specs/compact-binary-and-mcu.md`](specs/compact-binary-and-mcu.md).
- **I want to change how data is fetched or parsed** →
  [ADR-0001](adr/0001-keyless-acquisition.md) …
  [ADR-0003](adr/0003-tolerant-wire-parsing.md),
  [`specs/confdata-wire-format.md`](specs/confdata-wire-format.md),
  [`specs/calendar-resolution.md`](specs/calendar-resolution.md),
  [`diagrams/confdata-parsing.md`](diagrams/confdata-parsing.md).
- **I want to touch the offline layer** →
  [ADR-0005](adr/0005-disk-snapshot-layer.md),
  [`specs/offline-snapshots.md`](specs/offline-snapshots.md),
  [`diagrams/snapshot-lifecycle.md`](diagrams/snapshot-lifecycle.md).
- **I want to add or change a test** →
  [`test-specs/README.md`](test-specs/README.md) first, then the spec of the
  tier you are touching.
- **I found a security bug / want to report one** →
  [`diagrams/threat-model.md`](diagrams/threat-model.md) and
  [`adr/0006-red-team-findings-workflow.md`](adr/0006-red-team-findings-workflow.md).

## Conventions

- **ADRs** are numbered `NNNN-short-name.md`, immutable once accepted; a
  decision that supersedes another says so explicitly. Statuses: `Accepted`,
  `Deprecated`, `Superseded by ADR-NNNN`.
- **Specs** describe current behavior and are updated with the code in the
  same change. Every normative statement is tied to the module that
  implements it (`src/…`).
- **Test specs** document the *contract* a suite pins — inputs, invariant,
  pass criteria — not just test names. Red-team findings are referenced by
  their `F#` number from the
  [findings ledger](test-specs/README.md#red-team-findings-ledger).
- **Diagrams** are Mermaid inside Markdown (GitHub renders them natively);
  each pairs the diagram with the prose needed to read it.
- Red-team findings are numbered `F1`, `F2`, … and keep their number for the
  life of the project, whether open or fixed. Fixed findings stay documented
  as regression anchors.
