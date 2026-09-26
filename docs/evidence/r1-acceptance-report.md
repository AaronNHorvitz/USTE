# T-19 R1 acceptance and operating-limitations report — DRAFT, NOT ACCEPTED

Date: 2026-09-23 · Status: **draft**. T-19 is not ticked, no release gate is ticked, and no benchmark
target is relabelled. This document states exactly what passes, what is measured but not
qualified, and what remains blocked, so the owner can decide acceptance after the qualifying runs.

Tested tree: lane branch (now `main`) at `f50f3a5` plus Decisions 0271–0274, branched from `codex/uste-implementation`
at `2263dc5`; toolchain `rust-toolchain.toml` 1.95.0; locked workspace and experiment manifests;
Linux x86_64, Btrfs. The lane ran inside a 5/6 GiB memory high/max, 512 MiB swap, three-CPU
scope with a 4 GiB per-process address-space limit; every measurement below labels that.

## 1. What passes (R1 correctness kernel)

The R1 verification scope is VT-01 through VT-08 as applicable to the kernel, VT-17 kernel cases,
and the VT-18/19 schema and VT-23 transaction subsets. Each closed R1 task links its evidence in
[TASKS.md](../../TASKS.md); the mapping is summarized here without restating it.

| Suite | Owning tasks | Evidence | Status |
|---|---|---|---|
| VT-01 | T-09, T-10, T-45, T-48 | [canonical types](canonical-types.md), [reference model](reference-model.md), [time normalization](time-normalization.md), [spatial schema/history](spatial-schema-history.md) | passes at kernel scope |
| VT-02 | T-14, T-17 | [transaction coordinator](transaction-coordinator-foundation.md), [transactional evidence graph](transactional-evidence-graph.md) | passes; graph phantom predicates covered by T-17 |
| VT-03 | T-12, T-13, T-15 | [I/O fault harness](io-fault-harness.md), [journal foundation](journal-foundation.md), [blob store](blob-store-foundation.md) | passes every initial publication boundary and hard-corruption case tested; no power-cut claim |
| VT-04 | T-13, T-18, T-20 (partial) | [journal foundation](journal-foundation.md), [replay checkpoints](replay-checkpoints.md), [journal-anchored disk recovery](journal-anchored-disk-recovery.md) | passes cold/checkpoint equivalence and suffix recovery; larger-than-memory recovery is not qualified |
| VT-05 | T-17, T-20 (partial) | [transactional evidence graph](transactional-evidence-graph.md), [disk-index foundation](disk-index-foundation.md) | adjacency/reference/rebuild slice passes; disk-cache pressure is observed at 20,000 entities only |
| VT-06 | T-16, T-17 | [authorization foundation](authorization-foundation.md) | passes default denial, no cross-scope leakage, exact-byte quotas, graph concealment |
| VT-07 | T-11 | [crypto boundary](crypto-boundary.md) | T-11 slice passes; rotation/clone/restore remain later tasks |
| VT-08 | T-15 | [blob store](blob-store-foundation.md) | passes exact unknown-binary round-trip at 12 GiB with 267,636 KiB peak RSS |
| VT-14 | T-18, T-20 (partial) | [replay checkpoints](replay-checkpoints.md), [native BM-06 recovery controls](native-bm06-recovery-controls.md) | snapshot equivalence and fail-closed fallback pass; scrub/backup/restore are R3 |
| VT-17 | T-45 | [time normalization](time-normalization.md) | 12 R0 vectors and envelope malformed matrix pass |
| VT-18/19 | T-48 | [spatial schema/history](spatial-schema-history.md) | schema subset passes |
| VT-23 | T-49 | [atomic import transactions](atomic-import-transactions.md) | transaction subset passes |

Gate: `bash scripts/check.sh` (format, strict Clippy, full workspace tests, rustdoc with warnings
denied, experiment manifests, docs/task-graph checks, R0 vectors, storage-publication model,
isolated experiment builds/tests). Result in this lane on the Decision 0267 tree (693 min 58 s,
plain debug profile): every step passed except one legacy BM-06 process test,
`recovery_process::bm06_native_resumes_authenticated_prefixes_without_duplicate_commits`, whose
fixed 30-second child-startup wait is exceeded in this lane's unoptimized three-CPU environment
(57.9–58.7 s to the revision-99/100 marker on both the changed and the unchanged `2263dc5`
binaries). The gate is therefore **not green in this lane**; the failure is retained and its
scope is the lane environment, not the kernel. The gate's final bench-Clippy step, skipped by the
abort, passed separately. Decision 0267 records the details.

