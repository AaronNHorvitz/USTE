# Decision 0052 — Warm disk-backed live graph state

Date: 2026-09-17

Status: accepted as a bounded T-20 live-write increment. T-20 remains open because pending-state
process recovery, coordinator metadata, candidate discovery/scrub and recovery suffixes remain
memory-resident, and BM-01/BM-06 are unqualified.

## Context

Decision 0051 can admit a cold graph root and the proof loader can prepare one transaction from it,
but the authoritative coordinator still publishes into a complete `GraphState`. Keeping that
reducer only to bridge journal commit and terminal-root publication preserves the full-RAM live
write boundary. Replacing it must not let an optional index become commit authority or permit a
second mutation against a stale root after the journal has advanced.

## Decision

`GraphDiskLiveState` is a warm, one-pending-commit reducer. A coordinator may consume a complete
`GraphState` and replace only its reducer representation after that authoritative source validates
the proposed disk state's scope, revision, policy, canonical logical digest and exact journal
certificate anchor. The source fixes its one allowed equivalent destination as an associated type;
a downstream destination cannot self-attest or add an escape transition from pending disk state.
Journal ownership, retry outcomes, transaction metadata, blob ownership, retention and recovery
health remain in the same coordinator.

Only a `GraphDiskCommit` produced from an exact disk preparation proof and its bounded terminal-
root plan can enter this reducer. Ordinary request preparation is unsupported. The normal
coordinator sequence still validates the canonical request, appends and certifies the journal,
then publishes reducer state. Publication retains the request-bounded plan as pending and hides the
now-stale base. Exact idempotent retry remains available from coordinator metadata; every distinct
commit and every new proof load fails while pending.

Postcommit maintenance receives only a disjoint immutable reducer borrow and a narrow index
capability. It can read the journal anchor, merge invisible runs and publish an authenticated root;
it cannot append a transaction or change retry metadata. The existing streamed output validator
reproduces counts and the canonical digest. Only the exact outcome/root/count/policy/certificate
tuple can be installed as the next ready `GraphDiskBase`. Any merge or validation failure leaves
the bounded pending plan intact and the reducer repair-only, so publication can be retried without
accepting stale progress.

The disk state deliberately does not implement checkpoint encoding or seeded recovery. A process
loss with a pending plan falls back to the existing full `GraphState` journal recovery path. This
is an explicit warm-state boundary, not a claim that a pending plan or coordinator metadata is
durably recoverable without full replay.

## Rejected alternatives

Publishing the new root before the journal was rejected because a derived index cannot become
commit authority. Clearing pending state on merge failure was rejected because the prior base is
stale after journal certification. Keeping the complete `GraphState` beside the disk base was
rejected because it would not remove the live reducer boundary. Claiming `CheckpointState` with an
unsupported encoder was rejected because it would overstate recovery support.

## Evidence and limits

The disk-index fixture cold-admits revision one, transitions the same coordinator to
`GraphDiskLiveState`, proof-prepares and durably commits revision two, confirms exact retry, rejects
a distinct commit and proof load while pending, forces a one-byte history-validation failure,
confirms pending state survives, retries publication, installs the new base and prepares revision
three from it. Existing disk preparation, cold recovery, root reconstruction and transaction tests
remain green.

The pending plan is request-bounded, but candidate discovery/initial scrub still use absolute
storage maxima. Coordinator retry/transaction/blob-owner maps and pending recovery remain full-
memory. No BM-01/BM-06 performance or larger-than-memory recovery claim follows.

## Verification

~~~text
cargo test -p uste-graph --all-targets --locked
# 43 passed
cargo test -p uste-txn --all-targets --locked
# 27 passed
cargo clippy -p uste-graph -p uste-txn --all-targets --locked -- -D warnings
# passed
bash scripts/check.sh
# 265 workspace tests passed; documentation=ok with 116 links, 113 active IDs and 146
# definitions; task_graph=ok; 12 R0 vectors, 4 storage-publication tests, 4 fixture-generator
# tests, 4 content-fixture tests and 31 isolated T-20 tests passed; 2 exact-profile debug tests
# remained intentionally ignored
~~~
