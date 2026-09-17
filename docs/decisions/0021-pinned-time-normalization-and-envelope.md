# Decision 0021 — Pinned time normalization and source envelope

Date: 2026-09-17

Status: accepted and locally qualified for T-45. This implements the R1 kernel slice of FR-26
and Decision 0003. Temporal indexes and point-in-time query planning remain T-21; file-format
timestamp extraction remains T-24.

## Component and dependency boundary

`uste-types` continues to own only the invariant-bearing `UtcInstant` pair and remains std-only.
The new first-party safe-Rust `uste-time` crate owns strict parsing, normalization, source provenance,
canonical presentation and envelope conversion. It has no filesystem, environment, clock,
randomness or network capability.

Named-zone conversion uses exactly `jiff 0.2.37` with default features disabled and only `std`,
plus the directly embedded `jiff-tzdb 0.1.8` data. No Jiff system-zone, zoneinfo-directory,
concatenated-file or automatic bundle-selection feature is enabled. The admitted IANA release is
`2026c`. USTE hashes a domain-separated sequence consisting of the version and every canonical
zone name/TZif byte sequence in sorted name order; its SHA-256 is
`8e18c4fd3aad2c58d45340e356a02229ce6811c83427c5b086ad4468b7582209`. The normalizer cannot be
constructed until the embedded version and digest match. Local display also requires an explicit
zone identifier and uses the same verified object. Jiff is pure Rust but internally uses unsafe
tagged-pointer/`Arc` representation code; this is inventoried rather than hidden by USTE's
first-party `forbid(unsafe_code)` boundary.

Jiff supplies checked civil-date validation and TZif rule interpretation. USTE performs checked
Gregorian/epoch arithmetic itself so the accepted `0001-01-01` through `9999-12-31` UTC range is
not narrowed by a dependency's internal offset headroom. An exhaustive test round-trips all
3,652,059 admitted civil days.

## Strict normalization profile

`strict-rfc3339-v1` accepts ASCII four-digit years, uppercase `T`, seconds, an optional one-to-nine
digit fraction and uppercase `Z` or `+/-HH:MM`. Calendar errors, 24:00, excess fraction digits and
range-crossing offsets are invalid. RFC 3339 `-00:00` means an unknown offset and remains ambiguous;
it is never treated as UTC. A `:60` leap second is retained as unsupported. Numeric epochs require
an explicit seconds/milliseconds/microseconds/nanoseconds unit and use Euclidean floor division,
including negative nonmultiples. TAI, GPS, leap-aware UTC and smeared scales are retained as
unsupported rather than converted.

Local civil input resolves only with a checked numeric offset or an embedded IANA zone. A fold
requires an explicit earlier/later choice even when a supplied offset could identify an occurrence;
the selected occurrence must also match any supplied offset. A gap is invalid. A fold choice on an
unambiguous instant is invalid rather than ignored. Naive and date-only values remain ambiguous.
Successful conversion is not a claim that a source clock is accurate.

Canonical UTC output trims fractional trailing zeroes. Source precision is recorded separately.
Local presentation includes the canonical zone name, effective offset, TZDB version/digest and
`uste-local-presentation-v1` profile and never changes the stored instant.

## Source envelope and codec

`uste-time-envelope-v1` is a closed schema carried inside the existing canonical generic `Value`
frame; Decision 0012 assigns no new record kind. It records semantic role, scoped source artifact
and nonzero version, bounded locator and exact token, scale/unit/offset/zone/fold interpretation,
resolution status and reason, resolved pair only when resolved, source precision, independent
clock uncertainty, bounded authorized assumptions and the parser/normalization/TZDB profiles.
Private construction rejects inconsistent status/instant/reason, profile/zone, numeric-unit/
precision, scale and offset combinations.

The exact caps are: original token and locator 4 KiB each, zone identifier 255 bytes, assumption
statement 1 KiB and 16 assumptions. These are below the generic value codec's fixed limits. The
generic decoder performs its existing bounded allocation first; the time schema then applies its
narrower cap and rejects unknown, missing or extra fields. Format changes require a profile bump
and updated golden vector. The initial 849-byte fold-envelope fixture has SHA-256
`52920090900e58df57da9fb816a032b76c3a9d6c227f94a9c9605790923f5cc9`.

## Replay and ordering

Transactions journal the complete canonical envelope value. Replay and checkpoint recovery see
ordinary canonical reducer bytes: they neither parse the original token nor resolve a named zone.
Envelope decode accepts only the recorded known profiles and returns the stored `UtcInstant` pair.
Changing timezone rules can therefore create only a new correction/version, never mutate a prior
accepted interpretation.

Commit revision remains the only database ordering authority. Equal wall samples and a wall-clock
rollback produce distinct consecutive revisions; restart recovers the same order. Source event
time, commit wall observation and derivation availability remain distinct roles.

## Qualification and limits

Production tests execute every point-normalization row in `acceptance/r0/time.tsv`; graph tests
execute its unknown/unbounded interval rows. Tests cover offsets, negative units, all calendar
days, endpoint formatting and explicit-zone display, folds/gaps/conflicts, missing/date-only/
unsupported values, strict codec truncation and malformed profiles, graph cold replay and
equal/rollback wall samples across restart. `acceptance/r1/time-envelope-v1.tsv` binds the TZDB
and envelope goldens.

This does not implement temporal query indexes, parser-worker timestamp extraction, clock-drift
estimation or leap/TAI/GPS conversion tables. T-21/T-24 own those additions. T-62 remains an
independent external executable-distribution prerequisite and is not satisfied by this decision.
