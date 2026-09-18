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
  Authorized handles can also zeroize their pages without resetting cumulative counters and report
  successful authorized reads, completed raw operations, authenticated pages, fragments and
  logical result bytes without disclosing keys or plaintext. Because these counters reveal
  candidate-dependent work, current `ManageSchema` authorization and an issuer-instance-bound
  opaque root capability guard both report and clear operations.
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
# 263 workspace tests and 31 isolated t20-bench tests passed (2 exact-profile release tests ignored
# in the debug suite); format, clippy, rustdoc,
# docs/task graph, R0 vectors, storage publication model and isolated builds passed
~~~

The authorized graph regression proves a newly admitted handle starts empty, a cold one-hop read
performs three raw operations and reads authenticated pages, a repeated read produces cache hits
without another page read, clearing returns accounted bytes to zero while preserving every counter,
and the next read misses and reads pages again. Unauthorized and policy-revoked principals cannot
report or clear, and a root issued by a prior coordinator instance is rejected. This is explicit
USTE-cache control only; an outcome-uncertain coordinator also rejects both operations before
consulting possibly stale policy. No kernel/device cache eviction or BM-01 timing claim follows.

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
`acceptance/r1/bm01-materialization-v1.tsv` and checked in the normal repository script. It now also
has a production-backed development verifier capped at 1,000 entities. Its 20/200 golden
materializes and accepts records through the authorized durable coordinator, restarts/replays,
loads the encrypted index and matches all 384 measured query shapes against the independent oracle.
It explicitly emits `engine_benchmark: false`, uses the memory fault model and test key wrapper,
and records no timing/RSS; BM-01 remains unrun. Decision 0041 replaces profile-sized operation
collection with maximum-10,000-operation streaming transactions. The accepted profile has exactly
212 durable revisions: one policy, 11 evidence/entity, 100 relationship-create and 100 acceptance
revisions. The accepted TSV and generated manifest pin that plan without presenting it as a run.

Decision 0042 adds Linux-only `linux-create`, `linux-resume` and `linux-open` phases around that
mapping. They use the production Btrfs adapter, OS entropy and the portable Argon2id recovery
envelope. Password files are opened no-follow and must be owner-only, singly linked regular files
of 1–1024 exact bytes. The authenticated shared Evidence record binds the exact mapping and
materialization digest to the requested profile. Resume validates the recovered frontier, every
returned transaction revision and the final authorized read-view revision. Stable transaction
identities are designed to replay a durable prefix within the fixed 30-day outcome-retention
interval. Reports are content-free, set `engine_benchmark:false`, identify uncontrolled host caches
and disclose that graph state remains full-memory.

A release-built real-Btrfs development smoke created the 20/200 profile at revision 4, opened it in
a fresh process with exactly one current root, performed an idempotent resume that stayed at
revision 4 and reopened the same frontier/root again. Create reported 399 ms; the two opens and
resume reported 396 ms, 357 ms and 344 ms on the reference host. An attempted same-frontier open as
30/300 failed with the fixed `USTE_BM01_PROFILE_BINDING` code. These aggregate phase times are not
query latency, were not sampled repeatedly and are explicitly nonqualifying.
That initial evidence covered a completed-frontier retry only; Decision 0044 below subsequently
adds interrupted-prefix process-loss coverage at small scale.

Decision 0043 adds `bm01-oracle-summary-v1` generation outside the Linux query process and a
`linux-query` correctness phase. The summary is bounded to 256 KiB, binds the engine mapping and
measured corpus, and pins the exact-profile outcome split at 299 successful outputs, zero visit
limits and 85 expected result limits with digest
`5e9cb81200b2016ab470419021561e0a304e1eb1d6610e0633b35925b27df402`. The Linux phase reopens
portable recovery, clears only USTE's page cache before each authorized traversal, and compares
visits, result counts, `bm01-result-v1` logical bytes and digest or the typed limit outcome.

