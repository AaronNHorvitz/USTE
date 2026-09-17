# Decision 0035 — Explicit-I/O graph preparation proofs

Date: 2026-09-17

Status: accepted as bounded current-state T-20 groundwork. T-20 remains open; deletion,
historical predicates, persistent overlays, disk-backed coordinator publication and BM-01/BM-06
qualification are not implemented by this increment.

## Context

The pure `TransactionState::prepare` contract cannot safely hide filesystem access. Decision 0034
bounded retained graph deltas but still read their base from the complete in-memory reducer. A
disk-backed write path therefore needs a distinct phase that authenticates the exact records a
request may observe before ordinary deterministic preparation runs.

## Decision

`load_graph_disk_preparation_view` is a privileged explicit-I/O phase over one currently admitted
`graph-state-v1` root. It consumes the transaction, authenticates the current policy and exact
current-record positive or negative proofs, and retains only the transaction dependency closure.
Caller limits cap unique record proofs, reference occurrences and deterministic logical proof
bytes. The caller supplies the bounded decrypted-page cache; observed lookups, pages, cache hits
and fragments are reported.

The resulting `GraphDiskPreparationView` owns no coordinator, filesystem, key-vault or cache
capability. Its consuming `prepare` method rechecks proof completeness and invokes the existing
graph reducer against a private partial current-state base. The opaque `DiskPreparedGraph` exposes
only revision, change count and result digest; it is not a commit capability. Journal commits and
derived roots retain their existing authority.

This profile supports creates, entity replacement, assertion/relationship lifecycle actions and
corrections, policy changes, and multi-operation overlay semantics with `Absent` or `Version`
preconditions. Non-correction lifecycle actions also prove the unchanged references retained in
the new record version. Exact negative lookups are stored distinctly from missing proofs.

Deletion and every `ReadView` precondition are rejected as `UnsupportedRequest` before root or
page access. Deletion needs a bounded complete reverse-family bucket; historical predicates need a
bounded predecessor/history proof. Treating either as current-record absence would be unsound.

## Consequences and limits

Exact index values remain bounded by the immutable format, while proof accounting describes
logical retained keys/values rather than allocator RSS. The live coordinator still retains the
complete reducer, root-delta metadata still requires a complete snapshot, and no live disk overlay
is published. The next T-20 increment must add the missing reverse/history proof primitives and a
persistent base/overlay coordinator contract before scale qualification.
