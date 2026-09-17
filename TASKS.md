# USTE — Product development tasks

Database design draft 1.2 · 2026-09-16

**All implementation tasks are open.** This document is a plan, not evidence of a working
database. No release gate has passed. The current documentation update does not close R0.

## How to use this file

Each row is one work package. Split it into smaller implementation tasks as needed while
preserving its ID, requirement mapping, dependencies and completion evidence.
Status starts [ ]; change it to [x] only with a linked reviewed change and test report.
Record partial work below its row rather than claiming completion. A blocked task records
the exact decision or external prerequisite; do not bypass it.

Requirements are in [PRD](PRD.md). D-01 through D-09 are in
[architecture](architecture.md). VT/BM evidence groups are in
[verification](docs/verification-and-benchmarks.md).
Proposed output paths below do not currently exist and are not instructions to fabricate tests.
Every code task also inherits NFR-01, NFR-03, NFR-04 and NFR-05.
The [implementation plan](docs/implementation-plan.md) explains increments. IDs are stable;
appended spatial/physics tasks appear at their dependency gate rather than numeric order.

## R0 — Foundational decisions

| Status / ID | Work package and requirement | Depends on | Artifact to produce | Completion evidence |
|---|---|---|---|---|
| [x] T-01 | Close D-06: records, schema/version compatibility, constrained transaction preconditions, retry lifetime, time codec/ranges and pinned normalization profile; FR-01/02/05/06/26 | None | Data/time/compatibility ADR and literal state-transition vectors | Reviewed semantics cover conflicts, UTC, ambiguity, leap limits, idempotency and unknown versions |
| [x] T-02 | Close D-01: journal/root/index design and platform failure model; FR-02/03/16 | T-01 | Storage ADR, publication state machine, crash matrix | Torn commit metadata cannot silently erase acknowledged history under the stated model |
| [x] T-03 | Close D-02/D-04: encryption, keys, retention epochs, deletion and backups; FR-09/10/11/12 | T-01, T-02 | Security/privacy ADRs and key/retention lifecycle diagrams | Exact crypto dependencies, nonce strategy, trust boundaries and purge/restore rules reviewed |
| [x] T-04 | Close D-05: parser/decoder/model matrix and sandbox feasibility; FR-17/19/20/21/22 | T-01, T-03 | Versioned adapter registry and dependency/license inventory | Every baseline family has exact candidate, limits, profile and acceptance fixtures; unresolved native dependency is explicit |
| [x] T-05 | Close D-03: target workloads, budgets and runner; NFR-02 | T-01, T-02, T-04, T-46, T-47 | Versioned synthetic fixture/benchmark manifests | Numeric limits include space, motion, physics and mixed load; hardware/corpus recorded; no unmeasured speed claim |
| [ ] T-06 | Close D-07: maintainership, provenance and disclosure readiness; NFR-05 | None | Contribution/release policy and verified private reporting instructions | Maintainer-controlled reporting route tested; no fabricated address/SLA |
| [x] T-46 | Close D-08: frames, geometry, units, spatial indexes, trajectory interpretation and navigation; FR-27/28/29/30/33 | T-01, T-03 | Spatial ADR, supported matrix, reference predicates and literal boundary vectors | Rust-only algorithmic dependency feasibility, tolerances, antimeridian/poles, transforms and path limits reviewed |
| [x] T-47 | Close D-09: kinematics/contact scope, arithmetic, replay, UTC mapping and Rust dependency feasibility; FR-31/32 | T-01, T-03, T-46 | Physics ADR, numerical profile, analytic/contact fixtures | Exact baseline implementable; deterministic ordering, overflow, collision limits and profile compatibility defined |
| [ ] T-07 | Review R0 contracts together | T-01, T-02, T-03, T-04, T-05, T-06, T-46, T-47 | R0 decision/evidence record | No conflicting authority/durability/deletion/parser/spatial/physics contracts; all D decisions resolved for initial profile |

