# USTE — Product development tasks

Database design draft 1.4 · 2026-09-17

**R0 design readiness is complete; implementation tasks remain open.** This document is not
evidence of a working database. Passing a local gate does not authorize executable distribution.

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

Application scope includes future game-world consumers and item tracking linked to imported
asset-price records, alongside agent memory. T-28/T-54/T-56 own the offline examples in
[application use cases](docs/application-use-cases.md). No native price-feed API, external
account, provider password/token or trading implementation is required by these tasks.
This clarification preserves task IDs, dependencies and completed R0 evidence.

## R0 — Foundational decisions

| Status / ID | Work package and requirement | Depends on | Artifact to produce | Completion evidence |
|---|---|---|---|---|
| [x] T-01 | Close D-06: records, schema/version compatibility, constrained transaction preconditions, retry lifetime, time codec/ranges and pinned normalization profile; FR-01/02/05/06/26 | None | Data/time/compatibility ADR and literal state-transition vectors | Reviewed semantics cover conflicts, UTC, ambiguity, leap limits, idempotency and unknown versions |
| [x] T-02 | Close D-01: journal/root/index design and platform failure model; FR-02/03/16 | T-01 | Storage ADR, publication state machine, crash matrix | Torn commit metadata cannot silently erase acknowledged history under the stated model |
| [x] T-03 | Close D-02/D-04: encryption, keys, retention epochs, deletion and backups; FR-09/10/11/12 | T-01, T-02 | Security/privacy ADRs and key/retention lifecycle diagrams | Exact crypto dependencies, nonce strategy, trust boundaries and purge/restore rules reviewed |
| [x] T-04 | Close D-05: parser/decoder/model matrix and sandbox feasibility; FR-17/19/20/21/22 | T-01, T-03 | Versioned adapter registry and dependency/license inventory | Every baseline family has exact candidate, limits, profile and acceptance fixtures; unresolved native dependency is explicit |
| [x] T-05 | Close D-03: target workloads, budgets and runner; NFR-02 | T-01, T-02, T-04, T-46, T-47 | Versioned synthetic fixture/benchmark manifests | Numeric limits include space, motion, physics and mixed load; hardware/corpus recorded; no unmeasured speed claim |
| [x] T-06 | Close development-governance portion of D-07: maintainership, provenance, dependency/release policy and selected disclosure procedure; NFR-05 | None | Contribution/release/security policy and Decisions 0008/0011 | Roles, admission/signing/support policy and honest unverified-channel status reviewed; no fabricated address/SLA |
| [x] T-46 | Close D-08: frames, geometry, units, spatial indexes, trajectory interpretation and navigation; FR-27/28/29/30/33 | T-01, T-03 | Spatial ADR, supported matrix, reference predicates and literal boundary vectors | Rust-only algorithmic dependency feasibility, tolerances, antimeridian/poles, transforms and path limits reviewed |
| [x] T-47 | Close D-09: kinematics/contact scope, arithmetic, replay, UTC mapping and Rust dependency feasibility; FR-31/32 | T-01, T-03, T-46 | Physics ADR, numerical profile, analytic/contact fixtures | Exact baseline implementable; deterministic ordering, overflow, collision limits and profile compatibility defined |
| [x] T-07 | Review R0 contracts together | T-01, T-02, T-03, T-04, T-05, T-06, T-46, T-47 | R0 decision/evidence record | No conflicting authority/durability/deletion/parser/spatial/physics contracts; all D decisions resolved for initial profile |

### R0 decision progress (2026-09-17)

- T-01–T-05 and T-46–T-47 are closed at their R0 decision/evidence scope. This does not mark
  their later implementation requirements complete: unsafe review continues, T-23 owns the
  production worker supervisor, and benchmark targets are explicitly unmeasured.
- T-06/D-07's development-governance scope is closed by Decisions 0008/0011 and the reviewed
  contribution, dependency, signing, support and disclosure policies. Operational GitHub private
  vulnerability reporting remains unverified under open distribution-only task T-62.
