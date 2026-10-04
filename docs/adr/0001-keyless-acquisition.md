# ADR-0001: Keyless acquisition — public search endpoint plus page-embedded confData

- **Status:** Accepted
- **Date:** 2026-09 (initial architecture)
- **Decides:** Where all data comes from, and why the client needs no account.

## Context

mawaqit.net is a mosque prayer-times SaaS. The reference integrations use an
authenticated REST API: a mosque registers, gets an API key, and the app
ships that key. For [mawaqit-desktop](https://github.com/Bilel-Eljaamii/mawaqit-desktop)
this was a non-starter:

- every user would either share one baked-in key (a single revocation kills
  the app) or register their own (unacceptable onboarding for a tray app);
- a key in a binary is a leaked key;
- the data we need is *public*: the website itself displays it to anonymous
  visitors.

Two public surfaces serve everything an app needs:

1. `GET https://mawaqit.net/api/2.0/mosque/search?word=…` — keyword search,
   no auth, returns mosque records including the page `slug`.
2. The public mosque page `https://mawaqit.net/{lang}/{slug}` — its HTML
   embeds one JavaScript object literal, `var confData = {…}`, carrying the
   day's times, the **whole-year** adhan calendar, the iqama calendar, mosque
   metadata and announcements.

## Decision

The client is **keyless**: every datum is acquired from those two public
surfaces. `MawaqitClient::new()` takes no credentials; there is no
configuration file, nothing personal is stored or sent. The only
transport-level requirement is a browser-like `User-Agent` (the site rejects
default HTTP-client UAs) — see [ADR-0009](0009-bounded-transport.md).

## Consequences

**Positive**

- Zero onboarding, zero credential management, zero key-revocation risk.
- The library can never leak a secret because it never holds one.
- One page fetch carries a full year of adhan + iqama data, so the request
  rate is tiny (see [ADR-0002](0002-one-page-one-year.md)).

**Negative / accepted costs**

- We depend on an *embedded page artifact*, not a versioned API contract:
  if mawaqit.net renames `confData` or changes the page template, parsing
  breaks (surfaced as `MawaqitError::ConfDataNotFound`). Mitigated by
  [ADR-0002](0002-one-page-one-year.md)'s tolerant scanner and
  [ADR-0003](0003-tolerant-wire-parsing.md)'s tolerant model.
- The search endpoint is undocumented and may change shape; the model
  treats every field as optional and keeps the rest verbatim
  (`Mosque::extra`).
- No authenticated endpoints: features that need an account (editing mosque
  data) are out of scope by construction.

**Alternatives rejected**

- *Official API + API key*: rejected for the onboarding/revocation reasons
  above; also the API returns per-day data, multiplying requests.
- *HTML DOM scraping of rendered times*: slower, depends on CSS layout,
  which changes far more often than the embedded JSON.
