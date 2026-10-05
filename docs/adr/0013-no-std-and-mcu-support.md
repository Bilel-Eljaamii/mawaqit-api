# ADR-0013: no_std and MCU support — heapless zero-alloc execution, compact binary packaging, and Cargo feature tiers

- **Status:** Accepted
- **Date:** 2026-10-05
- **Decides:** Target platform expansion to bare-metal microcontrollers (ESP32, RP2040, STM32, RISC-V), feature gating architecture, compact binary layout (`MQTC`), and zero-allocation runtime.

## Context

`mawaqit-api` was initially built for desktop, server, and CLI environments running an operating system with thread scheduling, POSIX sockets, and dynamic memory allocation (`tokio`, `reqwest`, `std::fs`, `std::time`).

However, physical prayer clocks, mosque status displays, and ambient adhan devices are predominantly powered by **microcontrollers (MCUs)** such as ESP32, Raspberry Pi Pico (RP2040/RP2350), STM32, and RISC-V chips. These environments have strict constraints:

1. **No standard library (`#![no_std]`)**: There is no OS kernel, no filesystem, and no POSIX socket layer.
2. **RAM scarcity**: MCUs often have 32 KB to 512 KB of SRAM. The raw Mawaqit mosque page embeds ~15–25 KB of JSON with over 4,000 strings and arrays for a full year. Repeatedly buffering, parsing, and allocating this JSON at runtime is inefficient or impossible on small MCUs.
3. **Flash availability**: MCUs typically feature 2 MB to 16 MB of SPI/NOR Flash ROM, which can be memory-mapped directly into the processor's address space.
4. **Zero-heap safety**: Many embedded applications deliberately forbid dynamic heap allocation (`alloc`) to prevent heap fragmentation and out-of-memory panics.

We need a design that enables MCU support in the **exact same crate and folder**, maintains 100% backward compatibility for existing `std` consumers, and provides a zero-RAM, zero-heap runtime for microcontrollers.

## Decision

| Area | Decision | Rationale |
| :--- | :--- | :--- |
| **Package structure** | Single crate in the same folder | Cargo features allow conditional compilation without splitting into multiple crates (`mawaqit-core`), avoiding repository churn and publishing friction. |
| **Feature tiers (Option A)** | `default = ["std"]`, opt-in `alloc` and `heapless` | Existing consumers continue using `mawaqit-api` with zero configuration changes. Embedded developers opt into `no_std` via `default-features = false`. |
| **Compact binary (`MQTC`)** | 12-byte header + 22 bytes per day | Time strings (`"05:42"`) are packed as minutes from midnight (`u16`). A full year is ~7.8 KB; a week is 166 bytes. |
| **User scoping** | User chooses scope before flashing | Developers configure the exact date range needed (weekly, $N$ months, or full year) using a pre-flash tool (`pack_for_mcu`). |
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

1. The developer or build pipeline runs `pack_for_mcu` on a workstation or server.
2. The user selects the mosque slug, desired scope (week, 1/3/6 months, full year), and export format:
   - `.rs`: A static Rust array (`pub static PRAYER_DATA: &[u8] = &[...];`) compiled directly into the binary.
   - `.bin`: A raw binary file flashed to LittleFS/SPIFFS/NVS or raw flash offset.
   - `.h`: A C header for ESP-IDF or STM32 C/C++ firmware.
3. At runtime, the MCU accesses the data through `CompactCalendarView::from_bytes(PRAYER_DATA)` in $O(1)$ time by day offset.

## Consequences

**Positive**

- **Zero desktop churn**: `MawaqitClient`, desktop caching, and existing tests remain completely intact and unchanged.
- **Microcontroller native**: Works out of the box on `thumbv7em-none-eabihf` (Cortex-M4/M7), `riscv32imc-unknown-none-elf` (ESP32-C3/RISC-V), and other bare-metal targets.
- **Zero RAM consumption**: Memory-mapped flash lookup uses 0 bytes of heap and minimal stack space.
- **Predictable execution**: Looking up prayer times is a simple array index computation ($O(1)$ arithmetic), eliminating parsing jitter.

**Negative / accepted costs**

- `MawaqitClient` cannot run on bare-metal MCUs without an OS. MCUs that wish to perform live network updates must either use pre-packaged flash data or fetch raw data via their platform-specific HTTP client (`esp-idf-svc`, `embedded-nal-async`, AT commands) and feed the buffer to `parse_page` (if `alloc` is enabled).
- Two distinct representations: `ConfData` (verbose, wire-tolerant, dynamic `alloc`) and `CompactCalendarView` (packed, fixed-size, zero-alloc `heapless`).

**Alternatives rejected**

- **Separate crates (`mawaqit-core` + `mawaqit-client`)**: Adds repository fragmentation, multiple version numbers, and publishing overhead for minimal gain. Cargo features solve this in the same crate.
- **Parsing raw JSON on all MCUs**: Requires a dynamic heap and buffers capable of holding 25 KB JSON, which excludes smaller MCUs and violates zero-alloc firmware policies.
- **Only supporting `alloc` without `heapless`**: Excludes embedded systems that operate under strict zero-heap policies (e.g. automotive, safety-critical, or ultra-low-power microcontrollers).
