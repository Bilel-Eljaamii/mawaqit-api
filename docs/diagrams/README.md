# Diagrams

PlantUML sources (`.puml`) — one diagram per file, prose folded into
`note`/`legend` blocks. Each pairs with a spec or ADR that carries the
normative text.

Rendering: GitHub does not render PlantUML inline. Use any of:

- the PlantUML jar: `plantuml docs/diagrams/*.puml` (writes `.svg`/`.png` next to sources),
- the VS Code PlantUML extension (preview + export),
- drag-and-drop onto <https://www.plantuml.com/plantuml> or
  <https://kroki.io> (or embed as
  `https://plantuml.com/plantuml/svg/<deflated-source>`).

The rendered form is a view, never the source of truth — the `.puml`
files are.

| Diagram | Shows | Pair with |
| --- | --- | --- |
| [`system-architecture.puml`](system-architecture.puml) | modules, DDD layers, data ownership | [`../specs/README.md`](../specs/README.md), [ADR-0014](../adr/0014-ddd-layering-and-plantuml.md) |
| [`feature-tiers.puml`](feature-tiers.puml) | cargo feature tiers × layers × gate | [ADR-0013](../adr/0013-no-std-and-mcu-support.md), [ADR-0014](../adr/0014-ddd-layering-and-plantuml.md) |
| [`mcu-pipeline.puml`](mcu-pipeline.puml) | pre-flash packaging → MQTC → zero-alloc MCU runtime | [`../specs/compact-binary-and-mcu.md`](../specs/compact-binary-and-mcu.md) |
| [`compact-binary-layout.puml`](compact-binary-layout.puml) | MQTC header + both record layouts | [`../specs/compact-binary-and-mcu.md`](../specs/compact-binary-and-mcu.md) |
| [`request-flow-search.puml`](request-flow-search.puml) | sequence: search, cache, caps | [`../specs/transport-and-caching.md`](../specs/transport-and-caching.md) |
| [`request-flow-confdata.puml`](request-flow-confdata.puml) | sequence: confData fetch, slug policy, snapshot store | [`../specs/transport-and-caching.md`](../specs/transport-and-caching.md) |
| [`request-flow-offline-fallback.puml`](request-flow-offline-fallback.puml) | sequence: offline fallback + staleness date | [`../specs/offline-snapshots.md`](../specs/offline-snapshots.md) |
| [`confdata-parsing-pipeline.puml`](confdata-parsing-pipeline.puml) | confData extraction pipeline | [`../specs/confdata-wire-format.md`](../specs/confdata-wire-format.md) |
| [`confdata-brace-scanner.puml`](confdata-brace-scanner.puml) | brace-scanner states | [`../specs/confdata-wire-format.md`](../specs/confdata-wire-format.md) |
| [`calendar-row-shapes.puml`](calendar-row-shapes.puml) | row-shape decision tree | [`../specs/calendar-resolution.md`](../specs/calendar-resolution.md) |
| [`calendar-iqama-resolution.puml`](calendar-iqama-resolution.puml) | iqama resolution + hostile outcomes | [`../specs/calendar-resolution.md`](../specs/calendar-resolution.md) |
| [`calendar-month-extraction.puml`](calendar-month-extraction.puml) | month/date paths, F5 dedupe | [`../specs/calendar-resolution.md`](../specs/calendar-resolution.md) |
| [`snapshot-lifecycle-states.puml`](snapshot-lifecycle-states.puml) | snapshot state machine | [`../specs/offline-snapshots.md`](../specs/offline-snapshots.md) |
| [`snapshot-store-flow.puml`](snapshot-store-flow.puml) | store flow (atomic, best-effort) | [`../specs/offline-snapshots.md`](../specs/offline-snapshots.md) |
| [`snapshot-load-flow.puml`](snapshot-load-flow.puml) | load flow (total function) | [`../specs/offline-snapshots.md`](../specs/offline-snapshots.md) |
| [`snapshot-storage-choice.puml`](snapshot-storage-choice.puml) | why struct + raw extras (F10) | [ADR-0005](../adr/0005-disk-snapshot-layer.md) |
| [`error-taxonomy.puml`](error-taxonomy.puml) | failure → `MawaqitError` variant | [`../specs/public-api.md`](../specs/public-api.md) |
| [`test-pyramid.puml`](test-pyramid.puml) | tiers and what gates what | [`../test-specs/README.md`](../test-specs/README.md) |
| [`threat-model.puml`](threat-model.puml) | attack surfaces → defenses → findings | [ADR-0006](../adr/0006-red-team-findings-workflow.md) |

Convention: diagrams document *today's behavior*. Open findings and
known gaps are drawn as dashed edges (`.->` in PlantUML) so the picture
shows what is not yet guaranteed — currently only F3 in
[`threat-model.puml`](threat-model.puml).
