# Decision 0026 — Streaming checkpoint transport and benchmark groundwork

Date: 2026-09-17

Status: accepted as T-20 groundwork. T-20 remains open; no BM-01 or BM-06 result is claimed.

This decision extends Decisions 0020 and 0025 without changing their frozen checkpoint or
`graph-current-v1` bytes. It removes avoidable whole-state copies from replay metadata paths,
introduces bounded checkpoint publication and index visitation surfaces, and pins the exact BM-01
fixture materialization needed for later qualifying measurements.

## Borrowed state and bounded transport

`CheckpointState` gains borrow-aware current scope, revision, digest and encoding operations.
Compatibility defaults preserve existing reducers, but graph and spatial states read their retained
snapshots directly; ingest exposes its composite revision without cloning. Cold replay checks the
current revision after each publication without creating a snapshot. Graph canonical checkpoint
bytes can be emitted incrementally, while the allocating capture API remains for compatibility and
small correctness fixtures.

Checkpoint publication accepts a declared-length fallible producer, retains at most one 1 MiB
plaintext chunk of the new payload, hashes it incrementally, encrypts and synchronizes each chunk,
and publishes the terminal manifest only after the producer emits exactly the declared length.
Producer failure, under-production and over-production cannot make a cache candidate visible. The
existing 256 MiB format cap and exact format-1.0 bytes remain unchanged. Candidate selection and
recovery still collect authenticated existing chunks into `Vec`s; this publication change is not
larger-than-memory recovery.

Index prefix scans gain a visitor surface with the same global candidate and returned-byte limits.
The original collecting scan is a compatibility wrapper over it. Visitor failure stops the scan
and has no authority or publication effect.

## BM-01 materialization contract

`bm01-materialization-v1` fixes the accepted seed, typed ID mapping, topology construction and query
corpora. Its qualifying-size fixture has exactly 100,000 entities and 1,000,000 relationships:
800,000 uniform directed non-self edges, 100,000 edges balanced across 100 hubs, and a 100,000-edge
clockwise ring. Each topology/depth stratum has 32 measured roots and 8 disjoint warm-up roots for
depths one through four. The independent adjacency-array BFS uses stable ordinal order and the
unchanged global one-million-visit and 100,000-unique-result limits.

The standalone fixture crate and `acceptance/r1/bm01-materialization-v1.tsv` pin complete synthetic,
materialization, topology and query-corpus digests. Its manifest is content-free and states
`engine_benchmark: false`. A qualifying-size fixture is not a qualifying benchmark: no durable
database, authorization, encrypted query, cache-state control, latency, I/O or RSS measurement is
performed by this component.

## Required next state profile

The current graph and ingest reducers still clone complete candidates during prepare, validate and
rebuild full in-memory indexes, and the recovery checkpoint decoder still materializes the full
payload. `graph-current-v1` is a frozen derived projection and will not be repurposed as mutable
state. The scalable path therefore requires new versioned graph/ingest state profiles with:

- encrypted scratch runs and certificate-bound roots for current records, histories, adjacency,
  provenance, policy, spatial history, jobs, receipts, source ownership and coordinator metadata;
- delta application and affected-closure validation rather than whole-state clone/rebuild;
- tombstones and bounded merge/read overlays with an independently checked canonical digest; and
- streaming checkpoint recovery or equivalent root-based reconstruction whose admitted state is
  verified against sequential replay without holding the complete logical history in RAM.

Authoritative baseline promotion, root rollover and orphan reclamation remain T-35. The optional
T-20 state/index structures cannot become a second commit authority.

## Remaining acceptance

BM-01 must still run the exact fixture through authorized encrypted disk queries with normal
durability, cold and warm cache states, required sample durations and latency/visit/result-byte/RSS
reporting. BM-06 must define and pin its exact 10-million-event workload, recover it within the
unchanged 120-second budget, and demonstrate state exceeding the memory allowance rather than a
counter or full-RAM surrogate. Current checkpoint publication alone satisfies neither benchmark.
