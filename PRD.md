# USTE — Product requirements

Version: database design draft 1.4 · Date: 2026-09-17

Status: R0 design ready; T-08/T-09 workspace and canonical types implemented. Requirement and test
identifiers below belong to this database design, not the archived simulation PRD. External
executable distribution remains blocked by unverified private vulnerability reporting (T-62).

## Purpose and users

Provide an independent, natively built Rust database with a physics/simulation kernel for
application and agent builders who need connected knowledge, temporal history, arbitrary
content, spatial world models and reproducible motion simulation. Intended applications
include future game development and item tracking linked to imported asset-price observations.
Items can have optional locations, movement, attached files and explicit asset/price relationships.
Primary consumers are Rust applications, restricted local clients, and human operators.
No specific consuming application is part of the product identity.

## Scope

The engine owns storage, revisioned graph state, transactional constraints, evidence/artifact
identity, bounded queries, and worker admission. Consumers own domain-specific approval
policy, model selection, and real-world actions. Persisting a procedure does not authorize it.

Initial operation is single-machine/single-owner-process. Distributed consensus, full SQL/
Cypher/AQL compatibility, rendering, general-purpose 3D/fluids/celestial physics, automatic
real-world actions, universal file interpretation and arbitrary native plugins are outside R4.
Spatial indexes, bounded navigation and the constrained physics baseline are explicitly in scope.

Asset prices are caller-supplied data, not a promise of native market-data acquisition.
Support local-file/ETL ingestion and retrieval through existing graph, temporal and content
contracts. Exchange/broker/feed API clients, provider credential collection, account linking,
automatic refresh and order execution are excluded from the current scope. A separate caller
may obtain data elsewhere and submit ordinary records; no connector is required to test or
use local tracking, pricing links, game-world storage or physics.

No external-service account, password or API token is a core runtime prerequisite. Local
database key management and authorization remain required. Repository administration and
release credentials are separate development/distribution concerns, not runtime dependencies.
Decision 0011 separates local development governance from external distribution readiness.
No external-service credential is needed for implementation; repository-owner verification of
the selected security-reporting route remains mandatory before distributing an executable.

See [application use cases](docs/application-use-cases.md) for R2 synthetic acceptance examples
under FR-15/27/28/31/33/34. Game rendering and broader physics remain future consumer work.

## Earlier delivery milestone — M1 local memory pilot

Deliver the [bounded memory-first milestone](docs/memory-first-milestone.md) before resuming
the full-scale T-20 acceptance campaign and broader spatial/physics rollout. M1 is a separate
experimental local integration checkpoint, not a renamed R1/R2 gate or a production release.
Its scope is approved source-backed records, small-corpus retrieval/citations, corrections,
access revocation, restart and a rebuildable generic consumer adapter. T-63–T-68 own its
profile, implementation and acceptance. No required FR/NFR obligation or existing benchmark
target is removed; unsupported operations must be refused explicitly.

An in-memory index over durable encrypted state is admissible only within a tested total-state
cap that includes replay, history and metadata, with safe refusal before exceeding the cap.
It is not evidence of the larger-than-memory disk engine. Actual consumer adoption remains
separately reviewed; M1 does not authorize authoritative migration or sensitive production use.

## Requirements

SHALL means a release obligation, not an implemented feature. Each row names its first
required release and its detailed contract.

Decision 0018/T-16 implement the default-deny namespace/record policy primitive and authorized
transaction/blob facade. Decision 0019/T-17 implements durable graph policy plus authorized direct,
history, adjacency and evidence-backed projections for FR-01/04/06/09. Search, export, branches,
disk indexes and composed queries remain obligations of their feature tasks and must repeat
authorization-before-expansion tests.

