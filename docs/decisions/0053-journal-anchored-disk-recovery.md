# Decision 0053 — Journal-anchored warm disk recovery

Date: 2026-09-17

Status: accepted as a bounded T-20 recovery increment. T-20 remains open because candidate
discovery/initial scrub and coordinator maps remain memory-resident, metadata is replayed from the
journal origin, only zero or one suffix is supported, and BM-01/BM-06 are unqualified.

## Context

Decision 0052 removed the complete graph from the warm write loop but required complete
`GraphState` replay after process loss. Serializing its pending terminal-root plan would duplicate
a derived cache and would still need to prove the exact journal outcome and predecessor root. The
authenticated journal already carries the canonical request, identities, inventory binding,
outcome and certificate needed to rebuild that plan deterministically.

## Decision

An `AuthenticatedIndexRecovery` may retain an opaque owned copy of only the final authenticated
transaction while opening the complete journal. The token contains the bounded canonical request
and inventory plus its revision, certificate, logical-event and inventory digests, principal,
idempotency key and outcome. It has no public constructor and redacts request/certificate content.
Later corruption or authentication failure returns no token.

Recovery selects and semantically admits a graph root:

- if its revision equals journal frontier `F`, it is a ready `GraphDiskLiveState`; or
- if its revision is exactly `F-1`, the final token's request is decoded and proof-prepared against
  that base under the existing current/history/reverse/delta/cache bounds.

No larger suffix is accepted. The temporary recovery owner is dropped, then
`open_journal_anchored_prepared` independently reopens and authenticates the journal. It rebuilds
retry, transaction and first-blob-owner maps from every decoded group without reconstructing
reducer prefix state. At the base revision it requires exact scope and certificate equality. For
one suffix it compares every captured group field, calls the reducer's external-prepared validator,
compares the result digest and only then publishes pending state. Any intervening append, missing
group, second suffix, wrong preparation or anchor fails before a coordinator is returned.

`JournalAnchoredTransactionState` is implemented for `GraphDiskLiveState` only while ready. A
pending state cannot be used as a new base. `GraphDiskLiveState` still does not pretend to implement
`CheckpointState`; this path is explicitly journal-anchored.

## Crash behavior

- Loss before certificate sync recovers the prior frontier/root ready.
- Loss after certificate sync and before/during pending installation or root merge rebuilds the
  one pending plan from root `R` plus group `R+1`.
- Loss after the new root becomes durable admits the frontier root ready, whether or not the prior
  process installed it in memory.
- Unrooted scratch runs remain invisible. Root publication still exposes only the old or exact new
  manifest according to the existing storage protocol.
- If neither the frontier nor predecessor root admits, or the gap exceeds one, this path fails
  closed; the explicit complete-`GraphState` fallback remains available.

## Rejected alternatives

A new serialized pending-plan format was rejected because deterministic proof reconstruction is
already bounded and final reopen must revalidate the journal regardless. Requiring an exactly
paired coordinator-metadata root was deferred: replaying metadata from the journal handles a graph
root that became durable before an optional metadata cache and avoids another crash window. Calling
the existing `open_seeded` was rejected because it requires checkpoint decoding and pure reducer
suffix preparation, neither of which describes the disk reducer.

## Evidence and limits

The graph fixture loses the process with revision two pending, captures the authenticated frontier,
cold-admits revision one, rebuilds its disk proof, independently reopens pending, preserves exact
retry and stale-progress refusal, repairs a deliberately failed publication, then loses the process
again and opens ready from the revision-two root. The transaction fixture rejects a wrong prepared
result, accepts the exact suffix and retry metadata, then proves an intervening third append makes a
captured revision-two handoff fail closed.

Coordinator retry/transaction/blob-owner maps are still complete in memory and rebuilt from the
journal origin. Candidate discovery and initial carrier scrub retain absolute format maxima rather
than caller-selected resumable budgets. Zero/one-suffix recovery is not compaction, BM-06 or a
general multi-revision disk replay claim.

## Verification

~~~text
cargo test -p uste-txn --all-targets --locked
# 28 passed
cargo test -p uste-graph --all-targets --locked
# 43 passed
cargo clippy -p uste-txn -p uste-graph --all-targets --locked -- -D warnings
# passed
~~~
