# USTE — Architecture

Database design draft 1.3 · 2026-09-17 · Production engine not implemented

## Component boundaries

~~~text
Rust client / authenticated local service
                 |
Authorization + schema + admission limits
                 |
Transaction coordinator and ordered durable journal
                 |
Deterministic reducers + graph/temporal/spatial indexes
                 |
Queries, source retrieval, snapshots, branch views

Streaming blob store <--- authorized artifact commits
       |
Restricted parsing / OCR / media / embedding workers
       |
Validated derived artifacts and provenance transactions
~~~

Workers cannot write directly to storage or grant permissions. Source bytes, derived content,
and graph state share transaction visibility but may use different physical layouts.
The content pipeline is specified in [content ingestion](docs/content-ingestion-and-parsing.md).

The database and physics kernel are native local components usable by future games, agent
systems and tracking applications. Asset-price observations use ordinary typed records and
relationships, not a dedicated network subsystem. Data acquisition, provider authentication
and any trading logic belong to a separate consumer/ETL outside the engine. No such connector
is required by the implementation plan. Database API calls remain distinct from outbound
provider API calls; local encryption/IPC authorization are not provider credentials.
See [application use cases](docs/application-use-cases.md).

## Proposed Rust workspace

These are component boundaries, not existing directories or a fixed public API.

| Crate | Responsibility |
|---|---|
| uste-types | Stable identities, schemas, canonical encodings, bounded values |
| uste-policy | Principal/capability checks, labels, retention decisions |
| uste-storage | Journal, commit metadata, blob segments, checkpoints, I/O adapters |
| uste-txn | Commit sequencing, optimistic validation, idempotency, reader revisions |
| uste-graph | Native graph records, adjacency/property/temporal indexes, traversal |
| uste-spatial | Typed geometry, versioned frames/transforms, spatial predicates and native indexes |
| uste-motion | Observations, trajectories, state-at-time, uncertainty and correction dependencies |
| uste-content | Artifact lifecycle, parser protocol, derivation and citation validation |
| uste-query | Typed plans, budgets, ranking, explain output, lexical retrieval |
| uste-replay | Deterministic reducers, logical hashing, recovery replay |
| uste-sim | Branches, model identity, virtual clock, pure simulation scheduling |
| uste-physics | Pure bounded kinematics/contact models under pinned numerical profiles |
| uste-ingest | Mapping/preview/batch/resume orchestration through normal transaction authority |
| uste-api | Consumer API and generic integration adapter contracts |
| uste-cli / uste-service | Operations and authenticated local IPC |
| uste-testkit | Independent reference model, adversarial fixtures, fault simulation |

Dependency direction follows authority: types do not depend on host adapters; reducers do
not acquire files, time, randomness, model handles, or network access. Storage is not allowed
to interpret document instructions. Platform workers receive only narrow capabilities.

## Cross-component invariants

1. An acknowledged commit is durable under the declared platform/failure model.
2. A public reader sees one coherent committed revision; staged changes are private.
3. Journal records and all required blob objects are durable before their references publish.
4. Graph records, both adjacency directions, and idempotency outcomes agree atomically.
5. Derived indexes can be rebuilt; retained authoritative evidence cannot be fabricated.
6. Historical visibility never restores revoked current authorization.
7. Replay uses recorded inputs, not a fresh model interpretation.
8. Branches and parser outputs cannot silently promote themselves to authoritative knowledge.
9. Encryption, retention, deletion, and resource accounting include derived and temporary data.
10. Unsupported content is preserved or rejected explicitly, never reported as understood.
11. UTC normalizes known instants; revisions order commits. Uncertain source dates never
    become invented facts, and replay never reinterprets them using current timezone rules.
12. Identity survives movement; geometry always names a frame and units. Unknown is not zero.
13. Observed, estimated and simulated state remain distinct; viewing never advances physics.
14. Combined queries use one coherent knowledge view and current authorization for every source.
15. Physics effects commit as whole branch-local event groups, never direct storage mutations.

The state equation is:

~~~text
logical_state(revision) =
    reduce(schema_version, retained_baseline, committed_events_through_revision)
~~~

