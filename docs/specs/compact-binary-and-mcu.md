# Spec: Compact binary (`MQTC`) and MCU runtime

Normative description of `src/compact.rs` and the `pack_for_mcu` tooling.
Implemented by `src/compact.rs` and `examples/pack_for_mcu.rs`.

---

## Invariants & Design Principles

1. **Total parsing**: Missing, truncated, corrupt, or hostile byte slices yield `Err(CompactError)` or `None` — **never a panic**.
2. **Zero heap allocation**: Querying prayer times via `CompactCalendarView` requires **0 bytes of heap memory** (`#![no_std]` + `heapless`).
3. **Strict 4-byte memory alignment**: Every header and day record is a multiple of 4 bytes (24 bytes). This prevents Hardware Fault (HardFault) crashes on ARM Cortex-M0/M0+ architectures and allows safe zero-copy casting.
4. **C1 instant rollover preservation**: Iqama entries carry a 1-bit rollover flag to distinguish between same-day and next-day instants (preventing broken alarms when Isha/Iqama crosses midnight).
5. **Payload integrity**: A standard CRC-32-IEEE checksum guarantees that incomplete flash writes, corrupted OTA transmissions, or flipped bits are rejected before execution.
6. **Jumu'ah support**: Friday prayer times (`jumua`, `jumua2`) are preserved in the header so displays show correct Friday schedules.
7. **Two-tier zero-RAM compression**: The user chooses between direct $O(1)$ Flash ROM access (24 bytes/day) or on-the-fly stack-decoded delta compression (12 bytes/day, ~4.3 KB/year) before flashing.

---

## Binary Specification (`MQTC` v1)

All multi-byte integers are serialized in **little-endian** byte order.

### 1. Header Layout (24 bytes, 4-byte aligned)

| Offset | Field | Type | Description |
| :--- | :--- | :--- | :--- |
| `0x00` | `magic` | `[u8; 4]` | Magic bytes: `b"MQTC"` (`0x4D, 0x51, 0x54, 0x43`) |
| `0x04` | `version` | `u8` | Format version: `0x01` |
| `0x05` | `scope_type` | `u8` | `0` = Week, `1` = Months, `2` = Full Year, `3` = Custom |
| `0x06` | `flags` | `u8` | Bitfield: `0x01` = `imsak_mode`, `0x02` = `has_iqama`, `0x04` = `has_jumua`, `0x08` = `delta_compressed` |
| `0x07` | `reserved` | `u8` | Alignment padding (`0x00`) |
| `0x08` | `start_year` | `u16` | Gregorian year of the first record (e.g. `2026`) |
| `0x0A` | `start_day_of_year` | `u16` | 1-based day of year (1..=366) of the first record |
| `0x0C` | `day_count` | `u16` | Total consecutive day records in payload |
| `0x0E` | `jumua` | `u16` | Friday 1st prayer in minutes from midnight (`0xFFFF` if none) |
| `0x10` | `jumua2` | `u16` | Friday 2nd prayer in minutes from midnight (`0xFFFF` if none) |
| `0x12` | `pad` | `[u8; 2]` | Alignment padding (`0x00, 0x00`) |
| `0x14` | `crc32` | `u32` | CRC-32-IEEE checksum of the header (with crc32=0) + day records |

*Total header length: exactly 24 bytes (`0x18`). The first day record begins at byte offset 24.*

---

### 2. Day Record Layout — Direct Flash Format (24 bytes per day)

Used when `flags & 0x08 == 0` (uncompressed, $O(1)$ random-access from Flash ROM):

| Offset in Record | Field | Type | Description |
| :--- | :--- | :--- | :--- |
| `0x00` | `fajr` | `u16` | Fajr adhan time (`0..=1439` minutes from midnight) |
| `0x02` | `shurouq` | `u16` | Sunrise time (`0..=1439` minutes from midnight) |
| `0x04` | `dhuhr` | `u16` | Dhuhr adhan time (`0..=1439` minutes from midnight) |
| `0x06` | `asr` | `u16` | Asr adhan time (`0..=1439` minutes from midnight) |
| `0x08` | `maghrib` | `u16` | Maghrib adhan time (`0..=1439` minutes from midnight) |
| `0x0A` | `isha` | `u16` | Isha adhan time (`0..=1439` minutes from midnight) |
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
- **Bit 14 (`0x4000`)**: `VALID` flag. `1` = valid iqama present; `0` = no iqama configured for this prayer.
- **Bits 0..11 (`0x07FF`)**: Wall-clock display time in minutes from midnight (`0..=1439`).
  - Accessing wall-clock display: `raw & 0x07FF` (always `< 1440`, strictly satisfies `HH:MM`).
  - Checking next-day alarm instant: `(raw & 0x8000) != 0`.

---

### 3. Day Record Layout — Delta-Compressed Format (12 bytes per day)