| ID | Obligation | Gate | Contract |
|---|---|---|---|
| FR-01 | Stable namespace-scoped identities and validated typed entities, assertions, relationships, evidence, and artifacts | R1 | [Data](docs/data-model.md) |
| FR-02 | Atomic transactions with durable acknowledgments, conflict validation, coherent readers, and retry idempotency | R1 | [Storage](docs/storage-and-recovery.md) |
| FR-03 | Versioned journal, commit metadata, strict recovery, canonical encoding, and verified checkpoints | R1 | [Storage](docs/storage-and-recovery.md) |
| FR-04 | Maintain graph endpoint integrity and both adjacency directions transactionally; bound traversals | R1 | [Data](docs/data-model.md) |
| FR-05 | Support valid-time and recorded-revision queries, corrections, supersession, and unresolved contradictions | R2 | [Data](docs/data-model.md) |
| FR-06 | Preserve evidence versions and distinguish authorized acceptance from factual truth | R1 | [Data](docs/data-model.md) |
| FR-07 | Deterministically replay retained accepted inputs without external tools, network, or inference | R1 | [Replay](docs/replay-and-simulation.md) |
| FR-08 | Isolate hypothetical branches, pin model/input versions, and require authorized promotion transactions | R2 | [Replay](docs/replay-and-simulation.md) |
| FR-09 | Enforce namespace/record authorization on all paths, including history, indexes, blobs, and exports | R1 | [Security](docs/security-and-privacy.md) |
| FR-10 | Authenticated encryption and defined key lifecycle for sensitive persistent artifacts | R1 | [Security](docs/security-and-privacy.md) |
| FR-11 | Retraction, expiry, purge, compaction, branch invalidation, and supported restore cannot silently resurrect erased content | R3 | [Security](docs/security-and-privacy.md) |
| FR-12 | Verified backups, non-destructive restore, explicit migrations, and documented compatibility | R3 | [Storage](docs/storage-and-recovery.md) |
| FR-13 | Scoped bounded retrieval, source explanations, lexical search, optional versioned embeddings, and stale-result exclusion | R2 | [API](docs/api-and-integration.md) |
| FR-14 | Rust API, structured errors, operational CLI, and authenticated local service adapter | R2 | [API](docs/api-and-integration.md) |
| FR-15 | Generic derived-index and authoritative-storage integration modes with migration and rollback boundaries | R2 | [API](docs/api-and-integration.md) |
| FR-16 | Bounded-memory disk-backed indexes, time partitioning, and correction-aware materialized summaries | R3 | [Storage](docs/storage-and-recovery.md) |
| FR-17 | Store arbitrary file bytes without requiring format understanding, subject to explicit admission limits | R1 | [Content](docs/content-ingestion-and-parsing.md) |
| FR-18 | Immutable artifact versions, streaming ingest, authenticated blob verification, and atomic metadata/blob publication | R1 | [Content](docs/content-ingestion-and-parsing.md) |
| FR-19 | Isolated, capability-limited, versioned parsing with explicit unsupported/partial/error states and cancellation | R2 | [Content](docs/content-ingestion-and-parsing.md) |
| FR-20 | Link derived text, tables, chunks, and metadata to exact source versions and honest byte/page/sheet/time locators | R2 | [Content](docs/content-ingestion-and-parsing.md) |
| FR-21 | Deliver the release-scoped format matrix, including rich documents, images/OCR, and media adapters | R3 | [Content](docs/content-ingestion-and-parsing.md) |
| FR-22 | Treat content and parser output as untrusted data; do not execute macros, code, links, or embedded instructions | R2 | [Content](docs/content-ingestion-and-parsing.md) |
| FR-23 | Propagate source changes, revocation, and deletion to derivations, search indexes, branches, and caches | R3 | [Content](docs/content-ingestion-and-parsing.md) |
| FR-24 | Separate simulated from observed results and retain model, parser, embedding, and schema version identities | R2 | [Replay](docs/replay-and-simulation.md) |
| FR-25 | Bounded change subscriptions with resumable cursors and explicit retention gaps | R3 | [API](docs/api-and-integration.md) |
| FR-26 | Normalize resolvable instants to UTC; preserve source interpretation/precision; distinguish clock domains, unresolved dates and commit order; replay accepted values without reinterpretation | R1 kernel; R2 query/parser integration | [Time](docs/time-and-ordering.md) |
| FR-27 | Scoped worlds, stable item identities, optional geographic/local geometry, typed units and versioned acyclic frames/transforms | R1 types; R2 transforms | [Space](docs/spatial-world-model.md) |
| FR-28 | Source-backed movement observations, trajectory versions, late corrections and observed/estimated/simulated/conflicting/unknown state-at-time | R2 | [Space](docs/spatial-world-model.md) |
| FR-29 | Bounded geographic/local spatial queries; correction-aware historical regions/crossings and native disk spatial indexes | R2 point/box/radius; R3 regions/history/indexes | [Space](docs/spatial-world-model.md) |
| FR-30 | Bounded deterministic path queries over supplied traversable graphs, named cost/constraints and explicit unreachable/budget outcomes | R2 | [Space](docs/spatial-world-model.md) |
| FR-31 | Rust deterministic 2D/3D kinematic branches with pinned numerical profiles, explicit virtual-time mapping and provenance | R2 | [Physics](docs/physics-and-motion.md) |
| FR-32 | Rust constrained 2D contact physics with bounded resources, atomic multi-body results, checkpoint/resume and model-specific accuracy tests | R3 | [Physics](docs/physics-and-motion.md) |
| FR-33 | Coherent structured queries combining identity, graph, content, space and time; return actual records, evidence and authorized original-byte handles | R2 baseline; R3 full matrix | [Retrieval](docs/ingestion-and-unified-retrieval.md) |
| FR-34 | Previewable bounded batch ingestion with explicit mappings, source provenance, idempotency, checkpoints and honest partial-job outcomes | R1 transaction contract; R2 CSV/JSON tooling | [Ingestion](docs/ingestion-and-unified-retrieval.md) |
| NFR-01 | Memory-safe-first implementation with reviewed transitive dependencies and documented unsafe/platform boundaries | R0 onward | [Security](docs/security-and-privacy.md) |
| NFR-02 | Repeatable performance and recovery budgets on named hardware with encryption/security enabled | R0 onward | [Verification](docs/verification-and-benchmarks.md) |
| NFR-03 | Reference-model, fuzz, fault-injection, concurrency, and long-run evidence tied to the tested build | R1 onward | [Verification](docs/verification-and-benchmarks.md) |
| NFR-04 | Offline core, explicit network permissions, no required telemetry, and bounded parsing/query resource use | R1 onward | [Security](docs/security-and-privacy.md) |
| NFR-05 | Reviewable licenses/provenance, reproducible release process, documented vulnerability-reporting policy, verified reporting before executable distribution, and honest support claims | R0 governance; distribution and R4 verification | [Contributing](CONTRIBUTING.md) |

