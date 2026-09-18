# Admitted terminal-root handoff evidence

Decision 0049 keeps the semantically admitted result of proof-only terminal publication usable as
the next disk base without a complete live-snapshot comparison.

## Verified behavior

- Storage returns the exact recovered handle represented by the synchronized root manifest; the
  legacy publication API still returns the same revision/generation receipt.
- Coordinator checks for outcome uncertainty, scope and current certificate remain in force.
- Graph wraps the recovered root only after all eight proof-derived merges, output validation,
  family counts and the canonical logical digest have succeeded.
- The disk-preparation integration fixture retains that opaque handle across filesystem restart and
  coordinator reopen, then proves both a bounded failure and a successful next transaction from it.
- Existing cold reconstruction and full-state equivalence coverage remains unchanged.

~~~text
cargo test -p uste-graph --test disk_index --locked
# 4 passed
cargo test -p uste-storage --lib --locked \
  journal::tests::every_index_root_publication_boundary_recovers_an_old_or_exact_new_root -- --exact
# 1 passed
cargo clippy -p uste-storage -p uste-txn -p uste-graph --all-targets --locked -- -D warnings
# passed
bash scripts/check.sh
# 263 workspace tests and 31 isolated t20-bench tests passed; 2 exact-profile release tests ignored
~~~

## Deliberate boundary

The handoff proves no new cold candidate. Retaining an in-memory opaque descriptor is not process
recovery, and a descriptor recovered from disk still needs full semantic admission. Implementing
that admission requires a resumable authenticated run cursor, bounded predecessor lookup and shared
iterator-oriented graph validation before a cold `GraphDiskBase` can replace full-map recovery.
