# Authenticated bounded index-merge evidence

Decision 0032 adds a privileged current-frontier merge from an optional authenticated `index-v1`
base plus exact ordered before/after deltas into one unpublished encrypted run. The implementation
uses the same page writer as ordinary run publication and a single-handle cursor shared with the
complete-run visitor.

Focused verification on 2026-09-17:

~~~text
cargo test -p uste-storage --lib 'index_merge_' --locked --offline
# 3 passed; 0 failed
cargo test -p uste-storage --lib --locked --offline
# 59 passed; 0 failed
cargo test -p uste-txn --all-targets --locked --offline
# 27 passed; 0 failed
cargo clippy -p uste-storage -p uste-txn --all-targets --locked --offline -- -D warnings
# passed
bash scripts/check.sh
# workspace format/clippy/test/doc pass; 252 workspace tests and all isolated acceptance suites pass
~~~

The primary test builds a three-entry encrypted base with a value spanning multiple pages, applies
two insertions, one byte-exact replacement and one tombstone, publishes the returned terminal run
under the next certificate and visits the exact four-entry result. It then deletes every entry and
proves the merge returns no run, rejects a wrong expected value and non-increasing deltas, and
constructs a new family from a no-base stream. The same matrix rejects a divergent certificate,
non-current frontier, scope/profile/family mismatch, present/absent precondition conflicts,
duplicate deltas and independent base/delta/output budget overages. A maximum 4,096-byte key is
successfully written into the 16,384-byte logical page; the first byte over the public key limit is
rejected by the constructor. An empty-delta copy rewrites the exact four-entry family while opening
the authenticated source run exactly once.

The late-failure test loads a valid anchored root, corrupts a later encrypted source page, emits an
earlier insertion provisionally and then proves terminal authentication returns
`IntegrityFailure`. No target root exists; only optional orphan run bytes can remain.

A fault matrix injects crash-before and crash-after at target create, write, exact-size, file-sync
and directory-sync boundaries. After restart every case exposes only the previously published base
root; a complete target can remain only as an unreferenced optional run.

This is scratch-merge transport evidence, not graph-state integration or T-20 closure. The current
graph reducer, recovery candidate and ingest reducer still retain complete maps, candidate discovery
still performs an absolute-max scrub before caller reconstruction limits, and no allocator/RSS or
BM-01/BM-06 result is claimed.
