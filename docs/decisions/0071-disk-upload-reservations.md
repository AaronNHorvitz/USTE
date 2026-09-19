# Decision 0071 — Bounded disk-coordinator upload reservations

Date: 2026-09-18

Status: T-20 partial implementation; inventory commit and qualification remain open.

`AuthorizedDiskUploads` owns a bounded staging ledger, not a reconstructed committed-owner
map. Trusted construction fixes committed-accounting read limits and a private 64 KiB lookup
cache. Exact committed namespace/principal charges stream from the admitted owner base plus
bounded overlays. Identity-only owner lookup is privileged, namespace-fixed, and refuses an
uncertain coordinator; it grants neither blob access nor commit authority.

Every construction starts closed to new uploads. The trusted adapter must durably retain every
token before staging bytes and reconcile its complete outbox. Tokens must be committed, durably
aborted, or lack durable bytes. Uncommitted resumable or finalized tokens keep admission closed.
This is not orphan enumeration. `DiskUploadUsage` distinguishes known reservations from complete
staging accounting; a newly constructed empty ledger is never advertised as proven zero staging.

Keep the established authorization, upload-handle ownership, policy lease, byte quota, live-handle
and 32-reservation semantics. Dropping a handle releases live admission, not its unresolved charge.
Accepted bytes remain charged after partial write errors. Durable abort releases an unfinalized
reservation; immutable finalized blobs cannot be aborted and retain their charge. Handles are
bound to the exact capability ledger, not merely a matching authenticated principal.

This capability stages, resumes, finalizes and aborts uploads only. It cannot submit blob inventories,
expose raw storage, or bypass the graph writer's inventory rejection. Inventory commit requires a
future capability binding staging ownership/quota proof to certification and post-commit charge
transfer, including exact retry and uncertain outcomes. No physical-erasure, M1-interface change,
production qualification, or larger-than-memory benchmark claim follows from this increment.

Tests exercise closed construction, permission denial, exact byte quota and one-byte refusal,
durable token reconciliation/resume/abort, live-handle exhaustion, retained dropped-handle
reservations, the 32-reservation limit, finalization charge retention and capability reconstruction.
Disk owner identity tests preserve first-owner data across metadata rebase.