Used when `flags & 0x08 != 0` (`--compress delta` in `pack_for_mcu`):
Designed for constrained storage while requiring **zero heap memory and only 12 bytes of stack** to decode on the fly:

| Offset in Record | Field | Type | Description |
| :--- | :--- | :--- | :--- |
| `0x00` | `base_fajr` | `u16` | Fajr adhan time (`0..=1439` minutes) |
| `0x02` | `delta_shurouq` | `u8` | `shurouq - fajr` (minutes) |
| `0x03` | `delta_dhuhr` | `u8` | `dhuhr - shurouq` (minutes) |
| `0x04` | `delta_asr` | `u8` | `asr - dhuhr` (minutes) |
| `0x05` | `delta_maghrib` | `u8` | `maghrib - asr` (minutes) |
| `0x06` | `delta_isha` | `u8` | `isha - maghrib` (minutes) |
| `0x07` | `offset_fajr` | `u8` | Iqama offset after Fajr adhan (mins, or `0xFF` if none) |
| `0x08` | `offset_dhuhr` | `u8` | Iqama offset after Dhuhr adhan (mins, or `0xFF` if none) |
| `0x09` | `offset_asr` | `u8` | Iqama offset after Asr adhan (mins, or `0xFF` if none) |
| `0x0A` | `offset_maghrib` | `u8` | Iqama offset after Maghrib adhan (mins, or `0xFF` if none) |
| `0x0B` | `offset_isha` | `u8` | Iqama offset after Isha adhan (mins, or `0xFF` if none) |

*Total record length: exactly 12 bytes (divisible by 4). Decoded in registers via simple integer addition.*

---

### 4. Footprint by Scope and Format

| Scope | Days | Direct Flash (24 B/day) | Delta-Compressed (12 B/day) | RAM Required |
| :--- | :--- | :--- | :--- | :--- |
| **Weekly** | 7 | **192 bytes** | **108 bytes** | **0 bytes** |
| **1 Month** | 30 | **744 bytes** | **384 bytes** | **0 bytes** |
| **3 Months** | 90 | **2,184 bytes** (~2.1 KB) | **1,104 bytes** (~1.1 KB) | **0 bytes** |
| **6 Months** | 182 | **4,392 bytes** (~4.3 KB) | **2,208 bytes** (~2.2 KB) | **0 bytes** |
| **Full Year** | 365 | **8,784 bytes** (~8.6 KB) | **4,404 bytes** (~4.3 KB) | **0 bytes** |

---

## Runtime Lookup Contract (`CompactCalendarView`)

### 1. Integrity Verification on Load

```rust
let view = CompactCalendarView::from_bytes(data)?;
```

`from_bytes` executes in order:

1. Verifies `data.len() >= 24`.
2. Validates magic `data[0..4] == b"MQTC"`.
3. Validates version `data[4] == 1`.
4. Computes expected size:
   - Direct format: `24 + day_count * 24`
   - Delta format: `24 + day_count * 12`
   If `data.len() < expected_len`: returns `Err(CompactError::BufferTooSmall)`.
5. Computes CRC-32-IEEE over slice and verifies against header checksum. Mismatch $\Rightarrow$ `Err(CompactError::ChecksumMismatch)`.

### 2. O(1) Day Access from Flash

Given `target_date: NaiveDate`:

1. `delta_days = target_date.signed_duration_since(start_date).num_days()`.
2. If `delta_days < 0` or `delta_days >= day_count`: returns `None` (out of bounds).
3. If Direct format:
   `offset = 24 + (delta_days as usize * 24)`.
   Reads 24 bytes from memory-mapped flash.
4. If Delta format:
   `offset = 24 + (delta_days as usize * 12)`.
   Reconstructs times via base + deltas directly in stack registers.

### 3. Display and Instant Helpers (`CompactTime`)

```rust
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
        let h = self.hours();
        let m = self.minutes();
        let _ = write!(s, "{:02}:{:02}", h, m);
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
| `--file` | `<PATH>` | Local page HTML or JSON file | None |
| `--scope` | `<week\|months\|year\|custom>` | Date scope | `year` |
| `--months` | `<N>` | Number of months (when `--scope months`) | `3` |
| `--start` | `<YYYY-MM-DD>` | Start date for scope | Current date |
| `--compress` | `<none\|delta>` | Compression tier | `none` (direct flash) |
| `--format` | `<rust\|bin\|c>` | Output format | `rust` |
| `--clamp` | flag | Allow clamping scope to Dec 31 if year boundary crossed | false |
| `--out` | `<PATH>` | Output file path | stdout |

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
| `BufferTooSmall` | Buffer length is less than 24 bytes, or less than expected day records |
| `ChecksumMismatch { expected: u32, computed: u32 }` | CRC-32 verification failed (corrupt flash/OTA) |
| `InvalidDate` | `start_year` and `start_day_of_year` cannot be resolved to a valid `NaiveDate` |
| `DateOutOfBounds` | Queried date is earlier than `start_date` or later than `end_date` |
