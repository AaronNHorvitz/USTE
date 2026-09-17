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

## Requirements

SHALL means a release obligation, not an implemented feature. Each row names its first
required release and its detailed contract.

Decision 0018 and T-16 implement the default-deny namespace/record policy primitive and the
authorized transaction/blob facade for FR-09. Concrete graph/history/search/export/branch paths
remain obligations of their feature tasks and must repeat authorization-before-expansion tests;
the foundation is not a claim that those unimplemented paths already exist.

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
