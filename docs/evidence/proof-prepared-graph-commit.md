# Proof-prepared authoritative graph commit evidence

Decision 0038 connects the complete authenticated graph preparation proof to the authoritative
journal coordinator without allowing request/prepared-state substitution.

## Verified behavior

- The disk-prepared record-plus-policy transaction produces its root plan before the opaque reducer
  result is consumed by the authoritative commit.
- Supplying that result with different canonical graph request bytes returns `InvalidRequest`
  before journal or reducer revision advancement.
- The exact request commits through the normal encrypted journal path. A second proof prepared on
  the old base and supplied under the same principal/idempotency/transaction identity returns the
  exact prior outcome, preserving normal retry semantics.
- A third old-base proof supplied under a new identity returns `Conflict` and does not advance the
  journal. The already committed proof-derived terminal root still publishes and admits normally.
- After storage restart, ordinary journal replay of the canonical request reconstructs exactly the
  state produced by the externally prepared commit. Subsequent disk proof preparation continues
  from that reopened revision.

~~~text
cargo test -p uste-graph --test disk_index bounded_disk_preparation_supports_current_history_reverse_and_stale_roots -- --exact
# 1 passed
cargo test -p uste-txn --all-targets
# 27 passed; 0 failed
cargo clippy -p uste-txn -p uste-graph --all-targets -- -D warnings
# passed
bash scripts/check.sh
# workspace format/clippy/test/rustdoc/docs pass; 262 workspace tests; documentation links=97,
# active IDs=94, definitions=146; 62-task graph remains acyclic with T-62 distribution-only
~~~

## Deliberate boundary

This removes duplicate complete-state preparation from one authoritative write path; it does not
remove the complete live reducer. Base validation and publication still access that reducer,
postcommit terminal-root validation still scans it, and restart replay reconstructs it. No new
journal/index/reducer profile exists, no external-prepared path is enabled for other reducers, and
no allocator/RSS or BM-01/BM-06 result is claimed. T-20 remains open.
