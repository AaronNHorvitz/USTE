# T-14 transaction-coordinator foundation evidence

Date: 2026-09-17 · scope: partial T-14 evidence, not task completion or production qualification

## Implemented

- New safe-Rust `uste-txn` crate with a domain-neutral deterministic reducer boundary.
- Side-effect-free preparation of owned changes, ordered encrypted journal publication and
  infallible post-certificate live-state publication.
- Namespace/principal/idempotency scoping, transaction uniqueness, exact request/result digests,
  coordinator-owned 30–365-day retry policy, explicit clock sampling and a 10-million-outcome
  namespace cap. Policy changes affect new commits without invalidating historical groups.
- Exact retry and transaction outcome lookup, explicit expiry, coherent pinned read snapshots and
  restart reconstruction from authenticated `UTXN` groups.
- Pre-publication cancellation and fail-closed uncertain-outcome quarantine: after an ambiguous
  append, both reads and writes return `OutcomeUnknown` until recovery.

## Focused verification

`cargo test -p uste-txn --all-targets` passes exact retry without a second revision, changed-key and
stale-state conflicts without mutation, cancellation, pinned-reader isolation across a later
commit, restart equivalence, expiry, transaction-ID conflict, and lost-response recovery after a
certificate-sync crash. `cargo clippy -p uste-txn --all-targets -- -D warnings` passes.
The clock cases also reject adapter failure and demonstrate that wall-clock rollback does not
control revision ordering.

## Remaining T-14 work

The full journal operation/error matrix, concurrent caller stress, literal group goldens,
malformed-group recovery cases and cancellation at every eligible boundary remain. The owned
snapshot profile is a correctness implementation and intentionally not a scalability claim.
Authorization remains T-16; graph semantics remain T-17; compaction of retained outcome/tombstone
indexes remains T-35.