- The cross-decision review found and corrected BM-04's blob-size/cap mismatch. T-07 and every
  prerequisite are now closed. T-08 is unblocked. R0 evidence remains design evidence and must
  not be described as a working database, production disk format or distributable alpha.

## R1 — Correctness kernel

| Status / ID | Work package and requirement | Depends on | Artifact to produce | Completion evidence |
|---|---|---|---|---|
| [x] T-08 | Scaffold Rust workspace and quality automation; NFR-01/03/05 | T-07 | Workspace, lockfile, CI, doc/reference checks | Reproducible local setup; checks run against real files; unsafe/native exceptions inventoried |
| [x] T-09 | Bounded types, identities and canonical serialization; FR-01/03 | T-08 | [uste-types](crates/uste-types) with [golden encodings and evidence](docs/evidence/canonical-types.md) | VT-01 and encoding fuzz reject overflow, unknown versions, malformed lengths and ambiguous data |
| [x] T-10 | Independent in-memory reference model; FR-01/04/05/06 | T-09 | [uste-testkit model and generated histories](crates/uste-testkit), with [evidence](docs/evidence/reference-model.md) | VT-01 exposes both valid and invalid transitions independently of storage code |
| [x] T-11 | Encryption/key-adapter boundary; FR-10 | T-09 | [Reviewed crypto envelopes and key interfaces](docs/evidence/crypto-boundary.md) | T-11 slice of VT-07 covers wrong context/key, tampering, nonce lifecycle, lock/unlock and redaction; rotation/clone/restore remain later tasks |
| [x] T-12 | Filesystem/clock/random adapters and fault harness; FR-03, NFR-03 | T-09 | [Narrow I/O interfaces and deterministic failure injection](docs/evidence/io-fault-harness.md) | Reproducible short-write/read, file/directory-sync, rename, disk-full and SIGKILL/reopen scenarios; production Linux adapter remains T-13 |
| [x] T-13 | Journal, creation, ownership, commit roots and recovery; FR-03 | T-10, T-11, T-12 | [uste-storage journal/recovery evidence](docs/evidence/journal-foundation.md) | VT-03/04 pass every initial publication boundary and hard-corruption case |
| [x] T-14 | Commit coordinator, readers, conflict checks, idempotency; FR-02 | T-13 | [uste-txn coordinator and qualification](docs/evidence/transaction-coordinator-foundation.md) with durable outcome lookup | T-14 slice of VT-02 passes lost response, competing mutations, cancellation and restart; graph phantom predicates remain T-17 |
| [x] T-15 | Streaming encrypted blob store and artifact publication; FR-17/18 | T-11, T-14 | [Encrypted blob-store qualification](docs/evidence/blob-store-foundation.md): staging/finalization/resume/cleanup and inventory | T-15 slice of VT-08 plus blob-specific VT-03 pass: exact unknown-binary round-trip, 12 GiB at 267,636 KiB peak RSS, no committed dangling object; BM-04 throughput target remains unmet |
| [x] T-16 | Principal/namespace authorization and quotas; FR-09, NFR-04 | T-14, T-15 | [`uste-policy`](crates/uste-policy) and [authorization foundation evidence](docs/evidence/authorization-foundation.md) | VT-06 namespace/blob/outcome slice proves default denial, no cross-scope/existence leakage and exact-byte quotas; T-17 repeats through real graph paths |
| [x] T-17 | Graph records, evidence and transactional adjacency; FR-01/04/06 | T-10, T-14, T-16 | [`uste-graph`](crates/uste-graph) and [transactional graph evidence](docs/evidence/transactional-evidence-graph.md) | T-17 adjacency/reference/rebuild slice of VT-05 and encrypted restart cover create/correct/delete/conflict; VT-06 graph concealment passes |
| [x] T-18 | Deterministic replay and verified cache snapshots; FR-03/07 | T-13, T-17 | [`uste-replay`, encrypted checkpoints and evidence](docs/evidence/replay-checkpoints.md) | VT-04/14 show cold/checkpoint logical equivalence, exact journal anchors, suffix recovery and fail-closed fallback without model/parser/network dependencies |
| [x] T-45 | Shared UTC/time types, normalization, source envelopes and replay integration; FR-26 | T-09, T-12, T-18 | [`uste-time`, Decision 0021 and evidence](docs/evidence/time-normalization.md) | VT-17 kernel cases pass: no silent guessing, clock rollback does not reorder commits, replay preserves accepted interpretation |
| [x] T-48 | World/frame/geometry/observation schemas and typed units; FR-27/28 | T-09, T-45 | [`uste-types` primitives, `uste-spatial` records/history and evidence](docs/evidence/spatial-schema-history.md) | VT-18/19 schema subset rejects invalid units, frame cycles, nonfinite values and invented missing positions |
| [x] T-49 | Bounded import transaction contracts and durable checkpoints; FR-34 | T-14, T-15, T-16, T-17, T-18, T-48 | [`uste-ingest`, Decision 0023 and evidence](docs/evidence/atomic-import-transactions.md) | VT-23 transaction subset proves atomic batches, retry identity, changed-source refusal and no dangling references |
| [ ] T-19 | R1 acceptance and operating limitations | T-15, T-16, T-17, T-18, T-20, T-45, T-48, T-49 | R1 evidence report and runnable kernel instructions | Required R1 VT/BM scope passes; clear non-production and scalability limits |