Decision 0023 implements FR-34's R1 typed transaction slice: exact finalized source/mapping
bindings, authorized atomic graph/spatial batches, durable retry and logical job checkpoints. It
does not yet implement mapping execution, rejected-row handling, CSV/JSON tooling, an authorized
consumer preview surface or the item/price fixture; those remain T-54/R2.

Decision 0025 implements an initial FR-16/T-20 foundation of encrypted immutable sorted runs,
certificate-anchored rebuildable roots, a bounded decrypted-page cache and semantically checked,
authorization-preserving current graph projections. It does not close T-20 or satisfy the R2 gate:
streaming larger-than-memory recovery and BM-01/BM-06 evidence remain required, while compaction and
authoritative baselines remain T-35/R3.

Decision 0026 advances that foundation with borrow-aware replay state, bounded streaming checkpoint
publication, visitor-based index reads and a pinned deterministic BM-01 materialization/query
oracle. The fixture is not performance evidence. Full-state reducer recovery, clone-based reducer
writes and both qualifying benchmarks remain open, so no release-gate status changes.

The checkpoint transport now discovers authenticated opaque candidates and streams a selected
candidate under its live journal/key context without retaining the complete plaintext payload.
Reducer decoders still require complete state, so this narrows but does not close the recovery gap.

Decision 0027 removes full graph snapshot cloning, full-state validation and full derived-index
rebuilds from ordinary successful graph transactions. Preparation retains only ordered before/after
changes and validates them against a merged view; publication updates history and index
contributions incrementally without changing canonical bytes or the frozen current projection.
Decision 0028 adds incremental reverse dependencies and makes graph delete discovery proportional
to target fanout plus transaction changes instead of all records. At that increment, explicit
snapshots, checkpoint decoding and ingest writes remained full-state boundaries; Decision 0034
later bounds ordinary ingest preparation. Snapshots/decoding remain materialized and reverse fanout
has no accepted aggregate cap, so T-20 and both qualifying benchmarks remain open.

Decision 0029 freezes and implements the distinct certificate-anchored `graph-state-v1` derived
root with complete current/history, adjacency, provenance, reverse and policy families. Admission
requires the exact live snapshot and fully scrubbed encrypted pages; it neither seeds recovery nor
becomes commit authority. Full-memory publication/admission and bounded recovery remain open.

Decision 0030 adds an explicitly bounded, certificate-rechecked full-run visitor and semantic
reconstruction of all `graph-state-v1` families without a monolithic checkpoint byte buffer. The
ordinary reconstructed reducer still retains full state in memory and lacks coordinator retry/blob
metadata, so it is an intermediate candidate rather than larger-than-memory recovery or authority.

Decision 0031 adds the separate `coordinator-meta-v1` root and a temporary authenticated recovery
owner. An exact certificate/state pair can now form a graph coordinator seed before normal open;
seeded open independently verifies all prefix transaction metadata and replays the suffix. The
result remains full-memory recovery and does not close T-20 or qualify BM-01/BM-06.

