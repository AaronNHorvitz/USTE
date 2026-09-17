# Decision 0016 — Transaction coordinator and group format

Date: 2026-09-17

Status: accepted and implemented for T-14. This refines Decisions 0003 and 0015 without defining
graph mutations, authorization policy, blob inventory publication or compaction.

## Coordinator boundary

`uste-txn` owns serialized commit sequencing for one explicit database/namespace scope. A domain
reducer implements `TransactionState`: it receives bounded canonical request bytes and the proposed
revision, validates current-state preconditions, and returns a canonical result digest. The
reducer returns an owned prepared change without mutating live state. Logical failure discards that
value; durable success publishes it only after the journal certificate is synced.

`ReadView` owns a reducer-produced immutable snapshot and pins its revision. It never exposes staging. This is
the initial correctness profile, not a scalable MVCC claim. A mutable coordinator serializes calls;
later APIs may queue work but must preserve this single publication order.

Cancellation is observed before validation and again before journal publication. Once publication
starts it cannot turn a possibly committed operation into `Cancelled`. An uncertain journal result
returns `OutcomeUnknown`, quarantines that coordinator from both reads and writes, and requires
reopen/recovery before an outcome is reported.

## Retry identity and retention

One retry key is `(database, namespace, authenticated-principal digest, idempotency key)`. It binds
the SHA-256 digest of the exact canonical reducer request and the transaction ID. Reuse within the
retention interval returns the original revision/result; changed request or transaction identity is
`Conflict`. Transaction IDs are independently unique in the namespace and support outcome lookup.

The coordinator configuration supplies namespace retention of 30 through 365 whole days, while the
coordinator samples wall time from an explicit trusted `Clock` capability; neither is part of the
client request. A changed current policy governs new commits without invalidating authenticated
historical expiry intervals. Outcomes are never silently reused as new work after expiry. Wall rollback may
conservatively retain an outcome longer; wall time never chooses commit order. The in-memory indexes
are capped at Decision 0007's 10 million outcomes per namespace. T-35 owns compaction/retention
tombstones without allowing expired uncertain operations to become new.

## `UTXN` format 1.0

The exact group is a 192-byte header followed by the canonical reducer request. The database is
authenticated by the journal context; the namespace is repeated in the group. All integers are
big-endian and reserved bytes are zero.

| Offset | Bytes | Meaning |
|---:|---:|---|
| 0 | 4 | `UTXN` |
| 4 | 4 | major/minor `01 00`, two reserved zeros |
| 8 | 16 | namespace ID |
| 24 | 32 | trusted principal digest |
| 56 | 16 | idempotency key |
| 72 | 16 | transaction ID |
| 88 | 8 | accepted wall floor seconds |
| 96 | 4 | accepted nanoseconds |
| 100 | 8 | expiry wall floor seconds |
| 108 | 4 | expiry nanoseconds |
| 112 | 8 | request byte length |
| 120 | 32 | SHA-256 of exact request bytes |
| 152 | 32 | reducer result digest |
| 184 | 8 | reserved zeros |
| 192 | variable | exact canonical reducer request |

The complete group is limited to 16 MiB. Recovery first authenticates the journal, then verifies the
group shape, scope, lengths, request digest, whole-day retention bounds, logical-event digest,
duplicate retry/transaction identities and deterministic reducer result before publishing state.

## Qualification boundary

Qualification tests exact retry, changed-payload and stale-state conflicts, both pre-publication
cancellation polls, pinned readers, restart reconstruction, expiry, transaction lookup, every
initial journal publication error/crash boundary, short writes, authenticated malformed groups, a
32-caller stale-mutation race and a lost certificate-sync response that becomes a durable outcome
after recovery. The literal `txn-group-v1.hex` fixture fixes the complete format bytes. Later graph,
blob, authorization, compaction and full mixed-load tasks extend this coordinator rather than
weakening its publication contract. T-17 retains the phantom-sensitive predicate and reference-
model integration needed for the complete VT-02 suite.
