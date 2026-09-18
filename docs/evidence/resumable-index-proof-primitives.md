# Resumable authenticated index-proof evidence

Decision 0050 supplies the storage/recovery primitives required to validate a cold graph candidate
without holding the filesystem borrow across a complete-run callback.

## Verified behavior

- `IndexRunCursor` retains at most one complete entry and one decrypted page, releases the
  filesystem borrow between calls and yields a terminal report only after full count/order/digest/
  length authentication.
- Cursor operations preserve journal certificate, coordinator uncertainty and namespace scope
  checks through both live-coordinator and authenticated-recovery surfaces.
- Exact lookups can be interleaved while a cursor is paused; resumed output and accounting equal
  the established complete-run visitor.
- Early finish, late corruption, first-page corruption and appended run length fail without a
  terminal report.
- Predecessor proof returns the greatest prefix-matching key at or before an upper bound without
  using capped consumer prefix scans.
- One checked limit covers binary search, backtracking and both proof passes; one result limit is
  enforced before allocating only the final selected value.
- A fragmented large earlier match cannot exhaust the result cap or coexist with a later small
  predecessor; selected and preceding-page corruption fail closed.

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

## Deliberate boundary

These primitives do not decide graph semantics and do not create an admitted base. Cold graph
admission still must validate current/history equality, lifecycle transitions, reference closure,
derived-family correspondence, policy ordering and the canonical digest while fully exhausting
every cursor. Live coordinator state and recovery remain full-memory until later increments.
