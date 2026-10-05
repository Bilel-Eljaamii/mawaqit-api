//! MQTC compact binary codec (ADR-0013): a fixed-layout, 4-byte-aligned
//! prayer calendar that bare-metal firmware queries straight from
//! memory-mapped flash with **zero heap allocation**.
//!
//! Two halves, one module (ADR-0014 layering):
//!
//! - **Runtime view** ([`CompactCalendarView`]) — zero-copy borrowing parser
//!   over `&[u8]`; every query is O(1) and allocation-free, built for
//!   `#![no_std]` MCUs (`heapless` tier). Total parsing: hostile, truncated or
//!   corrupted input yields `Err`/`None`, never a panic.
//! - **Builder** ([`CompactCalendarBuilder`], `alloc` tier) — the packer side
//!   that turns day records into `.bin`/`.rs`/`.h` assets before flashing.
//!
//! Layout (normative text: `docs/specs/compact-binary-and-mcu.md`): a
//! 24-byte header (`MQTC`, v1, CRC-32-IEEE) followed by per-day records —
//! 24 bytes direct (O(1) flash reads) or 20 bytes fajr-relative (smaller
//! flash, decoded on the stack in registers).
//!
//! ```
//! use chrono::NaiveDate;
//! use mawaqit_api::compact::{
//!     CompactCalendarBuilder, CompactCalendarView, CompactDayInput,
//!     CompactIqamaInput, ScopeType,
//! };
//!
//! let mut b = CompactCalendarBuilder::new(
//!     NaiveDate::from_ymd_opt(2026, 10, 5).unwrap(),
//!     ScopeType::Week,
//!     false,
//! );
//! b.push_day(CompactDayInput {
//!     adhan: [330, 450, 780, 990, 1140, 1260],
//!     iqama: [
//!         Some(CompactIqamaInput { minutes: 345, rollover: false }),
//!         Some(CompactIqamaInput { minutes: 795, rollover: false }),
//!         Some(CompactIqamaInput { minutes: 1005, rollover: false }),
//!         Some(CompactIqamaInput { minutes: 1155, rollover: false }),
//!         Some(CompactIqamaInput { minutes: 10, rollover: true }),
//!     ],
//!     day_flags: 0,
//! });
//! let bytes = b.to_bytes().unwrap();
//!
//! let view = CompactCalendarView::from_bytes(&bytes).unwrap();
//! let day = view
//!     .times_for_date(NaiveDate::from_ymd_opt(2026, 10, 5).unwrap())
//!     .unwrap();
//! assert_eq!(day.adhan.fajr.to_hhmm().as_str(), "05:30");
//! assert_eq!(day.iqama.unwrap().isha.is_rollover(), true);
//! ```

use core::fmt::Write as _;

#[cfg(any(feature = "std", feature = "alloc"))]
use chrono::Datelike;
use chrono::{Days, NaiveDate};

/// Magic bytes every MQTC payload starts with.
pub const MAGIC: [u8; 4] = *b"MQTC";
/// The only wire version this codec reads and writes.
pub const VERSION: u8 = 0x01;

const HEADER_LEN: usize = 24;
const RECORD_LEN_DIRECT: usize = 24;
const RECORD_LEN_FAJR_REL: usize = 20;
const TIME_MASK: u16 = 0x07FF;
const VALID_BIT: u16 = 0x4000;
const ROLLOVER_BIT: u16 = 0x8000;
/// Iqama bitfield bits 12–13: reserved, must be zero on the wire. A set
/// bit marks the record corrupt beyond the CRC (FINDING F13).
const UNDEFINED_IQAMA_BITS: u16 = 0x3000;
const NO_JUMUA: u16 = 0xFFFF;
const MINUTES_PER_DAY: u16 = 1440;
/// Delta-record iqama offset sentinel: no iqama for this prayer.
const NO_IQAMA_OFFSET: u8 = 0xFF;
/// Largest iqama offset the fajr-relative record can carry (0xFF is the
/// absent sentinel).
const MAX_IQAMA_OFFSET: u16 = 254;

const FLAG_IMSAK: u8 = 0x01;
const FLAG_HAS_IQAMA: u8 = 0x02;
const FLAG_HAS_JUMUA: u8 = 0x04;
const FLAG_FAJR_REL: u8 = 0x08;

/// How the packed scope was selected (informational; the layout is
/// identical for all scopes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeType {
    Week,
    Months,
    Year,
    Custom,
}

