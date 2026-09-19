# Decision 0157 — Paired packed graph suffix recovery

Date: 2026-09-19

Status: implemented and locally verified; T-20 qualification remains open.

Consume one exclusive authenticated recovery owner and an independently admitted historical graph,
coordinator primary and quota triple with exact published receipts. Reuse the coordinator's common
owner/scope/profile/physical-family validation before replay; equality of hashes alone is not enough.
Keep the old roots published while rebuilding a private candidate.

Stream the exact bounded certificate range using the authenticated transaction cursor. For each
transaction, prepare bounded graph proofs from the current private base, verify the original
canonical request and certified result, then stage graph and primary/quota changes at that exact
receipt. Preserve retry collisions, transaction-ID collisions and first-owner accounting through
the existing coordinator staging algorithm. Retain one transaction/plan and fixed family handles,
not suffix-wide outcome maps or full graph maps. Bound aggregate journal work explicitly; per-revision
proof/delta/staging ceilings compose with the admitted revision count to bound total domain work.

Only after full cursor exhaustion, exact terminal anchors and consistent profiles may graph,
primary and quota roots be published and a ready packed coordinator installed with empty overlays.
The manifests are individually durable, not an atomic three-file transaction: any failure returns
no live coordinator, retains old published roots and permits authenticated restart/rebuild. A
partially published terminal triple is never admitted as a paired live state. Recovery never
appends, rolls back or recertifies an authoritative transaction.

Tests must cover zero suffix, exact/minus-one budgets, reference outcomes and state exports,
policy changes, late failures/corruption, owner/receipt mismatch, all observed I/O failures with
restart and continued bounded live commits after recovery. This raw trusted workflow is not an
authorized consumer facade, origin rebuild without a base, complete I/O accounting or qualifying
BM-01/BM-06 evidence. T-20 remains open.
