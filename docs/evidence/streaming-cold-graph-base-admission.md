# Streaming cold graph-base admission evidence

Decision 0051 turns a cold authenticated `graph-state-v1` candidate into an operational
`GraphDiskBase` without reconstructing the complete graph maps.

## Verified behavior

- Live-coordinator and authenticated-recovery owners admit the same certificate-bound candidate.
- All eight families are cursor-exhausted and terminally authenticated before a base is returned.
- History shape, lifecycle transitions, historical/current reference closure, current/history
  equality, derived-family membership/counts, policy history and canonical digest are checked.
- The admitted base carries the exact root, counts and policy, owns no I/O/key capability, redacts
  sensitive debug state and drives the existing bounded disk-preparation API.
- Memory beyond the caller page cache is one cursor entry/page, one current record and one
  caller-bounded history group; complete current/history/secondary maps are not built.
- Exact/predecessor operation counts, page visits, returned bytes, history-group shape and semantic
  reference comparisons are all checked aggregate budgets.
- Exact-minus semantic, lookup-page and lookup-byte fixtures fail with `ResourceLimit`; malformed
  yet storage-authenticated current/history and secondary-family candidates fail as corrupt.
- Authenticated exact lookup now enforces page and result limits before unbounded traversal or value
  allocation, including a fragmented multi-page value.

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

## Deliberate boundary

Candidate discovery and the initial carrier scrub still use absolute storage maxima. The live
coordinator/reducer, retry/outcome/blob-owner metadata and recovery suffix are still memory-
resident. Decision 0051 therefore proves cold semantic admission and operational handoff only; it
does not prove a disk-backed live reducer, larger-than-memory recovery or qualifying BM-01/BM-06
performance.