impl ScopeType {
    /// Wire byte (see the spec's header table).
    pub const fn to_byte(self) -> u8 {
        match self {
            ScopeType::Week => 0,
            ScopeType::Months => 1,
            ScopeType::Year => 2,
            ScopeType::Custom => 3,
        }
    }

    /// Unknown bytes decode as [`ScopeType::Custom`] — the byte is
    /// informational and must not break loading known-good data written by
    /// a newer packer.
    pub const fn from_byte(b: u8) -> Self {
        match b {
            0 => ScopeType::Week,
            1 => ScopeType::Months,
            2 => ScopeType::Year,
            _ => ScopeType::Custom,
        }
    }
}

/// Every way loading or packing MQTC data can fail. Total parsing: no
/// variant is reachable by panicking — hostile input always maps to one of
/// these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CompactError {
    #[error("bad magic: not MQTC data")]
    InvalidMagic,
    #[error("unsupported MQTC version {0}")]
    UnsupportedVersion(u8),
    #[error("buffer smaller than the MQTC payload requires")]
    BufferTooSmall,
    #[error(
        "CRC-32 mismatch: header says {expected:#010x}, bytes hash to {computed:#010x} — corrupt flash write or flipped bits"
    )]
    ChecksumMismatch { expected: u32, computed: u32 },
    #[error("start_year/start_day_of_year do not resolve to a valid date")]
    InvalidDate,
    #[error(
        "time value {minutes} is out of range 0..=1439 (prayer index {prayer})"
    )]
    TimeOutOfRange { prayer: u8, minutes: u16 },
    #[error(
        "fajr-relative record cannot encode delta {delta} at prayer index {prayer} (> 1439 from fajr)"
    )]
    DeltaOverflow { prayer: u8, delta: u16 },
    #[error(
        "iqama offset {delta} minutes cannot be encoded in one byte at prayer index {prayer} (max {MAX_IQAMA_OFFSET}); use the direct format"
    )]
    IqamaOffsetOverflow { prayer: u8, delta: u16 },
    #[error("{0} day records exceed the u16 wire field")]
    TooManyDays(usize),
}

/// One wall-clock time slot. Plain adhan times store minutes from midnight
/// (0..=1439); iqama fields additionally carry two status bits around the
/// same 11-bit minute value (C1 rollover fix):
///
/// - bit 15: `ROLLOVER` — the instant belongs to the *next* calendar day
/// - bit 14: `VALID` — an iqama is configured for this prayer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactTime(pub u16);

impl CompactTime {
    /// Minutes from midnight of the display day (0..=1439).
    pub const fn minutes_from_midnight(&self) -> u16 {
        self.0 & TIME_MASK
    }

    /// Wall-clock hour (0..=23) of the display day.
    pub const fn hours(&self) -> u8 {
        (self.minutes_from_midnight() / 60) as u8
    }

    /// Wall-clock minute (0..=59) of the display day.
    pub const fn minutes(&self) -> u8 {
        (self.minutes_from_midnight() % 60) as u8
    }

    /// Whether this iqama instant belongs to the next calendar day (C1).
    pub const fn is_rollover(&self) -> bool {
        self.0 & ROLLOVER_BIT != 0
    }

    /// Whether an iqama is configured for this prayer.
    pub const fn is_valid(&self) -> bool {
        self.0 & VALID_BIT != 0
    }

    /// Strict `HH:MM` of the display day with zero heap allocations.
    ///
    /// Precondition (ADR-0010 display contract): the minute value is
    /// `0..=1439`. That holds for every value this module produces — the
    /// builder rejects anything larger at pack time and the decoders drop
    /// records that carry it (FINDING F13) — but a caller hand-crafting a
    /// `CompactTime` from raw wire bits must apply the same mask/check
    /// first; for iqama fields, check [`CompactTime::is_valid`] before
    /// formatting.
    pub fn to_hhmm(&self) -> heapless::String<5> {
        let mut s = heapless::String::<5>::new();
        let _ = write!(s, "{:02}:{:02}", self.hours(), self.minutes());
        s
    }

    /// Pack an iqama display time + rollover into the wire bitfield.
    const fn packed_iqama(minutes: u16, rollover: bool) -> u16 {
        (minutes & TIME_MASK)
            | VALID_BIT
            | if rollover { ROLLOVER_BIT } else { 0 }
    }
}

