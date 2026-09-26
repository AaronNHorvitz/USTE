# USTE — Product development tasks

## Current Delivery Amendment (2026-09-24)

[The 48-capability roadmap](CAPABILITY-ROADMAP.md) and
[the implementation amendment](IMPLEMENTATION-AMENDMENT.md) define the current
Rust-first delivery direction and ownership. TASKS.md remains the only completion ledger.
This is accepted implementation scope, not evidence that the capabilities already work.
Existing security, independent-review, licensing and release gates remain in force.

### Capability Work Packages

These crosswalk packages extend, rather than replace or renumber, existing tasks. All start
open. Reconcile existing source/evidence, split into bounded subtasks here, then implement
in dependency order. Catalogue priority P0 precedes P1/P2; a grouped package cannot pull a
later capability ahead of an unblocked P0 dependency. Checked historical rows are not proof
of the new acceptance criteria. Detailed proof requirements are in the linked amendment.

| Status | Package | Capability mapping | Dependencies | Deliverable and acceptance |
| --- | --- | --- | --- | --- |
| [ ] | DB-R01 | CAP-05,20,25,29,30,36,46 | T-20, T-19 and applicable correctness gates | Finish bounded disk-backed lookup correctness, cache/reopen/fault behavior and honest resource measurements. Preserve benchmark targets, exact environments and unavailable reservation blockers. |
| [ ] | DB-R02 | CAP-20,22,27,29 | T-63-T-68; DB-R01 only where disk-scale guarantees are claimed | Specify versioned source/artifact/claim/edge records for research, docs and repository memory with scope, provenance, revisions, freshness and budgets. Do not fetch the web or execute tools inside storage. |
| [ ] | DB-R03 | CAP-20,23,24,29,30,34,42 | DB-R02; existing source and query authority | Test corrections, contradictions, revocation, source deletion, stale generations, cross-scope denial, restart and rebuild. A derived index is never permission or the sole source of truth before migration qualification. |
| [ ] | DB-R04 | CAP-08,17,21,25,28,32,40 | DB-R02, DB-R03; pinned generic consumer contract | Expose a restricted Rust adapter and conformance fixtures for runtime and coordinator consumers. Verify producer and consumer independently; combined functionality needs an actual authorized integration run. |
| [ ] | DB-R05 | CAP-35,36,43,45,46,47,48 | Original R1-R4 lifecycle and distribution gates | Retain full database/spatial/physics delivery, clean installation, no-egress and resource qualification, manual redacted diagnostics and MIT OR Apache-2.0 notices. No release claim from the synthetic memory pilot. |

DB-R02 progress: [Decision 0275](docs/decisions/0275-research-memory-record-specification.md)
accepts the [research memory record specification](docs/research-memory-records.md) and splits the
package into DB-R02.1 specification, DB-R02.2 codec and golden vectors, DB-R02.3 reducer, budgets
and reference model, DB-R02.4 bounded authorized queries, and DB-R02.5 pilot mapping fixtures.
[Decision 0276](docs/decisions/0276-research-memory-record-codec.md) implements DB-R02.2 and
[Decision 0277](docs/decisions/0277-research-memory-reducer.md) implements DB-R02.3 and
[Decision 0278](docs/decisions/0278-research-memory-queries.md) implements DB-R02.4 and
[Decision 0279](docs/decisions/0279-memory-pilot-to-research-mapping.md) implements DB-R02.5 in
`uste-memory`, each with focused verification. DB-R02 stays unchecked until a full gate and review
cover the package; migration of pilot data is not part of it.
DB-R03 progress: [Decision 0280](docs/decisions/0280-research-stale-views-and-lifecycle-matrix.md)
fixes stale research read views and adds the authorized durable lifecycle matrix (contradictions,
correction chains, revocation, cross-scope and per-record denial, stale generations, rebuild across
restart). Source deletion and purge remain T-34, so DB-R03 stays unchecked.
DB-R04 dependency clarification: [Decision 0281](docs/decisions/0281-producer-owned-memory-consumer-contract-delegation.md)
records the owner's delegation of the producer-owned contract design to USTE. The contract,
producer adapter and synthetic conformance fixtures are dependency-ready.
[Decision 0282](docs/decisions/0282-memory-consumer-contract-1-0.md) pins the candidate
[`uste-memory-consumer` 1.0 contract](docs/memory-consumer-contract.md) (fixture digest
`b145b45e…ef034`) and its producer passes all 34 conformance steps; consumer review, consumer
implementations and an authorized integration run remain open, so DB-R04 stays unchecked. DB-R02/DB-R03 reviewer acceptance is also outstanding.

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

## Completed early delivery — M1 experimental local memory pilot

