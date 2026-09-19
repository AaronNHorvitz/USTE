# Decision 0063 — Streaming metadata rebase and partial-pair durability

Date: 2026-09-18

Status: T-20 partial implementation; no full database or larger-than-memory qualification.

`DiskCommitCoordinator::rebase_metadata` now merges its admitted disk base with bounded overlays,
streaming sorted insertion-only retry, transaction-ID and first-owner deltas. Existing metadata
profiles are unchanged. Exact merge before-images enforce absence; output counts and insertion/
replacement/deletion reports must match. The trusted domain supplies the current scope, revision,
profile and logical digest without constructing a snapshot. Disk-graph state must be ready at the
exact journal certificate; pending domain publication cannot be bypassed by metadata publication.

The fixed metadata entry and up to three data runs use per-family merge limits. Metadata and
transaction roots are published at the same current certificate. Only success of both publications
installs the new pair and clears overlays. A failed derived-cache operation does not poison or
roll back an acknowledged journal transaction. Existing first owners are never replaced.

## Partial-pair safety

The existing two-slot roots are separate per profile, not a new atomic multi-root format. Repeated
partial publication must not rotate away the old complete pair. On retry, compare a current
candidate's complete anchor, family/count shape and logical run digests with freshly merged
canonical outputs. An exact match is reused; a same-revision semantic mismatch fails closed.
Reusing a visible root is not sufficient durability evidence: storage reauthenticates each run
under explicit per-run limits, syncs each run and the exact matching manifest, and syncs the
directory without rotating either slot. This handles authenticated but unsynced bytes left by a
failed in-process publication.

Once rebase starts, new commits are refused until rebase succeeds, while exact retries remain
available. Recovery detects roots newer than the selected old base and preserves this barrier.
An intermediate newer root at a different revision than the current journal frontier causes
rebase to refuse before root publication; this can arise from another trusted legacy writer path
advancing past a partial rebase. Explicit cache rebuild is required, not silent destruction of the
pinned pair. No executable cache-rebuild completion is claimed here.

## Limits and verification boundary

Merge/reuse limits are per family, not aggregate. New metadata roots now select the safe overwrite
slot through caller-bounded fallback validation: the reuse limits apply to every run in both fixed
slots. Resource or I/O refusal stops publication without overwriting a slot; only authenticated
integrity/profile failure makes a candidate unusable. Graph terminal-root publication likewise
uses explicit fallback limits, taking the largest declared base page, entry and logical-byte
budgets across its families. An older fallback exceeding those limits requires a larger explicit
budget, not silent rejection as corruption. The legacy publication API retains its compatibility
scrub. Scratch files left by failure are optional unreferenced index data;
later maintenance owns reclamation. No journal history, source data or physical-erasure claim
changes. Storage certificate/blob collections and first-owner admission's read amplification
also remain open.

Synthetic tests cover cold reopen of the new base with empty overlays, further commits, exact
retries, preserved/new owners and merge-limit refusal. A 48-case rebase matrix injects errors and
crash-before/after at five file syncs, seven directory syncs and four root writes. Every planned
fault must fire; one case repeats a partial-pair failure before restart. Same-process retry must
resynchronize unsynced manifests; crash recovery must retain the pinned older pair and finish the
new pair. These model crashes do not replace filesystem/process/power-loss qualification.
