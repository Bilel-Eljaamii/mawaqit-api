# Spec: Compact binary (`MQTC`) and MCU runtime

Normative description of `src/compact.rs` and the `pack_for_mcu` tooling.
Implemented by `src/compact.rs` and `examples/pack_for_mcu.rs`.

## Invariants & Design Principles

1. **Total parsing**: Missing, truncated, corrupt, or hostile byte slices yield `Err(CompactError)` or `None` — **never a panic**.
2. **Zero heap allocation**: Querying prayer times via `CompactCalendarView` requires **0 bytes of heap memory** (`#![no_std]` + `heapless`).
3. **Zero RAM execution**: Uncompressed records have fixed length (22 bytes/day), allowing $O(1)$ random-access direct reads from memory-mapped Flash ROM (`&'static [u8]`) without allocating RAM.
4. **Display contract**: Surfaced display strings formatted via `CompactTime::to_hhmm()` strictly adhere to the `HH:MM` contract (ADR-0010).
5. **Pre-flash flexibility**: The user chooses the exact date scope (weekly, specified months, or full year) and export format before flashing to the device.

---

## Binary Specification (`MQTC` v1)

All multi-byte integers are serialized in **little-endian** byte order.

### 1. Header Layout (12 bytes)

| Offset | Field | Type | Description |
| :--- | :--- | :--- | :--- |
| `0x00` | `magic` | `[u8; 4]` | Magic bytes: `b"MQTC"` (`0x4D, 0x51, 0x54, 0x43`) |
| `0x04` | `version` | `u8` | Format version: `0x01` |
| `0x05` | `scope_type` | `u8` | `0` = Week (7 days), `1` = Months, `2` = Full Year, `3` = Custom |
| `0x06` | `flags` | `u8` | Bitfield: `0x01` = `imsak_mode`, `0x02` = `has_iqama`, bits 2–7 reserved (`0`) |
| `0x07` | `reserved` | `u8` | Reserved byte (`0x00`) for 32-bit alignment |
| `0x08` | `start_year` | `u16` | Gregorian year of the first record (e.g. `2026`) |
| `0x0A` | `start_day_of_year` | `u16` | 1-based day of year (1..=366) of the first record |
| `0x0C` | `day_count` | `u16` | Total number of consecutive day records in the payload |

*Note: Total header length is exactly 12 bytes (`0x0C`). The first day record starts at byte offset 12.*

### 2. Day Record Layout (22 bytes per day)

Consecutive day records follow the header. Each record is fixed at 22 bytes:

| Offset in Record | Field | Type | Description |
| :--- | :--- | :--- | :--- |
| `0x00` | `fajr` | `u16` | Fajr adhan time (minutes from midnight, `0..=1439`) |
| `0x02` | `shurouq` | `u16` | Sunrise time (minutes from midnight, `0..=1439`) |
| `0x04` | `dhuhr` | `u16` | Dhuhr adhan time (minutes from midnight, `0..=1439`) |
| `0x06` | `asr` | `u16` | Asr adhan time (minutes from midnight, `0..=1439`) |
| `0x08` | `maghrib` | `u16` | Maghrib adhan time (minutes from midnight, `0..=1439`) |
| `0x0A` | `isha` | `u16` | Isha adhan time (minutes from midnight, `0..=1439`) |
| `0x0C` | `iqama_fajr` | `u16` | Fajr iqama time (`0..=1439`) or `0xFFFF` if unavailable |
| `0x0E` | `iqama_dhuhr` | `u16` | Dhuhr iqama time (`0..=1439`) or `0xFFFF` if unavailable |
| `0x10` | `iqama_asr` | `u16` | Asr iqama time (`0..=1439`) or `0xFFFF` if unavailable |
| `0x12` | `iqama_maghrib` | `u16` | Maghrib iqama time (`0..=1439`) or `0xFFFF` if unavailable |
| `0x14` | `iqama_isha` | `u16` | Isha iqama time (`0..=1439`) or `0xFFFF` if unavailable |

### 3. Total Payload Sizes by Scope

Total size = $12 + (\text{day\_count} \times 22)$ bytes.