## 2. Measured but not qualified — BM-01 development observations

BM-01's accepted target is warm one-hop p99 ≤ 20 ms and four-hop p99 ≤ 250 ms at 100,000
entities / 1,000,000 relationships on the reserved reference runner with one warm-up and five
complete ≥ 60-second samples. Everything below is the **20,000-entity development protocol**
(96 warm-ups, 768 measured executions, one round, 96/128/32 MiB page/lookup/range split,
uncontrolled kernel/device caches). It is not qualification.

| observation | host | commit | 1-hop p99 ms | 2-hop | 3-hop | 4-hop p99 ms | semantic report |
|---|---|---|---|---|---|---|---|
| Decision 0265 | reference host | `ede012f` | 2.634 | 87.526 | 61.596 | 368.917 | identical |
| Decision 0266 (lane baseline) | lane | `2263dc5` | 2.765 | 100.721 | 67.027 | 374.535 | identical |
| Decision 0268 run 1 | lane | D0267 | 2.292 | 63.261 | 45.136 | 227.248 | identical |
| Decision 0268 run 2 | lane | D0267 | 2.429 | 63.196 | 44.428 | 231.487 | identical |
| Decision 0268 run 3 | lane | D0267 | 2.370 | 63.707 | 46.042 | 233.320 | identical |
| Decision 0272 | lane | D0267 + D0271 | 2.372 | 65.824 | 39.905 | 211.094 | identical |
| Decision 0274 run 1 | lane | D0267 + D0271 + D0273 | 2.320 | 59.547 | 40.537 | 208.501 | identical |
| Decision 0274 run 2 | lane | D0267 + D0271 + D0273 | 2.466 | 60.453 | 41.401 | 199.612 | identical |

"Semantic report identical" means the complete report with only timing, percentile and RSS
fields removed hashes identically (lane recipe `0334c5cd…`): outcomes, logical results, cache
counters, adapter reads, vault work and residency did not change across these binaries.

At the development scale the four-hop and one-hop targets are met in this lane in every run from
Decision 0268 onward. Decision 0271 removes harness bookkeeping inside the measured interval, not
engine work, and is disclosed as such; Decision 0273's timing change is within run-to-run spread. The empty-USTE-cache half (four-hop p99 ≈ 3.58 s) is dominated by physical
page reads and decryption; no cold target is set and none is claimed.

## 3. Blocked — what T-19 acceptance still requires

R1 requires BM-01, BM-02, BM-04 and BM-06 to genuinely pass. None does today.

| benchmark | target | status | blocker |
|---|---|---|---|
| BM-01 | 1-hop ≤ 20 ms, 4-hop ≤ 250 ms p99 at 100k/1m, five samples | development pass at 20k in lane only | exact sampler needs the 24 GiB reserved host; the native packed engine refuses profiles above 20,000 entities (`USTE_BM01_DISK_DEVELOPMENT_LIMIT`) until a decision raises the ceiling with scale evidence |
| BM-02 | single durable commit p99 ≤ 50 ms, ≥ 2,000 events/s in batches | **unqualified**; development runner (Decision 0285) observed 1.5–1.8 ms single-commit p99 and about 143,000 events/s wall-clock in 1,000-event batches (Decision 0286, single writer, same-thread reads) | the owner must pin the qualifying manifest (counts, reader concurrency, duration; only the seed and budget are pinned), then a reserved-host run with concurrent readers and queue/backpressure reporting |
| BM-04 | ≥ 250 MiB/s streaming ingest, peak RSS ≤ 1 GiB above cache | **failed**: 95.923 MiB/s at 267,636 KiB peak RSS (T-15, Decision 0017 evidence) | throughput work on the encrypted chunk pipeline; target may not be lowered without a versioned decision |
| BM-06 | 10,000,000 events recovered from a checkpoint ≤ 120 s | **unmeasured**; fixture pinned (`bm06-materialization-v1`), native development recovery controls pass at ≤ 8,192 records | exact-size native construction and 30 reserved-host recovery trials; packed native admission capped at 8,192 records |

