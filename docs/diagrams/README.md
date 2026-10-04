# Diagrams

Mermaid diagrams (GitHub renders them inline) with the prose needed to
read them. Each pairs with a spec or ADR that carries the normative text.

| Diagram | Shows | Pair with |
| --- | --- | --- |
| [`system-architecture.md`](system-architecture.md) | modules, layers, data ownership | [`../specs/README.md`](../specs/README.md) |
| [`request-flow.md`](request-flow.md) | sequence: search, confData fetch, cache, offline fallback | [`../specs/transport-and-caching.md`](../specs/transport-and-caching.md) |
| [`confdata-parsing.md`](confdata-parsing.md) | the confData extraction pipeline and brace-scanner states | [`../specs/confdata-wire-format.md`](../specs/confdata-wire-format.md) |
| [`calendar-resolution.md`](calendar-resolution.md) | row-shape decision tree + iqama resolution | [`../specs/calendar-resolution.md`](../specs/calendar-resolution.md) |
| [`snapshot-lifecycle.md`](snapshot-lifecycle.md) | snapshot state machine and store/load flows | [`../specs/offline-snapshots.md`](../specs/offline-snapshots.md) |
| [`error-taxonomy.md`](error-taxonomy.md) | which failure produces which `MawaqitError` | [`../specs/public-api.md`](../specs/public-api.md) |
| [`test-pyramid.md`](test-pyramid.md) | the test tiers and what gates what | [`../test-specs/README.md`](../test-specs/README.md) |
| [`threat-model.md`](threat-model.md) | attack surfaces → defenses → finding status | [ADR-0006](../adr/0006-red-team-findings-workflow.md) |

Convention: diagrams document *today's behavior*. Open findings would be
drawn as dashed edges so it is visible in the picture what is not yet
guaranteed — currently the only dashed edge is F3 (the documented
transport residual).
