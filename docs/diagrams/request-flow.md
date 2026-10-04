# Request flows

The three sequences an embedder triggers. Normative text:
[`../specs/transport-and-caching.md`](../specs/transport-and-caching.md).

## 1. Mosque search

```mermaid
sequenceDiagram
    participant App
    participant C as MawaqitClient
    participant SC as searches cache (30 min)
    participant S as mawaqit.net

    App->>C: search_mosques("Paris")
    C->>C: trim → empty? ⇒ Ok([])
    C->>SC: get("paris")
    alt hit
        SC-->>C: Vec<Mosque>
        C-->>App: Ok(mosques)
    else miss
        C->>S: GET /api/2.0/mosque/search?word=Paris
        S-->>C: 200 JSON array (capped, UTF-8 enforced)
        C->>C: serde Vec<Mosque> — one bad element ⇒ whole Err
        C->>SC: insert("paris", mosques)
        C-->>App: Ok(mosques)
    end
    Note over C,S: 404 ⇒ MosqueNotFound · other !2xx ⇒ Api{status} · checked before body read
```

## 2. confData fetch (online)

```mermaid
sequenceDiagram
    participant App
    participant C as MawaqitClient
    participant PC as pages cache (6 h)
    participant D as disk::store
    participant S as mawaqit.net

    App->>C: conf_data(slug)
    C->>PC: get(slug)
    alt hit
        PC-->>App: Arc<ConfData> (no request)
    else miss
        C->>C: is_valid_slug(slug)?
        Note over C: invalid ⇒ placeholder "-"×clamp(len,4,64)<br/>(ADR-0008 / F2)
        C->>S: GET /en/{slug}
        S-->>C: 200 HTML
        C->>C: read_capped (20 MiB, UTF-8)
        C->>C: scraper::extract_conf_data
        C->>PC: insert(slug, Arc<ConfData>)
        opt disk cache enabled
            C->>D: store (atomic, best-effort)
        end
        C-->>App: Ok(conf)
    end
    Note over C,S: 404 ⇒ MosqueNotFound · !2xx ⇒ Api · parse errors ⇒ Parse / ConfDataNotFound
```

## 3. Offline fallback (`conf_data_dated`)

```mermaid
sequenceDiagram
    participant App
    participant C as MawaqitClient
    participant PC as pages cache
    participant S as mawaqit.net (down)
    participant D as snapshot dir

    App->>C: conf_data_dated(slug)
    C->>PC: get(slug)
    alt memory hit
        PC-->>App: (conf, None)
    else
        C->>S: GET /en/{slug}
        S-->>C: ✗ connection refused / timeout
        alt snapshot exists
            C->>D: load(slug) — total contract
            D-->>C: Some(fetched_at, conf)
            C-->>App: (conf, Some(fetched_at))
        else
            C-->>App: Err(original network error)
        end
    end
    Note over App: as_of = Some ⇒ UI shows "offline, as of date"
```

Key asymmetry: a **successful** fetch stores best-effort and reports
`None`; a **failed** fetch reports `Some(date)` — the date is the
"how stale is this" signal, never silently enforced as a TTL
([`../specs/offline-snapshots.md`](../specs/offline-snapshots.md)).
