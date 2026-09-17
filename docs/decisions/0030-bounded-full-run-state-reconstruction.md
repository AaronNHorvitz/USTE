# Decision 0030 — Bounded full-run state reconstruction

Date: 2026-09-17

Status: accepted as T-20 recovery-transport groundwork. T-20 remains open; this does not provide a
disk-backed reducer or qualify BM-01/BM-06.

## Context

The consumer prefix scan is deliberately capped at one million results and 64 MiB and does not
certify that it consumed an entire immutable run. The storage scrub authenticates and hashes a full
run but discards its entries. Neither operation can safely decode `graph-state-v1` without either
weakening consumer bounds or first materializing a complete result vector.

An authenticated graph-state root also contains only reducer state. It does not contain retained
idempotency outcomes, transaction-ID bindings or committed-blob ownership required by
`CoordinatorRecoverySeed`; treating the graph root alone as a coordinator checkpoint would lose
durable transaction metadata.

## Decision

`index-v1` now exposes a trusted maintenance/recovery full-run visitor, propagated only through
`JournalStore` and `CommitCoordinator`. It is not part of the authorized consumer-index facade.
The caller supplies nonzero page, entry and aggregate logical-byte limits within the carrier
maxima. The journal rechecks the root's exact certificate anchor before I/O. The reader preflights
descriptor page/entry counts, opens the immutable run once, checks exact length, reads and
authenticates pages in
order without the shared cache, assembles at most one bounded key/value, enforces strict cross-page
key order and checked cumulative bytes, and recomputes exact entry count and the run logical digest.
It rechecks the stable handle length at completion.

Visitor effects are provisional: entries necessarily arrive before the terminal digest is known,
so callers must discard staged work unless the operation returns success. Operational errors remain
distinct; missing, truncated, appended, malformed or cryptographically inconsistent run bytes fail
closed.

`graph-state-v1` has a separate opaque candidate type. Candidate discovery accepts only roots on
the authenticated journal certificate chain with the graph reducer and state profiles, without the
circular requirement for a matching live snapshot. Reconstruction:

1. verifies the exact metadata entry, revision, counts, optional-family set and descriptor counts;
2. applies explicit caller record/version/policy/aggregate-entry/page/logical-byte budgets before
   state allocation;
3. streams current records, complete histories and policy families into private maps with exact
   key/content bindings;
4. reuses the checkpoint decoder's shared semantic constructor for version sequencing, lifecycle
   transitions, historical reference closure, policy ordering and current/history equality;
5. rebuilds adjacency, provenance and reverse indexes, then compares every streamed derived entry
   with the canonical rebuilt projection; and
6. recomputes every expected descriptor and the complete graph logical-state digest before
   returning the staged state.

Historical certificate-anchored candidates remain reconstructible after a later commit. They do
not become current or advance the journal frontier merely by being decoded.

## Limits

Storage candidate discovery currently scrubs roots before returning them, and reconstruction reads
the selected runs again through its stable visitor. `GraphStateLoadLimits` govern only that second
reconstruction pass: candidate discovery runs first under the carrier's absolute `index-v1` format
maxima, before any caller reconstruction limit exists. Therefore its page and logical-byte limits
are not an end-to-end discovery budget. This correctness-first double pass is not a BM-06
throughput result. The reconstructed ordinary `GraphState` still owns all current records,
histories and rebuilt indexes in memory; B-tree/vector allocation and derived-index amplification
are not bounded by the logical-byte counter. Only the monolithic encoded-checkpoint buffer is
removed.

No coordinator seed is constructed. A future recovery integration must pair the reducer candidate
with authenticated coordinator metadata or replay that metadata through the candidate revision,
then let `open_seeded` revalidate the exact certificate and suffix. Larger-than-memory T-20 closure
still requires a disk-backed/lazy reducer base plus bounded overlays and scratch merge.
