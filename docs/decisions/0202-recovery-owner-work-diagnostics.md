# Decision 0202: Diagnostics across trusted owner lifetimes

Date: 2026-09-20

Status: Accepted implementation; transaction regression, strict workspace lint and docs passed.

Expose the existing fixed-size vault decrypt and nonce reports on the privileged ordinary
`CommitCoordinator` and `AuthenticatedIndexRecovery` owners. Delegate the packed coordinator's
existing methods through the ordinary owner. Preserve uncertain-owner `OutcomeUnknown`,
storage-poison and key-vault refusal behavior. Never construct a reducer snapshot, perform I/O,
reset counters/nonces, create append authority or expose these methods through consumer facades.

These are per-vault cumulative observations. A consuming recovery-to-coordinator handoff keeps
the same vault/counters and must not be counted twice. Reopening creates another owner; a caller
must explicitly aggregate disjoint lifetimes. Reports omit key unwrap, other vaults, failed
pre-vault decoding, physical-device traffic and encryption-byte accounting. This increment only
enables later explicit attribution; it does not make any existing benchmark report complete.

Verify ordinary create/commit/reopen observations against the storage owner, no-snapshot/no-I/O
behavior, uncertain-owner refusal, recovery cursor work and exact counter continuity through
consuming packed handoff. Retain the original packed diagnostic/security tests. No on-disk,
authorization, nonce/session, memory-pilot or benchmark-target contract changes.

Verified all 141 transaction-crate tests, including three new diagnostic tests and the extended
uncertain-outcome case. The final source's three focused tests also passed after removing an
unused `mut`; workspace strict Clippy and warning-denied documentation passed. PROGRESS records
exact commands and resource limits. Consumer integration and benchmark-accounting completion
do not follow from this privileged diagnostic surface.