### R0 implementation progress (2026-09-16)

- T-01–T-05 and T-46–T-47 are closed at their R0 decision/evidence scope. This does not mark
  their later implementation requirements complete: unsafe review continues, T-23 owns the
  production worker supervisor, and benchmark targets are explicitly unmeasured.
- T-06/D-07 is blocked on repository-owner enablement and a harmless end-to-end test of GitHub
  private vulnerability reporting. The local GitHub CLI credential is invalid; no repository
  security setting was changed. Decision 0008 records the selected policy and exact unblock.
- The cross-decision review found and corrected BM-04's blob-size/cap mismatch. T-07 and every
  dependent R1 task remain open solely because T-06 is unresolved. Pre-gate code is limited to
  design experiments and must not be described as a production disk format or alpha release.

## R1 — Correctness kernel

| Status / ID | Work package and requirement | Depends on | Artifact to produce | Completion evidence |
|---|---|---|---|---|
| [ ] T-08 | Scaffold Rust workspace and quality automation; NFR-01/03/05 | T-07 | Workspace, lockfile, CI, doc/reference checks | Reproducible local setup; checks run against real files; unsafe/native exceptions inventoried |
| [ ] T-09 | Bounded types, identities and canonical serialization; FR-01/03 | T-08 | uste-types with golden encodings | VT-01 and encoding fuzz reject overflow, unknown versions, malformed lengths and ambiguous data |
| [ ] T-10 | Independent in-memory reference model; FR-01/04/05/06 | T-09 | uste-testkit model and generated operation histories | VT-01 exposes both valid and invalid transitions independently of storage code |
| [ ] T-11 | Encryption/key-adapter boundary; FR-10 | T-09 | Reviewed crypto envelopes and key interfaces | VT-07 covers wrong context/key, tampering, nonce lifecycle, lock/unlock and redaction |
| [ ] T-12 | Filesystem/clock/random adapters and fault harness; FR-03, NFR-03 | T-09 | Narrow I/O interfaces and deterministic failure injection | Reproducible short-write, fsync, rename, disk-full and process-restart scenarios |
| [ ] T-13 | Journal, creation, ownership, commit roots and recovery; FR-03 | T-10, T-11, T-12 | uste-storage journal/recovery | VT-03/04 pass every initial publication boundary and hard-corruption case |
| [ ] T-14 | Commit coordinator, readers, conflict checks, idempotency; FR-02 | T-13 | uste-txn and durable outcome lookup | VT-02 includes lost response, competing mutations, cancellation and restart |
| [ ] T-15 | Streaming encrypted blob store and artifact publication; FR-17/18 | T-11, T-14 | Staging/finalization/resume/cleanup and inventory | VT-08 plus blob-specific VT-03: exact unknown-binary round-trip, bounded RSS, no committed dangling object |
| [ ] T-16 | Principal/namespace authorization and quotas; FR-09, NFR-04 | T-14, T-15 | uste-policy and trusted adapter checks | VT-06 proves no cross-scope graph/blob/existence leakage; quotas enforced on actual bytes |
| [ ] T-17 | Graph records, evidence and transactional adjacency; FR-01/04/06 | T-10, T-14, T-16 | uste-graph basic records and traversal | VT-05 agrees with reference through create/correct/delete/conflict and restart |
| [ ] T-18 | Deterministic replay and verified cache snapshots; FR-03/07 | T-13, T-17 | uste-replay, checkpoint and scrub prototypes | VT-04/14 show exact logical equivalence, no model/parser/network dependency |
| [ ] T-45 | Shared UTC/time types, normalization, source envelopes and replay integration; FR-26 | T-09, T-12, T-18 | Versioned time codec, pinned local timezone profile and golden vectors | VT-17 kernel cases pass: no silent guessing, clock rollback does not reorder commits, replay preserves accepted interpretation |
| [ ] T-48 | World/frame/geometry/observation schemas and typed units; FR-27/28 | T-09, T-45 | uste-types spatial records and reference histories | VT-18/19 schema subset rejects invalid units, frame cycles, nonfinite values and invented missing positions |
| [ ] T-49 | Bounded import transaction contracts and durable checkpoints; FR-34 | T-14, T-15, T-48 | Mapping types, batch IDs, checkpoint/outcome records | VT-23 transaction subset proves atomic batches, retry identity, changed-source refusal and no dangling references |
| [ ] T-19 | R1 acceptance and operating limitations | T-15, T-16, T-17, T-18, T-45, T-48, T-49 | R1 evidence report and runnable kernel instructions | Required R1 VT/BM scope passes; clear non-production and scalability limits |

