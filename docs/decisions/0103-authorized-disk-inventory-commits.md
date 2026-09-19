# Decision 0103 — Authorized disk inventory commits

Date: 2026-09-19

Status: implemented; verification evidence is recorded in PROGRESS.md. T-20 remains open.

Extend the bounded upload facade with a trusted opt-in constructor for ordinary,
policy-preserving inventory commits through Decision 0102. The original constructor remains
staging-only. Storage and accounting limits are constructor configuration, not consumer request
parameters. The facade owns its 64 KiB metadata cache and fixes ownership lookups at 64 visits
and 136 result bytes. No complete committed-owner ledger is reconstructed.

Require current kernel-bound identity, namespace commit permission and exact durable policy
before I/O. Reject policy-changing requests and require every reducer-declared target permission
before ownership lookups. A reference must exactly match either this principal's committed
first-owned reference or its finalized staged reservation. Unknown, changed and foreign references
share authorization denial. Raw coordinator permissions are not exposed.

Stream committed accounting under the configured owner bound; admit both newly added owner count
and exact logical bytes before certification. Repeated references do not add committed charge.
The accounting scan is still O(total owners); this is a bounded reference implementation, not a
persisted aggregate index or larger-than-memory performance result. Format limits bound the
input inventory; at most 32 reservation-release keys are retained. Zero-byte blobs consume owner
and reservation metadata even though their byte charge is zero.

Acquire the private reservation-ledger lock and allocate release keys before calling the shared
coordinator admission/publication path. On success, remove only this principal's exact finalized
reservations without fallible I/O or allocation. Failure preserves reservations. Uncertain
publication closes state/accounting access until recovery; a complete durable consumer token
outbox remains necessary to reopen new-upload admission. Exact recovered retries can release
their resumed finalized reservations without double-charging. No physical erasure is implied.

Current authorization and quota checks apply to exact retries as in the resident facade; raw
retry precedence over new-write storage limits remains unchanged. If a trusted reducer violates
its policy-preserving descriptor after certification, return a distinct `CommittedPolicy` error
carrying the known outcome and fail closed on subsequent policy checks. Never report rollback
for that certified outcome.

Synthetic tests cover quota transfer and cold reconciliation, first-owner isolation, target and
foreign-kernel denial before I/O, current revocation of old retries, six uncertain flush cases,
zero-byte reservation release, configured owner admission, staging-only refusal, indistinguishable
invalid references and adversarial reducer policy drift. The existing graph consumer writer
remains inventory-free; this generic ordinary-reducer capability does not claim graph content
integration, a changed M1 interface, production qualification or completion of T-20/T-19.
