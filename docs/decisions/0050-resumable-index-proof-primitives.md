# Decision 0050 — Resumable authenticated index-proof primitives

Date: 2026-09-17

Status: accepted as bounded T-20 cold-admission groundwork. T-20 remains open because graph
semantic admission, the live graph/coordinator state and recovery remain memory-resident and
BM-01/BM-06 are unqualified.

## Context

Cold admission of a `graph-state-v1` candidate must stream every family while resolving current
and historical reference requirements from the same authenticated root. The existing complete-run
visitor holds the mutable filesystem borrow for the entire callback, so graph code cannot
interleave exact or historical lookups. Consumer prefix scans are not a substitute for historical
absence proof: their one-million-entry/64-MiB caps can truncate a valid long record history.

## Decision

The existing single-entry/single-page authenticated run reader is exposed as an opaque
`IndexRunCursor<F>`. `JournalStore`, `CommitCoordinator` and `AuthenticatedIndexRecovery` provide
open, next and finish operations. The cursor owns no filesystem borrow between calls, so trusted
semantic code can pause after one decoded entry and perform another authenticated lookup.

Entries remain provisional. Opening validates the certificate anchor, scope/database, immutable
run descriptor, limits and file length. `next` yields at most one assembled entry and retains at
most one encrypted page/entry. Only exhaustion authenticates terminal entry count, order, logical
bytes, run digest and stable file length; consuming `finish` releases the report only after that
terminal state. Early drop or finish grants no report, and coordinator outcome uncertainty and
scope checks remain in force.

Storage also provides an authenticated predecessor proof: the greatest complete key beginning
with a required prefix and less than or equal to a required upper-bound key. It is not constrained
by consumer prefix-result caps. A binary page search locates the boundary, bounded backtracking
finds any fragment start, and a two-pass local scan first proves the terminal matching key and then
assembles only that value. One explicit limit caps all page visits, including cache hits and both
passes; another caps the returned key/value bytes before value allocation.

The two-pass form is required. Applying the byte cap while scanning earlier matching entries would
incorrectly reject a small final predecessor after a large earlier value and could retain two
values simultaneously. Discovery therefore retains only one bounded key; the second pass applies
the result cap exactly once to the selected value.

## Evidence and limits

The storage regression pauses a cursor after its first entry, performs an authenticated exact
lookup, resumes it and proves byte-for-byte/report equality with the existing complete-run
visitor. It rejects early finish, late page corruption, first-page corruption and appended length;
no failed cursor yields a terminal report.

The same fixture proves predecessor absence before the first matching key, an exact/between-key
multi-page result, selected-page and preceding-page corruption, invalid prefix/bound pairing,
exact-minus byte/page limits and the critical large-earlier/small-final case. The final small value
succeeds under a cap that cannot hold the earlier fragmented value.

These are trusted proof primitives, not consumer query APIs and not graph semantic admission by
themselves. The next increment must share iterator-oriented graph validators, stream all eight
families, resolve historical requirements through predecessor proofs and return a `GraphDiskBase`
only after the candidate's logical digest and every semantic invariant succeed. Coordinator
metadata, retry maps, suffix replay and the live reducer remain full-memory boundaries.

## Verification

~~~text
cargo test -p uste-storage --lib --locked \
  journal::tests::encrypted_index_runs_round_trip_large_values_with_bounded_cache_and_root_fallback -- --exact
# 1 passed
cargo test -p uste-storage --lib --locked
# 59 passed
cargo test -p uste-txn --all-targets --locked
# 27 passed
cargo clippy -p uste-storage -p uste-txn --all-targets --locked -- -D warnings
# passed
bash scripts/check.sh
# 263 workspace tests passed; documentation=ok with 112 links, 109 active IDs and 146
# definitions; task_graph=ok; 12 R0 vectors, 4 storage-publication tests, 4 fixture-generator
# tests, 4 content-fixture tests and 31 isolated T-20 tests passed; 2 exact-profile debug tests
# remained intentionally ignored
~~~
