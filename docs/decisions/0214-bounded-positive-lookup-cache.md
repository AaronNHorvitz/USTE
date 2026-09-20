# Decision 0214: Bounded positive packed-lookup reuse

Date: 2026-09-20

Status: Implemented and locally verified; not qualified or enabled in native benchmarks.

Decision 0211 measures substantial repeated lookup cost. Add an explicitly opt-in positive
result cache inside the existing privileged packed cache, preserving the default page-only
constructor. Partition a single admitted total budget between pages and lookup results; do
not add an unreported allocation or assume the whole graph fits. Report the partition,
resident accounting and separate result-cache hits/misses/evictions/bypasses. Logical accounting
is not RSS, physical I/O or erasure. Oversized positive values bypass retention, not query limits.

Reuse only a complete successful authenticated positive lookup, never a partial proof, failure
or negative result. Identity includes exact database/namespace, profile, family, logical revision,
physical root locator and full expected commitment, plus exact key bytes. The enclosing cache
must still validate the live certificate owner and unlocked vault session before lookup. Clear,
owner/session rules, current authorization and cancellation remain unchanged. A cache entry is
not authority or an alternative to canonical-root admission. Stored values are still decoded
and filtered by the domain on every use; no consumer result/permission cache is introduced.

Retain the successful lookup's exact proof-work report and recheck every requested limit on
hits. Return the same page, encoded-byte, branch and chunk work units so aggregate admission
does not depend on warmth. Keep upper-cap validation ahead of reuse. Preserve uncached APIs,
cursor behavior and page-only configuration. Warm immutable values can survive later disk
mutation under Decision 0162's contract; clearing/uncached reads must reauthenticate.

Use bounded ordered maps with exact recency, shared zeroizing keys and zeroizing values; no
history-sized recency log. Charge retained capacities plus declared fixed/entry metadata.
Transient key/result copies remain bounded by existing key/value caps and separate from resident
cache accounting. Checked arithmetic and invalid diagnostic reporting must not fabricate totals.

Require reference LRU/byte accounting, every identity component, exact/minus-one proof limits,
oversized bypass, owner/session/lock/clear controls, late corruption, injected faults and full
storage/transaction/graph regression before selecting this mode in a consumer or benchmark.
No on-disk format, cryptographic primitive, nonce registry, M1 interface, qualifying size,
performance target or reserved-host requirement changes.

Ten new tests cover these boundaries, including a 10,000-access independent variable-byte LRU
trace and shared page-only/positive-cache encrypted fault, owner/session and corruption fixtures.
The full workspace passes 746 tests across 47 executables, strict Clippy and warnings-denied
documentation. Exact commands, baseline and process-group resource observations are in PROGRESS.md.
