# ADR-0015: `iqama_at` is mosque-local wall clock; the zone comes from `ConfData::timezone()`

- **Status:** Accepted
- **Date:** 2026-10
- **Decides:** What the `iqama_at` instants mean in absolute terms, where
  the mosque's timezone comes from, and how DST edge cases are handled.
- **Raised by:** red-team round 2, finding **F28** ("no timezone contract
  for iqama_at").
- **Related:** [ADR-0002](0002-one-page-one-year.md),
  [ADR-0010](0010-display-time-contract.md),
  [`specs/calendar-resolution.md`](../specs/calendar-resolution.md).

## Context

`TodayTimes::iqama_at` reports the five iqama prayers as
`chrono::NaiveDateTime`. That type carries no zone by design — it is the
rollover-correct *wall clock* of the mosque (C1: a "+600" after a 23:30
adhan belongs to the next day, and the wall-clock day is the adhan's
calendar day). Before this ADR nothing said which zone those wall clocks
live in, so an alarm scheduler had to guess: the user's local zone? UTC?
The mosque's zone from somewhere? A wrong guess shifts every alarm — a
safety bug by the project's own definition.

The mosque's zone is also DST-ambiguous twice over: in the spring-forward
gap a wall-clock time simply does not exist, and a mosque's published
times do not move with the consumer's zone.

## Decision

1. **`iqama_at` is mosque-local wall clock.** It answers "what does the
   clock on the mosque's wall read when iqama starts", with C1 rollover
   already applied. It is deliberately not `DateTime<Utc>`: the library
   does not fabricate an absolute instant it cannot know.

2. **The zone source is `ConfData::timezone()`.** The accessor reads the
   page's `timezone` field (the same field the official screens use) and
   returns it only when it is a plausible IANA designator: non-empty, at
   most 64 bytes, characters `[A-Za-z0-9_./+-]`, never an absolute path,
   never a `..` segment. Anything else — missing, non-string, hostile —
   is `None`, i.e. "the mosque publishes no zone". The validation is a
   shape check, not a tz-database lookup: the library stays
   dependency-free and only bounds what reaches a caller's resolver.

3. **DST edge cases are the consumer's, with a documented rule.** A caller
   converts `iqama_at` + `timezone()` to an absolute instant with chrono's
   `LocalResult`: *raise* (`single`/`None`) rather than guess when a time
   falls in the spring-forward gap; on the ambiguous autumn hour, the
   *earlier* offset is the safe choice for alarms (an alarm at the later
   interpretation fires up to an hour late, never early — and a prayer
   alarm that is late is less harmful than one that lies about having
   passed). The library never picks an offset itself.

4. **No fabrication.** When `timezone()` is `None` the caller must not
   silently substitute its own zone for *display claims* — it may use the
   user's zone as an explicit product decision, but the data the library
   ships stays "mosque-local wall clock, zone unpublished".

## Consequences

**Positive**

- Alarm schedulers have one sanctioned zone source; the ambiguity F28
  flagged is a documented contract instead of an accident.
- A hostile `timezone` value (a path, 4 MB of junk, control characters)
  collapses to `None` — it can never reach a tz resolver.

**Negative / accepted**

- Consumers that ignore this ADR and interpret `NaiveDateTime` in the
  wrong zone are wrong by hours; the library cannot force the check. The
  ut pin (`finding_f28_timezone_accessor_validates_the_wire_designator`)
  pins the accessor; the conversion is documented, not enforced.
- The wire key (`timezone`) is confirmed against the live site in the HIL
  campaign; if mawaqit renames the field, the accessor degrades to `None`
  (safe) and the key is corrected here.
