# Decision 0025 — Encrypted derived-index profile and current-graph projection

Date: 2026-09-17

Status: accepted as the T-20 foundation. T-20 remains open until BM-01 and streaming
larger-than-memory BM-06 acceptance work passes.

This decision fixes the first native Rust disk-index representation without promoting it to a
second commit authority. It extends Decisions 0004, 0013, 0019 and 0020. T-35 continues to own
authoritative retained baselines, compaction, certificate-log rollover and orphan reclamation.

## Authority and publication

The encrypted journal and exact commit certificate remain authoritative. An index root is an
optional immutable cache anchored to one namespace, journal revision, certificate digest, reducer
profile, logical-state digest and index profile. It cannot create a commit or advance recovery.
Missing, stale, semantically inconsistent or corrupt roots and runs are omitted and rebuilt from
retained authority.

Each profile has two key-derived opaque root-slot names. Publication creates and flushes immutable
runs first, selects an overwrite slot only after fully authenticating and logically scrubbing both
candidate roots and all referenced run pages, removes the unusable/older slot, flushes that removal,
then creates, writes, sizes and flushes the new root and its directory entry. A crash may expose the
old usable root or the exact new root. Same-length run corruption cannot cause the only usable
fallback to be selected for removal. Expected absence, truncation, authentication failure and
unsupported formats decline a cache candidate; operational I/O, lock, key and resource failures
propagate and cannot influence overwrite selection. Failed publications leave only rebuildable
orphan runs; T-35 owns their bounded enumeration and reclamation.

Role `0D` (`IndexName`) is assigned to the HKDF public-token domain for root-slot names. Index page
and root encryption retains role `05` (`IndexPage`); their distinct object IDs and sequence values
are authenticated. Run filenames contain fresh random 128-bit object IDs and reveal only object
count. Root-slot filenames are deterministic opaque tokens and do not reveal namespace or profile.

## `index-v1` binary profile

All integers are unsigned big endian. Reserved bytes and unused fixed-buffer tails must be zero.
Unknown versions, trailing bytes, non-canonical ordering and inconsistent counts fail closed.

An exact logical page is 16,384 plaintext bytes. It is encrypted with the `crypto-v1` 4 KiB frame;
the authenticated length prefix therefore occupies five frames and produces exactly 20,545 encoded
bytes including the public envelope and tag. Its 80-byte header is:

| Offset | Bytes | Meaning |
|---:|---:|---|
| 0 | 4 | `UIPG` |
| 4 | 1 | major `01` |
| 5 | 1 | minor `00` |
| 6 | 1 | nonzero family |
| 7 | 1 | reserved zero |
| 8 | 8 | journal revision |
| 16 | 8 | zero-based page index |
| 24 | 4 | fragment count |
| 28 | 4 | used plaintext bytes including header |
| 32 | 32 | index profile |
| 64 | 16 | run object ID |

Fragments follow in sorted entry order as `key_len:u32`, `total_value_len:u32`,
`value_offset:u32`, `fragment_len:u32`, exact key bytes and exact value fragment bytes. One value may
span pages; every fragment repeats the complete key and total length. Offsets must be contiguous,
keys must strictly increase between completed entries and padding through byte 16,384 is zero.

A root plaintext is exactly 2,048 bytes. Its 192-byte header is:

| Offset | Bytes | Meaning |
|---:|---:|---|
| 0 | 4 | `UIRT` |
| 4 | 1 | major `01` |
| 5 | 1 | minor `00` |
| 6 | 1 | run count |
| 7 | 1 | reserved zero |
| 8 | 16 | namespace ID |
| 24 | 8 | journal revision |
| 32 | 8 | nonzero root generation |
| 40 | 32 | exact certificate digest |
| 72 | 32 | reducer profile |
| 104 | 32 | logical-state digest |
| 136 | 32 | index profile |
| 168 | 16 | root object ID |
| 184 | 8 | reserved zeros |

Each sorted 72-byte run descriptor contains family at byte 0, seven reserved zeros, 16-byte run
object ID, page count, entry count and 32-byte logical run digest. The digest is SHA-256 over the
literal `USTE-INDEX-RUN-V1\0`, namespace ID, revision, index profile, family and each complete entry
as `key_len:u32 || value_len:u64 || key || value`. The database, namespace, revision, profile, key
epoch and writer incarnation of an in-process run descriptor must exactly match the root publication
context; decoded descriptors inherit those authenticated root/context bindings.

## Limits, cache and graph families

The profile admits at most 16 runs per root, 16,777,216 pages and 1,000,000,000 entries per run,
4 KiB keys and 16 MiB values. Prefix scans admit at most 1,000,000 candidates and 64 MiB of returned key
plus value bytes. The decrypted LRU-style page cache defaults to 64 MiB, rejects budgets above
256 MiB, uses fixed 16 KiB boxed pages with conservative per-entry accounting and keys entries by
the complete database/namespace/epoch/writer/revision/profile/generation/run/page identity. Cache
diagnostics expose counters only; clearing and eviction zeroize plaintext buffers on normal drop.
Scrub always bypasses cached plaintext and rereads durable ciphertext.

`graph-current-v1` has mandatory metadata family 1 and conditionally nonempty current-record,
outgoing-adjacency, incoming-adjacency and provenance families 2 through 5. Admission requires the
supplied frozen `GraphSnapshot` to match the coordinator's borrowed live reducer state in scope,
revision and logical digest before index I/O. It then requires the exact current certificate and
reducer/logical/index profiles, a full storage scrub, the exact family set, counts and independently
recomputed family digests. Every read rechecks the live journal frontier, so a previously admitted
handle becomes stale after a commit.
The raw functions remain privileged maintenance/projection surfaces. Facade publication and root
discovery require the namespace `ManageSchema` action before reducer or filesystem access. They
return a policy-admitted handle whose bounded page cache is opaque, preventing consumers from using
cache counters as a hidden-candidate side channel. Consumer reads use the reducer-owned
`AuthorizedIndexedReadState` path: it validates the issuing view and current policy, authorizes
top-level targets before index I/O, filters each discovered candidate and embedded reference,
preserves reference result limits, shares one million-candidate/64 MiB scan budgets across both
adjacency directions, and rejects current-index use for historical reads. Handle admission binds
the full logical digest once; reads then check view revision in constant time and raw operations
revalidate the live journal certificate.

## Deliberate limits and remaining acceptance

The graph reducer and canonical checkpoint still retain and clone full state in memory. This index
therefore proves immutable encrypted runs, bounded page caching, restart recovery and current graph
projection equivalence; it does not prove that a 10-million-event recovery fits outside RAM. T-20
must add streaming checkpoint/state construction, the BM-01 100k/1m one-hop and four-hop workload,
and BM-06 10-million-event checkpoint recovery before its
checkbox can close. Temporal/property/lexical/spatial families belong to T-21/T-25/T-37/T-59.
Authoritative baseline switching, compaction and garbage collection remain T-35.