/// The six adhan times of one day (minutes from midnight each).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactAdhanTimes {
    pub fajr: CompactTime,
    pub shurouq: CompactTime,
    pub dhuhr: CompactTime,
    pub asr: CompactTime,
    pub maghrib: CompactTime,
    pub isha: CompactTime,
}

/// The five iqama times of one day. Absent prayers keep their per-field
/// validity bit unset ([`CompactTime::is_valid`] is `false`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactIqamaTimes {
    pub fajr: CompactTime,
    pub dhuhr: CompactTime,
    pub asr: CompactTime,
    pub maghrib: CompactTime,
    pub isha: CompactTime,
}

/// One decoded day: adhan times, optional iqama block, and the day flags
/// byte (bit 0: mosque-level custom Jumu'ah override).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactDayTimes {
    pub adhan: CompactAdhanTimes,
    pub iqama: Option<CompactIqamaTimes>,
    pub day_flags: u8,
}

/// Zero-copy runtime view over MQTC bytes — flash-resident data, queried
/// in O(1) with zero heap. Load validates magic, version, bounds, the
/// CRC-32 checksum, and the start date; after that, reads are infallible.
#[derive(Debug, Clone, Copy)]
pub struct CompactCalendarView<'a> {
    data: &'a [u8],
    scope: ScopeType,
    flags: u8,
    start_year: u16,
    start_doy: u16,
    day_count: u16,
    jumua: u16,
    jumua2: u16,
}

impl<'a> CompactCalendarView<'a> {
    /// Validate and borrow an MQTC payload. Ordered checks per the spec:
    /// length, magic, version, expected size, CRC-32, start-date
    /// resolvability.
    pub fn from_bytes(data: &'a [u8]) -> Result<Self, CompactError> {
        if data.len() < HEADER_LEN {
            return Err(CompactError::BufferTooSmall);
        }
        if data[0..4] != MAGIC {
            return Err(CompactError::InvalidMagic);
        }
        if data[4] != VERSION {
            return Err(CompactError::UnsupportedVersion(data[4]));
        }
        let day_count = u16::from_le_bytes([data[12], data[13]]);
        let record_len = if data[6] & FLAG_FAJR_REL != 0 {
            RECORD_LEN_FAJR_REL
        } else {
            RECORD_LEN_DIRECT
        };
        let expected_len = HEADER_LEN + day_count as usize * record_len;
        if data.len() < expected_len {
            return Err(CompactError::BufferTooSmall);
        }

        let mut header = [0u8; HEADER_LEN];
        header.copy_from_slice(&data[..HEADER_LEN]);
        header[0x14..0x18].fill(0);
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&header);
        hasher.update(&data[HEADER_LEN..expected_len]);
        let computed = hasher.finalize();
        let expected = u32::from_le_bytes([
            data[0x14], data[0x15], data[0x16], data[0x17],
        ]);
        if computed != expected {
            return Err(CompactError::ChecksumMismatch { expected, computed });
        }

        let start_year = u16::from_le_bytes([data[0x08], data[0x09]]);
        let start_doy = u16::from_le_bytes([data[0x0A], data[0x0B]]);
        if Self::resolve_date(start_year, start_doy).is_none() {
            return Err(CompactError::InvalidDate);
        }

