# Decision 0148 — Packed live metadata rebase

Date: 2026-09-19

Status: implemented and locally verified T-20 live rebase; qualification remains open.

Rebase the opt-in packed coordinator by streaming only its bounded post-base authenticated
journal suffix. Reuse the recovery cursor and private primary/quota staging algorithms through
internal borrowed-journal helpers; do not move or release the exclusive journal owner, duplicate
the commit state machine, construct complete maps or publish intermediate revision roots.
Certificate lookahead remains bounded to at most 64 receipts; exact range bytes and group limits
cover acquisition and selected-group reads. Per-transaction tree work remains explicitly bounded.

Check exact correspondence between recovered suffix outcomes/transactions and resident bounded
overlays. Advance primary and quota candidates together using the first-reference witnesses and
original-principal charges. Require complete suffix consumption and exact new-owner cardinality.
Only a ready domain state may supply terminal publication claims through a separate trusted trait;
claims must bind the actual journal frontier and preserve the installed reducer/commitment profile.
No graph-state-v1 digest substitution is authorized.

Publish terminal primary and quota manifests only after all private stages succeed. Install the
new pair and clear overlays only after both publications succeed. A failed rebase preserves the
old installed pair and every retry outcome; block fresh writes until a successful rebase, while
allowing exact retries. A durable primary-only candidate is not a paired live base. Recovery may
independently admit a complete pair or explicitly rebuild the absent derived quota projection.
No claim of atomic multi-file publication, orphan reclamation or physical erasure is made.

Tests must cover repeated commit/rebase operation with tiny overlays, old/new first ownership,
exact retries, cold independent admission, inclusive suffix limits, ready-domain rejection,
corrupt suffix/tree rejection and injected faults through both terminal publications. Existing
run-backed/M1 behavior remains unchanged. Packed authorized/domain integration, complete I/O
accounting and qualifying BM-01/BM-06 campaigns remain separate requirements.

Verification: five integration tests passed, including 738 injected fault cases, repeated tiny
overlays, cold admission/exact retries, inclusive budgets, four domain/profile refusals and suffix/
primary/quota corruption. Full workspace verification passed 560 tests across 47 executables,
warnings-denied Clippy/docs and documentation checks. The extracted shared cursor algorithm was
compared against its prior implementation with no changes beyond whitespace. Exact commands,
corrected test assumptions and resource observations are recorded in PROGRESS.md.