The baseline identifies the history/deletion epoch. Replay before the retained boundary is
unavailable. Seeds are simulation inputs, not substitutes for source bytes. Logical hashes
exclude randomized ciphertext and rebuildable physical index layout.

## Transaction and storage shape

One owner process holds an exclusive database lock. Clients submit bounded transactions to
an ordered commit coordinator. Snapshot readers use pinned committed revisions; commit-time
validation checks read sets/predicates or rejects unsupported isolation patterns. Do not
claim serializability for arbitrary transactions until phantom/conflict tests demonstrate it.
The first supported transaction API uses constrained operations with explicit preconditions.

Begin with an append journal and rebuildable reference indexes. The release engine adds
immutable disk-index runs with bounded caches, versioned roots, and atomic compaction.
The exact binary layout and index algorithm are gated by D-01; no other database engine is
silently introduced. Hot graph adjacency and cold temporal history require separate access
paths, not a time-partition-only layout.

Avoid two independent sources of commit truth. The journal owns commits; projections and
worker queues derive from it. A checkpoint is a cache until explicitly promoted to a new
recovery baseline during retention/compaction.

## Security scope

Core storage/query code is safe Rust by default. Standard-library/platform boundaries,
cryptographic implementations, native build dependencies, and optional workers are separately
inventoried. “No C/C++ database engine” is not “no C anywhere in the operating system.”
No mandatory C/C++ parser or model runtime is hidden behind Rust bindings. The strict engine
requires Rust storage, graph, spatial, query and physics implementations, including algorithmic
dependencies. A native physics/GIS engine behind bindings does not satisfy that profile.

Strict-local is the default: no network egress, no telemetry, no automatic model download.
An optional external worker profile requires explicit operator activation and must not be
represented as meeting a stricter dependency profile. Engine authenticity and privacy do not
imply immunity to a compromised host or correctness of the underlying evidence.

## Open decisions — R0 blockers

| ID | Required decision and evidence | Owner of contract |
|---|---|---|
| D-01 | Journal/commit-root protocol, disk-index layout, locking, Linux/filesystem support, power-loss assumptions; demonstrate torn metadata cannot silently roll back acknowledged commits | Storage |
| D-02 | Cryptographic suite/library, key store, nonce uniqueness across crashes/backups/clones, metadata leakage, rotation, key-loss and rollback limits | Security |
| D-03 | Exact raw upload, query, graph, parser, branch, cache, and history limits; hardware and performance budgets | Verification |
| D-04 | Retention epochs, purge/backup interaction, holds, deduplication boundary, branch pins, historical availability | Security + storage |
| D-05 | Required parser/model/decoder candidates by format, licenses, native dependencies, sandbox controls, coverage and strict-profile feasibility | Content |
| D-06 | Commit/serialization/schema version compatibility, idempotency lifetime, baseline migration, read/write preconditions, and time codec/ranges/calendar/precision/leap handling with pinned local timezone profiles | Data + storage + [time](docs/time-and-ordering.md) |
| D-07 | Private disclosure route, maintainer/reviewer responsibilities, release signing, dependency admission and support policy | Contributing + security policy |
| D-08 | Spatial types, geographic/local frame definitions, transforms, numeric tolerances, predicates/boundaries, disk index design, navigation costs and reference vectors | [Space](docs/spatial-world-model.md) |
| D-09 | Rust physics dependency feasibility, arithmetic/integrator/contact profile, supported shapes/forces, simulation-to-UTC mapping, budgets and deterministic restart vectors | [Physics](docs/physics-and-motion.md) |

Resolve these in follow-on decision records. A choice may be established for a constrained
release profile and revisited with migration tests; marking it “TBD” does not close R0.
Design experiments and test harnesses are allowed before R0, but no production-format claim
or irreversible implementation commitment is.

## References and authority

[PRD](PRD.md) owns scope; the focused specifications in the [README](README.md) own details.
[Decision 0001](docs/decisions/0001-product-direction.md) supersedes the historical kernel
plan; [Decision 0002](docs/decisions/0002-spatial-world-model.md) expands spatial/physics scope.
Established property graphs, temporal partitioning, and deterministic fault testing
are inspirations, not claims of implementation equivalence or new invention.
