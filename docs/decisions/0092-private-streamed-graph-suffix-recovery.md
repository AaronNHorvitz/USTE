# Decision 0092 — Private streamed graph suffix recovery

Date: 2026-09-19

Status: verified partial T-20 implementation; native runner integration and qualification remain open.

Connect Decisions 0088 and 0091 through opt-in `DiskRecoveryDomain` hooks. The coordinator admits
an already validated domain base D and metadata base M with M <= D <= authenticated frontier F.
It streams M+1 through F under one group/certificate byte allowance and existing bounded metadata
overlays. Retry uniqueness, transaction collisions, first-owner identity and canonical authentication
remain in the coordinator, not delegated to the domain hooks. If D > M, the exact certificate at D
must be encountered before any domain advancement.

The domain admits F-D against its suffix-count ceiling before filesystem work. For each later
transaction it prepares an explicit bounded proof. The coordinator independently checks exact
request/inventory/revision/base binding and the stored result digest before publishing into private
state. The domain can then materialize an unpublished certified-revision root and install it as the
next private base. No full graph/history map or set of intermediate root handles is retained.
The core verifies each advanced state's exact journal anchor. The shared transaction cursor must
finish and all coordinator suffix/root checks must succeed before terminal domain publication.

Graph recovery reuses the existing proof loader, reducer preparation, exact root deltas and complete
merged-output semantic validator. Each intermediate root has generation zero and never replaces a
durable slot. Metadata publication/admission rejects such unpublished graph bases. Only the final
graph root goes through the existing current-frontier bounded-fallback publication path; only then
may a coordinator and terminal work report escape. Failure before that point discards private state;
derived scratch files remain orphanable under T-35. Errors during terminal publication retain the
existing old-or-exact-new recovery behavior.
An already-current discovered graph root is boundedly reauthenticated and resynchronized without
rotating either slot, so a visible candidate from an earlier uncertain publication is not mistaken
for completed durability. This additional cold-open I/O is explicit, not a free cache admission.

`GraphDiskSuffixRecoveryLimits` declares a total revision ceiling and per-revision proof, delta and
per-family merge bounds. Total domain work is bounded by the admitted count times those per-revision
bounds, not mislabeled as a single shared merge budget. The same caller-bounded page cache is reused
for metadata and graph proof reads. Reports aggregate actual staged runs, base/output entries,
output logical bytes and merge page reads; they are trusted diagnostics, not consumer-authorized
cardinality surfaces. Encoded journal bytes retain Decision 0088's explicit inventory/header exclusions.

The recovery index-maintenance guard is scoped to an opaque authenticated transaction and holds the
exclusive recovery owner borrow. Transaction tokens now retain their decoded namespace scope for
this check; no persisted transaction encoding changes. Terminal publication is separately scoped
and still requires the exact current frontier. Existing ordinary-reducer and ready/one-pending APIs
continue through the shared coordinator path without changing their external semantics.

Tests compare a three-revision disk suffix to full-reducer output, cover M=D, M<D and D=F, exact
retry lookup, authorized concealment, restart, count/byte admission, late corruption and old-or-terminal
root visibility through I/O/crash injection. Passing these is not native process-loss qualification,
BM-01/BM-06 acceptance, removal of storage's resident maps or completion of T-20.
