# Spec: Compact binary (`MQTC`) and MCU runtime

Normative description of `src/compact.rs` and the `pack_for_mcu` tooling.
Implemented by `src/compact.rs` and `examples/pack_for_mcu.rs` (v0.4.2,
[ADR-0013](../adr/0013-no-std-and-mcu-support.md) +
[ADR-0014](../adr/0014-ddd-layering-and-plantuml.md)).

---

## Invariants & Design Principles

1. **Total parsing**: Missing, truncated, corrupt, or hostile byte slices yield `Err(CompactError)` or `None` — **never a panic**.
2. **Zero heap allocation**: Querying prayer times via `CompactCalendarView` requires **0 bytes of heap memory** (`#![no_std]` + `heapless`); the fajr-relative decode runs in registers on ~20 bytes of stack.
3. **Strict 4-byte memory alignment**: Every header (24 B) and day record (24 B direct / 20 B fajr-relative) is a multiple of 4 bytes. This prevents Hardware Fault (HardFault) crashes on ARM Cortex-M0/M0+ architectures and allows safe zero-copy reads.
4. **C1 instant rollover preservation**: Iqama entries carry a 1-bit rollover flag distinguishing same-day from next-day instants (preventing broken alarms when Isha/Iqama crosses midnight). The packer derives it from the resolved iqama: a display time *before* its adhan belongs to the next day.
5. **Payload integrity**: A standard CRC-32-IEEE checksum guarantees that incomplete flash writes, corrupted OTA transmissions, or flipped bits are rejected before execution.
6. **Jumu'ah support**: Friday prayer times (`jumua`, `jumua2`) are preserved in the header so displays show correct Friday schedules.
7. **Two-tier zero-RAM records**: The packer chooses between direct $O(1)$ Flash ROM access (24 bytes/day, every value explicit, no encode limits) and fajr-relative records (20 bytes/day, ~7.3 KB/year, decoded on the stack) before flashing.
8. **Per-format honesty**: each record format rejects *at pack time* exactly what it cannot encode (`DeltaOverflow`, `IqamaOffsetOverflow`, `TimeOutOfRange`, `TooManyDays`) — no silent wrapping, no cross-format validation leaks.

---

## Binary Specification (`MQTC` v1)

All multi-byte integers are serialized in **little-endian** byte order.

### 1. Header Layout (24 bytes, 4-byte aligned)

| Offset | Field | Type | Description |
| :--- | :--- | :--- | :--- |
| `0x00` | `magic` | `[u8; 4]` | Magic bytes: `b"MQTC"` (`0x4D, 0x51, 0x54, 0x43`) |
| `0x04` | `version` | `u8` | Format version: `0x01` |
| `0x05` | `scope_type` | `u8` | `0` = Week, `1` = Months, `2` = Full Year, `3` = Custom (unknown bytes decode as Custom) |
| `0x06` | `flags` | `u8` | Bitfield: `0x01` = `imsak_mode`, `0x02` = `has_iqama`, `0x04` = `has_jumua`, `0x08` = `fajr_relative` |
| `0x07` | `reserved` | `u8` | Alignment padding (`0x00`) |
| `0x08` | `start_year` | `u16` | Gregorian year of the first record (e.g. `2026`) |
| `0x0A` | `start_day_of_year` | `u16` | 1-based day of year of the first record; **must be `1..=366`** — validated before date resolution, `0` or `> 366` ⇒ `Err(InvalidDate)` (never a chrono panic) |
| `0x0C` | `day_count` | `u16` | Total consecutive day records in payload |
| `0x0E` | `jumua` | `u16` | Friday 1st prayer in minutes from midnight (`0xFFFF` if none) |
| `0x10` | `jumua2` | `u16` | Friday 2nd prayer in minutes from midnight (`0xFFFF` if none) |
| `0x12` | `pad` | `[u8; 2]` | Alignment padding (`0x00, 0x00`) |
| `0x14` | `crc32` | `u32` | CRC-32-IEEE over the **whole payload with bytes `0x14..0x18` zeroed** — i.e. compute over header-with-zeroed-CRC plus all day records, then write the result little-endian into `0x14..0x18`. Readers must zero `0x14..0x18` before hashing; hashing the buffer with the stored CRC in place never matches |

*Total header length: exactly 24 bytes (`0x18`). The first day record begins at byte offset 24.*

---

### 2. Day Record Layout — Direct Format (24 bytes per day)

