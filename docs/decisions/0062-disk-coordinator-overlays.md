# Decision 0062 — Explicit-I/O disk coordinator and bounded overlays

Date: 2026-09-18

Status: T-20 partial implementation; no larger-than-memory or consumer authorization qualification.

Install Decision 0061's admitted metadata base in a separate `DiskCommitCoordinator`. Consume the
exclusive `AuthenticatedIndexRecovery` owner directly without reopening or reconstructing complete
coordinator maps. Require the exact current journal frontier and a trusted `DiskCoordinatorState`
proof of matching domain state. `GraphDiskLiveState` checks the complete independently admitted
anchor and refuses a pending state. Overlay maps begin empty.

Reuse the existing coordinator commit implementation with an optional disk-metadata context.
Lookups consult the overlay first, then the immutable disk base under explicit page/result limits.
Exact retries still precede transaction-ID collision, admission and cancellation checks. Expired
retries remain tombstones; they do not become fresh writes. Preserve journal certification before
infallible reducer publication and outcome insertion, and preserve outcome-unknown poisoning.
Legacy in-memory callers supply no disk context and retain their original path.

Before preparation or journal writes, admit overlay outcome growth and resolve each reference's
first owner. Existing owners from either base or overlay consume no new owner slots. New owners
are admitted before temporary-vector allocation; after certification no disk metadata reads occur.
The caller-selected outcome/owner overlay ceilings are bounded by the existing format maxima;
the base plus outcome overlay also remains within the existing namespace outcome cap. Reaching
an overlay ceiling returns `ResourceLimit` without a partial transaction. Automatic rebase/flush
is not yet implemented and must not be implied by this bounded write path.

The new coordinator offers explicit-I/O retry/transaction outcomes with principal isolation and
expiry, first-owner queries, privileged upload operations, and the existing narrow derived-index
maintenance and postcommit domain-publication hooks. It does not expose its internal legacy
coordinator: that object's memory-only reads would omit the disk base. No legacy authorized view
or authorization adapter is reused. These APIs remain trusted maintenance APIs, not consumer
capabilities. A disk-aware authorization adapter, bounded suffix recovery, scalable first-owner
admission, metadata rebase and disk-specific fault matrices remain required.

Storage's certificate/blob metadata collections remain memory-resident. The current tests use
small synthetic fixtures and capped processes, not BM-01/BM-06 qualification. T-20 stays open.

## Ordinary-reducer suffix recovery

`recover_from_admitted_base` now validates the supplied base state, preflights the suffix outcome
count against overlay capacity, and revalidates the metadata root against the current owner's
authenticated historical certificate chain. This binding check also applies to a zero-length
suffix. It streams only subsequent canonical transactions through the range visitor's byte budget.
Each retry key and transaction ID must be absent from both base and preceding suffix. First-owner
references are checked against base/overlay; only new owners consume admitted overlay entries.
The ordinary reducer prepares each revision and must reproduce the stored result digest before
its private state advances. Expiry is restored exactly, never recomputed from the recovery clock.

Only a terminal successful replay returns a coordinator. Any late byte/admission/I/O/integrity or
reducer failure drops the entire provisional state and overlays. This path supports ordinary
`TransactionState::prepare` reducers; it does not supply the explicit-I/O graph proof preparation
or intermediate graph-root handling needed for multi-revision `GraphDiskLiveState` recovery.
No fallback materializes a graph map. The generic suffix capability does not close that graph gap,
the disk-specific crash/fault matrix, authorization, rebase or large-scale qualification.

## Commit fault regression coverage

The disk coordinator now has a 13-case deterministic regression matrix: error/crash-before/
crash-after at each of two group/certificate writes and two data-sync boundaries, plus a cold
metadata-read error before publication. Every injected point must fire. A read error leaves the
base state usable and the overlay empty. Any publication error produces `OutcomeUnknown`, denies
state/outcome reads and exposes no new overlay. Restart from the disk base restores exactly the
old frontier or the new certified frontier; an exact retry then yields revision two once. The
new frontier is required specifically for crash-after certificate sync, never guessed from a
lost response. These memory-filesystem crash cases do not simulate power loss or replace real
process/filesystem qualification. Rebase and disk-graph recovery fault matrices remain open.