## R2 — Developer alpha

| Status / ID | Work package and requirement | Depends on | Artifact to produce | Completion evidence |
|---|---|---|---|---|
| [ ] T-20 | Disk-index foundation and bounded cache; FR-16 | T-19 | Versioned graph/index runs and root publication | VT-05/14 and BM-01/06 verify rebuild, visibility and cache pressure; no full-RAM assumption hidden |
| [ ] T-21 | Bitemporal queries, corrections and contradictions; FR-05/26 | T-20, T-45 | Temporal indexes and typed query plans | VT-01/05/17 distinguish valid time from recorded revision, including late facts, derivation availability and retained-boundary errors |
| [ ] T-22 | Artifact/evidence/derivation graph and locators; FR-06/18/20 | T-20 | Version lineage, dependencies, chunk and citation schemas | Exact source-version resolution; no invented locators; source replacement is distinguishable |
| [ ] T-23 | Worker supervisor, leases, protocol and isolation; FR-19/22 | T-16, T-22 | Restricted process worker framework | VT-10 proves timeout/cancel/descendant cleanup/no-egress/no-host-file access; revoked output rejected |
| [ ] T-24 | Initial text/structured format adapters; FR-19/20/21/22/26 | T-23 | R2 adapters and coverage registry | VT-09/10/17 for text, Markdown, source/logs, JSON, CSV/TSV, XML/HTML; preserve timestamp provenance/ambiguity; no external entities/scripts |
| [ ] T-25 | Lexical retrieval, source reads and explanations; FR-13/20 | T-21, T-24 | uste-query chunk search/range reads/citation API | VT-06/11 with bounded output, partial coverage, authorization-before-expansion and stale results |
| [ ] T-26 | Deterministic branch kernel and dependency-delay model; FR-08/24 | T-18, T-21, T-22 | uste-sim, branch compare and promotion proposals | VT-12 proves repeatability, isolation, conflict checks and no ambient effects |
| [ ] T-27 | Rust API, operator CLI and authenticated local IPC; FR-14 | T-25, T-26 | uste-api/cli/service and error reference | Tests cover structured errors, OutcomeUnknown, streaming, IPC identity and locked state |
| [ ] T-28 | Generic derived/authoritative consumer adapters; FR-15 | T-27 | Project-notebook fixtures and migration/cutover contract | VT-15 passes import/approve/restart/correct/cite/branch paths with foreign namespace denied |
| [ ] T-50 | Local/geographic frame transforms and geometry reference implementation; FR-27 | T-19 | uste-spatial types, transforms and geometry predicates | VT-18 verifies axis/units, frames, transforms, antimeridian/poles and declared accuracy |
| [ ] T-51 | Observation histories, explicit estimates and state-at-time; FR-28 | T-20, T-21, T-50 | uste-motion with source/correction dependencies | VT-19 proves late/conflicting observations, bounded gaps, no future-data interpolation and unchanged identity |
| [ ] T-52 | Bounded point/box/radius/nearest queries and candidate planning; FR-29 | T-51 | Authorized spatial query operators | VT-18/19 match reference scan under limits; BM-10 preliminary results are labeled, not release capacity |
| [ ] T-53 | Unified graph/content/space/time query plans and object/source projections; FR-33 | T-25, T-52 | Typed query composition, read handles and explanations | VT-22 proves coherent revision, source-byte retrieval, exact predicates, partial coverage and stale-handle denial |
| [ ] T-54 | CSV/JSON ETL mapping, preview, streaming batches and resume CLI; FR-34 | T-24, T-49 | uste-ingest tooling and synthetic import fixtures | VT-23 covers bad rows, units/time/frame errors, changed input, quotas, cancellation, lost responses and restart |
| [ ] T-55 | Supplied-graph navigation with constraints and stable path ordering; FR-30 | T-52, T-53 | Bounded path operator and route provenance | VT-18/22 checks directed/disconnected graphs, costs, hidden routes, equal-cost ties and resource exhaustion |
| [ ] T-57 | Deterministic 2D/3D kinematics and branch checkpoint/resume; FR-31 | T-26, T-50, T-51 | uste-physics baseline and virtual-time mappings | VT-20 matches independent analytic cases, pins profile and preserves observed/simulated separation after restart |
| [ ] T-56 | Synthetic world end-to-end CLI/read fixture and consumer example; FR-27/28/29/30/31/33/34 | T-27, T-53, T-54, T-55, T-57 | Moving objects, attached documents, query/navigation/branch example | VT-15/17/18/19/20/22/23 R2 scope passes with UTC/local display, permissions, corrections and byte retrieval |
| [ ] T-29 | R2 acceptance and developer documentation | T-20, T-21, T-24, T-25, T-26, T-27, T-28, T-56 | Alpha evidence and examples | R2 suites pass; unsupported formats explicitly visible; no R3 capabilities implied |