Used when `flags & 0x08 == 0` (uncompressed, $O(1)$ random-access from Flash ROM). Every value is explicit: **no ordering, offset, or rollover limits**.

| Offset in Record | Field | Type | Description |
| :--- | :--- | :--- | :--- |
| `0x00` | `fajr` | `u16` | Fajr adhan time (`0..=1439` minutes from midnight) |
| `0x02` | `shurouq` | `u16` | Sunrise time (`0..=1439`) |
| `0x04` | `dhuhr` | `u16` | Dhuhr adhan time (`0..=1439`) |
| `0x06` | `asr` | `u16` | Asr adhan time (`0..=1439`) |
| `0x08` | `maghrib` | `u16` | Maghrib adhan time (`0..=1439`) |
| `0x0A` | `isha` | `u16` | Isha adhan time (`0..=1439`) |
| `0x0C` | `iqama_fajr` | `u16` | Fajr iqama packed field (see Bitfield Encoding below) |
| `0x0E` | `iqama_dhuhr` | `u16` | Dhuhr iqama packed field |
| `0x10` | `iqama_asr` | `u16` | Asr iqama packed field |
| `0x12` | `iqama_maghrib` | `u16` | Maghrib iqama packed field |
| `0x14` | `iqama_isha` | `u16` | Isha iqama packed field |
| `0x16` | `day_flags` | `u8` | Bit 0: has custom Jumu'ah override; bits 1–7 reserved |
| `0x17` | `reserved` | `u8` | Alignment padding (`0x00`) |

#### Iqama Bitfield Encoding (`u16`)

To eliminate the C1 rollover bug while maintaining strict `HH:MM` display validation:

- **Bit 15 (`0x8000`)**: `ROLLOVER` flag. `1` = instant belongs to the **next calendar day** (`date + 1 day`).
- **Bit 14 (`0x4000`)**: `VALID` flag. `1` = valid iqama present; `0` = no iqama configured for this prayer (`0x0000` total).
- **Bits 0..10 (`0x07FF`)**: Wall-clock display time in minutes from midnight (`0..=1439`).
  - Accessing wall-clock display: `raw & 0x07FF` (always `< 1440`, strictly satisfies `HH:MM`).
  - Checking next-day alarm instant: `(raw & 0x8000) != 0`.
- **Bits 12–13 (`0x3000`)**: reserved, **must be zero on the wire** (FINDING F13).

**Strict decode beyond the CRC (FINDING F13).** An attacker who can write
flash can also recompute the CRC, so validity does not end at the
checksum. A day record is **corrupt beyond the CRC** — and its lookup
returns `None` for that day, other days unaffected — when any of:

- an adhan `u16` is `> 1439` (the field carries no flag bits; any bit ≥ 12 set, or the 1440..=2047 dead zone),
- an iqama packed field has bits 12–13 set,
- an iqama packed field's masked minutes exceed 1439,
- an iqama packed field is nonzero without the `VALID` bit (e.g. a bare `ROLLOVER`).

Corrupt records are **dropped, never clamped and never fabricated** —
the same degradation semantics as a calendar-rejected day (F4). A
clamped "23:59" out of hostile bytes would itself be fabrication.

`to_hhmm()` precondition (ADR-0010 display contract): the minute value is
`0..=1439`. Every value this module produces satisfies it (builder
rejects larger values at pack time; decoders drop records that carry
them). Callers hand-crafting `CompactTime` from raw wire bits must
mask/check first, and check `is_valid()` on iqama fields before
formatting.

---

### 3. Day Record Layout — Fajr-Relative Format (20 bytes per day)

Used when `flags & 0x08 != 0` (`--compress delta` in `pack_for_mcu`).
Designed for constrained storage while requiring **zero heap memory and ~20 bytes of stack** to decode on the fly.

