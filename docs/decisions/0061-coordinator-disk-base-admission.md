# Decision 0061 — Journal-validated coordinator disk base

Date: 2026-09-18

Status: T-20 partial implementation; bounded-memory compatibility admission, not benchmark qualification.

Decision 0060 supplies a journal-validated transaction-ID ordering. Pair it with the existing
`coordinator-meta-v1` root at exactly the same namespace, revision, certificate, reducer profile
and logical state digest. The resulting opaque `CoordinatorDiskBase` retains roots, not complete
retry, transaction or owner maps. Its anchor must still match an independently admitted domain
state before installation into a coordinator. Existing in-memory coordinator APIs are unchanged.

Admission authenticates metadata counts and the complete retry run under aggregate run budgets.
Retry cardinality must equal the journal revision; every streamed canonical journal outcome must
exactly match its principal/idempotency-key disk entry. Two commits sharing a retry key cannot
both match because outcomes include their distinct revisions. The separately admitted transaction
index similarly proves transaction-ID uniqueness. No expiry filtering is performed during recovery.

The existing owner format does not contain an earliest-revision witness. Preserve that format and
use an explicitly read-amplified compatibility path: retain one owner entry from a bounded run
cursor, stream the journal prefix, and compare the earliest matching reference and principal.
Require at least one matching reference. Exhaust the owner cursor and its terminal authentication.
Then check every committed reference against the owner root while validating retries. This proves
no extra or missing owners, exact reference identity, and first-owner rather than last-owner
semantics, including reuse by another principal. All provisional work is discarded on failure.

The caller provides metadata entry/page/logical-byte limits, per-lookup page/result limits,
aggregate journal-group work and a certificate/group-envelope byte limit per pass. Before owner
replay, checked arithmetic admits `(owner_count + 1) * revision` group visits. This excludes the
separately budgeted transaction-index admission. Inventories retain their independent format caps;
payload bytes are not reread. Complexity is O(owners * revisions), retaining one owner, one
transaction/inventory and bounded cursor/cache state. It is not the final large-scale algorithm.
An authenticated first-reference witness index or bounded external sorting can replace repeated
passes without changing the first-owner proof obligation.

Raw `retry_at_base` and `owner_at_base` APIs are privileged recovery maintenance reads of this
immutable revision. They do not grant consumer access or apply current expiry/revocation policy.
Storage certificate/blob maps still exist, and live coordinator mutation maps are not yet replaced.
T-20 remains open for installation, bounded overlays, publication/recovery faults, authenticated
multi-revision domain suffix replay and qualifying BM-01/BM-06 campaigns.
