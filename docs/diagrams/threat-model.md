# Threat model

What can attack this library, what stops it, and where the gaps are.
Findings workflow: [ADR-0006](../adr/0006-red-team-findings-workflow.md);
transport decisions: [ADR-0009](../adr/0009-bounded-transport.md).

## Position

The client fetches *public religious data* from a server it does not
control, using identifiers (slugs, search words) that cross the boundary
in both directions, and it persists that data to disk. There are no
credentials to steal — the assets are **integrity of displayed times**
(nobody should be able to fake prayer times), **availability** (no hang,
no OOM, no crash), and **clean degradation** (hostile input ⇒ error, not
garbage on screen).

## Surfaces → defenses

```mermaid
flowchart TD
    subgraph S1["① HTTP server (fully hostile)"]
        A1["garbage / lying JSON"] --> D1["typed strict parse ⇒ Err"]
        A2["lying Content-Length, truncation, reset"] --> D2["reqwest ⇒ Http"]
        A3["20 MiB+ body"] --> D3["response cap ⇒ Parse"]
        A4["redirect to attacker host"] --> D4["F1 fixed: Policy::none()<br/>302 surfaces as Api{302}"]
        A5["gigabyte stream"] -. "RESIDUAL F3: cap after buffering" .-> X2["OOM before cap trips"]
    end

    subgraph S2["② confData content (attacker-authored page)"]
        B1["structural garbage"] --> D5["scraper tolerance ⇒ Err/None, never panic<br/>(ut corpus + libFuzzer)"]
        B2["lying times (25:70)"] --> D6["F4 fixed: day rejected whole"]
        B3["huge +N offsets"] --> D7["clamp 0..=1440, no overflow"]
        B4["bidi/control chars in strings"] --> D8["F6 fixed: sanitize_text strips C0/C1<br/>+ bidi from free-text fields"]
        B5["XSS markup in strings"] --> D9["passes through by design —<br/>render layer must neutralize"]
        B6["deep nesting"] --> D10["serde 128-level limit ⇒ Err"]
    end

    subgraph S3["③ slugs / search words (untrusted input crossing out)"]
        C1["../, ?x, #frag, %00"] --> D11["F2 fixed: is_valid_slug +<br/>placeholder fetch, namespace-locked"]
        C2["CRLF in search word"] --> D12["percent-encoded query param<br/>(single-line request pinned)"]
    end

    subgraph S4["④ snapshot dir (attacker-writable, same user)"]
        E1["hostile file contents"] --> D13["total load ⇒ None, never panic"]
        E2["hostile slug → path escape"] --> D14["hash-derived filename<br/>(fuzz-pinned confinement)"]
        E3["cross-mosque swap"] --> D15["envelope slug verification"]
        E4["wire-tolerated shapes on disk"] --> D16["F10 fixed: struct+overlay storage"]
    end
```

Solid arrows: enforced today, pinned by tests. Dashed arrows: known gaps,
tracked as findings (currently one: F3).

## Findings on this map

| Finding | Surface | State | One-line fix |
| --- | --- | --- | --- |
| F1 | ① | Fixed | — (regression-pinned: `Policy::none()`) |
| F3 | ① | Documented residual | stream body, abort past cap |
| F6 | ② | Fixed | — (`sanitize_text`) |
| F2 | ③ | Fixed | — |
| F4 | ② | Fixed | — |
| F5 | ② (day keys) | Fixed | — (canonical key wins) |
| F10 | ④ | Fixed | — |

## Explicit non-goals

- **Content sanitation for rendering**: the parser cannot know the render
  context; XSS-neutralization belongs to the frontend (the contract is
  "never panic, never mis-shape" — pinned by
  `conf_page_hostile_xss_content_parses_structurally`).
- **TLS-level MITM**: rustls validates the host; a hostile *CA* is out of
  scope (standard library-level trust).
- **Local malware with the user's privileges**: it can do anything the app
  can; the snapshot rules only ensure it cannot make the *library* crash
  or serve one mosque's data as another's.
- **Denial of wallet / rate abuse by the embedder**: the caches bound
  request volume per slug; the embedder owns its own fetch scheduling.