| Offset in Record | Field | Type | Description |
| :--- | :--- | :--- | :--- |
| `0x00` | `base_fajr` | `u16` | Fajr adhan time (`0..=1439` minutes) |
| `0x02` | `offset_shurouq` | `u16` | `shurouq − fajr` in minutes |
| `0x04` | `offset_dhuhr` | `u16` | `dhuhr − fajr` in minutes |
| `0x06` | `offset_asr` | `u16` | `asr − fajr` in minutes |
| `0x08` | `offset_maghrib` | `u16` | `maghrib − fajr` in minutes |
| `0x0A` | `offset_isha` | `u16` | `isha − fajr` in minutes |
| `0x0C` | `offset_iqama_fajr` | `u8` | Minutes after the fajr adhan (`0xFF` = none) |
| `0x0D` | `offset_iqama_dhuhr` | `u8` | Minutes after the dhuhr adhan (`0xFF` = none) |
| `0x0E` | `offset_iqama_asr` | `u8` | Minutes after the asr adhan (`0xFF` = none) |
| `0x0F` | `offset_iqama_maghrib` | `u8` | Minutes after the maghrib adhan (`0xFF` = none) |
| `0x10` | `offset_iqama_isha` | `u8` | Minutes after the isha adhan (`0xFF` = none; ≥ 1440 ⇒ rollover bit) |

*Total record length: exactly 20 bytes (divisible by 4). Decoded in registers via integer addition.*

**Encode limits** (violations are typed pack-time errors, never wraps):

- Adhan offsets are minutes-after-fajr, so the input must be ascending from fajr and every value ≤ 1439. This includes the **White-Night case** (FINDING F11): high-latitude summer calendars can wrap Isha to 00:00 — before Maghrib and Fajr — which is a *negative* offset-from-fajr and has no encoding; the packer rejects it with `DeltaOverflow { prayer: 5 }` instead of wrapping, and the direct format carries the same day explicitly.
- Iqama offsets must fit one byte (0..=254, `0xFF` reserved for absent ⇒ `IqamaOffsetOverflow`). A rollover iqama needs `adhan + (1440 − iqama) ≤ 254` — e.g. a 23:50 adhan with a 00:10 iqama (offset 20) packs, but a 19:40 adhan with a 00:10 iqama (offset 270) must use the direct format.
- `day_flags` has no fajr-relative byte; the field decodes as `0` in this format.

---

### 4. Footprint by Scope and Format

| Scope | Days | Direct (24 B/day) | Fajr-relative (20 B/day) | RAM Required |
| :--- | :--- | :--- | :--- | :--- |
| **Weekly** | 7 | **192 bytes** | **164 bytes** | **0 bytes** |
| **1 Month** | 30 | **744 bytes** | **624 bytes** | **0 bytes** |
| **3 Months** | 90 | **2,184 bytes** (~2.1 KB) | **1,824 bytes** (~1.8 KB) | **0 bytes** |
| **6 Months** | 182 | **4,392 bytes** (~4.3 KB) | **3,664 bytes** (~3.6 KB) | **0 bytes** |
| **Full Year** | 365 | **8,784 bytes** (~8.6 KB) | **7,324 bytes** (~7.2 KB) | **0 bytes** |

---

## Runtime Lookup Contract (`CompactCalendarView`)

### 1. Integrity Verification on Load

```rust
let view = CompactCalendarView::from_bytes(data)?;
```

`from_bytes` executes in order (each failure short-circuits, so the
diagnosis is deterministic):

1. `data.len() >= 24`, else `Err(CompactError::BufferTooSmall)`.
2. Magic `data[0..4] == b"MQTC"`, else `Err(InvalidMagic)`.
3. Version `data[4] == 1`, else `Err(UnsupportedVersion(actual))`.
4. Expected size — direct: `24 + day_count * 24`, fajr-relative: `24 + day_count * 20`. `data.len() < expected` ⇒ `Err(BufferTooSmall)`.
5. CRC-32-IEEE over header (with the crc32 field zeroed) + day records. Mismatch ⇒ `Err(ChecksumMismatch { expected, computed })`.
6. `start_year`/`start_day_of_year` must resolve to a valid date (`from_yo_opt`), else `Err(InvalidDate)` — a payload is never accepted that cannot anchor its own calendar.

### 2. O(1) Day Access from Flash

Given `target_date: NaiveDate`:

1. `delta_days = (target_date - start_date).num_days()`.
2. If `delta_days < 0` or `delta_days >= day_count`: returns `None` (out of bounds — a lookup miss is `None`, not an error; there is no `DateOutOfBounds` variant).
3. Direct format: `offset = 24 + delta_days * 24`; read the record.
4. Fajr-relative format: `offset = 24 + delta_days * 20`; reconstruct times via base + offsets in registers. An adhan offset > 1439 (impossible from the packer, possible from crafted bytes) yields `None` rather than a fabricated time.
5. **Corrupt-beyond-CRC records** (see the strict-decode rule above) drop the queried day as `None`; other days of the same payload stay queryable.