## 4. Owner-run qualifying steps (unperformed in this lane)

These need the accepted 24 GiB reservation and a Btrfs root on the reference host; the lane's
6 GiB cap cannot run them and the profile must not be shrunk to fit. Build once with
`cargo build --release --locked --offline --manifest-path experiments/t20-bench/Cargo.toml`
and pin the binary hash.

BM-01 exact sampler (legacy v1 engine, full-memory graph boundary retained):

```sh
B=experiments/t20-bench/target/release/uste-t20-bench
$B oracle-bundle > "$ROOT/oracle-bundle"            # default --entities 100000
$B linux-create --root "$ROOT" --password-file "$PW"  # or linux-resume after interruption
$B linux-open   --root "$ROOT" --password-file "$PW"
$B linux-sample --root "$ROOT" --password-file "$PW" --oracle-file "$ROOT/oracle-bundle"
```

Run each under `/usr/bin/time -v`, a 1,800-second `timeout`, and the owner's 24 GiB scope;
retain `*.json`, `*.stderr`, `*.time` and scope peaks as the existing decisions do. The
sampler enforces one warm-up plus five ≥ 60-second samples at the exact profile and cannot be
lowered from the command line.

BM-01 packed engine at qualifying scale: first raise `MAX_NATIVE_DEVELOPMENT_ENTITIES`
(`experiments/t20-bench/src/linux_runner/disk.rs`) by a versioned decision with construction
and memory evidence, then repeat `linux-packed-create`, `linux-packed-open`,
`linux-packed-query` and `linux-packed-wide-medium-range-pressure-sample` at `--entities 100000`.

BM-06: after the same kind of admission decision for records above 8,192, run
`bm06-manifest`, `bm06-packed-linux-create --records 10000000`, then the 30 reserved-host
`bm06-packed-linux-recover-checkpoint` trials with the fault/control scenarios from Decision 0114.

BM-02 has a development runner (`linux-bm02-development`, Decision 0285) but no qualifying command
until its manifest is pinned. BM-04 needs pipeline work (Decision 0283) before a rerun of the T-15
12 GiB probe.

## 5. Runnable kernel instructions

- Full gate: `bash scripts/check.sh` (about 40 minutes with one test thread and three Cargo jobs).
- Development BM-01 fixture and observation at 20,000 entities (about 2.5 minutes to create,
  11–13 minutes per observation, 350 MiB peak RSS): see the exact commands in
  [Decision 0266](../decisions/0266-standalone-lane-baseline-observation.md) and the
  `linux-packed-*` usage in [`experiments/t20-bench/README.md`](../../experiments/t20-bench/README.md).
- Offline memory pilot demo: [docs/memory-pilot-demo.md](../memory-pilot-demo.md).

## 6. Operating limitations (non-production)

- Not a released or supported database executable; local gate success does not authorize
  distribution (T-62 private vulnerability reporting remains unverified).
- Single machine, one owning process, Linux x86_64 with Btrfs as the only exercised profile;
  ext4 was verified separately only for creation outcomes.
- Native packed BM-01 admission is capped at 20,000 entities; native BM-06 at 8,192 records;
  the memory-model verifiers at 1,000 entities / 2 records. These are safety ceilings, not
  measured capacity.
- Coordinator metadata, journal-origin metadata replay and parts of discovery remain
  memory-resident; larger-than-memory recovery (BM-06's property) is not demonstrated.
- Multi-hop traversal is client-side composition of one-hop authorized reads; there is no native
  multi-hop request, and each hop re-reads and re-decodes neighbour records.
- No compaction, orphan reclamation, backup/restore, retention purge or key rotation (T-34–T-36);
  no parsers, workers or retrieval beyond the graph kernel (R2/R3).
- Complete authenticated I/O accounting is partial (single-owner vault decrypt ledger);
  reported adapter counters are not physical device I/O.
- Cache clearing affects USTE's own partitions only; kernel/filesystem/device caches are
  uncontrolled in every development observation.
- No independent security review (T-41); automated checks are not human assessment.

## 7. Acceptance decision

Withheld. Accept T-19 only when BM-01 (qualifying profile), BM-02, BM-04 and BM-06 each have a
retained passing result under the accepted protocol, or when versioned decisions with rationale
and rerun evidence change a target. This draft does not change any target.
