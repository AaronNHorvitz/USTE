# Decision 0067 — Authorized disk record reads

Date: 2026-09-18

Status: T-20 partial consumer implementation, not benchmark qualification.

`AuthorizedDiskReader` exposes domain-owned projections, never a raw graph snapshot or index
root. Trusted adapter construction supplies the exact durable current policy and fixed work
limits. Each operation checks current namespace permission, reducer-declared target requirements
and namespace containment before index I/O. A private 64 KiB page cache has no consumer telemetry.
The immutable coordinator/policy borrow prevents mutation during an operation; a new facade is
required after policy or root publication. Missing policy, uncertain state and pending graph roots
fail closed. This does not replace the future disk-aware write/revocation workflow.

Graph's first implementation supports current `Record` and historical `RecordAt` using the
admitted state-root current/history families. It requires an exact current journal anchor and
checks decoded record identity/revision; history uses a bounded authenticated predecessor lookup
and validates its key revision. Future revisions fail explicitly. Historical reads require current
`ReadHistory` and `ReadRecord` permissions, not historical permissions. All embedded record
references use the same candidate filter as the reference graph projection. Denied references
suppress the record rather than expose partial contents.

Current limits count value bytes; historical limits count key plus value bytes. Page visits
include binary search and cache hits. These limits are selected by the trusted adapter, not
adjustable by a consumer attempting to measure hidden candidate work. Budget failure does not
return a truncated successful record. Cancellation is checked before dispatch, during candidate
authorization and after dispatch; cancellation cannot turn filtering into a successful partial
result. No constant-time or physical-I/O side-channel resistance is claimed.

`Adjacent` and `SupportedBy` remain explicitly unsupported by this implementation. Their bounded
expansion and candidate filtering, authorized writes, staged-upload quota/reconciliation,
scalable first-owner admission and storage recovery metadata still require implementation.
No M1 interface or pinned handoff changes. No larger-than-memory or BM-01/BM-06 claim follows
from these local point-read tests.