### 3. Display and Instant Helpers (`CompactTime`)

```rust
use core::fmt::Write as _;

pub struct CompactTime(pub u16);

impl CompactTime {
    /// Wall-clock hour (0..=23), strictly satisfies display contract
    pub fn hours(&self) -> u8 { ((self.0 & 0x07FF) / 60) as u8 }

    /// Wall-clock minute (0..=59)
    pub fn minutes(&self) -> u8 { ((self.0 & 0x07FF) % 60) as u8 }

    /// Minutes from midnight of the display day (0..=1439)
    pub fn minutes_from_midnight(&self) -> u16 { self.0 & 0x07FF }

    /// Whether this iqama instant belongs to the next calendar day (C1 rollover)
    pub fn is_rollover(&self) -> bool { (self.0 & 0x8000) != 0 }

    /// Whether an iqama is configured for this prayer
    pub fn is_valid(&self) -> bool { (self.0 & 0x4000) != 0 }

    /// Formats strict "HH:MM" with zero heap allocations
    pub fn to_hhmm(&self) -> heapless::String<5> {
        let mut s = heapless::String::<5>::new();
        let _ = write!(s, "{:02}:{:02}", self.hours(), self.minutes());
        s
    }
}
```

---

## Pre-Flash Packaging Tool (`pack_for_mcu`)

### Command-Line Usage

```bash
cargo run --example pack_for_mcu -- [OPTIONS]
```

| Flag | Argument | Description | Default |
| :--- | :--- | :--- | :--- |
| `--slug` | `<STRING>` | Mosque slug on mawaqit.net | *Required (or `--file`)* |
| `--file` | `<PATH>` | Local page HTML file (offline) | None |
| `--scope` | `<week\|months\|year>` | Date scope (`custom` is reserved) | `year` |
| `--months` | `<N>` | Number of months (when `--scope months`) | `3` |
| `--start` | `<YYYY-MM-DD>` | Start date for scope | Current date |
| `--compress` | `<none\|delta>` | Record format (`delta` = fajr-relative) | `none` (direct) |
| `--format` | `<rust\|bin\|c>` | Output format | `rust` |
| `--clamp` | flag | Allow clamping scope to Dec 31 if year boundary crossed | false |
| `--out` | `<PATH>` | Output file path | stdout |

Day records resolve through the tested calendar pipeline
(`month_times` / `month_iqama_times` — F4/F5 semantics included). A date
the calendar dropped as malformed aborts the pack with an actionable
error: a consecutive-range format cannot carry gaps. Before any byte is
written, the payload must pass `CompactCalendarView::from_bytes` — the
packer never emits data it cannot load back.

### Year-End Boundary Handling

Mawaqit only publishes data for the current calendar year (ending Dec 31).

- If the requested scope extends past December 31st and `--clamp` is **not** set:
  Fails fast with:
  `Error: requested scope extends past 2026-12-31. Upstream mawaqit only publishes data through the current calendar year. Use --clamp to package up to Dec 31.`
- If `--clamp` is set:
  Truncates the packaged range at December 31st and outputs an informational warning with the actual day count.

---

## Error Taxonomy (`CompactError`)

| Variant | Condition |
| :--- | :--- |
| `InvalidMagic` | First 4 bytes do not match `b"MQTC"` |
| `UnsupportedVersion(u8)` | Version byte is not `1` |
| `BufferTooSmall` | Buffer shorter than 24 bytes, or shorter than the expected day records |
| `ChecksumMismatch { expected, computed }` | CRC-32 verification failed (corrupt flash/OTA) |
| `InvalidDate` | `start_year`/`start_day_of_year` cannot be resolved to a valid date |
| `TimeOutOfRange { prayer, minutes }` | Packer input: a time value outside `0..=1439` (either format) |
| `DeltaOverflow { prayer, delta }` | Fajr-relative pack: an adhan offset from fajr is negative or exceeds 1439 (non-ascending input) |
| `IqamaOffsetOverflow { prayer, delta }` | Fajr-relative pack: an iqama offset is negative or exceeds 254 (one byte, `0xFF` reserved) |
| `TooManyDays(usize)` | Packer input: more than `u16::MAX` day records |

A queried date outside `[start_date, end_date]` is **`None`**, not an
error — lookups are total, and `DateOutOfBounds` intentionally does not
exist.
