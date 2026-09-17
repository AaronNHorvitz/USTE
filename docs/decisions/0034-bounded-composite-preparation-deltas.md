# Decision 0034 — Bounded composite preparation deltas

Date: 2026-09-17

Status: accepted as T-20 in-memory write-path groundwork. T-20 remains open: reducers, explicit
snapshots, checkpoint decoding and semantic closure scans still retain complete state in memory,
and no BM-01/BM-06 qualification is claimed.

## Context

Decision 0027 made direct graph preparation change-bounded, but Decision 0023's composite ingest
reducer still cloned the complete graph, spatial catalog, job ledger and global source-event map
before validating each request. The spatial reducer also cloned its complete catalog. Those copies
were bounded by R1 correctness caps, but their retained memory scaled with existing state rather
than the admitted request and obstructed T-20's later disk-backed preparation contract.

## Decision

Composite preparation retains explicit unpublished deltas:

- Graph preparation returns its existing ordered before/after delta. An allocation-free
  `PreparedGraphView` overlays changed current records on the borrowed base for import closure
  checks. The opaque plan retains its base policy version, and publication rechecks scope, revision,
  policy version and every touched before-value before mutation. Plans remain same-owner
  capabilities rather than transferable same-revision snapshots.
- Spatial preparation stages at most the admitted request in borrowed overlay maps. Category and
  exact-version lookups combine staged and base history; all records are staged before semantic
  closure validation so forward world/root/frame references remain valid. Publication owns only
  records that actually insert. Exact observation retries retain an outcome and original recorded
  revision but no second record copy.
- The spatial catalog maintains exact entry/logical-byte counters plus a domain-separated
  SHA-256 entry-fingerprint accumulator. This private, non-authoritative fingerprint makes
  same-revision foreign-plan rejection constant-time and is reconstructed from checkpoint entries;
  it is not a disk format, journal authenticator or substitute for canonical logical-state hashes.
- Import preparation owns the prepared graph delta, optional spatial delta and one private ledger
  mutation. A batch ledger delta retains one receipt and at most 10,000 row/event commitments;
  unrelated jobs, batches and source events remain borrowed. Graph-only closure scans borrow the
  live spatial catalog and job map.

The composite publisher checks its top-level revision, graph base, optional spatial base, job
checkpoint, batch sequence and every new global source event before publishing any component.
After preflight it performs no fallible semantic validation. Allocator/process failure retains the
same crash/replay treatment as existing reducer publication; the journal coordinator remains the
only durable transaction authority.

## Compatibility

Canonical graph, spatial and ingest requests, results, checkpoints, reducer profiles and journal
bytes are unchanged. Spatial result hashing still encodes one request-ordered stored effect per
requested record, including the original recorded revision and insert/duplicate outcome. Pinned
pre-change digest vectors and existing golden/checkpoint/restart fixtures protect this contract.

Checkpoint reconstruction contains stored spatial entries only. A retry-only revision can
therefore leave the rebuilt catalog's internal last-entry revision behind the outer spatial
snapshot revision; later preparation uses the outer revision for transaction ordering and accepts
the intentional internal gap.

## Verification and limits

Tests cover request-order-sensitive sequential versions, forward references, same-batch exact
observation retries, old-retry-plus-correction acceptance, new-same-batch correction rejection,
retry-only checkpoint reconstruction, pinned result/chain digests, one-record preparation over a
populated catalog, stale and same-revision foreign-plan rejection, preview/direct equivalence and
composite preflight before mutation. Existing encrypted coordinator restart/retry and malformed
checkpoint suites remain unchanged.

This decision removes full retained-state copies from ordinary graph, spatial and composite ingest
preparation. It does not make the reducers larger-than-memory: current maps, complete closure
scans, canonical current-policy result encoding, explicit snapshots, checkpoint codecs and recovery
reconstruction remain materialized.
Decision 0035 subsequently supplies explicit-I/O proof loading and storage-free preparation for a
bounded current-state graph subset. Complete reverse/history proofs, live persistent base/overlay
lifecycle and qualifying BM-01/BM-06 runs remain T-20 work. Native spatial query indexes and BM-10
remain T-59.