Decision 0032 adds a bounded authenticated merge from one optional `index-v1` base plus exact
ordered before/after deltas into one unpublished encrypted terminal run. It validates absent/
present preconditions, tombstones, source integrity and independent source/delta/output budgets
without changing the frozen format or journal authority. Decision 0033 adds the graph-owned
two-phase bridge: an opaque precommit plan binds exact family deltas to an admitted base and result
digest, and postcommit maintenance merges all eight families, compares them with the actual live
reducer and publishes only a complete matching root. The independent comparison and reducer remain
full-memory; a live disk-backed reducer and qualifying BM-01/BM-06 measurements remain open.

Decision 0034 removes complete retained-state candidate copies from ordinary graph, spatial and
composite ingest preparation. Borrowed overlays retain only graph changes, spatial request effects
and one job/row-ledger mutation; all component bases are preflighted before publication. Canonical
requests, results, checkpoints and reducer profiles are unchanged. Current reducers, closure scans,
snapshots and checkpoint reconstruction remain full-memory, so this is T-20 write-path groundwork,
not larger-than-memory or benchmark qualification.

Decision 0035 separates bounded authenticated graph proof loading from pure reducer preparation.
Supported current-state transactions retain exact positive/negative record proofs and policy from
the admitted disk root, then prepare without an I/O capability. Deletion and historical predicates
remain explicitly unsupported pending complete reverse/history proofs; the live coordinator,
root-delta metadata and recovery state remain full-memory. T-20 and BM-01/BM-06 remain open.

Decision 0036 completes that preparation proof vocabulary with bounded authenticated history
prefixes for `ReadView` and reverse-owner buckets for deletion. All graph operation/precondition
variants can now use the storage-free partial reducer phase. Decision 0037 authenticates the base
metadata/counts in that proof and derives the bounded terminal-root plan directly from its result.
Decision 0038 binds that opaque result to the exact canonical request and current live base so the
authoritative coordinator can commit it without repeating complete-state preparation. Publication,
independent postcommit validation and recovery still use the complete live reducer;
larger-than-memory recovery and qualification remain open.

Decision 0048 removes the complete live reducer from proof-derived postcommit root validation.
The authenticated merge exposes a provisional ordered-output visitor; graph publication checks
proof-derived family counts and reproduces the existing canonical logical-state digest from the
actual merged primary entries under an explicit one-history-bucket memory bound. The live reducer,
coordinator metadata, root admission and recovery remain full-memory, so T-20 and BM-01/BM-06 stay
open.

Decision 0049 preserves that terminally validated root as an opaque admitted handle. Consecutive
disk-backed preparations can use it directly without rediscovering the root through a complete
live-snapshot comparison, including after a coordinator/filesystem reopen when the caller retained
the handle. Cold candidate admission and process recovery still require the planned streaming
cursor/predecessor work and do not inherit this warm-handoff proof.

Decision 0050 adds those underlying storage proofs: an opaque resumable authenticated run cursor
that releases the filesystem borrow between entries, plus a bounded greatest-prefix-key-at-or-
before lookup for historical reference resolution. Terminal cursor reports require full run
authentication, and predecessor values use a two-pass one-value bound. Graph semantic admission
and a cold `GraphDiskBase` remain the next T-20 increment.

Decision 0051 completes that cold semantic-admission increment. Live and recovery owners can
stream all eight families, validate history/current/reference/derived/policy invariants, reproduce
the canonical digest and return an I/O-capability-free `GraphDiskBase` for bounded preparation.
History groups, exact/predecessor proof work and semantic reference comparisons have explicit
caller limits; no complete graph map is reconstructed. Candidate discovery/scrub, the live
coordinator/reducer, suffix replay and qualifying BM-01/BM-06 evidence remain open T-20 work.

Decision 0052 replaces the warm coordinator's complete graph publication target with
`GraphDiskLiveState`: one admitted base plus at most one request-bounded pending root plan. The
ordinary journal commit remains authoritative; pending state hides the stale base, permits exact
retry and repair, and rejects distinct progress until streamed terminal-root validation installs
the exact next base. Representation handoff proves scope/revision/policy/digest/certificate
equivalence and preserves coordinator metadata. Pending crash recovery, coordinator metadata,
candidate discovery/scrub, suffix replay and BM-01/BM-06 remain open T-20 work.

Decision 0039 makes the authorized encrypted index's userspace cache explicitly clearable and
reports cumulative cache bytes/events plus authenticated page, fragment and result-byte work. It
does not expose keys, plaintext or candidate identities, but its candidate-dependent counters are
cardinality-sensitive and therefore require current `ManageSchema` authority plus an opaque root
bound to the issuing coordinator. It does not claim control of kernel/device caches. This enables
honest BM-01 measurement through the authorization boundary but supplies no performance result;
the exact qualifying run and full-memory boundaries remain open.

