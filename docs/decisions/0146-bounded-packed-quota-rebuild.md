# Decision 0146 — Bounded packed quota rebuild

Date: 2026-09-19

Status: implemented and locally verified T-20 private rebuild; qualification remains open.

Rebuild the packed quota profile from an independently journal-admitted packed primary ledger,
including already-populated or owner-free historical bases. Authenticate the exact primary target
against the live frontier and retain its private maintenance capability throughout construction.
Recheck all primary live-owner bindings before I/O. No transaction, manifest or owner assignment
is created by rebuilding a derived accounting projection.

Stream the primary owner ordering with a bounded cursor. Accumulate at most a caller-selected
1–512 owners per batch; sort only this batch into principal/blob order and aggregate only its
principals. Read prior staged principal totals with explicit per-lookup and aggregate allowances,
then stage exact head/principal/owner deltas using the existing bounded copy-on-write primitive.
Every private stage shares the exact primary target certificate, not a synthetic revision.
Temporary maps/vectors are bounded by the batch ceiling, not total owners.

Partial totals and families remain private. Require cursor exhaustion and exact primary owner
cardinality before returning an opaque paired quota prefix; any failure leaves prior published
roots untouched and returns no partial accounting. Empty ledgers still produce the explicit zero
head and empty families. Batches may reuse unchanged older packs; no intermediate root publishes.
Checked arithmetic, zero-byte ownership and original-principal charging match the inductive and
cold-admission paths. Rebuilt commitments must be independent of batch partition and agree with
per-transaction construction; cold bijection admission remains independently available.

Certificate, owner-cursor, total-owner, batch-count, per-family batch, per-lookup and aggregate
lookup limits are explicit. Reports are successful primitive work, not complete adapter traffic.
Quota recovery is still privileged maintenance, not authorization, reservation reconciliation,
domain installation or benchmark qualification. Existing v1/M1 behavior is unchanged.

Verification: five rebuild tests cover empty/populated partition independence, exact 512-owner
batching, inclusive aggregate/cursor limits, foreign-owner and ciphertext rejection, and 318
injected read/write/crash cases with cold restart and unchanged published roots. The workspace
gate passed 549 tests across 47 executables, followed by warnings-denied Clippy and transaction
crate documentation. Commands, resource limits and remaining integration work are in PROGRESS.md.
