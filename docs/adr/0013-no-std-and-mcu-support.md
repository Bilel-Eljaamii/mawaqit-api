# ADR-0013: no_std and MCU support — heapless zero-alloc execution, compact binary packaging, and Cargo feature tiers

- **Status:** Accepted — implemented in v0.4.2; layering refined and the 12-byte delta layout replaced by the 20-byte fajr-relative layout in [ADR-0014](0014-ddd-layering-and-plantuml.md)
- **Date:** 2026-10-05
- **Decides:** Target platform expansion to bare-metal microcontrollers (ESP32, RP2040, STM32, RISC-V), feature gating architecture, hardened compact binary layout (`MQTC`), and zero-allocation runtime.

## Context

`mawaqit-api` was initially built for desktop, server, and CLI environments running an operating system with thread scheduling, POSIX sockets, and dynamic memory allocation (`tokio`, `reqwest`, `std::fs`, `std::time`).

However, physical prayer clocks, mosque status displays, and ambient adhan devices are predominantly powered by **microcontrollers (MCUs)** such as ESP32, Raspberry Pi Pico (RP2040/RP2350), STM32, and RISC-V chips. These environments have strict constraints:

1. **No standard library (`#![no_std]`)**: There is no OS kernel, no filesystem, and no POSIX socket layer.
2. **RAM scarcity**: MCUs often have 32 KB to 512 KB of SRAM. The raw Mawaqit mosque page embeds ~15–25 KB of JSON with over 4,000 strings and arrays for a full year. Repeatedly buffering, parsing, and allocating this JSON at runtime is inefficient or impossible on small MCUs.
3. **Flash availability**: MCUs typically feature 2 MB to 16 MB of SPI/NOR Flash ROM, which can be memory-mapped directly into the processor's address space.
4. **Zero-heap safety**: Many embedded applications deliberately forbid dynamic heap allocation (`alloc`) to prevent heap fragmentation and out-of-memory panics.
5. **Silicon alignment rules**: Architectures like ARM Cortex-M0/M0+ trigger a Hardware Fault (HardFault) on unaligned pointer reads. Structs and memory layouts must be strictly 4-byte aligned.
6. **Flash integrity**: Microcontroller flash memory is vulnerable to interrupted writes and bit flips. Data must have cryptographic/checksum integrity verification before execution.

We need a design that enables MCU support in the **exact same crate and folder**, maintains 100% backward compatibility for existing `std` consumers, and provides a zero-RAM, zero-heap runtime for microcontrollers.

## Decision

| Area | Decision | Rationale |
| :--- | :--- | :--- |
| **Package structure** | Single crate in the same folder | Cargo features allow conditional compilation without splitting into multiple crates (`mawaqit-core`), avoiding repository churn and publishing friction. |
| **Feature tiers (Option A)** | `default = ["std"]`, opt-in `alloc` and `heapless` | Existing consumers continue using `mawaqit-api` with zero configuration changes. Embedded developers opt into `no_std` via `default-features = false`. |
| **Aligned binary (`MQTC`)** | 24-byte header + 24 bytes per day (or 12 bytes delta) | Strictly 4-byte aligned to guarantee zero HardFaults on ARM Cortex-M0/M0+. Allows safe zero-copy `#[repr(C)]` casting. |
| **Integrity validation** | CRC-32-IEEE checksum in header | Guarantees that incomplete flash writes, corrupted OTA transmissions, or flipped bits are rejected before running. |
| **C1 rollover bitfield** | Bit 15 of iqama field marks next-day instant | Eliminates the midnight rollover bug (C1) without violating the strict `HH:MM` display contract. |
| **Jumu'ah support** | Mosque-level Friday times in header | Prevents displays from showing regular weekday Dhuhr times on Friday. |
| **Two-tier zero-RAM compression** | Direct flash ($O(1)$ flash mapping, 24 B/day) or on-the-fly stack delta decoding (12 B/day) | Avoids 8 KB RAM decompression buffers required by LZ4/Deflate, maintaining the zero-RAM, zero-heap promise while cutting payload to ~4.3 KB/year. |
| **Year-end clamping** | Fails fast or clamps past Dec 31st | Mawaqit only serves the current year. Prevents packaging corrupt zeroed data across the year boundary. |
| **Zero-allocation runtime** | `heapless` integration (`CompactCalendarView`) | Zero-copy borrowing parser over `&'static [u8]` memory-mapped from Flash ROM. Returns `heapless::String<5>` for display strings with **0 bytes of RAM allocated**. |
| **OS isolation** | `MawaqitClient`, `disk`, `cache`, `download_voice` gated behind `cfg(feature = "std")` | Isolates all `tokio`, `reqwest`, and `std::fs` dependencies to desktop/server builds. |

