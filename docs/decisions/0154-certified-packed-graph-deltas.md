# Decision 0154 — Certified packed graph delta staging

Date: 2026-09-19

Status: implemented and locally verified; T-20 qualification remains open.

Apply Decision 0153's opaque proof-prepared result only at its exact next authenticated journal
transaction. Compare canonical request/inventory binding, certified result digest, base scope,
certificate, ordered commitment, counts and policy before any cache writes. Prepare family deltas
through the same bounded graph delta algorithm used by v1, with storage-independent delta data
separated from the v1-only root anchor. Never synthesize a v1 anchor containing an ordered digest.

Stage sorted compare-and-swap deltas in bounded chunks at the target certificate, preserving old
canonical subtrees. All eight resulting family cardinalities must equal the prepared metadata.
Only complete staging returns a new typed packed graph base; old bases remain unchanged on error.
No intermediate root is published. A freshly staged base has no cached v1 digest: the independent
streaming compatibility export must compute it. The ordered commitment and journal result digest
remain distinct contracts.

Explicit limits bound prepared deltas, chunk size/count, aggregate packed reads and written pages.
Private durable cache packs left by failures remain reclaimable derived artifacts, not commits.
Exact old/new reference exports, request/result/base mismatch refusals, resource boundaries,
corruption and all observed staging faults with restart are required before local acceptance.
Live repair-only state, authorized commits, packed cold semantic admission and qualifying campaigns
remain separate integration obligations.
