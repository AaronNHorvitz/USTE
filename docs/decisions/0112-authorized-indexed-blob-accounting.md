# Decision 0112 — Authorized indexed blob accounting

Date: 2026-09-19

Status: accepted and locally verified. T-20 remains open.

Connect Decision 0111's admitted first-owner quota projection to opt-in authorized facades.
`AuthorizedDiskUploads::new_with_indexed_accounting` is staging-only;
`new_with_indexed_inventory_commits` separately enables the same ordinary inventory capability
as Decision 0103. Both require an already admitted quota index, an exact current durable policy
and an owner ceiling no greater than the existing journal hard cap. They start recovery-closed;
the trusted adapter still owns the complete durable token outbox and must reconcile it.
Existing constructors and their streaming accounting semantics remain unchanged.

Indexed accounting uses the existing maximum-total-owner admission and fixed per-lookup limits:
64 page visits, 24 result bytes. These cover the quota profile's 24-byte metadata and 16-byte
principal totals across index-v1's hard page bound. Consumers cannot tune per-lookup ceilings to
probe another principal. A local 64 KiB quota cache is additional to the existing 64 KiB upload
lookup cache, is not owner-sized, and is dropped before certification. Read-only metadata uses
its existing bounded cache. These are logical cache budgets, not maximum-RSS qualification.

Every quota inspection requires current `InspectQuota` authority before I/O. Inventory commits
retain their existing current authorization, reference/first-owner checks and accounting before
the raw coordinator call; this does not move raw retry/collision handling or invent a no-I/O
inventory retry guarantee. Projected owner/namespace/principal charges and existing policy quotas
still apply. Indexed reads never silently fall back to a scan or interpret a missing/failed
projection as zero. An accounting failure occurs before certification and retains staged charges.
Uncertain commits retain reservations until exact recovery/reconciliation; acknowledged commits
release the same precomputed reservations without new fallible postcertificate work.

The indexed metadata inspection API exposes the same privileged exact charges as its streaming
counterpart, not another principal's individual totals. Foreign authentication and absent grants
fail before cache access. A current revocation still rejects an old exact inventory retry before
I/O, even when the old outcome and quota pages are cached.

Tests reuse unchanged streaming assertions for quota transfer, cold reconciliation, revoked retry
and all six selected journal-sync error/crash cases, exercising both accounting strategies.
Additional checks cover missing-index refusal, staging-only refusal, populated principal totals,
foreign/denied inspection, precommit quota-read failure with retained reservations, and every
actual indexed inspection read failure. Cold projection validation/publication remains covered
by Decision 0111. This is opt-in implementation evidence, not a qualifying BM-01/BM-06 result,
production readiness, an authoritative migration or a change to the pinned M1 interface.
