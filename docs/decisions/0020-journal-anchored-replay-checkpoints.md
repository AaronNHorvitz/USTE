# Decision 0020 — Journal-anchored encrypted replay checkpoints

Date: 2026-09-17

Status: accepted and locally qualified for T-18. This implements the deterministic replay and
optional-cache slice of Decisions 0003, 0004, 0015, 0016 and 0019. Checkpoints remain derived
objects; the encrypted certificate journal remains the commit authority. Retention baseline
promotion, disk-index roots, migrations and restore epochs remain later tasks.

## Replay and reducer contract

`uste-replay` has no filesystem, clock, parser, model or network capability. Cold replay admits
only contiguous committed revisions, reapplies the exact canonical request and committed blob
inventory, compares the reducer result digest before publication and computes a fallible streaming
logical-state digest. Empty replay has a deterministic genesis digest but cannot be published as a
checkpoint.

A checkpoint-capable reducer declares a 32-byte profile, scope/revision accessors, a streaming
logical digest and fallible canonical encode/decode operations. Decode must validate the complete
authoritative reducer state, re-encode byte-for-byte and rebuild rather than trust derived indexes.
The graph profile preserves all current records, record histories, current policy and policy
history. It validates revision ordering, lifecycle transitions and historical reference closure,
then deterministically rebuilds adjacency and provenance indexes. The graph payload and the outer
coordinator payload are each limited to 256 MiB.

## Coordinator checkpoint format and provenance

The canonical `UCCP` payload contains scope, checkpoint revision, the exact journal certificate
digest, reducer profile and logical digest, framed reducer bytes, retained retry outcomes and
first-commit blob ownership. Retry keys and blob references are strictly ordered. Outcome
revisions cannot exceed the checkpoint, transaction IDs are unique, and the same retained-outcome
and one-million committed-blob limits used by recovery apply during both encode and decode.
Counts are checked against remaining bytes before fallible allocation.

Storage returns an opaque `RecoveredCheckpoint`; external code cannot construct one. A recovery
seed can only be created from that authenticated carrier and must match its scope, revision,
profile and logical digest. On seeded open, the journal authenticates its complete certificate and
blob chain before callbacks. Prefix groups reconstruct retry, transaction and blob-owner metadata
and must equal the seed exactly at the named certificate; only reducer application before that
revision is skipped. Suffix groups use the normal result-digest-checked reducer replay. The anchor
is checked again after the candidate-reading owner is released and the writer is reopened.

This trust boundary assumes the engine-owned checkpoint publisher captured the current coherent
reducer state through the maintenance API. The opaque carrier prevents an ordinary caller from
substituting state. A caller that controls the raw journal owner, key adapter or reducer
implementation is already inside the trusted engine boundary. Cache rejection always permits a
full cold replay; no checkpoint can create a journal commit.

## Encrypted two-slot publication

The journal owner publishes namespace-scoped cache bytes in alternating A/B slots. Payloads are
split into 1 MiB plaintext chunks. Every chunk uses the existing key vault with
`ObjectRole::Snapshot` and authenticated database, namespace, key epoch, writer incarnation,
random object ID, sequence, profile and frame context. The terminal encrypted manifest binds the
certificate anchor, reducer profile, logical digest, payload length/count/digest, generation and
object ID.

Publication first removes and directory-syncs the target manifest, writes and fully syncs each
replacement chunk, syncs their directory entries, then writes, fully syncs and directory-syncs
the terminal manifest. The other complete slot is retained throughout. Recovery authenticates and
bounds the manifest before allocation, authenticates every exact-position chunk and payload digest,
and considers only candidates whose certificate digest is present at that exact journal revision.
Missing, malformed, corrupt, reordered, future or divergent candidates are omitted. Equal-
generation divergent slots fail closed for publication. The newest codec-valid candidate is tried
first and an older candidate or cold replay remains available.

## Qualification and limits

The deterministic memory adapter exercises crash-before and crash-after at all fourteen mutation
and durability boundaries of replacement publication. Every restart exposes an old complete cache
or the exact new cache, and retry converges. Tests also cover missing/corrupt/swapped chunks,
manifest corruption and bounds, malformed authenticated coordinator bytes, exact anchor refusal,
encrypted graph suffix recovery and cold/checkpoint logical equivalence.

Candidate discovery currently authenticates the full journal, releases ownership, and seeded open
authenticates it again. This deliberate double scan closes the handoff race but is not the T-20
disk-index or BM-06 performance result. A 256 MiB cache is a correctness limit, not a claim that
large graphs fit in memory or meet recovery targets. T-20 owns bounded disk projections; T-35 owns
baseline promotion/compaction; T-38/T-39 own backup, restore epochs and migration. No parser,
network service, external model or provider credential enters this path.
