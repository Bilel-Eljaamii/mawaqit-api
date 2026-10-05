# Test Spec: `ut/compact.rs` — the MQTC codec

- **Tier:** unit (`cargo test --test ut`), offline, deterministic, no I/O.
- **Target:** `mawaqit_api::compact` (ADR-0013) — `CompactCalendarBuilder`
  (alloc side) and `CompactCalendarView` (zero-alloc runtime side).
- **Contract:** roundtrips preserve every field in both record formats;
  corrupt/truncated/hostile bytes yield `Err`/`None`, never a panic; the
  C1 rollover bit, Jumu'ah header fields and year boundaries survive the
  wire; each format rejects exactly what it cannot encode.

## Tests

### `direct_roundtrip_preserves_every_field`
Three days packed direct (24 B/record), loaded back: adhan minutes,
iqama validity, the C1 rollover bit (isha iqama past midnight) and the
strict `HH:MM` display all roundtrip per day index.

### `fajr_relative_roundtrip_preserves_every_field`
Same payload in fajr-relative format (20 B/record): rollover and same-day
iqamas decode identically from the on-the-fly offset math.

### `payload_sizes_match_the_layout_contract`
365 days → exactly `24 + 365×24` bytes direct, `24 + 365×20` fajr-relative
(the flash-footprint numbers the spec and ADR advertise).

### `crc_bit_flip_is_rejected_before_any_lookup`
One flipped bit in the last record → `ChecksumMismatch` — integrity is
checked at load, before any query can observe data.

### `truncated_and_tiny_buffers_are_rejected`
Buffers shorter than the header, and header-plus-partial records in both
formats → `BufferTooSmall`.

### `bad_magic_and_version_are_rejected`
Wrong magic → `InvalidMagic`; wrong version byte → `UnsupportedVersion(2)`
(validated before the CRC, so the diagnosis is deterministic).

### `out_of_bounds_dates_return_none_never_error`
Dates before the start day, after the end day, and in another year →
`None`. A lookup miss is `None`, not an error (no fabrication).

### `year_boundary_days_are_addressable`
A scope starting Dec 30 addresses Jan 1 of the next year — date math is
calendar-true, not year-relative.

### `jumuah_header_roundtrips`
`with_jumuah(750, 810)` loads back as `12:30` / `13:30`; absence stays
absent (`0xFFFF` sentinel → `None`).

### `invalid_day_records_are_rejected_at_pack_time`
Adhan minute past 23:59 → `TimeOutOfRange`; shurouq before fajr →
`DeltaOverflow` in fajr-relative format but packs fine direct; iqama
offset past the one-byte limit (254 min) → `IqamaOffsetOverflow` in
fajr-relative format but packs fine direct. Each format rejects exactly
its own limitations.

### `hostile_mutations_never_panics_and_ok_payloads_stay_queryable`
2,000 deterministic xorshift mutations (1–4 bit flips) of 7-day payloads
in both formats: `from_bytes` is always `Ok` or `Err`, and a payload that
loads must serve every in-range day with in-range display times.

### `emitters_embed_the_exact_payload`
`to_rust_code` and `to_c_header` embed the identical payload bytes
(magic visible as `77,81,84,67,` / `0x4D,0x51,0x54,0x43,`).

### `empty_builder_yields_a_header_only_payload`
Zero days → a 24-byte payload that loads with `day_count == 0` and
answers no date.

## Run

```sh
cargo test --test ut compact
```

A failure means the binary contract the MCU firmware depends on drifted
from the spec (`docs/specs/compact-binary-and-mcu.md`), or a hostile blob
can crash or fabricate times on a bare-metal device.
