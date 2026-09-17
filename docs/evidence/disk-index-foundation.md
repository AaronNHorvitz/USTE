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

The journal remains the sole commit authority. Privileged raw disk APIs cannot advance recovery or
create a commit. Consumer current-graph reads now go through `AuthorizedIndexedReadState`, which
reuses the mandatory lease/top-level/candidate/reference policy checks before returning disk data.

## Verification

~~~text
cargo test -p uste-storage index --locked
# 4 passed; 0 failed
cargo test -p uste-graph --test disk_index --locked
# 1 passed; 0 failed
cargo test -p uste-graph --test authorized_graph --locked
# 3 passed; 0 failed
cargo clippy -p uste-crypto -p uste-storage -p uste-txn -p uste-graph \
  --all-targets --locked -- -D warnings
# passed
bash scripts/check.sh
# 237 workspace tests and 10 isolated t20-bench tests passed; format, clippy, rustdoc,
# docs/task graph, R0 vectors, storage publication model and isolated builds passed
~~~

The storage regressions cover large fragmented values, exact/prefix reads, cache eviction and
cross-database identity, cached-plaintext bypass, same-length corruption, trailing bytes, root
corruption fallback, transient root/run read errors, and crash before/after every root publication
operation. Graph coverage includes records, both adjacency directions, provenance, encrypted
restart, stale handles and policies, denied maintenance and read requests, hidden candidates,
shared mixed-direction scan limits, reference-result equivalence, a self-consistent but logically
wrong root, and a same-scope/same-revision foreign snapshot. Authorized handles retain an opaque
bounded cache, and per-read view binding is constant-time after full admission.

## Remaining T-20 acceptance

- Replace the 256 MiB materialized graph/coordinator checkpoint and full-state clone path with a
  streaming larger-than-memory recovery design.
- Implement and run BM-01 at its exact 100k/1m one-hop and four-hop sizes with normal encryption and
  authorization, including cold/warm latency and RSS.
- Implement and run BM-06 for 10 million events from a checkpoint within the unchanged 120-second
  budget and report RSS/I/O amplification.
- Complete the applicable VT-05/VT-14 rebuild and visibility matrix. T-35 separately owns
  authoritative baseline promotion, compaction and orphan reclamation.

No benchmark or T-20 task completion is claimed by this increment.

## Streaming and fixture groundwork

Decision 0026 adds borrow-aware current-state checkpoint methods so graph/spatial replay metadata
does not clone complete snapshots, and a regression reducer whose `snapshot()` panics proves cold
replay and capture use that path. Graph checkpoint encoding emits canonical format-1.0 bytes to a
fallible sink; the old collecting API returns identical bytes. Storage publication retains one
1 MiB plaintext chunk of the new payload, hashes incrementally and withholds the terminal manifest on explicit
producer error or declared-length mismatch.

Candidate discovery now verifies manifests and complete chunk digests with bounded plaintext and
returns opaque certificate-anchored metadata. Selected recovery revalidates the exact manifest and
emits authenticated chunks under the live owner/key context; sink failure stops immediately, and
callers must not publish partial decoded state before the final digest succeeds. The compatibility
collector remains byte-identical. Reducer decoders still require complete logical state and
therefore do not yet provide BM-06's larger-than-memory property.

Decision 0027 separately removes full-state graph cloning and index rebuilding from successful
transaction prepare/publish. Its ordered before/after deltas are the input contract for a future
disk-backed state root, but explicit snapshot/checkpoint decoding, ingest preparation and graph
delete scans remain full-state boundaries. See
[`bounded-graph-deltas.md`](bounded-graph-deltas.md).

Decision 0028 then adds the incrementally maintained target/owner reverse map and removes delete's
full-record scan. The map remains in memory and is not yet part of a durable state profile; see
[`reverse-dependency-index.md`](reverse-dependency-index.md).

Decisions 0032 and 0033 add an authenticated bounded base/delta run merge and the graph-owned
terminal-root bridge. One precommit plan now maps exact graph changes to every `graph-state-v1`
family and postcommit publication independently checks the complete result before visibility; see
[`graph-state-root-deltas.md`](graph-state-root-deltas.md). The live reducer and semantic comparison
remain full-memory, so this is not T-20 closure.

Decision 0034 separately bounds ordinary spatial/composite ingest preparation; see
[`bounded-composite-preparation.md`](bounded-composite-preparation.md). It does not change the
full-memory live reducer or recovery boundary listed above.

Decision 0035 adds bounded explicit-I/O positive/negative current-record proofs followed by a
storage-free preparation phase for the supported graph subset; see
[`explicit-io-graph-preparation.md`](explicit-io-graph-preparation.md). Deletion, historical
predicates, live overlay publication and the full-memory recovery boundary remain open.

Index prefix scans can now yield entries to a fallible visitor under the existing shared result
limits. The collecting and visitor forms return identical entries/statistics, and visitor failure
stops after the first delivered entry in the regression.

The isolated `experiments/t20-bench` crate pins the exact BM-01 seed, 100,000/1,000,000 fixture,
80/10/10 uniform/hub/ring topology, typed identifiers, measured/warm-up query corpora and an
independent adjacency-array BFS oracle. Its exact digests are recorded in
`acceptance/r1/bm01-materialization-v1.tsv` and checked in the normal repository script. It neither
opens USTE nor measures it and explicitly emits `engine_benchmark: false`; BM-01 remains unrun.

The full `bash scripts/check.sh` gate passed after this extension: 237 workspace tests, all docs,
strict clippy/rustdoc, the storage publication model, and 10 isolated T-20 fixture tests passed.