## R3 — Hardened beta

| Status / ID | Work package and requirement | Depends on | Artifact to produce | Completion evidence |
|---|---|---|---|---|
| [ ] T-30 | PDF parsing and page-aware extraction; FR-19/20/21/22 | T-29 | Isolated PDF adapter and scanned/encrypted fixtures | VT-09/10: exact pages, partial/password states, bounded resources, OCR requests separately governed |
| [ ] T-31 | Office and archive adapters; FR-19/20/21/22 | T-29 | OOXML and ZIP/TAR workers | VT-09/10: cell/slide/member citations, no macro/formula execution, traversal/bomb/link denial |
| [ ] T-32 | Images, OCR, audio/video and local transcription; FR-19/20/21/24 | T-29 | D-05-pinned media/model workers and weights manifests | VT-09/10/11: tested baseline formats, truthful time/region locators, model identity, no download/cloud fallback |
| [ ] T-33 | Optional embedding and vector/hybrid retrieval; FR-13/24 | T-25, T-29 | Versioned embedding jobs/indexes and relevance baseline | VT-11 measures relevance/recall and speed; dimension drift, permissions and stale-source invalidation tested |
| [ ] T-34 | Retention/expiry/purge and derivation invalidation; FR-11/23 | T-22, T-26, T-29 | Deletion planner, receipts, lease/pin invalidation and minimal tombstones | VT-06/13 across originals, chunks, worker scratch, summaries, embeddings and branches |
| [ ] T-35 | Atomic compaction, baselines and blob reclamation; FR-03/11/16/18 | T-20, T-34 | Versioned compaction/root switch and safe garbage collector | VT-03/13/14: old/new complete roots, no committed blob loss, bounded pins and explicit history cutoff |
| [ ] T-36 | Encrypted backup, restore and migration; FR-12 | T-35 | Inventory verification, restore-to-new-path and migration tools | VT-13/14 include stale deletion epoch, wrong keys, interrupted upgrade and rollback limits |
| [ ] T-37 | Time partitioning and correction-aware summaries; FR-16 | T-21, T-35 | Partition/index management and summary invalidation | VT-05/11 with late corrections; BM-03/07 meet declared budgets and retained-state equivalence |
| [ ] T-38 | Resumable bounded change subscriptions; FR-25 | T-27, T-34 | Cursor/lease/backpressure implementation | Duplicate delivery is deduplicable; retention gaps explicit; revocation prevents buffered leaks |
| [ ] T-39 | Full adversarial and mixed-load campaign; NFR-02/03/04 | T-30, T-31, T-32, T-33, T-34, T-35, T-36, T-37, T-38 | Pinned VT-01…17 and BM-01…09 results | Real kill tests, fuzz regressions, larger-than-RAM workload and long-run bounded resource use |
| [ ] T-58 | Constrained Rust 2D contact physics and atomic multi-body replay; FR-32 | T-29, T-57 | Admitted disc/boundary collision model, checkpoints and crash fixtures | VT-21 independent contacts/conservation-tolerance checks, ordering, budget limits and atomic crash outcomes pass |
| [ ] T-59 | Native disk spatial indexes, polygons/regions and trajectory crossings; FR-29 | T-37, T-52 | Correction-aware encrypted spatial/history indexes | VT-18/19/22 match reference; BM-10/11 include larger-than-RAM, degenerate geometry, late corrections and rebuild |
| [ ] T-60 | Spatial/physics/content lifecycle and privacy hardening; FR-09/11/23/28/29/32/33 | T-34, T-35, T-58, T-59 | Dependency invalidation, spatial threat fixtures and purge receipts | VT-23 denies hidden proximity/routes, stale source handles, revoked branches and restored deleted locations |
| [ ] T-61 | Full world-model mixed-load/fault campaign; NFR-02/03/04 | T-39, T-56, T-58, T-59, T-60 | VT-01…23 and BM-01…13 pinned reports | No correctness/privacy regressions under movement ingest, parsing, physics, compaction and bounded query load |
| [ ] T-40 | R3 acceptance and complete integration lifecycle | T-39, T-61 | Beta evidence and updated generic integration fixtures | Purge/restart/rebuild/stale-restore paths pass; entire R3 content/spatial/physics matrix documented and tested |

