# Decision 0037 — Proof-derived terminal root deltas

Date: 2026-09-17

Status: accepted as T-20 write-path groundwork. T-20 remains open because the coordinator and
postcommit validator still retain the complete graph, recovered live state is full-memory, and
BM-01/BM-06 are unqualified.

## Context

Decision 0036 made every graph operation and precondition preparable from a bounded authenticated
current/history/reverse proof, but its opaque result could only expose the reducer digest. Terminal
`graph-state-v1` delta construction still reacquired the complete live snapshot for base family
counts, current policy bytes and changed-record before-values. This prevented the proof result from
feeding the already bounded Decision 0033 root-merge path.

## Decision

The explicit-I/O loader now authenticates the single family-1 `graph-state-v1` metadata entry in
addition to the transaction-specific proof closure. It validates the metadata revision, format,
policy invariants and every declared family count against the admitted root descriptors. The
metadata key/value bytes and index read are included in the existing proof budget/report.

The storage-free preparation capability retains the admitted root anchor, the eight graph-state
metadata counters (including separate policy-history/current-policy counters), and exact decoded
current policy alongside the reducer result. Consuming that capability through
`prepare_graph_state_root_delta_from_disk` derives all eight bounded terminal family delta sets
without a filesystem, coordinator, page cache or key-vault capability. The original full-snapshot
entry point now shares the same derivation routine after independently checking every changed
record's before-value against its snapshot.

The resulting opaque plan uses the unchanged Decision 0033 publication contract: the exact base
anchor and durable outcome must match, storage merge is bounded, and publication independently
compares every merged descriptor and the logical digest with the complete postcommit reducer.

## Consequences and limits

One admitted-root proof can now cover graph preparation and precommit terminal-root derivation.
This removes the complete snapshot from that precommit path and does not weaken postcommit
validation or make a derived index authoritative. No journal, reducer, checkpoint or index format
changes.

The commit coordinator still prepares and publishes its authoritative transaction through the
complete live reducer, while terminal publication still scans that reducer as an independent
validator. Proof buckets are bounded collections rather than streaming reducer inputs. Recovery
still constructs complete live state, and no allocator/RSS or BM-01/BM-06 claim is made.
