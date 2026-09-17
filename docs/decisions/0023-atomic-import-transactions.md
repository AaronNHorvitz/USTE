# Decision 0023 — Atomic import transaction authority

Date: 2026-09-17

Status: accepted and locally qualified for T-49. This implements the R1 transaction-contract
subset of FR-34. CSV/JSON parsing, mapping execution, rejected-row reports and the operator CLI
remain T-54.

## One authority and closed inputs

`uste-ingest` is a capability-free reducer above `uste-graph` and `uste-spatial`. One
`IngestState` owns their coherent snapshots plus a private import ledger. The original T-49
implementation prepared a cloned candidate; Decision 0034 replaces that internal path with
preflighted graph, optional spatial and private-ledger deltas, published only after graph, spatial,
import and external-reference validation all pass. It cannot read files, clocks, environment,
credentials or networks. Original bytes and a
mapping manifest arrive only as already-finalized `BlobReference` values under the existing
journal owner. Neither their content nor retrieved instructions receive execution authority.

The canonical `UIRQ` 1.0 request profile is scoped and limited to 16 MiB. Graph-only transactions
carry one canonical graph request. An import start binds an active job entity, immutable source
Evidence version/blob digest, mapping Evidence version/blob digest and the closed
`TypedRecordsV1` mapping profile. Its blob inventory must equal exactly those source and mapping
references. A batch carries a stable job/sequence identity, the exact prior checkpoint, source
cursor, one to 10,000 namespace-unique source-event receipts, caller-declared payload commitments, one graph
request and an optional spatial request. Starts and batches require a nonempty graph mutation so
the graph reducer advances at every composite revision; spatial state may advance sparsely.

T-49's cursor advances over accepted typed rows. T-54 must extend the tooling contract before it
claims skip/quarantine behavior, rejected-row cursor advancement or bounded error reports. No
current API claim implies a CSV/JSON parser or arbitrary mapping program. The reducer retains each
declared payload digest for retry/audit identity but cannot recompute it from aggregate graph and
spatial requests; T-54 must introduce and verify canonical per-row effect envelopes before those
digests can support mapping-provenance claims.

## Atomic closure and authorization

Every import mutation requires namespace `Import` and `Commit`, job/source/mapping record access,
all requirements derived from its graph operations, and `ReadRecord` plus target `Commit` access
for every spatial identity and external reference. The normal authorized coordinator authenticates
the principal and revalidates these requirements; the reducer does not accept a caller-supplied
principal.

Import graph mutations cannot change policy or delete entities. After both component prepares,
every spatial identity, world, frame, observed entity, source, predecessor and correction target
must be an active graph Entity, and observation evidence must be graph Evidence. Frame-transform
targets must exist as active Entities; T-50 still owns their schema, version and numeric semantics.
Graph-only transactions recheck the entire retained spatial catalog, so deletion cannot create a
dangling spatial history. Any failure discards the complete candidate.

## Retry, preview and durable resume

The trusted raw-reducer preview helper runs the exact unpublished prepare path and returns the base/proposed revision,
typed outcome and result digest. It is advisory: commit repeats authorization, source binding,
checkpoint and closure checks against current state. It is not yet an authorized consumer/API
preview surface.

The coordinator's principal-scoped idempotency key and transaction ID are the retry authority.
An exact retry returns its durable prior outcome before reducer execution, including after restart.
Reusing a semantic batch under a different coordinator identity reaches the durable ledger and
conflicts; the reducer does not pretend it is a new no-op commit. A checkpoint pins source and
mapping references, next batch, cursor, accepted rows, first/last revisions, status and a hash
chain. Changed source or mapping bindings fail with `SourceChanged`; stale/future checkpoints and
duplicate source events fail closed.

Canonical composite checkpoints include graph state, optional spatial state, private job/batch
receipts and source-event digests. Restore validates ordering, limits, unique import revisions,
component revisions, binding evidence, ledger arithmetic and complete graph/spatial closure before
publication. The encrypted journal remains authoritative and the checkpoint remains a verified
cache. Logical state hashing includes genesis and all private ledger state.

## Limits and boundaries

- 10,000 rows per batch, 1,000,000 durable batches and 10,000 jobs per namespace reducer.
- 1,000,000,000 accepted rows and source-event receipts per job.
- 16 MiB canonical composite request and 256 MiB canonical composite checkpoint.
- The R1 job ledger and component catalogs are in memory; these are correctness bounds, not an
  ingest-throughput or larger-than-RAM claim.
- Structural maxima are not simultaneous capacity claims: the in-memory maps and 256 MiB cache
  checkpoint can bind far below the job/batch/row counters. Decision 0034 bounds ordinary prepare
  deltas but does not make the reducer larger-than-memory. No RSS, throughput, concurrency,
  real-process crash or power-loss qualification is claimed.
- Batches are nonempty and fully accepted, cannot attach new row blobs, and expose only `Open` and
  `Completed`; there is no durable rejected/failed/cancelled job state in T-49.
- T-54 owns local-file parsing, mapping validation/execution, rejected-row policy, cancellation UI
  and exact item/price examples. T-35/T-36 own lifecycle compaction and migration.

Local agent review and tests are not independent security certification. T-62 remains the separate
unverified executable-distribution prerequisite.
