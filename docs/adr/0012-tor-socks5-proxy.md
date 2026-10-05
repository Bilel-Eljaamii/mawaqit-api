# ADR-0012: Optional Tor routing via a SOCKS5 proxy with remote DNS

- **Status:** Accepted
- **Date:** 2026-10
- **Decides:** How all mawaqit.net traffic can be routed through Tor (or
  any SOCKS5 proxy), and what the library refuses to do.
- **Refines:** [ADR-0009](0009-bounded-transport.md) (timeouts become
  constructor options; a proxy raises the defaults).
- **Introduced in:** 0.3.0 (additive — every existing signature unchanged).

## Context

mawaqit-desktop users in censored or surveilled networks have two problems
with plain HTTPS to mawaqit.net: the *destination* (which mosques they
follow) and the *DNS lookups* (which mosques exist at all) are both
visible to the local network. Tor solves both — but only if DNS resolves
**inside** the network, i.e. through the proxy (SOCKS5h semantics).

Explicit non-goals, decided up front: the library must never enable Tor
by default, never bundle or spawn a `tor` daemon, and never embed Arti.
It is a **transport capability only**: point the client at a proxy that
is already running.

## Decision

Two additive, chainable builders on `MawaqitClient` (0.3.0):

```rust
pub fn with_socks_proxy(self, addr: impl Into<String>) -> Result<Self>
pub fn with_timeouts(self, connect: Duration, request: Duration) -> Self
```

1. **`socks5h://` only.** `validate_socks_proxy` (pure, fail-fast before
   any network use) parses the address as a URL and requires the scheme
   `socks5h` exactly: plain `socks5://` resolves DNS locally and defeats
   the entire purpose; http(s) proxies are not SOCKS. Empty, garbage,
   wrong-scheme, empty-host, and path/query/fragment-carrying addresses
   are rejected with the new `MawaqitError::InvalidProxy(String)`
   (`"invalid SOCKS5 proxy address: {0}"`).
2. **Default port 9050** (system tor) when the address carries no port;
   Tor Browser users pass 9150 explicitly. Userinfo (`socks5h://user:
   pass@host:port`) is preserved for authenticated proxies; the host may
   be an IPv6 literal.
3. **Timeouts rise with a proxy**: 30 s connect / 90 s request
   (`PROXY_CONNECT_TIMEOUT` / `PROXY_REQUEST_TIMEOUT`) replace the
   10 s / 30 s defaults — unless `with_timeouts` overrode them, which
   wins in either call order.
4. **One construction path.** The reqwest client is immutable once built
   and `Inner` sits behind an `Arc`, so the builders cannot mutate it;
   instead every constructor funnels through a private
   `from_parts(api_base, site_base, disk, proxy, explicit_timeouts)`
   that (re)builds the transport. `new()`, `with_base_urls`,
   `with_disk_cache`, `with_socks_proxy` and `with_timeouts` therefore
   compose in any order (each rebuild is construction-time; caches start
   empty). The proxy is applied with `reqwest::Proxy::all(addr)?`;
   errors map through the existing `Http` variant.
5. Everything else is untouched: UA, redirect policy (F1), the 20 MB
   response cap, the TTL caches, and the offline snapshot fallback in
   `conf_data_dated` — the snapshot layer is what keeps a proxied client
   useful when the circuit itself is down.

## Consequences

**Positive**

- Privacy routing is one builder call; validation errors are typed and
  happen before any request.
- The socks5h rule makes "DNS leaked around Tor" a construction-time
  error instead of a silent property of a mistyped URL.

**Negative / accepted costs**

- `reqwest` gains the `socks` feature (pure-Rust; no new direct
  dependency, still rustls/MSVC-cross-compilable).
- With a proxy set, worst-case latency before the offline fallback
  answers grows to 90 s; accepted — the fallback triggers on any error,
  and circuit setup genuinely needs the headroom.
- `with_socks_proxy`/`with_timeouts` rebuild the transport (fresh empty
  caches). Builders are meant to be chained before first use; chaining
  mid-session drops cached pages (documented).
- There is no in-tree SOCKS5 server in the test suite, so the proxy path
  is pinned by pure validation/timeout/composition tests
  (`src/client.rs`), not by an end-to-end circuit test. Manual
  verification recipe: `tor`, then
  `just example next_prayer` with `with_socks_proxy("socks5h://127.0.0.1:9050")`.

**Alternatives rejected**

- `socks5://` support: local DNS — the one mistake that makes Tor
  pointless; rejected outright rather than warned about.
- Accepting http(s) proxy URLs: different protocol, no DNS privacy gain.
- Injecting the proxy after construction (interior mutability on `Inner`):
  would make the transport mutable mid-flight for every clone sharing the
  `Arc`; the parts-based rebuild keeps the client immutable after
  construction, like the rest of the builder surface.
- Bundling/embedding Arti: explicitly out of scope (process weight,
  bootstrap UX, and a second TLS stack).
