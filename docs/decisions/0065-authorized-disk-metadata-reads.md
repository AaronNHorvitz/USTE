# Decision 0065 — Restricted authorized disk metadata reads

Date: 2026-09-18

Status: T-20 partial consumer capability; not the complete disk-aware authorization facade.

`AuthorizedDiskMetadata` borrows the disk coordinator and policy kernel and exposes only own retry
outcomes, own transaction-ID outcomes and committed-byte usage. It never exposes raw reducer
state, index maintenance, owner lookups, writes, uploads or graph reads. The trusted adapter
constructs it; consumers receive only this restricted surface.

`AuthorizedDiskPolicyState` supplies the exact current durable policy without constructing a
snapshot. Constructor and operation checks require exact policy equality in the same namespace.
Graph state must have a ready admitted root and a durable policy; pending state fails closed
until trusted terminal-root repair. The privileged coordinator still supports exact retries while
pending; this restricted read facade does not supply that repair workflow.

Every operation authorizes the kernel-authenticated principal before clock sampling, disk reads
or cardinality-sensitive admission. Outcome identity is derived from authentication, never from
an arbitrary supplied digest. `ReadOwnOutcome` applies to both lookup forms; `InspectQuota` gates
streaming committed charges. Foreign-kernel identities are denied, and another authorized
principal sees no outcome. Existing expiry semantics are preserved. Staging reservations are
not represented by committed-byte usage and must not be inferred to be zero.

Security hardening: the facade owns a private 64 KiB page cache and exposes no cache statistics.
Outcome lookup work limits are fixed at 64 page visits and 136 value bytes, not consumer-tunable.
The v1 format permits at most 2^24 pages and fixed-width retry/transaction outcomes; binary search
and their entry fragments fit this bound. An undersized consumer-selected budget could otherwise
distinguish another principal's transaction from absence before ownership filtering. Authorization
precedes locking the private cache, clock sampling and disk access. Cache-lock poisoning fails
closed. This removes those explicit telemetry/admission channels; it does not claim constant-time
execution or conceal storage corruption from the trusted adapter. Committed accounting limits
remain explicit under `InspectQuota`, which authorizes namespace usage disclosure.

Immutable Rust borrows prevent policy/domain mutation during an operation. The trusted adapter
must create a new facade after durable policy publication and supply the matching current kernel.
No persistent read-view lease is introduced. Missing or mismatched policy fails closed; no stale
policy fallback is allowed. This does not claim a completed durable revocation/write workflow.

Tests exercise base and overlay outcomes, repaired/rebased state, expiry, foreign principals,
default denial before clock access, principal isolation, quota permission, absent/mismatched
policy and pending-root rejection. Full disk-aware commit authorization, staged-upload quota and
reconciliation, graph candidate filtering, disk recovery fault coverage, scalable accounting and
larger-than-memory/BM-01/BM-06 qualification remain required. No M1 interface or handoff changes.