[Decision 0054](docs/decisions/0054-memory-first-delivery.md) adds this earlier delivery
checkpoint without changing existing R0–R4 dependencies or accepting failed benchmarks.
T-63–T-68 were executed in dependency order; current work resumes T-20/T-19 and the complete roadmap.
A full-product goal continues after M1; an explicitly M1-only goal ends at verified T-68.
[The milestone contract](docs/memory-first-milestone.md) owns the detailed exit cases.
T-63 through T-68 and M1 are complete. Resume T-20, then T-19 and the unchanged full roadmap.

| Status / ID | Work package and requirement | Depends on | Artifact to produce | Completion evidence |
|---|---|---|---|---|
| [x] T-63 | Reconcile interrupted work and freeze bounded pilot profile; FR-02/03/09/10, NFR-02/03/04 | T-15, T-16, T-17, T-18, T-45, T-49 | [Decision 0055, executable limits and exact baseline](docs/decisions/0055-memory-pilot-profile.md) with [T-63 evidence](docs/evidence/memory-pilot-baseline.md) | Pending Decision 0053 work preserved and verified at `7393def`; full check green under a 4 GiB process-group cap; pilot admission/measurement thresholds frozen before implementation |
| [x] T-64 | Durable source-backed memory writes and restart-safe ingestion; FR-01/02/03/06/17/18 | T-63 | [Bounded write/query/lifecycle decision](docs/decisions/0056-bounded-memory-write-query-lifecycle.md) and [T-64–T-66 core evidence](docs/evidence/memory-pilot-core.md) | Exact source admission and idempotent retry survive reopen; abandoned upload reconciliation preserves quotas and permits later ingestion |
| [x] T-65 | Bounded authorized graph/lexical retrieval and citation resolution; FR-05/09/13/20/26 | T-64 | [Small-corpus API, independent oracle and explicit time subset](docs/evidence/memory-pilot-core.md) | Exact citations, history/corrections/contradictions, cross-scope denial, work/output budgets and cooperative cancellation pass |
| [x] T-66 | Pilot lifecycle, revocation and fail-closed rebuild; FR-09/11/23 | T-65 | [Generation/revocation/rebuild contract and evidence](docs/decisions/0056-bounded-memory-write-query-lifecycle.md) | Source/policy revocation hides content/counts, stale views/generations refuse, and interrupted rebuild remains closed; no erasure claim |
| [x] T-67 | Generic local Rust consumer adapter and offline demo; FR-14/15, NFR-04 | T-66 | [Decision 0057](docs/decisions/0057-local-memory-consumer-adapter.md), [runnable demo](docs/memory-pilot-demo.md) and [adapter evidence](docs/evidence/memory-pilot-adapter.md) | Real Btrfs demo proves exact source authority, reopen/lock/version boundaries, cited corrections/revocation/rebuild and no cloud/feed/model dependency |
| [x] T-68 | M1 end-to-end acceptance and consumer handoff; NFR-02/03/05 | T-67 | [Decision 0058](docs/decisions/0058-memory-pilot-qualification.md), [M1-A–J evidence](docs/evidence/memory-pilot-acceptance.md) and [exact consumer handoff](docs/memory-pilot-handoff.md) | Every M1 case passes at pinned `b9689f3` under frozen bounds; integration/release/security admission remains separate |

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
| [ ] T-19 | R1 acceptance and operating limitations | T-15, T-16, T-17, T-18, T-20, T-45, T-48, T-49 | [R1 acceptance and limitations report (draft, not accepted)](docs/evidence/r1-acceptance-report.md) and runnable kernel instructions | Required R1 VT/BM scope passes; clear non-production and scalability limits |

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
| [ ] T-20 | Disk-index foundation and bounded cache; FR-16 | T-17, T-18, T-49 | [Versioned graph/index runs, authorized cache/I/O diagnostics and capped production/oracle equivalence](docs/evidence/disk-index-foundation.md), [terminal root deltas](docs/evidence/graph-state-root-deltas.md), [bounded composite preparation](docs/evidence/bounded-composite-preparation.md), [explicit-I/O preparation](docs/evidence/explicit-io-graph-preparation.md), [complete proof buckets](docs/evidence/complete-graph-preparation-proofs.md), [proof-derived root deltas](docs/evidence/proof-derived-root-deltas.md), [proof-prepared commit](docs/evidence/proof-prepared-graph-commit.md), [bounded terminal-root publication](docs/evidence/bounded-terminal-root-publication.md), [admitted root handoff](docs/evidence/admitted-root-handoff.md), [resumable index proofs](docs/evidence/resumable-index-proof-primitives.md), [streaming cold graph-base admission](docs/evidence/streaming-cold-graph-base-admission.md), [warm disk-backed live state](docs/evidence/warm-disk-live-state.md), [journal-anchored disk recovery](docs/evidence/journal-anchored-disk-recovery.md) and [bounded root-manifest discovery](docs/evidence/bounded-root-manifest-discovery.md) | VT-05/14 and BM-01/06 verify rebuild, visibility and cache pressure; no full-RAM assumption hidden |
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