T-13 progress: Decision 0015, the Linux capability adapter, exclusive ownership, encrypted
manifest/log/segment headers, opaque transaction groups, fixed commit certificates and streaming
recovery are implemented. Deterministic every-operation creation/commit/rollover crash tests,
byte-exhaustive corruption, real portable-key wiring, injected error/short-progress matrices and
Btrfs SIGKILL after group/certificate sync pass. Live cross-process exclusion and lock release on
death also pass. Real-process creation outcomes pass on Btrfs and the separately verified ext4
mount; T-13's local acceptance scope is complete without claiming power-cut behavior.

T-14 completion: Decision 0016, the domain-neutral prepare/publish reducer boundary,
encrypted `UTXN` publication, coherent owned-snapshot readers, durable retry/transaction outcome indexes,
expiry and uncertain-handle quarantine are implemented. The literal format golden, complete initial
publication fault matrix, both eligible cancellation boundaries, malformed authenticated recovery,
32-caller stale-mutation race, retry and restart cases pass.

T-15 completion: Decision 0017 adds 1 MiB encrypted blob chunks, temporary-to-canonical resumable
staging with authenticated progress witnesses, immutable finalization, paired durable marker-first
abort, opaque inventory names, commit-gated range reads and canonical certificate-bound inventories.
Complete modeled publication fault, short-I/O, corruption/context-replay, retry/restart and storage-
profile hard-limit cases pass. A release-built, normally encrypted and durable 12 GiB commit/reopen/
full-hash probe used 267,636 KiB peak RSS; its 95.923 MiB/s ingest misses BM-04's 250 MiB/s target.

T-18 completion: Decision 0020 adds contiguous cold replay, canonical graph/coordinator checkpoint
codecs, exact historical certificate anchoring and an encrypted two-slot cache. Seeded open
reauthenticates the journal, compares complete prefix retry/transaction/blob-owner metadata and
applies only the reducer suffix. The 28-case publication crash matrix, 21-case authenticated
malformed-carrier matrix, older-slot fallback and encrypted graph restart equivalence pass. The
journal is currently opened twice and checkpoints remain capped in-memory caches; BM-06 and disk
projections remain T-20 work rather than implied performance results.

T-45 completion: Decision 0021 adds strict explicit timestamp normalization, checked full-range
Gregorian arithmetic, hash-verified embedded TZDB 2026c local rules and bounded canonical source
envelopes. Graph cold replay restores the accepted pair without parsing or zone resolution; equal
and rolling-back wall samples remain ordered only by distinct recovered commit revisions. The 12
R0 time vectors, envelope malformed matrix and all supported Gregorian days pass. Temporal indexes
and content-adapter timestamp extraction remain T-21/T-24 rather than implied completion.

