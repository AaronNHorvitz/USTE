# Decision 0051 — Streaming cold graph-base admission

Date: 2026-09-17

Status: accepted as a bounded T-20 semantic-admission increment. T-20 remains open because the
live graph reducer, coordinator metadata, candidate discovery/scrub and recovery suffix remain
memory-resident, and BM-01/BM-06 are unqualified.

## Context

Decision 0050 supplied resumable authenticated family cursors and historical predecessor proofs,
but an authenticated root was still only a storage object. A cold root must not become an
operational graph base until its primary histories, current records, derived families, policy
history and canonical logical digest are mutually coherent. Reconstructing complete maps to make
that decision would preserve the full-RAM assumption T-20 is intended to remove.

## Decision

`GraphDiskBase` is the capability-free result of cold semantic admission. It retains the exact
`DerivedGraphStateRoot`, eight authenticated state counts and current namespace policy. It owns no
filesystem, journal, key or authorization capability. Its debug representation redacts scope and
counts, and its admitted root can immediately drive the existing bounded disk-preparation path.

Admission works through either a live `CommitCoordinator` or `AuthenticatedIndexRecovery` owner.
It streams and terminally authenticates every nonempty `graph-state-v1` family. At most one cursor
entry/page, one current record and one caller-bounded record-history group are retained, in
addition to the caller-owned bounded page cache. It checks:

- metadata, family presence/counts and exact root identity;
- every history key/version, first-state rule and legal successor transition;
- historical reference closure with authenticated predecessor proofs at the required revision;
- equality of each terminal history record and its current-record exact proof;
- current reference closure, lifecycle/kind rules and correction targets;
- exact outgoing, incoming, provenance and reverse-entry correspondence plus derived counts;
- policy ordering and complete terminal/current policy equality; and
- the canonical logical-state digest reproduced from the fully exhausted family streams.

The in-memory checkpoint constructor and cold admission share history-transition and reference-
requirement functions so their semantic rules cannot drift independently.

All proof work is caller bounded. Exact-key reads now accept `IndexGetLimits`; their binary search,
cache hits and value-fragment reads consume a page-visit budget, and the authenticated total value
length is rejected before allocation when it exceeds the result cap. Admission additionally caps
aggregate exact/predecessor operations, lookup page visits, returned bytes, history-group versions
and bytes, and semantic reference/evidence comparisons. Storage-originated resource-limit errors
are normalized at the graph admission boundary.

## Rejected alternatives

Reconstructing `GraphState` during admission was rejected because it would make the new API a
renamed full-memory recovery path. Trusting family counts or the root digest without entry-level
semantic checks was rejected because a self-consistent encrypted cache can still encode an invalid
world. Re-deriving all secondary entries for every observed secondary entry was rejected because
it creates quadratic unaccounted work; admission instead uses aggregate counts plus one bounded
owner-local membership check per authenticated entry.

## Evidence and limits

The graph fixture admits the same root through live and recovery owners, checks its policy/counts/
anchor and uses it for disk preparation. It rejects a current/history mismatch and a malformed
derived entry even when storage authentication succeeds. Exact-minus history-group, proof-count,
semantic-visit, lookup-page and lookup-byte limits fail closed. Storage coverage separately proves
fragmented exact values reject page and byte limits before uncontrolled work/allocation.

This decision does not make candidate discovery or carrier scrub caller-bounded, move the
coordinator's live reducer/retry maps to disk, replay a suffix into a disk overlay, or establish
larger-than-memory recovery. It is not BM-01/BM-06 evidence and does not close T-20.

## Verification

~~~text
cargo test -p uste-storage --all-targets --locked
# 77 passed
cargo test -p uste-txn --all-targets --locked
# 27 passed
cargo test -p uste-graph --all-targets --locked
# 42 passed
cargo clippy -p uste-storage -p uste-txn -p uste-graph --all-targets --locked -- -D warnings
# passed
bash scripts/check.sh
# 264 workspace tests passed; documentation=ok with 114 links, 111 active IDs and 146
# definitions; task_graph=ok; 12 R0 vectors, 4 storage-publication tests, 4 fixture-generator
# tests, 4 content-fixture tests and 31 isolated T-20 tests passed; 2 exact-profile debug tests
# remained intentionally ignored
~~~
