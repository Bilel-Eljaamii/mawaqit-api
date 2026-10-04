# Test Spec: `ct/hostile_http.rs` — the server is the attacker

- **Tier:** component (`cargo test --test ct`), offline, deterministic —
  real sockets, fake server, no live site.
- **Target:** `MawaqitClient` through its real HTTP boundary.
- **Contract:** against any server behavior, the client answers with a
  clean `Err` or well-formed data — never a panic, never a hang, never
  following the attacker somewhere else.

## Mock server (`spawn_mock`)

Raw `TcpListener` on `127.0.0.1:0` (ephemeral port), serving **canned byte
responses**:

- Route table: request path (query stripped) → exact response bytes;
  unrouted paths get a 404.
- Every request's full target line is logged (`mock.requests()`) so tests
  can assert what actually went on the wire (used by the CRLF-smuggling
  and slug-confinement tests).
- Responses are hand-built HTTP/1.1 bytes (`http_bytes`, `ok_json`,
  `ok_html`, `redirect_to`) — the mock can lie at any layer: status,
  Content-Type, Content-Length, body, truncation.

The client under test is `MawaqitClient::with_base_urls(base, base)` — the
production test seam. `trap_page()` is a valid mosque page whose confData
is fully attacker-authored (`"EVIL TRAP MOSQUE"`, evil image URL).

## Contract tests (always green)

### Search
| Test | Input | Contract |
| --- | --- | --- |
| `search_garbage_bodies_are_errors_never_panics` | `[1,2,` · `{"mosques":[]}` · `null` · `"array?"` · `12345` · empty · WAF HTML · half-object | every body ⇒ `Err` (no partial results) |
| `search_error_statuses_surface_as_errors` | 404 / 403 / 429 / 500 / 503 | 404 ⇒ `MosqueNotFound`; others ⇒ `Api` |
| `search_bounded_input_stays_bounded` | 10 000 minimal entries; then 100 entries + one `null` | 10k legal entries parse fully (the 20 MB cap bounds hostility, not entry count); one hostile element poisons the whole response ⇒ `Err` |
| `search_non_utf8_and_bom_bodies_are_rejected` | UTF-16LE bytes; UTF-8 BOM + `[]` | both ⇒ `Parse` errors (BOM is not JSON) |
| `search_query_is_percent_encoded_crlf_never_reach_the_wire` | word = `paris\r\nX-Injected: 1\r\n\r\nGET /admin HTTP/1.1` | exactly **one** request logged; its target is a single line — no request smuggling |

### confData page
| Test | Input | Contract |
| --- | --- | --- |
| `conf_page_hostile_xss_content_parses_structurally` | name/jumua/image/announcement carrying XSS payloads | parser accepts hostile *strings* (sanitation is the render layer's job) but never panics or mis-shapes; name round-trips verbatim |
| `conf_page_garbage_html_is_conf_data_not_found` | no confData · empty · `confData = not json` · array literal | all ⇒ `ConfDataNotFound` |
| `conf_page_lax_content_type_is_documented_behavior` | valid page as `text/plain` / `application/octet-stream` / `weird/vendor-type` | parses fine — Content-Type is not checked (deliberate, pinned) |
| `truncated_body_and_connection_reset_are_errors` | headers promise 10 000 bytes, 27 sent, socket closed | ⇒ `Err` (lying Content-Length can't produce partial data) |
| `oversized_body_is_rejected_by_the_cap` | 20 MiB + 1 of `A` | ⇒ `Err` mentioning the cap |
| `redirect_loop_stays_bounded_by_the_http_client` | `/en/loop` → itself | redirects are never followed (`Policy::none`) ⇒ `Api { 302 }` ⇒ `Err` — no loop is even possible |
| `cache_never_confuses_two_slugs` | two distinct pages, then a repeat | two requests total; repeat served from cache; names never swap |

## Findings

### F1 — FIXED, green: `finding_f1_cross_origin_redirect_is_not_followed`
`/en/victim` → `302 http://attacker.invalid/en/trap` (trap page served).
Contract: the 302 itself surfaces as `Api { status: 302 }` — no second
request is made, whatever the target. The reqwest client pins
`redirect::Policy::none()` in `with_base_urls`
([ADR-0009](../../adr/0009-bounded-transport.md)).

### F2 — FIXED, green: `finding_f2_hostile_slug_never_leaves_the_mosque_namespace`
Slugs `../trap`, `..%2Ftrap`, `%2e%2e/trap`, `victim?x=1`, `victim#frag`,
`victim/extra`, `victim%00`: each request must stay under `/en/` without
`..` after decoding, and each must yield `Err` (placeholder-fetch → 404 →
`MosqueNotFound`). Regression anchor for [ADR-0008](../../adr/0008-slug-validation.md).

### F3 — DOCUMENTED RESIDUAL, green:
`finding_f3_documented_cap_rejects_oversized_response`
Proves the 20 MB cap rejects an oversized body. The *finding* is that the
cap is applied after `response.bytes()` buffered everything — a streaming
hostile server can OOM before the cap trips. Fix direction: `bytes_stream()`
with a running total ([ADR-0009](../../adr/0009-bounded-transport.md)).