## R2 — Developer alpha

| Status / ID | Work package and requirement | Depends on | Artifact to produce | Completion evidence |
|---|---|---|---|---|
| [ ] T-20 | Disk-index foundation and bounded cache; FR-16 | T-17, T-18, T-49 | [Versioned graph/index runs, authorized cache/I/O diagnostics and capped production/oracle equivalence](docs/evidence/disk-index-foundation.md), [terminal root deltas](docs/evidence/graph-state-root-deltas.md), [bounded composite preparation](docs/evidence/bounded-composite-preparation.md), [explicit-I/O preparation](docs/evidence/explicit-io-graph-preparation.md), [complete proof buckets](docs/evidence/complete-graph-preparation-proofs.md), [proof-derived root deltas](docs/evidence/proof-derived-root-deltas.md), [proof-prepared commit](docs/evidence/proof-prepared-graph-commit.md) and root publication | VT-05/14 and BM-01/06 verify rebuild, visibility and cache pressure; no full-RAM assumption hidden |
| [ ] T-21 | Bitemporal queries, corrections and contradictions; FR-05/26 | T-20, T-45 | Temporal indexes and typed query plans | VT-01/05/17 distinguish valid time from recorded revision, including late facts, derivation availability and retained-boundary errors |
| [ ] T-22 | Artifact/evidence/derivation graph and locators; FR-06/18/20 | T-20 | Version lineage, dependencies, chunk and citation schemas | Exact source-version resolution; no invented locators; source replacement is distinguishable |
| [ ] T-23 | Worker supervisor, leases, protocol and isolation; FR-19/22 | T-16, T-22 | Restricted process worker framework | VT-10 proves timeout/cancel/descendant cleanup/no-egress/no-host-file access; revoked output rejected |
| [ ] T-24 | Initial text/structured format adapters; FR-19/20/21/22/26 | T-23 | R2 adapters and coverage registry | VT-09/10/17 for text, Markdown, source/logs, JSON, CSV/TSV, XML/HTML; preserve timestamp provenance/ambiguity; no external entities/scripts |
| [ ] T-25 | Lexical retrieval, source reads and explanations; FR-13/20 | T-21, T-24 | uste-query chunk search/range reads/citation API | VT-06/11 with bounded output, partial coverage, authorization-before-expansion and stale results |
| [ ] T-26 | Deterministic branch kernel and dependency-delay model; FR-08/24 | T-18, T-21, T-22 | uste-sim, branch compare and promotion proposals | VT-12 proves repeatability, isolation, conflict checks and no ambient effects |
| [ ] T-27 | Rust API, operator CLI and authenticated local IPC; FR-14 | T-25, T-26 | uste-api/cli/service and error reference | Tests cover structured errors, OutcomeUnknown, streaming, IPC identity and locked state |
| [ ] T-28 | Generic derived/authoritative consumer adapters; FR-15 | T-27 | Project-notebook, headless-world and item/price mappings plus migration/cutover contract | VT-15 passes import/approve/restart/correct/cite/branch paths with foreign namespace denied; local examples need no provider credentials |
| [ ] T-50 | Local/geographic frame transforms and geometry reference implementation; FR-27 | T-19 | uste-spatial types, transforms and geometry predicates | VT-18 verifies axis/units, frames, transforms, antimeridian/poles and declared accuracy |
| [ ] T-51 | Observation histories, explicit estimates and state-at-time; FR-28 | T-20, T-21, T-50 | uste-motion with source/correction dependencies | VT-19 proves late/conflicting observations, bounded gaps, no future-data interpolation and unchanged identity |
| [ ] T-52 | Bounded point/box/radius/nearest queries and candidate planning; FR-29 | T-51 | Authorized spatial query operators | VT-18/19 match reference scan under limits; BM-10 preliminary results are labeled, not release capacity |
| [ ] T-53 | Unified graph/content/space/time query plans and object/source projections; FR-33 | T-25, T-52 | Typed query composition, read handles and explanations | VT-22 proves coherent revision, source-byte retrieval, exact predicates, partial coverage and stale-handle denial |
| [ ] T-54 | CSV/JSON ETL mapping, preview, streaming batches and resume CLI; FR-34 | T-24, T-49 | uste-ingest tooling and synthetic imports including item-linked price records | VT-23 covers bad rows, exact amounts/quote units, time/frame errors, changed input, quotas, cancellation, retry and restart; no provider calls |
| [ ] T-55 | Supplied-graph navigation with constraints and stable path ordering; FR-30 | T-52, T-53 | Bounded path operator and route provenance | VT-18/22 checks directed/disconnected graphs, costs, hidden routes, equal-cost ties and resource exhaustion |
| [ ] T-57 | Deterministic 2D/3D kinematics and branch checkpoint/resume; FR-31 | T-26, T-50, T-51 | uste-physics baseline and virtual-time mappings | VT-20 matches independent analytic cases, pins profile and preserves observed/simulated separation after restart |
| [ ] T-56 | Synthetic world end-to-end CLI/read fixture and consumer examples; FR-27/28/29/30/31/33/34 | T-27, T-53, T-54, T-55, T-57 | Headless game-world state and tracked items linked to locally imported prices/documents | VT-15/17/18/19/20/22/23 R2 scope passes offline: UTC/local display, permissions, corrections, price provenance and byte retrieval; no provider accounts/secrets |
| [ ] T-29 | R2 local developer-alpha acceptance and documentation | T-19, T-20, T-21, T-24, T-25, T-26, T-27, T-28, T-56 | Local alpha evidence and examples | R2 suites pass; unsupported formats explicitly visible; no R3 capabilities or distribution readiness implied |

