# Decision 0038 — Proof-prepared authoritative graph commit

Date: 2026-09-17

Status: accepted as T-20 write-path groundwork. T-20 remains open because the coordinator still
retains the complete live graph for publication/replay, recovery is full-memory, and BM-01/BM-06
are unqualified.

## Context

Decision 0037 could derive a terminal root plan from the complete authenticated transaction proof,
but the authoritative journal commit still called `TransactionState::prepare` against the complete
live graph. Allowing a generic caller to supply an arbitrary prepared reducer value would be
unsafe: the coordinator could durably record one canonical request while publishing an unrelated
state transition.

## Decision

The transaction coordinator now exposes a separate opt-in
`ExternallyPreparedTransactionState` contract and `commit_prepared` entry point. A reducer using
that path must validate, before durable I/O, that its opaque prepared value is bound to:

- the exact canonical request bytes and optional blob inventory;
- the coordinator-selected next revision; and
- the reducer's current live base.

The common commit implementation retains the existing validation, retry, cancellation, retention,
journal append, uncertainty and publication ordering. Exact idempotent retries return their prior
outcome before consulting a now-stale supplied prepared value, just as ordinary commits do. No
other reducer opts into this path.

`PreparedGraph` now privately retains the SHA-256 digest of the canonical encoded
`GraphTransaction` that produced it. `GraphState` opts into external preparation only when the
commit request has no blob inventory, its exact byte digest matches, the target revision matches,
and the prepared scope/base revision/base policy version/changed-record before-values match the
current live graph. Public callers cannot construct or alter `PreparedGraph` fields.

`commit_graph_disk_prepared` consumes the proof-backed graph result through this contract. Root
delta derivation now borrows that result first, allowing one bounded proof to supply both the
authoritative commit and the separately published derived-root plan.

## Consequences and limits

The authoritative commit no longer repeats full-graph preparation for a proof-backed transaction.
The journal remains the only commit authority, records the original canonical request, and ordinary
recovery re-prepares/replays it, independently checking the stored result digest.

The coordinator still retains a complete `GraphState`; external validation checks touched values
against it, publication mutates it, postcommit root validation scans it, and recovery reconstructs
it. This is not yet the live persistent base/overlay state, larger-than-memory recovery, or
BM-01/BM-06 qualification required to close T-20.
