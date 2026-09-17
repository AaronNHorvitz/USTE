# Time normalization and ordering

Draft contract · 2026-09-16 · Not implemented

Owns FR-26 and the time semantics shared by FR-05/07/20/24. This contract defines
requirements; D-06 must approve the exact codec, supported ranges, dependencies and literal
acceptance vectors before implementation. UTC is a shared time reference, not the absence
of a time standard and not proof that source clocks are correct.

## One timeline, preserved source meaning

Normalize every resolvable real-world instant to UTC for comparison and indexing. Preserve
the original timestamp and its interpretation separately; never rewrite original file bytes.
For example, `2026-09-16T09:00:00-05:00` and `2026-09-16T14:00:00Z` identify the same instant.
Display may convert that instant into an explicitly selected user timezone without changing
its stored value. Never interpret source values using the desktop's current timezone.
Local-time output must include zone identifier, offset and formatting/rule profile. A query
may request source-local or viewer-local presentation; neither changes the canonical instant.
An object's coordinates do not silently choose its timezone. Physics tick-to-UTC mappings
are explicit and versioned under [physics and motion](physics-and-motion.md).

Each timestamp observation carries a bounded envelope:

- Semantic role and exact source artifact/version/field or locator.
- Original text or numeric token, declared numeric units and calendar/time scale.
- Supplied UTC offset and/or IANA zone identifier, without inventing missing information.
- Resolution status: resolved, missing, ambiguous, invalid or unsupported; bounded reason.
- Normalized instant only when resolved; explicit assumptions and their authority if used.
- Source precision separately from clock accuracy/uncertainty, which may be unknown.
- Parser/normalization profile version and, when used, timezone database/conversion version.

An inference made by a model is a proposed interpretation with provenance, not a trusted
timestamp. A correction creates a new version linked to the original interpretation.

## Representation and limits

The proposed initial profile, `posix-utc-v1`, uses signed 64-bit epoch seconds plus an
unsigned nanosecond fraction in `0..999999999`. Fractional negative instants use floor
seconds: half a second before the epoch is seconds `-1`, fraction `500000000`.
Do not use floating-point timestamps or an unchecked signed 64-bit total-nanoseconds field.
Ordering compares the numeric pair, not arbitrary source strings. The exact supported
calendar range and binary encoding remain D-06 decisions; overflow fails explicitly.

External resolved instants use the supported RFC 3339 UTC `Z` form, with canonical fractional
formatting fixed by D-06. Padding a fraction does not imply greater source accuracy.
Numeric epochs require explicit units; do not guess seconds versus milliseconds by magnitude.

POSIX time does not uniquely represent leap seconds. Initial normalization must explicitly
reject a `:60` leap-second value or unsupported TAI/GPS/smeared time as an ordinary instant,
while permitting retention of its original token and unsupported status. Never silently
clamp, discard or pretend to convert it. Future converters require versioned conversion
tables, declared uncertainty and compatibility tests. Elapsed-time measurement uses an
appropriate monotonic clock, not subtraction of POSIX wall timestamps across clock changes.

## Distinct meanings of time

| Field / concept | Meaning and authority |
|---|---|
| Source event time | When the source claims an event occurred; may be late, wrong or unresolved |
| Source created / modified / published time | Separate document metadata claims, not proof of occurrence or public availability |
| Valid interval | When an assertion applies in the modeled world, with explicit bounds and interpretation |
| Received time | Host clock observation at a documented ingest boundary; record clock/profile provenance |
| Recorded revision | Authoritative database commit order and first visibility of that version |
| Commit wall observation | Informational clock sample at a defined commit-path boundary, not the exact durability instant |
| Derivation availability | Revision at which extracted or computed output became available, distinct from source age |
| Simulation time | Explicit virtual clock belonging to a branch/model, not an observed world timestamp |

Valid intervals are half-open `[start, end)`. Unbounded endpoints must be explicitly tagged;
an unknown date is not an unbounded interval, epoch zero or the current time. Date-only values,
local wall times, durations and recurring schedules are distinct types, not fabricated UTC
instants. A domain may explicitly resolve a local date into a zone-specific interval with
recorded policy; a day need not be exactly 24 hours.

## Ambiguity and timezone rules

- A known numeric offset can resolve an ordinary local timestamp directly.
- An IANA zone requires the rules for that date and a pinned timezone database version.
- A daylight-saving fold with two possible instants stays ambiguous until explicitly resolved.
- A nonexistent local time in a daylight-saving gap is invalid; do not silently shift it.
- A zone/offset conflict is surfaced; do not pick one silently. Abbreviations alone may be ambiguous.
- Naive times with no trusted zone context remain unresolved. Ingest may still preserve the file.
- Updating timezone rules never silently changes previously accepted normalized instants.
  Reinterpretation produces a versioned correction. Future schedules preserve local intent
  and zone separately and use an explicit rule-update policy when resolved.
- Relative audio/video offsets stay relative unless a trustworthy absolute origin is supplied.

Explicit-offset inputs do not require timezone network access. Named-zone rules must be
available through a pinned local profile; no automatic download is permitted.

## Ordering, clocks and reproducibility

Commit revisions, not wall clocks, define database order. Equal timestamps and clock rollback
must not lose, merge or reorder commits. Source sequence identifiers can establish order only
within their declared source/session/channel scope; they do not establish global causality.
UTC conversion cannot correct clock drift, transport delays or forged document metadata.
Overlapping uncertainty intervals do not justify a claim about strict real-world ordering.

Use monotonic clocks for in-process elapsed durations and timeouts within their clock domain.
Do not compare monotonic values across restarts or machines without an explicit mapping.
Security-sensitive expiry needs a declared clock-trust and rollback policy in D-02/D-04;
time alone must not supply cryptographic uniqueness or authorization.

Journal accepted normalized values and interpretation provenance. Replay never queries the
current clock or reparses timestamps with whatever timezone library happens to be installed.
Unknown required profiles fail explicitly. Time indexes and summaries follow corrections
transactionally; raw source timestamps are not silently substituted for recorded revisions.

## Point-in-time queries

Queries specify the knowledge revision and, when needed, the valid-time instant/interval.
At revision 100, a document first imported at revision 120 is unavailable even if its stated
publication time is earlier. A derivation first committed at revision 130 is also unavailable
at revision 120. Corrections learned later cannot leak into an earlier knowledge view.
An explicit hypothetical branch may model different assumptions but must be labeled as such.

Wall-clock cutoffs are not automatically interchangeable with revision cutoffs: clock jumps
and uncertainty can make that mapping ambiguous. Return explicit limits or require a revision.
Queries must state whether unresolved timestamps are excluded or returned in a separate group;
never silently sort them as zero/current time. Missing retained history remains HistoryUnavailable.
These semantics support honest historical research; they do not certify a source's actual
publication time or make a backtest a prediction of future results.

## Acceptance and references

VT-17 covers equivalent offsets, negative epochs, precision/range checks, declared units,
folds/gaps, zone conflicts, missing/date-only values, unsupported time scales, timezone-rule
updates, equal timestamps, clock rollback/restart, and source/derivation knowledge boundaries.
R1 requires the type/normalization/replay subset; R2 adds temporal-query and parser integration.
T-01 freezes the profile; T-45 implements the shared kernel; T-21 integrates temporal queries.

Standards references: [RFC 3339](https://www.rfc-editor.org/rfc/rfc3339) defines interoperable
Internet timestamps; the [IANA Time Zone Database](https://www.iana.org/time-zones) supplies
versioned timezone rules. These references do not choose the engine's durability or ordering policy.
