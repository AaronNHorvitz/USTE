# Proof-derived terminal root delta evidence

Decision 0037 connects the complete Decision 0036 transaction proof to the Decision 0033 bounded
terminal-root merge without reacquiring a complete graph snapshot during precommit derivation.

## Verified behavior

- The explicit-I/O proof authenticates and retains the root anchor, the canonical 80-byte metadata
  entry, its eight graph-state counters (including separate policy history/current policy) and the
  exact current policy. Metadata key/value bytes and its authenticated lookup are charged to the
  existing aggregate proof report.
- The capability-free phase prepares a transaction that both creates a referenced assertion and
  replaces policy, then derives all terminal family deltas without any I/O capability.
- The proof-derived and complete-live-state paths report identical coalesced delta counts and
  logical bytes for that transaction.
- After the normal encrypted durable commit, the proof-derived plan passes the exact outcome/base
  checks, bounded family merge and independent complete-state descriptor validation. The resulting
  root is the only revision-two root and is admitted against the live reducer.
- Existing exact and exact-minus-one proof budgets include the new metadata proof. Stale roots,
  complete history/reverse buckets, correction IDs and overlay deletion behavior remain covered by
  the same end-to-end fixture.

~~~text
cargo test -p uste-graph --test disk_index bounded_disk_preparation_supports_current_history_reverse_and_stale_roots -- --exact
# 1 passed
cargo test -p uste-graph --test disk_index
# 4 passed
cargo clippy -p uste-graph --all-targets -- -D warnings
# passed
bash scripts/check.sh
# workspace format/clippy/test/rustdoc/docs pass; 262 workspace tests; documentation links=95,
# active IDs=92, definitions=146; 62-task graph remains acyclic with T-62 distribution-only
~~~

## Deliberate boundary

This is a precommit disk-proof-to-root-plan bridge, not yet a disk-backed transaction coordinator.
The authoritative commit still uses the complete live reducer, and terminal publication deliberately
retains its complete postcommit comparison. Recovered reducer state remains full-memory; proof
buckets are collected within limits; allocator/RSS and BM-01/BM-06 are not qualified. T-20 remains
open.
