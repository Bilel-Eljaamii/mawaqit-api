# Test Spec: `e2e/smoke.rs` — live keyless-path smoke test

- **Tier:** end-to-end (`cargo test --test e2e -- --ignored`), **hits the
  live mawaqit.net**, ~3 s. Moved out of `src/client.rs` when tests left
  `src/`.
- **Target:** the whole keyless path once — search endpoint, page fetch,
  parse, today view, month extraction.
- **Contract:** search for "Paris" returns at least one mosque with a slug;
  today's times resolve; January has days.

## Tests

### `live_search_and_calendar` (`#[ignore]`)
Search → first slug-carrying result → `today` (adhan + iqama printed) →
`month(slug, 1)` non-empty.

## Run

```sh
cargo test --test e2e -- --ignored --nocapture   # smoke + world tour
just live                                        # same, via just
```

A failure means the keyless acquisition path is broken against the real
site (layout change, endpoint change, transport regression). The fast
first check before committing to the full 131-mosque world tour.