A release-built 20/200 Btrfs smoke matched all 384 outputs after revision-4 recovery. It reported
891 ms for the one-pass query phase, 2.430/4.892/4.955 ms p50/p95/p99, 5,660 KiB current RSS and
265,104 KiB peak RSS. Host caches were uncontrolled and graph state was full-memory. The report
therefore states `engine_benchmark:false`; these are diagnostics, not BM-01 evidence.

Decision 0044 adds an externally killed durable-prefix probe. Release-built Btrfs children parked
after revisions 1, 2 and 3 only after flushing a content-free marker; the harness SIGKILLed each
exact child. Fresh `linux-resume` processes recovered and completed every prefix to revision 4 with
one current root, zero repaired certificate-tail bytes and zero ignored uncommitted journal bytes;
fresh `linux-open` processes admitted the same result. These cover all incomplete phases of the
20/200 plan, not exact-scale duration or intra-transaction byte boundaries.

Decision 0045 adds the separate `bm01-oracle-bundle-v1` prerequisite for repeated sampling. Its
bounded nested summaries preserve the existing measured digest while adding 96 disjoint warm-up
expectations. The exact warm-up split is 74 successful outputs, zero visit limits and 22 expected
result limits; the accepted combined digest is
`d52869f24d635476f86374813e754be364d0d2df470544b71221c24b96145fae`.

Decision 0046 adds the repeated production sampler. Qualifying scale fixes one checked warm-up and
five complete minimum-60-second samples with no CLI lowering control. Each measured query is timed
and checked first with an empty USTE cache and then with the retained cache; success and expected
limit-refusal populations never mix. Reports include all-class and per-class depth percentiles,
successful visits/logical bytes, RSS and authenticated cache/index deltas.

A release-built 20/200 Btrfs smoke validated 96 warm-up queries and one full 768-execution paired
round in 1,725 ms. It reported 5,788 KiB current RSS, 264,916 KiB process-lifetime peak RSS,
2,815 empty-cache page reads versus zero retained-cache page reads and exact outcome matches.
This is nonqualifying functional evidence: host caches were uncontrolled, graph state remained
full-memory and the synchronous query API only permits a post-return 30-second check.

Decision 0047 adds a persistent-worker parent supervisor around that synchronous API. Flushed
markers bracket only the engine call; the parent kills and reaps the exact child if finish is absent
after 30 seconds. A release-built 20/200 Btrfs CLI smoke preserved the complete warm-up and paired
round, set deadline enforcement true, completed the sample in 1,758 ms and retained the 2,815/zero
empty/retained page-read attribution. The direct library path remains labeled unsupervised.

Decision 0048 removes the complete snapshot from proof-derived postcommit root publication. The
authenticated merge provisionally visits its exact ordered output; graph validates target family
counts and reproduces the existing canonical logical-state digest from the emitted primary entries
while validating secondary framing/count constraints, with one caller-bounded record-history
buffer. Wrong outcomes, undersized merge/history budgets and
restart after failed publication expose no target root; adequate retry produces a root equivalent
to the independent full-state projection. The live reducer, root admission, coordinator metadata
and recovery remain full-memory boundaries.

The exact sampler was not launched: the contemporaneous preflight showed 6.2 GiB available RAM
and 212 KiB free swap, below the accepted 24-GiB reservation, while Btrfs had 998 GiB free. This
transient resource condition does not block implementation and was not bypassed by shrinking the
qualifying profile.

The exact production-backed run was not launched during the Decision 0039 increment because the
reference host could not supply the accepted 24 GiB reservation: `/proc/meminfo` reported
65,570,248 KiB total, 5,835,172 KiB available and only 13,980 KiB of 8,388,604 KiB swap free.
The Btrfs/NVMe volume had 999 GiB free. This transient host-load condition blocks only a qualifying
measurement, not implementation, and the workload was not reduced or mislabeled as a substitute.

The full `bash scripts/check.sh` gate passed after the latest extension: 263 workspace tests, all
docs, strict clippy/rustdoc, the storage publication model, and 31 isolated T-20 fixture/engine/
Linux-runner tests passed; two exact-profile oracle tests are reserved for release-profile
acceptance commands and ignored by the debug suite.