## R3 — Hardened beta

| Status / ID | Work package and requirement | Depends on | Artifact to produce | Completion evidence |
|---|---|---|---|---|
| [ ] T-30 | PDF parsing and page-aware extraction; FR-19/20/21/22 | T-29 | Isolated PDF adapter and scanned/encrypted fixtures | VT-09/10: exact pages, partial/password states, bounded resources, OCR requests separately governed |
| [ ] T-31 | Office and archive adapters; FR-19/20/21/22 | T-29 | OOXML and ZIP/TAR workers | VT-09/10: cell/slide/member citations, no macro/formula execution, traversal/bomb/link denial |
| [ ] T-32 | Images, OCR, audio/video and local transcription; FR-19/20/21/24 | T-29 | D-05-pinned media/model workers and weights manifests | VT-09/10/11: tested baseline formats, truthful time/region locators, model identity, no download/cloud fallback |
| [ ] T-33 | Optional embedding and vector/hybrid retrieval; FR-13/24 | T-25, T-29 | Versioned embedding jobs/indexes and relevance baseline | VT-11 measures relevance/recall and speed; dimension drift, permissions and stale-source invalidation tested |
| [ ] T-34 | Retention/expiry/purge and derivation invalidation; FR-11/23 | T-22, T-26, T-29 | Deletion planner, receipts, lease/pin invalidation and minimal tombstones | VT-06/13 across originals, chunks, worker scratch, summaries, embeddings and branches |
| [ ] T-35 | Atomic compaction, baselines, certificate-log rollover and storage/blob orphan reclamation; FR-03/11/16/18 | T-20, T-34 | Versioned compaction/root switch, bounded log maintenance and safe garbage collector | VT-03/13/14: old/new complete roots, no committed blob loss, bounded pins/garbage and explicit history cutoff |
| [ ] T-36 | Encrypted backup, restore and migration; FR-12 | T-35 | Inventory verification, restore-to-new-path and migration tools | VT-13/14 include stale deletion epoch, wrong keys, interrupted upgrade and rollback limits |
| [ ] T-37 | Time partitioning and correction-aware summaries; FR-16 | T-21, T-35 | Partition/index management and summary invalidation | VT-05/11 with late corrections; BM-03/07 meet declared budgets and retained-state equivalence |
| [ ] T-38 | Resumable bounded change subscriptions; FR-25 | T-27, T-34 | Cursor/lease/backpressure implementation | Duplicate delivery is deduplicable; retention gaps explicit; revocation prevents buffered leaks |
| [ ] T-39 | Full adversarial and mixed-load campaign; NFR-02/03/04 | T-30, T-31, T-32, T-33, T-34, T-35, T-36, T-37, T-38 | Pinned VT-01…17 and BM-01…09 results | Real kill tests, fuzz regressions, larger-than-RAM workload and long-run bounded resource use |
| [ ] T-58 | Constrained Rust 2D contact physics and atomic multi-body replay; FR-32 | T-29, T-57 | Admitted disc/boundary collision model, checkpoints and crash fixtures | VT-21 independent contacts/conservation-tolerance checks, ordering, budget limits and atomic crash outcomes pass |
| [ ] T-59 | Native disk spatial indexes, polygons/regions and trajectory crossings; FR-29 | T-37, T-52 | Correction-aware encrypted spatial/history indexes | VT-18/19/22 match reference; BM-10/11 include larger-than-RAM, degenerate geometry, late corrections and rebuild |
| [ ] T-60 | Spatial/physics/content lifecycle and privacy hardening; FR-09/11/23/28/29/32/33 | T-34, T-35, T-58, T-59 | Dependency invalidation, spatial threat fixtures and purge receipts | VT-23 denies hidden proximity/routes, stale source handles, revoked branches and restored deleted locations |
| [ ] T-61 | Full world-model mixed-load/fault campaign; NFR-02/03/04 | T-39, T-56, T-58, T-59, T-60 | VT-01…23 and BM-01…13 pinned reports | No correctness/privacy regressions under movement ingest, parsing, physics, compaction and bounded query load |
| [ ] T-40 | R3 local beta acceptance and complete integration lifecycle | T-39, T-61 | Local beta evidence and updated generic integration fixtures | Purge/restart/rebuild/stale-restore paths pass; entire R3 content/spatial/physics matrix documented and tested; distribution remains separate |

