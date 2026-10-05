# Spec: Transport, identity, and caching

Normative description of `src/client.rs` and `src/cache.rs`: what goes on
the wire and how repeated requests are served.

## Endpoints

| Purpose | URL | Auth |
| --- | --- | --- |
| Mosque search | `GET {api_base}/2.0/mosque/search?word=…` | none |
| Mosque page | `GET {site_base}/en/{slug}` | none |

- Defaults: `api_base = https://mawaqit.net/api`,
  `site_base = https://mawaqit.net`, page language pinned to `en`.
- `with_base_urls(api_base, site_base)` replaces both — the seam hostile
  tests use to point the client at a local mock.

## Request identity and limits

| Control | Value | Constant |
| --- | --- | --- |
| `User-Agent` | `Mozilla/5.0 (X11; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0` | `USER_AGENT` — the site rejects non-browser UAs |
| Total timeout | 30 s (90 s while a SOCKS proxy is set) | `REQUEST_TIMEOUT` / `PROXY_REQUEST_TIMEOUT` |
| Connect timeout | 10 s (30 s while a SOCKS proxy is set) | `CONNECT_TIMEOUT` / `PROXY_CONNECT_TIMEOUT` |
| Response size cap | 20 MiB | `MAX_RESPONSE_BYTES` |
| Redirect policy | `redirect::Policy::none()` — 302s are never followed; a redirect surfaces as `Api { status }` (finding F1, fixed) | — |

Both timeouts are constructor options (`with_timeouts(connect, request)`);
without it the defaults apply, raised automatically when a proxy is set
(see below and [ADR-0012](../adr/0012-tor-socks5-proxy.md)).

All bodies go through `read_capped`: full buffering, then
`len > 20 MiB ⇒ Parse("response of N bytes exceeds the … cap")`, then
UTF-8 enforcement (non-UTF-8 ⇒ `Parse`). Known residual: the cap applies
**after** buffering (finding F3) — see [ADR-0009](../adr/0009-bounded-transport.md).

The search word travels as a reqwest query parameter — percent-encoded, so
CRLF/header-injection words produce a single-line request target (pinned:
`ct/hostile_http.rs::search_query_is_percent_encoded_crlf_never_reach_the_wire`).

## SOCKS5 / Tor routing (opt-in)

`with_socks_proxy(addr) -> Result<Self>` routes **all** traffic through a
SOCKS5 proxy with remote DNS. `with_timeouts(connect, request)` overrides
the timeouts; both compose with every other builder in any order (all
constructors funnel through one private parts-based path that rebuilds
the transport — chaining rebuilds with fresh, empty caches).

Validation (`validate_socks_proxy`, pure, fail-fast before any network
use; violations ⇒ `MawaqitError::InvalidProxy`):

| Rule | Detail |
| --- | --- |
| Scheme is `socks5h` exactly | `socks5://` (local DNS — defeats Tor), `http://`, `https://`, no-scheme, garbage ⇒ rejected |
| Host non-empty | `socks5h://` alone ⇒ rejected; IPv6 literals and `user:pass@` userinfo are accepted |
| No path/query/fragment | a proxy address is `scheme://host[:port]` only |
| Missing port ⇒ 9050 | the system tor daemon; Tor Browser users pass 9150 explicitly |

Applied with `reqwest::Proxy::all(validated)?` on the builder (build
failures map through the `Http` variant). The library never enables a
proxy by default and never starts or bundles a Tor daemon.

## Slug handling on the wire

`fetch_conf_data`:

- `is_valid_slug(slug)` ⇒ fetch `…/en/{slug}` as-is.
- Invalid ⇒ fetch `…/en/` + `"-" × clamp(len, 4, 64)` (deterministic
  placeholder, [ADR-0008](../adr/0008-slug-validation.md)) — stays inside
  the namespace, 404s into `MosqueNotFound`.

## Caches (`TtlCache`, `src/cache.rs`)

| Instance | Key | Value | TTL |
| --- | --- | --- | --- |
| `pages` | slug (as given) | `Arc<ConfData>` | 6 h |
| `searches` | `word.to_lowercase()` | `Vec<Mosque>` | 30 min |

Behavior:

- Lazy expiry on `get` (`elapsed >= ttl` ⇒ evict + miss); zero TTL ⇒
  immediate expiry (unit-pinned).
- Mutex poisoning is swallowed (poisoned ⇒ miss/no-op), never propagated.
- `invalidate(Some(slug))` drops one page entry; `invalidate(None)` clears
  all pages (search cache has no invalidation — 30 min is short enough).
- Search contract details: trimmed-empty word ⇒ `Ok(vec![])` with **no**
  network and **no** cache write; success caches; error does **not**
  poison the cache.
- Cache isolation: two slugs never share an entry, and a cache hit issues
  no request (pinned: `ct/hostile_http.rs::cache_never_confuses_two_slugs`).

## Status-code mapping

| Response | `search_mosques` | `conf_data` |
| --- | --- | --- |
| 404 | `MosqueNotFound(word)` | `MosqueNotFound(slug)` |
| other non-2xx (including 3xx — redirects are never followed) | `Api { status, url }` | `Api { status, url }` |
| 2xx | body must parse as `Vec<Mosque>` | body must contain `confData = {…}` |

Note the check order: status first (before the body is read), then capped
body read, then parse — a 500 with a JSON body is an `Api` error, not a
parse error.

## Offline fallback flow

See [`offline-snapshots.md`](offline-snapshots.md#client-integration-decision-flow)
for the diagram. Summary: memory hit ⇒ serve; else fetch ⇒ (ok) store
snapshot best-effort, serve; (err) snapshot? serve `(conf, Some(date))` :
re-raise the network error. Without `with_disk_cache`, behavior is
identical to a cache-only client (pinned:
`ct/disk_cache.rs::without_disk_cache_the_client_behaves_as_before`).

## Concurrency

`MawaqitClient` is `Clone + Send + Sync` (internals behind `Arc`); clones
share one HTTP connection pool, one cache, and the same snapshot directory.
No request de-duplication: concurrent first fetches of one slug each hit
the wire (accepted — see [ADR-0004](../adr/0004-in-process-ttl-cache.md)).
