# Decision 0033 — Graph terminal-root deltas

Date: 2026-09-17

Status: accepted as bounded T-20 graph-integration groundwork. T-20 remains open; the live graph
reducer, semantic validation and recovery result still retain the complete state in memory, and no
BM-01/BM-06 qualification is claimed.

## Context

Decision 0027 made ordinary graph preparation change-bounded, Decision 0029 froze the eight-family
`graph-state-v1` projection, and Decision 0032 supplied an authenticated base/delta run merge. A
graph-owned bridge is required to translate one journal transaction into exact family changes
without making optional derived-cache I/O part of transaction authority or publishing a root whose
cross-family meaning has not been independently checked.

## Decision

Graph maintenance uses an opaque two-phase `GraphStateRootDelta`:

1. Before commit, `prepare_graph_state_root_delta` requires a semantically admitted root at the
   current journal certificate, independently prepares the supplied graph transaction and builds
   sorted, coalesced before/after changes for all eight families under explicit aggregate count and
   logical-byte limits. The plan binds the exact base anchor, scope, base/target revisions and
graph transaction result digest. It performs no durable writes.
2. The caller submits the same canonical transaction through the ordinary commit coordinator.
   Only a successful exact `TransactionOutcome` may be passed to
   `publish_graph_state_root_delta` with the opaque plan and admitted base root.
3. Publication checks the plan/base/outcome/current reducer and certificate revision before I/O,
   copy-merges every unchanged nonempty family, applies exact changes to affected families and
   omits empty outputs. It recomputes the canonical full graph logical digest and every expected
   run descriptor from the actual post-commit reducer, compares all merge results, and publishes
   one root only after the complete family set matches.

Family deltas use the frozen profile exactly: metadata carries checked target counts; current
records replace or insert canonical stored records; history appends at the target revision;
accepted relationship changes maintain outgoing/incoming keys; assertion and relationship evidence
maintains provenance; canonical target/owner role unions maintain reverse references; and policy
install/replace maintains both current and revision-history keys. Byte-identical contributions
coalesce away. Count arithmetic, allocation reservation and delta byte accounting fail closed.
The plan budget is debited while retained family entries are formed, with a mandatory-delta
preflight before record encoding. It does not replace the graph transaction's existing 10,000-
operation/100,000-reference preparation bounds, and one record's temporary canonical role-union
map remains governed by those graph limits rather than the derived-cache plan limit.

## Authority and failure semantics

The journal commit remains authoritative. The opaque plan is neither a commit receipt nor a root,
and a mismatched outcome is rejected before cache I/O. Failure during any family merge may leave
only encrypted unreferenced scratch bytes for T-35 reclamation; no partial family root is published
and the successful transaction is not reported as rolled back. Publication borrows rather than
consumes the opaque plan, so an in-process caller that still owns it can retry after corrected or
transient resource conditions. The plan is not durable or serializable; after actual process loss,
the caller must rebuild the complete root from the recovered live state.

No storage format changes. Output runs and roots remain ordinary `index-v1`/`graph-state-v1` data.
This path is deliberately one revision at a time: a stale or gapped base requires a full rebuild.

## Verification and limits

The disk integration fixture starts from an admitted revision-three root, prepares equivalent
plans, rejects a mandatory-count preflight, incremental count 19-versus-18 and logical-byte
exact-minus-one budget, and admits the exact 19-delta/byte budget. It commits a revision containing an entity reference
replacement, accepted-relationship retraction, evidence-backed assertion creation and policy
replacement, and rejects one plan with a corrupted outcome digest before I/O. The valid plan
exercises insertions, exact replacements and tombstones across all families, including empty
adjacency outputs. Its admitted/reconstructed state equals both the live reducer and a separately
published complete root before and after restart. A deliberately undersized postcommit merge fails
after the journal commit; storage restart preserves the transaction and exposes no target root. The
test harness deliberately retains the in-memory plan and then proves in-process retry under adequate
limits; this is not durable plan recovery.

The independently recomputed semantic comparison is intentionally still a complete in-memory
state scan. Explicit snapshots, ingest preparation, root reconstruction and coordinator maps also
remain full-memory. Persistent overlay lifecycle, streaming semantic validation, aggregate RSS
evidence, ingest deltas, orphan reclamation and qualifying BM-01/BM-06 runs remain future work.