### Feature Matrix

```toml
[features]
default = ["std"]

# std: includes everything (tokio, reqwest, std::fs, alloc, heapless, MawaqitClient, pack_for_mcu)
std = [
    "alloc",
    "heapless",
    "dep:reqwest",
    "dep:tokio",
    "chrono/std",
    "serde/std",
    "serde_json/std",
    "thiserror/std",
]

# alloc: for no_std targets with an embedded heap (ESP32, RP2040 with esp-alloc)
# provides dynamic ConfData, parse_page, times_for_date with String/Vec/BTreeMap
alloc = [
    "chrono/alloc",
    "serde/alloc",
    "serde_json/alloc",
]

# heapless: for pure zero-alloc MCU firmware
# provides CompactCalendarView, CompactDayTimes, heapless::String<5>
heapless = [
    "dep:heapless",
]
```

### Pre-Flash Workflow & Scoping

Rather than forcing an MCU to parse a 60 KB HTML page over Wi-Fi on boot:

1. The developer runs `pack_for_mcu` on a workstation or build server.
2. The user selects the mosque slug, desired scope (week, 1/3/6 months, full year), compression tier (`none` or `delta`), and export format:
   - `.rs`: A static Rust array (`pub static PRAYER_DATA: &[u8] = &[...];`) compiled directly into the binary.
   - `.bin`: A raw binary file flashed to LittleFS/SPIFFS/NVS or raw flash offset.
   - `.h`: A C header for ESP-IDF or STM32 C/C++ firmware.
3. At runtime, the MCU accesses the data through `CompactCalendarView::from_bytes(PRAYER_DATA)` in $O(1)$ time by day offset.

## Consequences

**Positive**

- **Zero desktop churn**: `MawaqitClient`, desktop caching, and existing tests remain completely intact and unchanged.
- **Hardware safe**: 4-byte aligned layouts prevent HardFault crashes on Cortex-M0/M0+.
- **Zero RAM consumption**: Memory-mapped flash lookup uses 0 bytes of heap and minimal stack space.
- **Data integrity**: CRC-32 protects against corrupted flash storage.
- **Correct prayer times**: Jumu'ah overrides and C1 midnight rollovers are preserved.

**Negative / accepted costs**

- `MawaqitClient` cannot run on bare-metal MCUs without an OS. MCUs that wish to perform live network updates must either use pre-packaged flash data or fetch raw data via their platform-specific HTTP client (`esp-idf-svc`, `embedded-nal-async`, AT commands) and feed the buffer to `parse_page` (if `alloc` is enabled).
- Two distinct representations: `ConfData` (verbose, wire-tolerant, dynamic `alloc`) and `CompactCalendarView` (packed, fixed-size, zero-alloc `heapless`).

**Alternatives rejected**

- **Separate crates (`mawaqit-core` + `mawaqit-client`)**: Adds repository fragmentation, multiple version numbers, and publishing overhead for minimal gain. Cargo features solve this in the same crate.
- **Parsing raw JSON on all MCUs**: Requires a dynamic heap and buffers capable of holding 25 KB JSON, which excludes smaller MCUs and violates zero-alloc firmware policies.
- **22-byte unaligned record layout**: Rejected due to catastrophic HardFault risk on ARM Cortex-M0 microcontrollers.
- **Deflate/LZ4 compression on MCUs**: Rejected because decompressing an 8 KB payload requires an 8 KB RAM buffer, violating the zero-heap, zero-RAM embedded requirement. Replaced with on-the-fly stack-based delta decoding (12 bytes/day).
