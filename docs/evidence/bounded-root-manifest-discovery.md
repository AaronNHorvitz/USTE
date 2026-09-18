# Bounded root-manifest discovery evidence

Decision 0059 removes the absolute-maximum run scrub that previously occurred before graph recovery
could apply its caller-selected admission limits.

## Verified behavior

- Storage authenticates at most the two fixed `index-v1` root manifests, validates their exact
  scope/profile/bindings and run-file lengths, orders generations and rejects conflicting equal-
  generation manifests without reading run pages.
- Transaction and authenticated-recovery owners expose that operation only as trusted provisional
  discovery. Exact journal certificate-chain filtering still occurs before a manifest is returned.
- Graph candidate loading now uses provisional manifests. Complete run cursors enforce the supplied
  aggregate page, entry and logical-byte limits, recompute every run digest and withhold the disk
  base until semantic and canonical-state validation finishes.
- The storage regression mutates an authenticated run page while preserving file length. Both root
  manifests remain discoverable, the fully scrubbed compatibility loader admits none, and cursor,
  predecessor and explicit scrub operations reject the damaged bytes.
- The fully scrubbed publication path still filters unusable roots before comparing generations,
  preserving its old/new fallback invariant.

~~~text
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M \
  bash -lc 'CARGO_BUILD_JOBS=1 cargo test -p uste-storage -p uste-txn -p uste-graph \
    --all-targets --locked --offline -- --test-threads=1'
# uste-graph: 43 passed; 0 failed
# uste-storage: 77 passed; 0 failed
# uste-txn: 28 passed; 0 failed

systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M \
  bash -lc 'CARGO_BUILD_JOBS=1 cargo clippy -p uste-storage -p uste-txn -p uste-graph \
    --all-targets --locked --offline -- -D warnings'
# passed

systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M \
  bash -lc 'CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 bash scripts/check.sh'
# passed; documentation=ok (132 links, 129 active IDs, 152 definitions); task_graph=ok;
# scaled T-20: 31 passed, 2 exact-profile acceptance cases ignored
~~~

## Deliberate boundary

The graph admission operation restarts rather than checkpointing a partially consumed semantic
scan after process loss. Root publication retains its complete fallback scrub. Coordinator retry,
transaction and blob-owner maps still replay from the journal origin into memory, and only zero or
one graph suffix is supported. This evidence does not close T-20 or qualify BM-01/BM-06.