## R4 — Production candidate

| Status / ID | Work package and requirement | Depends on | Artifact to produce | Completion evidence |
|---|---|---|---|---|
| [ ] T-41 | Independent security/recovery review and remediation; NFR-01/03/05 | T-40 | Reviewer scope, findings and regression evidence | External assessment is real; release-blocking findings resolved or release held |
| [ ] T-42 | Packaging, signing, provenance and supported platform trials; FR-14, NFR-05 | T-40 | Release pipeline, SBOM/notices, install/upgrade/uninstall guides | VT-16 repeats clean installation and recovery on each claimed platform/profile |
| [ ] T-43 | Operator/support documentation and commercial-distribution review; FR-12/15, NFR-05 | T-41, T-42 | Support matrix, security policy, limits, parser/model licenses and integration guide | Claims match evidence; exact shipped dependencies and model terms reviewed; no unsupported certification |
| [ ] T-44 | R4 release decision | T-41, T-42, T-43 | Versioned release checklist and evidence manifest | Every required task/requirement traced; all release gates pass; no hidden blocked work |

## Completion evidence template

For each task, record:

- Task and requirement IDs; exact artifact paths and reviewed commit.
- Acceptance fixture/version and tested build/features.
- Commands actually run and results, including failures and limitations.
- Performance evidence where relevant, with hardware and security configuration.
- Dependency/license changes, migration impact and residual security risks.
- Reviewer identity/type: automated checks are not independent human assessment.

## Deferred expansion — not hidden R4 work

Distributed replication/consensus, remote multitenancy, full SQL/Cypher/AQL compatibility,
GPU graph analytics, spatial/physics models beyond the declared baseline, arbitrary plugins, universal format
understanding and model reasoning, and additional operating systems require separate scope
decisions. This does not defer the baseline rich-content matrix required by R3.
