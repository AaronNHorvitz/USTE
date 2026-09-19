# Decision 0155 — Packed live graph state and repair

Date: 2026-09-19

Status: implemented and locally verified; T-20 qualification remains open.

Install a packed graph reducer only from an opaque semantically admitted base and an exact
published graph-root receipt, with all eight canonical tree and root owner/key/scope bindings
checked under the exclusive journal owner. Pair its distinct ordered-state profile and digest
with the independently admitted coordinator primary/quota roots. No full graph snapshot is made.
The coordinator rechecks the reducer's retained disk capabilities against its own journal owner
at installation; validation by an earlier caller-owned maintenance borrow is not sufficient.
Pure reducers retain the default no-disk-capability hook.

Ordinary pure prepare is unavailable for this disk representation. Explicit packed proofs produce
an opaque bounded delta plan; externally prepared commit checks exact request/inventory, revision,
base certificate/commitment/counts/policy before authoritative journal publication. Publication
installs one bounded pending plan, not an old graph view masquerading as current state.

Pending state is repair-only: fresh preparations, ready reads and metadata rebase refuse it.
Exact durable retries remain available through the coordinator. Trusted repair stages all eight
families against the exact current certificate and pending plan, publishes a terminal graph root,
then installs an opaque predecessor/result-bound publication. Failure retains the pending plan
and old base; it neither rolls back nor recertifies the committed transaction. Uncertain journal
publication quarantines maintenance as before. A ready state can subsequently rebase the paired
coordinator metadata to its new domain commitment.

Expose only metadata-sized snapshots and existing derived-maintenance/postcommit-install borrows
on the raw coordinator. Do not implement v1 checkpoint serialization using an ordered digest.
Consumer-authorized writes and cold packed graph semantic/recovery integration remain separate;
the raw maintenance surface is never a consumer permission grant. Tests must cover exact retries,
pending repair failures, uncertainty, policy readiness, repeated bounded overlays and cold receipt
binding. T-20 and larger-than-memory qualification remain open.
