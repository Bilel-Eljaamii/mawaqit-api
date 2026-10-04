# Test Spec: `e2e/world_tour.rs` — the hostile world tour

- **Tier:** end-to-end (`cargo test --test e2e -- --ignored --nocapture`,
  or `just live`). **Hits the live mawaqit.net**; takes minutes.
- **Target:** the whole production path — search-independent page fetch →
  parse → calendar resolution — against real mosques on five continents.
- **Posture:** real-world data *is* the attacker. Layouts differ (normal /
  imsak / legacy), slugs die, fields go missing, some mosques publish
  malformed times.

## Fixture: `tst/e2e/fixtures/world_mosques.json`

Hand-curated list of 100+ entries:

```json
{ "continent": "Europe", "city": "Paris",
  "name": "DŽEMAT BOSNIAQUES PARIS",
  "slug": "dzemat-paris-le-pre-saint-gervais-93500-france" }
```

Rules for the fixture (enforced at test start):
- ≥ 100 entries; every entry has a non-empty `slug`.
- Only mosques with a working public page; developer-owned mosques are
  left out (already covered elsewhere).
- Adding a mosque = append an entry; no code change.

## Per-mosque procedure (`hostile_world_tour`)

One shared `MawaqitClient`; a 300 ms pause every 10 mosques to stay polite
with the site.

1. **Fetch** `conf_data(slug)`.
   - Err containing "not found"/"404" ⇒ recorded as **dead slug** (upstream
     data-quality issue, tolerated in bounded numbers).
   - Other Err ⇒ **HARD failure**.
2. **Calendar completeness** — `conf.calendar.len() == 12`, else HARD
   failure.
3. **Today resolves** — `times_for_date(Local::now())`; every surfaced
   adhan time must satisfy `valid_hhmm`, else HARD failure.
4. **Soft sanity (anomalies, not failures)** — sunrise within
   `[fajr, dhuhr]`; `dhuhr < asr < maghrib < isha`. Violations are data
   oddities worth seeing, not code bugs.
5. **Iqama** — when present, all five resolved times must be strict
   `HH:MM`, else HARD failure; counted otherwise.
6. **Month extraction** — `month(slug, 1)` and `month(slug, 12)` must both
   succeed, else HARD failure.

## Pass criteria (the assertions)

| Assertion | Threshold | Rationale |
| --- | --- | --- |
| `dead_slugs.len() * 10 <= mosques.len()` | ≤ 10 % dead | search indexes pages that no longer exist; more than 10 % means the fixture or the site rotted |
| `failures.is_empty()` | zero | the app must never fail to parse or crash on real mosque data — this is the suite's whole point |

Output ends with a summary block (parsed count, iqama coverage, dead
slugs, anomalies, hard failures) — paste it into the release notes
([HIL](../hil/live-campaigns.md)).

## Failure interpretation

- A **hard failure** is always a parser/calendar bug (or a site contract
  change): reproduce with the mosque's slug against `parse_page`, add the
  hostile shape to the ut corpus, fix, keep the world-tour green.
- **Anomalies** are filed as data observations; no code change is expected
  unless they reveal a systematic layout the parser mishandles.
- Many dead slugs at once ⇒ refresh the fixture (mosques move/get renamed);
  the client behavior is correct (404 ⇒ `MosqueNotFound`).