        Ok(Self {
            data,
            scope: ScopeType::from_byte(data[5]),
            flags: data[6],
            start_year,
            start_doy,
            day_count,
            jumua: u16::from_le_bytes([data[0x0E], data[0x0F]]),
            jumua2: u16::from_le_bytes([data[0x10], data[0x11]]),
        })
    }

    fn resolve_date(year: u16, doy: u16) -> Option<NaiveDate> {
        if doy == 0 || doy > 366 {
            return None;
        }
        NaiveDate::from_yo_opt(year as i32, doy as u32)
    }

    /// The payload's scope byte decoded (unknown bytes are `Custom`).
    pub const fn scope_type(&self) -> ScopeType {
        self.scope
    }

    /// Raw header flags byte.
    pub const fn flags(&self) -> u8 {
        self.flags
    }

    /// `flags & 0x01` — the mosque displays the imsak line.
    pub const fn imsak_mode(&self) -> bool {
        self.flags & FLAG_IMSAK != 0
    }

    /// `flags & 0x02` — any iqama data is packed.
    pub const fn has_iqama(&self) -> bool {
        self.flags & FLAG_HAS_IQAMA != 0
    }

    /// `flags & 0x04` — header Jumu'ah times are present.
    pub const fn has_jumua(&self) -> bool {
        self.flags & FLAG_HAS_JUMUA != 0
    }

    /// `flags & 0x08` — records are fajr-relative (20 B) rather than
    /// direct (24 B).
    pub const fn is_fajr_relative(&self) -> bool {
        self.flags & FLAG_FAJR_REL != 0
    }

    /// First packed day.
    pub fn start_date(&self) -> Result<NaiveDate, CompactError> {
        Self::resolve_date(self.start_year, self.start_doy)
            .ok_or(CompactError::InvalidDate)
    }

    /// Last packed day.
    pub fn end_date(&self) -> Result<NaiveDate, CompactError> {
        let start = self.start_date()?;
        start
            .checked_add_days(Days::new(self.day_count as u64 - 1))
            .ok_or(CompactError::InvalidDate)
    }

    /// Number of consecutive day records.
    pub const fn day_count(&self) -> u16 {
        self.day_count
    }

    /// Friday 1st prayer (minutes from midnight), or `None` (`0xFFFF`).
    pub fn jumua(&self) -> Option<CompactTime> {
        (self.jumua != NO_JUMUA).then_some(CompactTime(self.jumua))
    }

    /// Friday 2nd prayer (minutes from midnight), or `None` (`0xFFFF`).
    pub fn jumua2(&self) -> Option<CompactTime> {
        (self.jumua2 != NO_JUMUA).then_some(CompactTime(self.jumua2))
    }

    /// O(1) lookup of one day's times. `None` when the date is outside
    /// `[start_date, end_date]` — a lookup miss is `None`, not an error
    /// (spec: total parsing, no fabrication).
    pub fn times_for_date(&self, date: NaiveDate) -> Option<CompactDayTimes> {
        let start = self.start_date().ok()?;
        let delta_days = (date - start).num_days();
        if delta_days < 0 || delta_days >= self.day_count as i64 {
            return None;
        }
        let index = delta_days as usize;
        let record_len = if self.is_fajr_relative() {
            RECORD_LEN_FAJR_REL
        } else {
            RECORD_LEN_DIRECT
        };
        let offset = HEADER_LEN + index * record_len;
        let record = &self.data[offset..offset + record_len];
        if self.is_fajr_relative() {
            Self::decode_fajr_relative(record.try_into().ok()?)
        } else {
            Self::decode_direct(record.try_into().ok()?)
        }
    }

    /// Decode a direct-format record. Strict beyond the CRC (FINDING F13):
    /// minutes ≥ 24:00, reserved bits 12–13, or a rollover bit without a
    /// VALID bit mark the record corrupt — the day is dropped (`None`),
    /// never clamped or fabricated (ADR-0010: degradation, never
    /// fabrication).
    fn decode_direct(
        record: &[u8; RECORD_LEN_DIRECT],
    ) -> Option<CompactDayTimes> {
        let mut adhan = [0u16; 6];
        for (i, slot) in adhan.iter_mut().enumerate() {
            *slot = u16::from_le_bytes([record[i * 2], record[i * 2 + 1]]);
            // Adhan fields carry no flag bits: anything ≥ 24:00 (including
            // any bit ≥ 12) is corrupt beyond CRC.
            if *slot > MINUTES_PER_DAY - 1 {
                return None;
            }
        }
        let mut iqama = [0u16; 5];
        let mut any_valid = false;
        for (i, slot) in iqama.iter_mut().enumerate() {
            *slot = u16::from_le_bytes([
                record[0x0C + i * 2],
                record[0x0D + i * 2],
            ]);
            if *slot & UNDEFINED_IQAMA_BITS != 0
                || *slot & TIME_MASK > MINUTES_PER_DAY - 1
                || (*slot & VALID_BIT == 0 && *slot != 0)
            {
                return None;
            }
            any_valid |= *slot & VALID_BIT != 0;
        }
        Some(CompactDayTimes {
            adhan: Self::adhan_from(adhan),
            iqama: any_valid.then(|| Self::iqama_from(iqama)),
            day_flags: record[0x16],
        })
    }

    /// Decode a fajr-relative record by re-adding offsets in registers —
    /// no buffer, no heap, ~20 bytes of stack.
    fn decode_fajr_relative(
        record: &[u8; RECORD_LEN_FAJR_REL],
    ) -> Option<CompactDayTimes> {
        let fajr = u16::from_le_bytes([record[0x00], record[0x01]]);
        let mut adhan = [fajr; 6];
        for (i, slot) in adhan.iter_mut().enumerate().skip(1) {
            let offset = u16::from_le_bytes([record[i * 2], record[i * 2 + 1]]);
            if offset > MINUTES_PER_DAY - 1 {
                return None; // corrupt beyond CRC: reject, never fabricate
            }
            *slot = fajr + offset;
        }
        let mut iqama = [0u16; 5];
        let mut any_valid = false;
        for (i, slot) in iqama.iter_mut().enumerate() {
            let offset = record[0x0C + i];
            if offset == NO_IQAMA_OFFSET {
                continue;
            }
            let abs = adhan[Self::IQAMA_SLOTS[i]] as u32 + offset as u32;
            let rollover = abs >= MINUTES_PER_DAY as u32;
            *slot = CompactTime::packed_iqama(
                (abs % MINUTES_PER_DAY as u32) as u16,
                rollover,
            );
            any_valid = true;
        }
        Some(CompactDayTimes {
            adhan: Self::adhan_from(adhan),
            iqama: any_valid.then(|| Self::iqama_from(iqama)),
            day_flags: 0,
        })
    }

    /// Which adhan slot each iqama field follows (no iqama for shurouq).
    const IQAMA_SLOTS: [usize; 5] = [0, 2, 3, 4, 5];

    const fn adhan_from(mins: [u16; 6]) -> CompactAdhanTimes {
        CompactAdhanTimes {
            fajr: CompactTime(mins[0]),
            shurouq: CompactTime(mins[1]),
            dhuhr: CompactTime(mins[2]),
            asr: CompactTime(mins[3]),
            maghrib: CompactTime(mins[4]),
            isha: CompactTime(mins[5]),
        }
    }

    const fn iqama_from(packed: [u16; 5]) -> CompactIqamaTimes {
        CompactIqamaTimes {
            fajr: CompactTime(packed[0]),
            dhuhr: CompactTime(packed[1]),
            asr: CompactTime(packed[2]),
            maghrib: CompactTime(packed[3]),
            isha: CompactTime(packed[4]),
        }
    }
}