| Scope | Days | Payload Size | Flash ROM Footprint |
| :--- | :--- | :--- | :--- |
| **Weekly** | 7 | **166 bytes** | Negligible (< 0.01% of 2 MB Flash) |
| **1 Month** | 30 | **672 bytes** | < 0.7 KB |
| **3 Months** | 90 | **1,992 bytes** | ~1.9 KB |
| **6 Months** | 182 | **4,016 bytes** | ~3.9 KB |
| **Full Year** | 365 | **8,042 bytes** | ~7.8 KB (fits comfortably in all MCUs) |

---

## Runtime Lookup Contract (`CompactCalendarView`)

### Day Index Arithmetic ($O(1)$)

Given a target `date: NaiveDate`:

1. Calculate difference in days: `delta = date - start_date`.
2. If `delta < 0` or `delta >= day_count`: return `None` (date out of packaged bounds).
3. Compute byte offset:
   $$\text{offset} = 12 + (\text{delta} \times 22)$$
4. Read 22 bytes directly from slice:
   - Adhan times: decode 6 `u16` integers (little-endian).
   - Iqama times: if `flags & 0x02 != 0`, decode 5 `u16` integers; values equal to `0xFFFF` are mapped to `None`.
5. Return `CompactDayTimes`.

### Display Formatting with `heapless`

`CompactTime(u16)` exposes:

- `hours(&self) -> u8`: `(self.0 / 60) as u8`
- `minutes(&self) -> u8`: `(self.0 % 60) as u8`
- `minutes_from_midnight(&self) -> u16`: `self.0`
- `to_hhmm(&self) -> heapless::String<5>`:
  Constructs exact ASCII string:

  ```rust
  let mut s = heapless::String::<5>::new();
  // writes HH:MM directly into stack buffer, 0 heap allocations
  ```

---

## Pre-Flash Packaging Tool (`pack_for_mcu`)

### Command-Line Arguments

```bash
cargo run --example pack_for_mcu -- [OPTIONS]
```

| Flag | Argument | Description | Default |
| :--- | :--- | :--- | :--- |
| `--slug` | `<STRING>` | Mosque slug on mawaqit.net (e.g. `grande-mosquee-de-paris`) | *Required (or `--file`)* |
| `--file` | `<PATH>` | Local page HTML or JSON file to read instead of fetching | None |
| `--scope` | `<week\|months\|year>` | Date range scope to pack | `year` |
| `--months` | `<N>` | Number of months when `--scope months` is selected | `3` |
| `--start` | `<YYYY-MM-DD>` | Start date for custom/month/week scopes | Current date |
| `--format` | `<bin\|rust\|c>` | Output format | `rust` |
| `--out` | `<PATH>` | Output file path | stdout |

### Target Formats

#### 1. Rust Source (`.rs`)

Emits a compilable Rust module:

```rust
// Generated by mawaqit-api pack_for_mcu
pub static PRAYER_DATA: &[u8] = &[
    0x4D, 0x51, 0x54, 0x43, // magic: "MQTC"
    0x01, 0x02, 0x03, 0x00, // version, scope, flags, reserved
    0xEA, 0x07, 0x01, 0x00, // start_year: 2026, start_day: 1
    0x6D, 0x01,             // day_count: 365
    // ... 22 bytes per day ...
];
```

#### 2. Raw Binary (`.bin`)

Emits the exact byte sequence suitable for `include_bytes!`, SPI flash flashing, or LittleFS.

#### 3. C Header (`.h`)

Emits a C/C++ header for ESP-IDF, Arduino, or STM32 projects:

```c
#ifndef MAWAQIT_PRAYER_DATA_H
#define MAWAQIT_PRAYER_DATA_H

#include <stdint.h>

static const uint8_t PRAYER_DATA[] = {
    0x4D, 0x51, 0x54, 0x43,
    // ...
};
static const uint32_t PRAYER_DATA_LEN = 8042;

#endif
```

---

## Error Handling (`CompactError`)

| Variant | Condition |
| :--- | :--- |
| `InvalidMagic` | First 4 bytes do not match `b"MQTC"` |
| `UnsupportedVersion(u8)` | Version byte is not `1` |
| `BufferTooSmall` | Buffer length is less than 12 bytes, or less than $12 + (day\_count \times 22)$ |
| `InvalidDate` | `start_year` and `start_day_of_year` cannot be resolved to a valid `NaiveDate` |
| `DateOutOfBounds` | Queried date is earlier than `start_date` or later than `end_date` |
