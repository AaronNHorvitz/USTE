# Verification and benchmarks

Draft acceptance specification · 2026-09-16

R0 vectors and synthetic fixtures exist. T-15 records one local BM-04 large-object measurement;
the remaining production suites, benchmark workloads and CI gates remain unimplemented unless a
later evidence record says otherwise.
Owns NFR-02, NFR-03 and the evidence required by [PRD release gates](../PRD.md).

## Evidence format and independence

Each result records tested commit/tree, toolchain, dependency lock/features, fixture manifest
hash, OS/kernel/filesystem/device, hardware, cryptographic/parser/model versions, limits,
invocation and raw results. Sensitive fixtures are synthetic or lawfully redistributable.
Never commit real private source files, credentials, extracted personal data or model keys.

The manifest lists normalized relative paths and byte hashes in stable order; hash that
manifest, not a platform-specific directory representation. Tests without pinned fixtures
and declared expectations are exploratory, not release evidence.
Reference-model comparisons use independently simple implementations where possible;
replaying the same buggy reducer twice is not independent proof.

## Test suites

| ID | Minimum cases and pass condition |
|---|---|
| VT-01 | Schema/identity/temporal model: invalid types, bounds, namespaces, intervals and transitions rejected; generated valid histories agree with reference |
| VT-02 | Transactions: interleavings, failed preconditions, lost replies, duplicate retries, phantom-sensitive operations; no partial visibility or duplicate effects |
| VT-03 | Durability: inject crash/short write/disk full at every journal, metadata and blob publication boundary; acknowledged commits preserved under declared failure model or explicit integrity error |
| VT-04 | Recovery/replay: cold replay, wrong keys, corrupt committed bytes, missing blobs, torn commit metadata; no silent committed rollback or external calls |
| VT-05 | Graph/indexes: adjacency symmetry, endpoint constraints, cycles, high-degree hubs, temporal paths, index loss/rebuild and disk-cache pressure |
| VT-06 | Authorization: cross-namespace lookup/search/count/path/blob/history/branch/export denial; revocation while queries/workers/subscriptions are active |
| VT-07 | Cryptography: wrong context/key/tag, truncated data, nonce/domain tests across restart/clone/restore, rotation, redaction and locked-state behavior |
| VT-08 | Raw content: arbitrary bytes including zero-length and unknown types, interrupted streaming/resume, quota exhaustion, deduplication isolation, exact verified round-trip |
| VT-09 | Parsers: exact baseline format matrix, malformed/encrypted/partial inputs, encoding ambiguity, truthful coverage and source locator mappings |
| VT-10 | Worker hostility: traversal, links, archive/decompression bombs, external entities, macros/scripts, huge media, fork/egress attempts, cancellation and descendant cleanup |
| VT-11 | Retrieval: lexical/optional vector relevance baselines, model-version drift, citation resolution, stale sources, partial coverage and byte/token budgets |
| VT-12 | Simulation: pinned branch repeatability, no main-history mutation, promotion conflicts, source deletion and permission revocation |
| VT-13 | Lifecycle: purge all reachable derivations, dedup references, holds, pinned readers, compaction interruption, old backup restore and key-copy limits |
| VT-14 | Operations: snapshot equivalence, scrub, backup inventory, restore-to-new-location, interrupted migration, unsupported-version refusal |
| VT-15 | Generic integration: complete derived and authoritative project-notebook scenarios from API contract |
| VT-16 | Release/soak: reproducible build checks, dependency inventory, long-run resource bounds, clean install/uninstall, upgrade and external assessment findings |
| VT-17 | Time: equivalent offsets, negative epochs, precision/ranges/units, DST folds/gaps, zone conflicts, unresolved dates, leap/time-scale refusal, pinned rule updates, equal times, clock rollback/restart and no future source/derivation leakage into earlier knowledge views |
| VT-18 | Spatial/frame/navigation: typed units, unknown positions, frame cycles/versioned transforms, poles/antimeridian, boundaries/degenerate geometry, nearest/radius reference matches, directed paths/ties/constraints/budgets |
| VT-19 | Movement/history: out-of-order and duplicate observations, conflicts/corrections, bounded interpolation, no future endpoints, state-at-time, index rebuild and trajectory crossing classification |
| VT-20 | Kinematics: independent analytic references, 2D/3D units/ranges, fixed-step profiles, tick-to-UTC mapping, profile mismatch, cancellation and deterministic checkpoint/resume |
| VT-21 | Contacts: constrained shapes/forces, overlap/simultaneous/wall cases, tunneling bounds, physical tolerances, deterministic ordering, atomic multi-body crash/replay and resource refusal |
| VT-22 | Unified retrieval: authorized reference scan equals composed graph/content/space/time plans, coherent snapshots, actual object/byte retrieval, unknown-binary coverage, stale handles and query-plan equivalence |
| VT-23 | Ingest and cross-feature privacy: batch preview/atomicity/resume/retry, mapping/source changes, location sensitivity, hidden nearer objects/routes, revocation/purge of spatial indexes/trajectories/branches and stale restore |

