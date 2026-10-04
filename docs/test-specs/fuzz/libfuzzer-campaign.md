# Test Spec: libFuzzer campaign (`fuzz/`)

- **Tier:** beyond the pyramid — coverage-guided fuzzing with `cargo fuzz`
  (nightly toolchain). Not part of `cargo test`; run nightly and before
  releases, or on any parser change.
- **Relationship to `tst/fuzz`:** the in-tree mutation tier keeps a
  reproducible slice of the space in plain `cargo test`; this campaign
  explores past it with coverage feedback and keeps crashes in
  `fuzz/crashes/`.

## Targets

| Target | Harness path | Entry point | Oracle |
| --- | --- | --- | --- |
| `parse_page` | `fuzz/fuzz_targets/parse_page.rs` | `String::from_utf8_lossy(data)` → `parse_page(&html, "fuzz")` | on `Ok(conf)`: `dig(&conf)` — `times_for_date` + all 12 `month_times`/`month_iqama_times` must not panic |
| `conf_pipeline` | `fuzz/fuzz_targets/conf_pipeline.rs` | `serde_json::from_slice::<ConfData>(data)` | same `dig` oracle |

Both harnesses deliberately use the **same entry points as production**:
the page path goes through `parse_page` (the scraper), the model path
through serde with its real 128-level recursion limit (documented in both
harnesses — do not "optimize" it to `from_value`, which has no depth
limit and would blow the stack on deep input).

`dig` is duplicated in each harness (they cannot import the test crate's
`tst/common`); keep the two copies in sync with `tst/common::dig`.

## Running

```sh
just fuzz parse_page 60     # wrapper: checks cargo-fuzz + nightly, runs 60 s
# or directly:
cd fuzz && cargo fuzz run parse_page -- -max_total_time=60
cd fuzz && cargo fuzz run conf_pipeline -- -max_total_time=60
```

Corpus lives under `fuzz/corpus/<target>/`; crashes land in
`fuzz/crashes/` and reproduce with `cargo fuzz run <target> <crash-file>`.

## Campaign policy

- **Nightly** (when scheduled): 5–10 minutes per target.
- **On parser/model/calendar changes**: at least 60 s per touched target
  before merging.
- **On a crash**: minimize (`cargo fuzz tmin`), reduce to a deterministic
  case, add it to `tst/ut/corpus.rs` (so plain `cargo test` pins it
  forever), fix, re-run 10 minutes clean.

## Exit criteria per release

Both targets: 10 minutes each, zero crashes, corpus from the previous
release carried forward. Recorded in the HIL release checklist
([`../hil/live-campaigns.md`](../hil/live-campaigns.md)).
