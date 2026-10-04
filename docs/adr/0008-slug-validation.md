# ADR-0008: Slug validation with deterministic placeholder fetch (F2, fixed)

- **Status:** Accepted (finding F2 fixed)
- **Date:** 2026-10
- **Decides:** How untrusted mosque identifiers become URLs.
- **Fixes:** [F2](../test-specs/README.md#red-team-findings-ledger) —
  "hostile slugs escape the mosque namespace".
- **Test:** `tst/ct/hostile_http.rs::finding_f2_hostile_slug_never_leaves_the_mosque_namespace`.

## Context

The slug comes from an untrusted search response (or a tampered local
config) and was interpolated verbatim into
`https://mawaqit.net/en/{slug}`:

- `../trap` — dot-segment traversal out of the mosque namespace;
- `victim?x=1` / `victim#frag` — query/fragment injection that swaps the
  fetched page under a legit-looking slug;
- `victim%00`, `..%2Ftrap` — encoded variants.

A hostile search response could thus point a "Grande Mosquée de Paris"
entry at attacker-chosen content and have it parsed as that mosque's
prayer times.

## Decision

Two functions in `src/client.rs`, both pure and separately testable:

1. **`is_valid_slug(slug)`** — the only shape mawaqit.net publishes:
   non-empty, ASCII lowercase letters/digits/hyphens only, no leading or
   trailing hyphen, no `--`. Anything else is invalid.
2. **`fetch_conf_data` never sends an invalid slug verbatim.** An invalid
   slug is replaced by a deterministic placeholder: `"-"` repeated
   `slug.len().clamp(4, 64)` times. The placeholder
   - cannot escape the mosque namespace (no dots, slashes, `?`, `#`),
   - preserves request-volume realism (a 40-slug scrape does not collapse
     to one identical request),
   - always 404s into `MawaqitError::MosqueNotFound` — the caller asked
     for a nonsense mosque and is told so.

`page_url` is exposed as a pure function so hostile-slug URL building is
assertable without the network; the confinement contract is pinned in
`ct/hostile_http.rs` (green) and the slug fuzzing of `page_url` totality in
`tst/fuzz/mutation.rs`.

## Consequences

**Positive**

- The URL builder's reachable space from hostile input is exactly
  `/{lang}/[a-z0-9-]+` — no traversal, no injection, no encoded variants.
- `MosqueNotFound` keeps its semantic: invalid slug ≈ nonexistent mosque.

**Negative / accepted costs**

- A *valid-looking but wrong* slug (`grande-mosquee-de-pariis`) still
  fetches and 404s — validation is structural, not a directory listing.
- Unicode slugs (if mawaqit.net ever publishes them) would be rejected and
  placeholder-fetched; revisit if the site changes slug policy.

**Alternatives rejected**

- Percent-encoding the slug: still sends attacker-chosen paths, just
  encoded — the namespace escape survives encoding.
- Validating only at `Mosque::mosque_id()`: leaves `conf_data(&str)` (the
  actual sink) unprotected; the defense must sit at the fetch.
