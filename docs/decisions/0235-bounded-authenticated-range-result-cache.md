# Decision 0235: Bounded authenticated range-result cache

Date: 2026-09-22

Status: Implemented and locally verified; benchmark-mode integration and measurement pending.

Decision 0234 supplies exact cursor path-depth work. Add an explicitly opt-in complete-range
result partition to `PackedPageCache`, carved from the same declared total as pages and positive
lookups. Existing page-only and page/lookup constructors are unchanged. The new constructor
requires valid nonzero page, lookup and range partitions; `PackedCacheReport` reports range budget,
accounted bytes, resident ranges/entries, hits, misses, evictions and oversized bypasses. Total
accounted bytes include all enabled partitions exactly once and cannot exceed the declared total.

A range identity covers database, namespace, profile, family, revision, root locator, expected
entry count/logical bytes/digest, traversal direction and both byte-exact bounds, including absent
versus present upper bounds. The enclosing cache retains its certificate-owner and unlocked-key-
session binding. Session change, key lock or privileged clear drops pages, lookup values and ranges
together while preserving cumulative counters. Retained identity, keys and values use zeroizing
buffers; result copies cannot borrow from the cache.

Only a completed successful storage cursor result is admitted. Failed reads, partial traversal,
malformed ordering/ranges, inconsistent reports and duplicate admission never create a resident range.
Oversized results are successful non-admissions counted once. Entries are conservatively charged
with their buffers and container allowance; variable-size LRU eviction occurs before admission.
Hits validate the ordinary cursor request maxima and then require stored path depth, candidates,
returned bytes, pages and encoded bytes to fit the caller's limits before returning any entry.
They replay the exact successful `TreeCursorReport`, including boundary witnesses and value chunks,
while performing no packed-page I/O or decryption.

Expose a privileged whole-range maintenance operation and a trusted authorized-reader constructor.
Packed graph expansion selects it only when the new range partition exists. Page-only and positive-
lookup readers retain their compact streaming cursor path and behavior. Range-enabled graph scans
validate every cached key/value shape and compact to fixed relationship/neighbor identifiers before
shared semantics; authorization, cancellation, aggregate charging, ordering, self-loop behavior and
point-lookup handling remain unchanged. No persisted format or consumer-controlled cache selection
is added.

Tests cover total-budget refusal/accounting, full identity/direction/bounds separation, exact and
one-less admission for all five cursor limits, global-limit refusal before a warm probe, forward and
reverse results, zero-I/O/zero-decryption warm hits, variable-size LRU eviction, oversized bypass,
malformed order/report refusal, failed-read non-admission, owner/session lock clearing, privileged graph
configuration and unchanged cache clearing semantics. One fixture expectation initially assumed a
nonexistent `c` key; the actual reference keys (`a`, `ab`, `b`) showed the implementation's reverse
`b, ab` result was correct, and the test was corrected to derive its expectation from the reference.

The complete optimized gate passed **764 workspace tests**, strict workspace Clippy,
warnings-denied documentation, documentation/task validators, vectors/publication checks and
isolated tooling. The standalone T-20 driver passed **136 active tests with five unchanged opt-in
ignores** and strict Clippy. Log: `/tmp/uste-d235-workspace-verification.log`. It used one Cargo job,
one test thread, offline dependencies and the 4 GiB process address-space limit. The enclosing
shared scope retained its earlier 5,372,850,176-byte memory and 286,691,328-byte swap peaks;
soft-limit events reached 92,262, with zero maximum-limit, OOM or CPU-throttle events. Shared values
are not process RSS.

No performance, T-20, M1 or qualification gate claim follows. Add a distinct benchmark command,
profile/schema and fixed partition sizes before measuring this mode; do not reinterpret or replace
the existing page-only or positive-lookup observations.