## External executable distribution prerequisite

T-62 does not block local implementation, tests, integration acceptance or preparation of release
artifacts. It is mandatory before any executable alpha, beta, release candidate or release is
provided outside the authorized development participants.

| Status / ID | Work package and requirement | Depends on | Artifact to produce | Completion evidence |
|---|---|---|---|---|
| [ ] T-62 | Enable and verify the selected private vulnerability-reporting route; NFR-05 | T-06 | Dated owner/admin test record and current reporting instructions | Harmless report proves reporter participation, authorized security-triage receipt/response and non-public handling; no fabricated address/SLA |

## R4 — Production candidate

| Status / ID | Work package and requirement | Depends on | Artifact to produce | Completion evidence |
|---|---|---|---|---|
| [ ] T-41 | Independent security/recovery review and remediation; NFR-01/03/05 | T-40 | Reviewer scope, findings and regression evidence | External assessment is real; release-blocking findings resolved or release held |
| [ ] T-42 | Packaging, signing, provenance and supported platform trials; FR-14, NFR-05 | T-40 | Release pipeline, SBOM/notices, install/upgrade/uninstall guides | VT-16 repeats clean installation and recovery on each claimed platform/profile |
| [ ] T-43 | Operator/support documentation and commercial-distribution review; FR-12/15, NFR-05 | T-41, T-42 | Support matrix, security policy, limits, parser/model licenses and integration guide | Claims match evidence; exact shipped dependencies and model terms reviewed; no unsupported certification |
| [ ] T-44 | R4 release decision | T-41, T-42, T-43, T-62 | Versioned release checklist and evidence manifest | Every required task/requirement traced; private reporting verified; all release gates pass; no hidden blocked work |

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
