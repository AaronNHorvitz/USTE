# T-20 encrypted disk-index foundation evidence

Date: 2026-09-17

Status: implementation increment verified locally; T-20 remains open.

## Implemented scope

- `index-v1` immutable sorted runs with exact 16 KiB authenticated logical pages, fragmented values
  through 16 MiB and strict canonical page/root decoding.
- Two opaque certificate-anchored root slots. Publication fully scrubs candidate run bytes before
  choosing a replacement, propagates operational I/O/key failures and preserves a demonstrably
  usable fallback across every root publication boundary tested.
- Exact run binding to database/namespace, revision, profile, key epoch and writer incarnation.
- A fixed-byte-budget decrypted page cache keyed by the full authenticated identity. Diagnostics
  expose counters only; scrub clears cached plaintext and verifies exact durable file lengths.
- `graph-current-v1` current-record, outgoing/incoming adjacency and provenance families, with exact
  live-coordinator snapshot admission, independent expected family/count/digest comparison, restart
  recovery and per-read stale-frontier rejection.
- Literal page/root SHA-256 vectors in `acceptance/r1/index-v1.tsv` and an assigned/pinned opaque
  index-name derivation under crypto role `0D`.

The journal remains the sole commit authority. The disk APIs in this increment are privileged raw
maintenance/projection surfaces and cannot advance recovery or create a commit.

## Verification

~~~text
cargo test -p uste-storage index --locked
# 4 passed; 0 failed
cargo test -p uste-graph --test disk_index --locked
# 1 passed; 0 failed
cargo clippy -p uste-crypto -p uste-storage -p uste-txn -p uste-graph \
  --all-targets --locked -- -D warnings
# passed
bash scripts/check.sh
# 234 workspace tests passed; format, clippy, rustdoc, docs/task graph, R0 vectors,
# storage publication model and isolated dependency/fixture builds passed
~~~

The storage regressions cover large fragmented values, exact/prefix reads, cache eviction and
cross-database identity, cached-plaintext bypass, same-length corruption, trailing bytes, root
corruption fallback, transient root/run read errors, and crash before/after every root publication
operation. Graph coverage includes records, both adjacency directions, provenance, encrypted
restart, stale handles, a self-consistent but logically wrong root, and a same-scope/same-revision
foreign snapshot.

## Remaining T-20 acceptance

- Add an authorization-preserving consumer disk-query path and repeat graph disclosure fixtures
  through it; the raw API is not consumer integration.
- Replace the 256 MiB materialized graph/coordinator checkpoint and full-state clone path with a
  streaming larger-than-memory recovery design.
- Implement and run BM-01 at its exact 100k/1m one-hop and four-hop sizes with normal encryption and
  authorization, including cold/warm latency and RSS.
- Implement and run BM-06 for 10 million events from a checkpoint within the unchanged 120-second
  budget and report RSS/I/O amplification.
- Complete the applicable VT-05/VT-14 rebuild and visibility matrix. T-35 separately owns
  authoritative baseline promotion, compaction and orphan reclamation.

No benchmark or T-20 task completion is claimed by this increment.
