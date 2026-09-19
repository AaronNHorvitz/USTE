# Decision 0096 — Bounded committed-blob reference proofs

Date: 2026-09-19

Status: locally verified T-20 increment; map-free blob recovery/accounting remains open.

Add a privileged explicit-I/O proof binding one exact blob reference to the inventory in a
previously authenticated certificate. Revalidate the live-owner certificate proof before I/O,
then re-read the selected certificate and require its exact digest/context. Load only its bound
inventory under a caller-selected encrypted-byte allowance and reference count. Encoded length
is admitted before allocation/read; authenticated canonical count is admitted before the
reference-vector allocation. Scope, identity, byte length, chunk count and content digest must
all match. Finalized but uncommitted blobs, absent inventory entries and empty-inventory
certificates cannot produce this evidence.

Retain only the fixed reference and owner-bound certificate proof, never the inventory body or
a blob-history map. Report the selected certificate re-read plus inventory encoded bytes and
inventory reference count. The earlier certificate-chain proof has its separate explicit budget
and report; these costs are not silently included or called free. No blob payload is read by
proof construction. Normal open still authenticates every committed blob before logical replay.

The new proven range read uses this exact reference without consulting the resident blob map.
It retains the existing chunk/range limits and requires callers to discard partial output on
error. It authenticates requested chunks, not the whole blob on every call. Evidence survives
successful appends by the same exclusive owner but not reopen, foreign ownership or a poisoned
journal. It holds neither a key nor the ownership lock; diagnostics redact the reference.

These raw APIs do not grant current principal access, establish first ownership, reconstruct
quotas, transfer upload charges or authorize inventory commits. Authorized adapters must check
current policy before privileged proof/read I/O. Existing consumer APIs and M1 are unchanged.
The legacy inventory loader retains its absolute format limits and payload validation behavior.
This is a read-path building block: both journal recovery passes and append admission still use
resident blob/inventory/namespace collections. Do not claim map-free blob recovery, bounded total
RSS, larger-than-memory operation or BM-01/BM-06 qualification from this increment.

Verification covers exact and zero-byte committed reads with resident maps explicitly cleared,
successful-append continuity, reopen/foreign-owner/poison refusal before I/O, foreign scope and
changed reference fields, finalized-orphan rejection, exact/minus-one budgets with I/O counts,
certificate/inventory/chunk corruption, and every proof metadata I/O boundary (four boundaries,
12 error/crash attempts). Restart must reauthenticate fresh proof evidence. Clearing maps in
these read-path tests is explicitly not evidence that normal blob recovery is map-free.
Storage, transaction, replay and unchanged M1 integration suites pass; commands are in PROGRESS.md.