/// One iqama entry as the packer provides it: display-day minutes plus an
/// explicit rollover flag (the packer derives it from the resolved
/// instants — a "+600" after 23:30 belongs to the next day).
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactIqamaInput {
    pub minutes: u16,
    pub rollover: bool,
}

/// One day's records as the packer provides them: six adhan times in
/// minutes from midnight, five optional iqama entries (fajr, dhuhr, asr,
/// maghrib, isha), and the day-flags byte.
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Debug, Clone, Copy)]
pub struct CompactDayInput {
    pub adhan: [u16; 6],
    pub iqama: [Option<CompactIqamaInput>; 5],
    pub day_flags: u8,
}

/// Packer-side builder (alloc tier): collects validated day records and
/// serializes MQTC v1 payloads in either record format, plus the `.rs` and
/// `.h` source emitters the pre-flash tool writes out.
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Debug, Clone)]
pub struct CompactCalendarBuilder {
    start: NaiveDate,
    scope: ScopeType,
    fajr_relative: bool,
    imsak_mode: bool,
    jumua: Option<u16>,
    jumua2: Option<u16>,
    days: alloc::vec::Vec<CompactDayInput>,
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl CompactCalendarBuilder {
    /// A builder for `start`-anchored data. `fajr_relative` selects the
    /// 20-byte record format (smaller flash, one-byte iqama offsets) over
    /// the 24-byte direct format (no offset limit).
    pub fn new(
        start: NaiveDate,
        scope: ScopeType,
        fajr_relative: bool,
    ) -> Self {
        Self {
            start,
            scope,
            fajr_relative,
            imsak_mode: false,
            jumua: None,
            jumua2: None,
            days: alloc::vec::Vec::new(),
        }
    }

    /// Set the `imsak_mode` header flag (mosque displays the imsak line).
    pub const fn with_imsak_mode(mut self, on: bool) -> Self {
        self.imsak_mode = on;
        self
    }

    /// Friday prayer times in minutes from midnight (`0..=1439`).
    pub fn with_jumuah(
        mut self,
        first: Option<u16>,
        second: Option<u16>,
    ) -> Self {
        self.jumua = first;
        self.jumua2 = second;
        self
    }

    /// Append one day record (validated at [`Self::to_bytes`] time).
    pub fn push_day(&mut self, day: CompactDayInput) {
        self.days.push(day);
    }

    /// Number of packed days so far.
    pub fn len(&self) -> usize {
        self.days.len()
    }

    /// Whether no day has been pushed yet.
    pub fn is_empty(&self) -> bool {
        self.days.is_empty()
    }

    /// Encode one day as a direct-format record. The direct format carries
    /// every wall-clock value and the rollover bit explicitly, so its only
    /// invariants are the `0..=1439` ranges — no ordering, no offset limit.
    fn encode_day_direct(
        day: &CompactDayInput,
    ) -> Result<[u8; RECORD_LEN_DIRECT], CompactError> {
        let mut record = [0u8; RECORD_LEN_DIRECT];
        for (i, &mins) in day.adhan.iter().enumerate() {
            if mins > MINUTES_PER_DAY - 1 {
                return Err(CompactError::TimeOutOfRange {
                    prayer: i as u8,
                    minutes: mins,
                });
            }
            record[i * 2..i * 2 + 2].copy_from_slice(&mins.to_le_bytes());
        }
        for (i, entry) in day.iqama.iter().enumerate() {
            let Some(entry) = entry else {
                continue; // absent iqama stays 0x0000 (VALID bit unset)
            };
            let adhan_slot = CompactCalendarView::IQAMA_SLOTS[i];
            if entry.minutes > MINUTES_PER_DAY - 1 {
                return Err(CompactError::TimeOutOfRange {
                    prayer: adhan_slot as u8,
                    minutes: entry.minutes,
                });
            }
            let packed =
                CompactTime::packed_iqama(entry.minutes, entry.rollover);
            record[0x0C + i * 2..0x0C + i * 2 + 2]
                .copy_from_slice(&packed.to_le_bytes());
        }
        record[0x16] = day.day_flags;
        Ok(record)
    }

    /// Encode one day as a fajr-relative record: adhan times as offsets
    /// from fajr (u16), iqama as minutes-after-adhan (u8, `0xFF` = absent).
    /// Its invariants are the direct ones plus: ascending adhan (a negative
    /// gap has no encoding) and iqama offsets within one byte.
    fn encode_day_fajr_rel(
        day: &CompactDayInput,
    ) -> Result<[u8; RECORD_LEN_FAJR_REL], CompactError> {
        let mut record = [0u8; RECORD_LEN_FAJR_REL];
        let fajr = day.adhan[0];
        record[0x00..0x02].copy_from_slice(&fajr.to_le_bytes());
        for i in 1..6 {
            let offset = day.adhan[i] as i32 - fajr as i32;
            // Non-ascending input (e.g. shurouq before fajr) has no
            // fajr-relative encoding — reject instead of wrapping.
            if offset < 0 || offset > (MINUTES_PER_DAY - 1) as i32 {
                return Err(CompactError::DeltaOverflow {
                    prayer: i as u8,
                    delta: offset.unsigned_abs() as u16,
                });
            }
            record[i * 2..i * 2 + 2]
                .copy_from_slice(&(offset as u16).to_le_bytes());
        }
        for (i, entry) in day.iqama.iter().enumerate() {
            let Some(entry) = entry else {
                record[0x0C + i] = NO_IQAMA_OFFSET;
                continue;
            };
            let adhan_slot = CompactCalendarView::IQAMA_SLOTS[i];
            if entry.minutes > MINUTES_PER_DAY - 1 {
                return Err(CompactError::TimeOutOfRange {
                    prayer: adhan_slot as u8,
                    minutes: entry.minutes,
                });
            }
            let adhan_min = day.adhan[adhan_slot] as i32;
            let abs = if entry.rollover {
                entry.minutes as i32 + MINUTES_PER_DAY as i32
            } else {
                entry.minutes as i32
            };
            let offset = abs - adhan_min;
            if offset < 0 || offset > MAX_IQAMA_OFFSET as i32 {
                return Err(CompactError::IqamaOffsetOverflow {
                    prayer: adhan_slot as u8,
                    delta: offset.unsigned_abs() as u16,
                });
            }
            record[0x0C + i] = offset as u8;
        }
        Ok(record)
    }

    /// Serialize the MQTC v1 payload: 24-byte header (CRC-32-IEEE over the
    /// header with the CRC field zeroed plus all day records) followed by
    /// the day records.
    pub fn to_bytes(&self) -> Result<alloc::vec::Vec<u8>, CompactError> {
        if self.days.len() > u16::MAX as usize {
            return Err(CompactError::TooManyDays(self.days.len()));
        }
        let record_len = if self.fajr_relative {
            RECORD_LEN_FAJR_REL
        } else {
            RECORD_LEN_DIRECT
        };
        let mut out = alloc::vec::Vec::with_capacity(
            HEADER_LEN + self.days.len() * record_len,
        );

        let mut flags = 0u8;
        if self.imsak_mode {
            flags |= FLAG_IMSAK;
        }
        if self.days.iter().any(|d| d.iqama.iter().any(Option::is_some)) {
            flags |= FLAG_HAS_IQAMA;
        }
        if self.jumua.is_some() || self.jumua2.is_some() {
            flags |= FLAG_HAS_JUMUA;
        }
        if self.fajr_relative {
            flags |= FLAG_FAJR_REL;
        }

        let mut header = [0u8; HEADER_LEN];
        header[0x00..0x04].copy_from_slice(&MAGIC);
        header[0x04] = VERSION;
        header[0x05] = self.scope.to_byte();
        header[0x06] = flags;
        header[0x07] = 0;
        header[0x08..0x0A].copy_from_slice(
            &(Datelike::year(&self.start) as u16).to_le_bytes(),
        );
        header[0x0A..0x0C]
            .copy_from_slice(&(self.start.ordinal() as u16).to_le_bytes());
        header[0x0C..0x0E]
            .copy_from_slice(&(self.days.len() as u16).to_le_bytes());
        header[0x0E..0x10]
            .copy_from_slice(&self.jumua.unwrap_or(NO_JUMUA).to_le_bytes());
        header[0x10..0x12]
            .copy_from_slice(&self.jumua2.unwrap_or(NO_JUMUA).to_le_bytes());
        // header[0x12..0x14] stays zero (alignment padding); the CRC field
        // at 0x14..0x18 stays zero for the checksum input.
        out.extend_from_slice(&header);

        for day in &self.days {
            let record: &[u8] = if self.fajr_relative {
                &Self::encode_day_fajr_rel(day)?
            } else {
                &Self::encode_day_direct(day)?
            };
            out.extend_from_slice(record);
        }

        let computed = {
            let mut hasher = crc32fast::Hasher::new();
            hasher.update(&out[..HEADER_LEN]);
            hasher.update(&out[HEADER_LEN..]);
            hasher.finalize()
        };
        out[0x14..0x18].copy_from_slice(&computed.to_le_bytes());
        Ok(out)
    }

    /// Emit the payload as a Rust source file:
    /// `pub static NAME: &[u8] = &[...];` — compiled straight into MCU
    /// firmware and mapped from flash.
    pub fn to_rust_code(
        &self,
        const_name: &str,
    ) -> Result<alloc::string::String, CompactError> {
        let bytes = self.to_bytes()?;
        let mut out = alloc::format!(
            "// Generated by mawaqit-api pack_for_mcu — MQTC v1, do not edit.\n\
             pub static {const_name}: &[u8] = &[\n"
        );
        for chunk in bytes.chunks(16) {
            out.push_str("    ");
            for byte in chunk {
                let _ = write!(out, "{byte},");
            }
            out.push('\n');
        }
        out.push_str("];\n");
        Ok(out)
    }

    /// Emit the payload as a C header (`uint8_t` array + length macro) for
    /// ESP-IDF / STM32 firmware.
    pub fn to_c_header(
        &self,
        const_name: &str,
    ) -> Result<alloc::string::String, CompactError> {
        let bytes = self.to_bytes()?;
        let mut out = alloc::format!(
            "// Generated by mawaqit-api pack_for_mcu — MQTC v1, do not edit.\n\
             #pragma once\n\
             #include <stdint.h>\n\n\
             #define {const_name}_LEN {len}\n\
             static const uint8_t {const_name}[{len}] = {{\n",
            len = bytes.len(),
        );
        for chunk in bytes.chunks(16) {
            out.push_str("    ");
            for byte in chunk {
                let _ = write!(out, "0x{byte:02X},");
            }
            out.push('\n');
        }
        out.push_str("};\n");
        Ok(out)
    }
}
