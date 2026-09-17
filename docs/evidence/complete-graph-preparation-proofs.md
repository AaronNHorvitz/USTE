# Complete bounded graph preparation proof evidence

Decision 0036 extends Decision 0035's explicit-I/O phase with authenticated complete history and
reverse-prefix proofs while preserving the storage-free reducer phase.

## Verified behavior

- An accepted assertion referencing an entity produces one reverse-owner proof. A two-operation
  transaction retracts that base dependency and then deletes under `Reject`, proving the partial
  overlay suppresses the stale reverse descriptor; its digest equals the durable commit outcome.
- A `ReadView` replacement loads the target's complete history bucket, evaluates the historical
  and current predicates in the existing reducer, rejects a historically visible record that is
  currently deleted, and matches the durable commit digest for an unchanged predicate.
- Zero history/reverse entry budgets reject the corresponding one-entry proof before a partial
  bucket can become preparation input.
- Current positive/negative proof, correction-ID, syntactic/dynamic closure, exact logical-byte,
  stale-root and result-digest coverage from Decision 0035 remains in the same fixture.

~~~text
cargo test -p uste-graph --test disk_index bounded_disk_preparation_supports_current_history_reverse_and_stale_roots -- --exact
# 1 passed
cargo clippy -p uste-graph --all-targets -- -D warnings
# passed
bash scripts/check.sh
# workspace format/clippy/test/rustdoc/docs pass; 262 workspace tests; documentation links=93,
# active IDs=90, definitions=146; 62-task graph remains acyclic with T-62 distribution-only
~~~

## Deliberate boundary

This does not connect the proof result to the coordinator commit path or derive a terminal root
delta from the partial view. The live coordinator and admitted-root comparison still retain the
complete graph. Prefix collection is capped but not streaming reducer evaluation, per-prefix index
results remain capped at 64 MiB, and no allocator/RSS or BM-01/BM-06 result is claimed. T-20 stays
open.