Decision 0040 connects that boundary to the pinned fixture and independent oracle at a hard-capped
development scale. After durable transactions, index publication and simulated restart, all 384
measured query shapes must agree exactly. Its memory adapter, test key wrapper and absent timing/RSS
make it semantic groundwork only; it always reports `engine_benchmark:false`.

Decision 0041 makes that materializer stream maximum-10,000-operation transactions and pins the
exact qualifying plan at 212 durable revisions, including its shared Evidence record. The command
cap and nonqualifying label remain; bounded operation construction is not Linux/RSS/latency or
larger-than-memory evidence.

Decision 0042 adds resumable Linux/Btrfs creation and authenticated open phases using OS entropy
and the portable recovery profile. Its fixed content-free reports disclose uncontrolled host
caches and the full-memory graph boundary. This is platform runner implementation, not an exact
BM-01 run or a performance pass.

## Release gates

| Gate | Required outcome |
|---|---|
| R0 — design ready | D-01 through D-09 closed for development with recorded decisions; threat model and limits approved; versioned acceptance vectors and benchmark budgets specified; documentation consistent |
| R1 — correctness kernel | Reference model plus encrypted durable transactions, graph integrity, raw blob round-trip, evidence, authorization, idempotency, replay, and crash tests pass; not production-ready |
| R2 — local developer alpha | R1 maintained; temporal queries, snapshots, disk-index foundation, lexical retrieval, initial parser matrix, local API/CLI, generic adapter fixture, and isolated branch example pass; this is not distribution authorization |
| R3 — local hardened beta | R2 maintained; rich-format matrix, optional embedding path with an exact baseline, bounded-memory scale tests, deletion/retention, backup/restore, migrations, subscriptions, and adversarial suites pass; this is not distribution authorization |
| R4 — production candidate | All requirements demonstrated at declared limits; independent security review completed and release-blocking findings resolved; private vulnerability reporting verified; repeatable install/upgrade/restore trials, support policy, and signed release evidence available |

Additional mandatory gate scope under Decision 0002: R1 includes spatial/world types and
bounded import transaction contracts; R2 includes frames, movement, point spatial lookup,
navigation, unified retrieval, batch tooling and kinematics; R3 includes historical region
queries/native spatial indexes, constrained contact physics and cross-feature privacy/load tests.

R0 design readiness is recorded by T-01–T-07/T-46–T-47. This closes decisions, not
implementation, benchmark performance, security certification or production qualification.
Operational disclosure verification remains open as T-62 and blocks every external executable
alpha/beta/candidate/release distribution, while local R1–R3 work proceeds. Parser capabilities
cannot be silently dropped to pass a gate; unsupported cases within a declared supported family
require a documented coverage boundary and tests. Any material release scope change requires a
decision and PRD revision.

## Product-level acceptance scenario

A generic local client imports a document, preserves original bytes, requests authorized
extraction, proposes a sourced assertion, and records its approval. After restart it queries
the assertion with a resolvable citation, corrects it, queries both temporal perspectives,
runs a hypothetical branch, and exports a permitted result. It then revokes source access
and requests deletion. Retrieval, branches, indexes, and supported restore obey the revised
policy and do not resurrect the removed content. A foreign namespace sees none of it.

Tests additionally cover an unknown binary stored without parsing, an unsupported encrypted
document, malformed parser input, and a failed upload that never becomes an accessible artifact.
The [synthetic world scenario](docs/ingestion-and-unified-retrieval.md) additionally proves
moving objects, attached documents, combined spatial/content/graph queries, navigation,
UTC/local display, physics branches and correction-aware historical answers after restart.
Extend the R2 fixture with a headless game-world consumer and an item linked to a synthetic
asset-price history imported from local CSV/JSON. Verify exact scaled values, source/quote
units, corrections and knowledge cutoffs with network access disabled and no provider secrets.

## Risks and non-claims

Original storage increases implementation and maintenance obligations. A Rust label does not
prove security; checksums do not authenticate history; LLM extraction does not prove truth.
Local operation does not protect an unlocked process from a compromised OS. No throughput,
all-format understanding, certification, patent novelty, or production readiness is claimed.

The current roadmap includes a simulation kernel, but ordinary ingest/commit/read must work
with simulation and parsing workers absent. Optional GPU, model, and network features must
never become hidden core dependencies.
