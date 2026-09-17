# Decision 0029 — Certificate-anchored graph state root

Date: 2026-09-17

Status: accepted as T-20 format/root groundwork. T-20 remains open; this root does not seed reducer
recovery and no BM-01/BM-06 result is claimed.

## Context

The frozen `graph-current-v1` profile contains only current records, adjacency and provenance. It
cannot represent record history, policy history or general reverse references and must not be
silently reinterpreted as mutable reducer state. Decisions 0027 and 0028 provide ordered graph
deltas and an incrementally maintained target/owner reverse map, allowing every complete state
family to be emitted in canonical key order.

## Decision

`graph-state-v1` is a distinct optional derived-cache profile over the encrypted immutable
`index-v1` carrier. It has at most eight nonempty, single-run families: metadata, current records,
record history, outgoing adjacency, incoming adjacency, provenance, reverse references and policy.
Empty optional families are omitted; metadata is always present and records exact family entry
counts. The byte contract is pinned in `acceptance/r1/graph-state-v1.tsv`.

Metadata is exactly 80 bytes: `UGSM`, major `1`, minor `0`, two zero reserved bytes, the root
revision as `u64be`, then eight `u64be` counts in the documented family order. Reverse owner kinds
are entity `1`, assertion `2` and relationship `3`; entity states are active `1` and deleted `2`;
claim states proposed through expired are `1` through `7`. Role bits are, in order, entity property
`0x0001`, assertion subject `0x0002`, assertion object `0x0004`, relationship from `0x0008`,
relationship to `0x0010`, relationship property `0x0020`, evidence `0x0040` and correction-of
`0x0080`. Values outside these assignments are not `graph-state-v1`.

Record values reuse the canonical complete stored-record codec. History keys append the record's
modified revision. Reverse keys are target then owner; their fixed 24-byte values bind explicit
kind/state/role codes, owner version and modified revision. Policy key `00` contains the current
policy and keys `01 || revision` contain increasing history values; current must equal the last
history value because the source snapshot already enforces that invariant.

Publication first proves the supplied snapshot matches the coordinator's live scope, revision and
logical digest. It independently recomputes every expected run count/digest, compares each produced
descriptor, and publishes a root bound to the exact current certificate, reducer profile, logical
state digest and new state profile. Loading repeats those semantic checks against the live snapshot,
fully scrubs referenced encrypted pages and declines corrupt, unsupported, stale or self-consistent
wrong roots. Missing roots do not affect journal state.

## Authority and limits

The journal remains the only commit authority. This module neither constructs `GraphState` nor
selects a recovery baseline, changes the frontier, serves consumer queries or falls back to an older
revision. Root publication is trusted optional maintenance after the authoritative commit.

The publisher and admission checker still require a complete in-memory `GraphSnapshot` and encode
the state twice (expected digest and run publication). The current `index-v1` carrier permits one
terminal run per family; scratch/overlay runs must eventually be merged before root publication.
There is no aggregate/per-target reverse fanout cap, and the consumer prefix-scan ceiling is not a
privileged full-run recovery iterator. Bounded root-based recovery, scratch merge, ingest state and
BM-01/BM-06 evidence remain required before T-20 can close.
