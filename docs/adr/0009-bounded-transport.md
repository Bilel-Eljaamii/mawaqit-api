# ADR-0009: Bounded transport — response cap, timeouts, browser User-Agent

- **Status:** Accepted (with documented residual, F3)
- **Date:** 2026-09, revised 2026-10 after finding F3
- **Decides:** Transport-level limits and identity of the HTTP client.
- **Refined by:** [ADR-0012](0012-tor-socks5-proxy.md) — the timeouts are
  constructor options ([`MawaqitClient::with_timeouts`]), and a SOCKS
  proxy raises the defaults to 30 s / 90 s.
- **Test:** `tst/ct/hostile_http.rs::finding_f3_documented_cap_rejects_oversized_response`,
  `oversized_body_is_rejected_by_the_cap`, `truncated_body_and_connection_reset_are_errors`.

## Context

The server on the other end is not assumed benign (see the threat model).
Transport must guarantee the process cannot hang on a slow/held-open
connection and cannot balloon memory on a huge body. Separately, mawaqit.net
rejects requests without a browser-ish `User-Agent`.

## Decision

`MawaqitClient::with_base_urls` builds one `reqwest::Client` with:

| Control | Value | Rationale |
| --- | --- | --- |
| `User-Agent` | Firefox 132 on Linux (browser-ish string) | the site rejects default client UAs |
| `timeout` | 30 s | give up rather than hang the caller (desktop loop shares this client) |
| `connect_timeout` | 10 s | fail fast on dead networks so the snapshot fallback engages quickly |
| Response cap | 20 MiB (`MAX_RESPONSE_BYTES`) | a real mosque page is ~60 KB; anything near the cap is hostile. Exceeding it → `MawaqitError::Parse` mentioning "cap" |

All bodies (search JSON, page HTML) go through `read_capped`, which also
enforces UTF-8 on the result — a non-UTF-8 body is a `Parse` error, never a
lossy reinterpretation of attacker bytes.

Redirects: the client pins `redirect::Policy::none()` (finding **F1**,
fixed) — a 302, same-origin or not, is never followed and surfaces as
`Api { status: 302 }`. Nothing a redirect chain could do can swap the page
content out from under a slug.

## Consequences

**Positive**

- Worst-case transport cost per request is bounded: ≤ 30 s, ≤ 20 MiB
  buffered, and the snapshot fallback engages as soon as the client errors.
- The UA is a constant in one place; tests assert request *behavior*
  (paths, single-line request targets — see the CRLF-smuggling test)
  rather than header cosmetics.

**Negative / documented residual (F3)**

- The cap is applied **after** `response.bytes()` has buffered the whole
  body: a hostile server streaming gigabytes can OOM the app *before* the
  cap trips. Fix direction: stream through `bytes_stream()` and abort once
  the running total exceeds 20 MiB. The test documents the cap's
  rejection behavior; the buffering gap is the open residual.
- The 30 s timeout is generous for the desktop UI but also the worst-case
  latency before the offline fallback answers. Accepted: the fallback is
  triggered by *any* error, so perceived latency ≈ min(timeout, network
  failure time).

**Alternatives rejected**

- Content-Length-based pre-check: hostile servers lie about
  `Content-Length` (pinned by the truncated-body test), so a header check
  is advisory, not a guarantee.
- No cap at all: an easy memory DoS against every embedder.