Run fuzzing against file framing/decoding, APIs, parser output validation, import/recovery,
query plans and worker protocols. Apply concurrency-model tools where suitable and run
real process-kill tests alongside deterministic fault simulation.
Sandbox failure and unauthorized data disclosure are correctness failures, not acceptable
performance tradeoffs.

## Offline application acceptance

The R2 consumer matrix also covers [application use cases](application-use-cases.md): a
headless game-world client and an item linked to synthetic asset-price observations loaded
from local CSV/JSON. Extend VT-15/20/22/23 with exact scaled amounts, declared quote/unit
identity, missing/stale/conflicting prices, late corrections and foreign-namespace denial.
Execute with outbound networking disabled and no provider credential variables, retaining
normal local encryption and authorization. No live service, token or market-data mock is
needed. These cases add no claim of a general game engine or supported trading throughput.

## Benchmark workloads

Decision 0007 freezes these exact workload manifests, durations, result-size bounds, numeric
budgets and the named runner. They remain targets, not measured limits.

| ID | Proposed workload | Required measurements |
|---|---|---|
| BM-01 | 100,000 entities / 1,000,000 relationships; uniform plus hub/cycle graphs; 1–4 hop bounded queries | p50/p95/p99 latency, visits, result bytes, RSS; cold and warm |
| BM-02 | Mixed ingestion, correction and readers; single updates and batches with durable receipts | committed transactions/events/bytes per second, latency, queue/backpressure |
| BM-03 | 10,000,000 historical events with late corrections and multiple valid-time distributions | as-of latency, index size, retained history, summary refresh cost |
| BM-04 | Mixed small files, many duplicates, one multi-GiB unknown binary, and interrupted uploads | throughput, peak RSS, disk amplification, cleanup and encrypted round-trip |
| BM-05 | Pinned PDF/office/image/archive/media corpus with hostile controls | parse p95, CPU/RSS, output ratio, coverage accuracy, citation accuracy, cancellation latency |
| BM-06 | Crash/open with journal tails, checkpoints, corrupt caches and retained baselines | recovery duration, required I/O, integrity outcomes, rebuilt-index cost |
| BM-07 | Compaction, purge, backup and restore while readers/ingest run | p99 impact, write amplification, free-space reserve, reclamation completion |
| BM-08 | Repeated hypothetical branches and reference state comparisons | deterministic equality, branch cost, retention pins, main-state invariance |
| BM-09 | Local inference contention as an optional external load; database remains independent | CPU/RAM/disk contention effects, retrieval p99, fairness and memory ceilings |
| BM-10 | 1,000,000 spatial items across local/geographic frames; uniform and clustered distributions | cold/warm radius/nearest/box latency, candidates, final accuracy, index bytes, RSS and rebuild |
| BM-11 | 10,000,000 position observations with gaps, duplicates, late corrections and region crossings | ingest rate, state-at-time/history p99, correction cost, disk amplification and retained-boundary behavior |
| BM-12 | 100, 1,000 and 10,000 kinematic/contact bodies; sparse and deliberately dense contacts | simulated steps/second, contact pairs, durable event throughput, budget refusal, checkpoint/restart/cancel latency |
| BM-13 | Unified world fixture with simultaneous ETL, movement updates, parsing, physics and compaction | per-class p50/p95/p99, fairness, sustained ingest, RSS, queue bounds and source/query correctness |

Decision 0026 pins `bm01-materialization-v1` in
`acceptance/r1/bm01-materialization-v1.tsv`: the exact accepted seed, 100k/1m topology split,
typed IDs, stable depth-one-through-four query roots, independent BFS ordering and global limits.
The standalone generator emits `engine_benchmark: false`; fixture dimensions and golden digests do
not satisfy BM-01 until the production encrypted/authorized/durable engine runs the required cold
and warm samples and reports latency, visits, result bytes, RSS and environment evidence.

Decision 0039 exposes cumulative authenticated page/fragment/result-byte work and explicit
decrypted-page-cache clearing on that production authorized path. These candidate-dependent
counters are cardinality-sensitive and require current `ManageSchema` authority plus an
issuer-instance-bound root. Benchmark samples use counter deltas and must label an empty USTE cache
separately from process, kernel, filesystem, controller and device cache state. The clear operation
alone is not evidence of a fully cold host.

Decision 0040 connects the fixture and oracle to production authorized encrypted disk reads at an
explicitly capped development scale. Its restart-backed 20/200 golden checks all 384 measured query
shapes, but the memory fault-model adapter, test key wrapper, absent timings/RSS and client-composed
multi-hop path make it semantic groundwork only. It always reports `engine_benchmark:false`.

Decision 0041 streams the exact fixture mapping in maximum-10,000-operation transactions and pins
the qualifying plan at 212 durable revisions. This removes profile-sized operation collection but
does not turn the capped development verifier into BM-01 timing or memory evidence.

