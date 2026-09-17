# Decision 0003 — Data, time, transaction and compatibility profile

Date: 2026-09-16

Status: accepted for format profile `uste-v1`; revisiting any item requires a migration
decision and the vectors in `acceptance/r0`.

Closes D-06 and is the decision artifact for T-01. It narrows the draft contracts without
changing their authority boundaries.

## Canonical values and identities

`uste-v1` uses a bespoke, length-delimited binary encoding implemented in `uste-types`; it is
not Serde's implementation-specific representation. Every envelope starts with `USTE`, a
record-kind byte, a major and minor schema version, and a checked payload length. Integers use
minimal unsigned LEB128 plus an explicit signed tag. Strings are well-formed UTF-8 and retain
their exact scalar sequence; the engine does not normalize Unicode. Maps have unique UTF-8
keys in unsigned byte order. Floats are not admitted to authoritative generic values in v1;
domain numeric profiles use checked integers/fixed-point values. Maximum nesting is 32,
collection entries 65,536, an inline string/byte value 1 MiB, and a transaction value 16 MiB.
Larger content uses the blob protocol. Decoders reject duplicate/out-of-order fields, unknown
required fields, non-minimal integers, trailing bytes and allocation before a length check.

Database, namespace, record, transaction, source-event and idempotency identities are tagged
128-bit values generated from the OS CSPRNG. Their external form is a type prefix and exactly
32 lowercase hexadecimal digits. IDs contain no timestamp or authority. A reference encodes
its database and namespace explicitly; implicit cross-namespace references are invalid.

The manifest names format major/minor, enabled features and every codec/profile. Readers may
ignore an unknown optional field only when its enclosing operation version declares that
field optional. An unknown major, record kind, required feature, operation version or reducer
version is `UnsupportedVersion`; it is never reinterpreted. Minor writers must not emit a
required semantic an older declared reader cannot understand.

## Time profile

`posix-utc-v1` is signed floor epoch seconds plus a nanosecond field `0..999_999_999`, bounded
to Gregorian dates `0001-01-01` through `9999-12-31`. Resolved external input is RFC 3339 with
an explicit numeric offset or `Z`; leap seconds and non-POSIX time scales are retained only as
unresolved/unsupported source envelopes. Numeric epochs require a declared unit. Canonical
output is UTC: `YYYY-MM-DDTHH:MM:SSZ` for whole seconds, otherwise a fractional field with
trailing zeroes removed. Source precision is separately recorded and is not inferred from
canonical output.

Named local zones use the vendored, hashed IANA profile `tzdb-2026c`. A fold requires an
explicit earlier/later choice; a gap is invalid; an offset/zone conflict is invalid. Naive
local times and date-only values remain unresolved unless an authorized interpretation is
recorded as a new version. Previously accepted normalized pairs are replayed directly and
never recalculated against a newer ruleset.

Commit revision is a checked `u64`, starts at 1, increments once per committed transaction,
and is the only database ordering authority. Exhaustion makes the database read-only.
Half-open valid intervals use independently tagged bounded/unbounded endpoints; unknown is a
different state. Wall observations and monotonic timeout readings cannot choose revision or
cryptographic nonce values.

## Transactions, conflicts and retries

The v1 write API is constrained to create, compare-and-replace, lifecycle transition, and
reference/edge mutation operations. Every mutation names an expected absence, exact current
record version, or exact read-view revision plus a declared predicate token. Unsupported
range predicates are rejected instead of being presented as serializable. Commit validates
schema, current authorization, quotas, references and all preconditions at the latest root.

One idempotency key is scoped to database, namespace and authenticated principal. It binds a
canonical request digest. The durable outcome is retained for the greater of 30 days or the
namespace history-retention interval, with a hard v1 maximum of 365 days. Reuse with another
digest is `Conflict`; reuse within retention returns the original outcome. After expiry the
API returns `IdempotencyExpired` and never guesses whether a retry is new. Batch source-event
keys are separately stable for the life of retained imported records.

Allowed assertion transitions are `proposed -> accepted|rejected` and
`accepted -> disputed|superseded|retracted|expired`. Terminal states do not transition in
place. Correction creates a new linked assertion. Purge is a retention operation, not a
lifecycle state. Relationship creation requires visible endpoints; deletion defaults to
reject while explicit bounded cascade/retract is atomic.

## Alternatives and consequences

CBOR and MessagePack were rejected for v1 because accepting their broad value spaces while
also defining one canonical, safely bounded subset adds dependency and interoperability
ambiguity. UUIDv7 was rejected because it leaks time. Floating generic numbers were rejected
because NaN, negative zero and representation-equivalence rules complicate canonical hashes.
The conservative retry horizon bounds retained outcomes but means clients must reconcile
expired uncertain operations using domain identities.

## Acceptance

`acceptance/r0/time.tsv` and `acceptance/r0/transitions.tsv` are literal normative vectors.
The standalone Rust tests in `tests/r0_vectors.rs` execute their ordering, negative-epoch,
interval and transition cases. VT-01, VT-02 and VT-17 must extend these vectors rather than
replace them when the production codec is implemented.
