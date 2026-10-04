# Snapshot lifecycle

States of one mosque's offline snapshot and the flows between them.
Normative text: [`../specs/offline-snapshots.md`](../specs/offline-snapshots.md);
decision: [ADR-0005](../adr/0005-disk-snapshot-layer.md).

## State machine

```mermaid
stateDiagram-v2
    [*] --> Absent : dir exists, no file
    Absent --> Storing : successful network fetch (disk cache enabled)
    Storing --> Present : tmp written + atomic rename
    Storing --> Absent : any IO/serialize failure (best-effort — fetch still OK)

    Present --> Present : successful fetch refreshes (rewrite, atomic)
    Present --> ServingOffline : fetch fails, load(slug) OK
    ServingOffline --> Present : next successful fetch refreshes
    Present --> Absent : load fails (hostile or corrupt file)<br/>treated as no snapshot
    ServingOffline --> Absent : (only via external corruption)
    Absent --> [*]
```

Every arrow into `Absent` from a *load* is the total-load contract: a
hostile file cannot crash or poison — it is just gone.

## Store flow

```mermaid
flowchart TD
    S["store(dir, slug, conf)"] --> T1["conf_to_storage:<br/>struct serialized (raw nulled)<br/>+ raw extras or_insert'ed<br/>modeled keys always win"]
    T1 -- "serialize fails ⇒ None" --> Z["nothing written"]
    T1 --> T2["envelope v1:<br/>{version, mosque_slug, fetched_at, conf}"]
    T2 --> T3["write dir/{hash:016x}.json.tmp"]
    T3 -- "IO error ⇒ None" --> Z
    T3 --> T4["rename tmp → json (atomic)"]
    T4 --> Y["Some(fetched_at)"]
```

## Load flow (total function)

```mermaid
flowchart TD
    L["load(dir, slug)"] --> A{"file readable?"}
    A -- no --> N["None"]
    A -- yes --> B{"parses as envelope v1?"}
    B -- no --> N
    B -- yes --> C{"version == 1?"}
    C -- no --> N
    C -- yes --> D{"mosque_slug == requested slug?"}
    D -- "no (never serve another mosque)" --> N
    D -- yes --> E{"conf deserializes into ConfData?"}
    E -- no --> N
    E -- yes --> P["Some(fetched_at, conf)"]
```

## Why the storage value is neither raw nor bare struct

```mermaid
flowchart LR
    subgraph bad1["✗ store raw verbatim"]
        R1["wire shapes (nulls in rows,<br/>numeric name) written back"] --> R2["strict load rejects ⇒<br/>snapshot never loads (F10)"]
    end
    subgraph bad2["✗ store bare struct"]
        B1["unmodeled fields lost"] --> B2["silent data loss vs live path"]
    end
    subgraph good["✓ store struct + raw extras"]
        G1["modeled keys win over raw"] --> G2["loads under strict serde;<br/>unmodeled fields survive"]
    end
```

Regression anchors: `src/disk.rs::messy_wire_shapes_in_raw_never_break_loading`,
`roundtrip_survives_a_populated_raw_object`,
`tst/fuzz/mutation.rs::finding_f10_snapshot_roundtrips_wire_tolerated_shapes`.