Decision 0042 adds Linux/Btrfs `create`, `resume` and `open` phases with OS entropy, portable
recovery, fixed content-free reports and exact frontier/root admission checks. The report labels
host caches uncontrolled and full graph state memory-resident.

Decision 0043 adds a separately generated, bounded, content-free oracle summary and a Linux
correctness-query phase. The exact corpus pins 299 successful outputs, no visit-limit outcomes and
85 expected result-limit refusals. Each query starts with an empty USTE page cache and must match
the oracle's output or typed refusal, but host caches remain uncontrolled and the full graph is
memory-resident. Its one-pass aggregate timing includes expected refusals, is diagnostic only and
sets `engine_benchmark:false`; qualifying repeated cold/warm success samples remain required.

Decision 0044 supplies a real-process durable-prefix recovery probe. The 20/200 Btrfs plan resumes
after SIGKILL at each of its three incomplete frontiers, but this is recovery correctness only;
exact-scale recovery duration and BM-06's independent 10-million-event trials remain required.

Decision 0045 adds the bounded `bm01-oracle-bundle-v1`: 96 independently checked warm-up queries
with roots disjoint from the unchanged 384 measured corpus. Exact scale pins warm-up outcomes at
74 successful outputs and 22 expected result-limit refusals. The combined digest binds both
sections, but no timing occurs until the subsequent sampler phase.

Decision 0046 adds `linux-sample`. Exact scale has no lowering controls: one checked warm-up is
followed by five complete measured windows of at least 60 seconds. Each query is paired empty then
retained in USTE's cache; success and typed-refusal percentiles are separated by cache, depth and
graph class, with all-class depth aggregates. The runner post-checks returned queries against 30
seconds but cannot preempt a hung synchronous indexed read, so reports withhold budget evaluation
and disclose that deadline enforcement, host-cache control and the full-memory graph boundary are
unresolved. Oracle validation is outside the engine-call latency interval; successful work and
authenticated index/cache deltas are attributed separately to each USTE cache state.

Decision 0047 makes the documented CLI deadline preemptive without creating a process per query.
One worker emits flushed start/finish markers around engine calls; its parent kills and reaps it if
finish is absent after 30 seconds. The same worker retains USTE cache state across each pair. The
closed protocol and kill path are tested, while host-cache control, exact reserved-host evidence and
the full-memory graph boundary remain unresolved qualification inputs.

Decision 0048 makes proof-derived terminal-root publication independent of a complete postcommit
snapshot. Tests must compare its streamed canonical digest, family counts and reconstructed state
with the full-state oracle, reject exact-minus-bound failures without root visibility and preserve
the journal commit across restart. This narrows the write-path memory proof only; benchmark reports
must still disclose the full-memory live reducer, coordinator metadata, admission and recovery.

Measure with encryption, authentication and normal durability enabled. Run isolated controls
to explain costs, never advertise disabled-security throughput as the default.
Data exceeding RAM must be included before claiming bounded-memory scalability.
Report vector recall/quality alongside speed; faster incorrect or unauthorized answers fail.

## R0 budget registry

Decision 0007 records hard caps for transaction/request/file/object sizes, streaming buffers, namespace
quotas, graph expansion, reader lifetimes, query scratch, worker RSS/CPU/time/output, archive
depth/ratio/entries, media dimensions/duration, branches, and queues. It also records target
query/commit/recovery latencies, sustained rates, and acceptable background-work impact.

Choose targets against intended users and repeatable experiments. No implementer may lower
a failing budget silently. A change needs a versioned decision, rationale and rerun evidence.
Benchmarks on unpinned hardware are informational. Supported filesystem behavior must be
tested separately from timing variability.

## Release evidence

R1 requires VT-01 through VT-08 as applicable to the kernel and baseline BM-01/02/04/06.
Decision 0024 orders T-20 before T-19 solely because T-20 supplies BM-01/BM-06's disk-index and
bounded-cache prerequisites. All four benchmark targets still must genuinely pass before T-19;
a recorded failed or missing baseline is not acceptance.
R1 also requires VT-17 kernel normalization/clock/replay cases; R2 extends VT-17 to queries
and supported parsers, with explicit source and derivation availability cutoffs.
R2 adds VT-09 through VT-12 and VT-15 for its supported parser/API scope.
R1 adds VT-18/19 schema and VT-23 transaction subsets. R2 adds VT-18/19/20/22/23 at alpha
scope and baseline BM-10/11/12/13 measurements within its supported feature matrix.
R3 requires every suite VT-01 through VT-23 at beta scope, full baseline parser/physics/spatial
matrices and BM-01 through BM-13, including larger-than-RAM spatial history and mixed-load limits.
R4 requires reproducible release evidence, long-duration trials, independent security review
with release-blocking findings resolved, and exact supported-version/platform documentation.

The suite IDs are coverage groups, not a claim that one test per group is sufficient. Expand
them into positive/negative vectors and crash-boundary enumerations. Retain failures and
their regression fixtures. Never mark a task complete solely on a successful happy-path demo.
