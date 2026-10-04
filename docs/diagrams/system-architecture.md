# System architecture

One crate, seven modules, three data stores. Everything above `models` is
tainted by the network; everything below the scraper/calendar boundary
promises typed, validated data.

```mermaid
flowchart TB
    subgraph embedder["Embedding app (mawaqit-desktop, examples/)"]
        UI["UI / tray / alarms"]
    end

    subgraph lib["mawaqit-api (keyless client)"]
        direction TB
        API["Public API<br/>MawaqitClient · month_times · times_for_date<br/>parse_page · page_url · is_valid_slug · minutes_between"]

        subgraph net["network layer"]
            CLIENT["client.rs<br/>fetch orchestration · slug validation · response cap"]
            CACHE["cache.rs<br/>TtlCache · pages 6h · searches 30min"]
        end

        subgraph pure["pure core (no I/O)"]
            SCRAPER["scraper.rs<br/>find confData · balanced JSON scan"]
            CALENDAR["calendar.rs<br/>row layouts · iqama resolution · HH:MM contract"]
        end

        subgraph state["state layer"]
            DISK["disk.rs<br/>snapshot envelope · hashed filename · atomic rename"]
            MODELS["models.rs<br/>Mosque · ConfData · times types"]
            ERROR["error.rs<br/>MawaqitError"]
        end
    end

    MW["mawaqit.net<br/>search API + mosque pages"]

    UI --> API
    API --> CLIENT
    CLIENT <--> CACHE
    CLIENT -->|"HTTP (30s timeout, 20MiB cap, browser UA)"| MW
    CLIENT --> SCRAPER
    CLIENT <-->|"best-effort store / fallback load"| DISK
    SCRAPER --> MODELS
    API --> CALENDAR
    CALENDAR --> MODELS
    SCRAPER --> ERROR
    CALENDAR --> ERROR
    CLIENT --> ERROR
    DISK --> MODELS
```

Reading guide:

- **`client.rs`** owns all I/O decisions: which URL, which limits, cache
  consult, snapshot fallback ([`request-flow.md`](request-flow.md)).
- **`scraper.rs` + `calendar.rs` are pure** — that is what makes the whole
  hostile pyramid (ut tier, fuzz targets) able to attack the exact
  production path without sockets.
- **`disk.rs`** is the only filesystem writer, and only when the embedder
  opts in with `with_disk_cache`.
- **`models.rs`** is the shared currency: scraper produces it, calendar
  consumes/produces it, disk round-trips it, the embedder sees it.
