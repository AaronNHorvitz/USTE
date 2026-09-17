# T-14 transaction-coordinator foundation evidence

Date: 2026-09-17 · scope: completed local T-14 acceptance, not production qualification

Reviewed implementation: commit `04751345b284901cf0a8943020b5ed4c3539c938` · tree
`a0ad75c5aa8ec78af2c7971d88237b11a9ca6f4d`. A read-only Codex agent review checked reducer
isolation, publication ordering, retry identity, retention ownership, recovery and time handling.
The reported high-severity API/isolation findings were corrected before this commit, and the final
review found no remaining blocker or high-severity issue. This is automated implementation review,
not independent transaction or security assessment.

Completed T-14 implementation: commit `834c5fdb984af9636d74d872675d48b5a9ba7082` · tree
`ec570a5213cbdc37eb5765297a460f4811788e6e`. A read-only delta review checked the literal format,
malformed recovery, 12-case publication fault matrix, both cancellation polls and 32-caller Linux
conflict test and found no blocker or high-severity code defect. It identified and this commit
corrected an evidence overclaim: these results complete T-14's VT-02 slice, while phantom-sensitive
graph predicate integration remains explicitly assigned to T-17.

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

## Verification

`cargo test -p uste-txn --all-targets` passes 8 tests covering the literal group golden and a
malformed field matrix; exact retry without a second revision; changed-key and stale-state
conflicts without mutation; both eligible cancellation polls; pinned-reader isolation; restart
equivalence; expiry; transaction-ID conflict; every initial group/certificate write/sync error,
crash-before and crash-after boundary; exact short writes; authenticated malformed recovery; and
lost-response recovery after a certificate-sync crash. A 32-thread Linux test admits exactly one
of 32 simultaneously submitted stale mutations and recovers that sole revision. The clock cases
reject adapter failure and demonstrate that wall-clock rollback does not control revision order.
`cargo clippy -p uste-txn --all-targets -- -D warnings` passes.

## Deliberate later boundaries

The owned snapshot profile is a correctness implementation and intentionally not a scalability
claim. Blob inventory publication remains T-15, authorization remains T-16, graph semantics remain
T-17, and scalable MVCC plus compaction of retained outcome/tombstone indexes remain T-29/T-35.
In particular, T-17 must map the reference model's declared predicate tokens and
`UnsupportedPredicate` outcomes onto this durable coordinator and exercise phantom-sensitive graph
operations before the complete VT-02 suite can pass.
