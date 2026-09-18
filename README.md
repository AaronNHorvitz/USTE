# USTE — Universal Spatial-Temporal Engine

A local-first, Rust-native spatial-temporal graph and content database with a deterministic
physics/simulation kernel for queryable world models, agent systems and future game development.

USTE is being designed to store connected knowledge, arbitrary file bytes, source evidence,
and durable event history, including objects with geographic/local positions and movement.
Agents would use a bounded API to retrieve actual items and authorized content, navigate
relationships and locations, trace answers to sources, and explore hypothetical motion.

USTE is a natively built database system, not a hosted data service. Applications can use it
to persist game-world items, track real-world objects, or link items to imported asset-price
observations. These are uses of the same storage, graph, space, time and physics capabilities.
An item can link to a price record without USTE fetching that price from an external provider.

**Status: R0 design ready; the T-08–T-18/T-45/T-48/T-49 correctness foundation now includes the
workspace, encrypted journal/transactions/blobs, authorization, evidence graph, replay/checkpoints,
pinned time, spatial schemas, atomic typed import contracts and an encrypted disk-backed current-
graph index.** There is no general database executable, supported content parser, security
certification, or production release yet. One T-15 component benchmark is recorded and misses its
throughput target. The features below are requirements unless explicitly identified as implemented.

## Product direction

The product is an original Rust database engine, not a wrapper around ArangoDB, TimescaleDB,
SQLite, or RocksDB. Property graphs, time partitioning, and incremental summaries are
conceptual influences; persistence, transactions, graph indexes, and replay are our own
implementation responsibility. Reviewed supporting libraries remain permitted.

The former simulation-first design is preserved in [design history](docs/history/simulation/README.md).
[Decision 0001](docs/decisions/0001-product-direction.md) explains what changed.
[Decision 0002](docs/decisions/0002-spatial-world-model.md) adds required spatial lookup,
movement history, bounded navigation and a constrained physics baseline. Celestial mechanics
and rendering do not gate the database release; spatial indexing is now required by R3.

## Planned capabilities

Intended applications include future game development, item/asset tracking linked to supplied
price observations, and agent memory. A renderer or complete game engine is not required now.
The core does not implement exchange/broker/market-data API clients, collect provider
passwords/API keys, or place trades. Local CSV/JSON, synthetic fixtures and caller-supplied
records are sufficient. Native Rust/local IPC APIs are database interfaces, not outbound
price-feed calls. No cloud account or external-provider credentials are required for these
local workflows; local encryption keys and authorization remain necessary security controls.
See [application use cases](docs/application-use-cases.md) for mappings and acceptance scope.

- Persistent entities, typed relationships, assertions, evidence, and artifacts.
- Valid-time and recorded-time history, corrections, contradictions, and provenance.
- UTC-normalized instants with original timezone provenance and explicit ambiguous-date handling.
- Atomic durable transactions, verified snapshots, crash recovery, backup, and migration.
- Storage of arbitrary file formats within declared size, quota, and admission policies.
- Immutable content versions, isolated extraction, searchable chunks, and source citations.
- Explicit format support: storing a file does not imply the ability to parse or understand it.
- Authenticated encryption, scoped authorization, deletion, and bounded resource use.
- Deterministic historical replay and isolated hypothetical branches.
- A Rust API, operational CLI, and optional authenticated local service.
- Geographic/local frames, movement observations, spatial history and explicit uncertainty.
- Bounded supplied-graph navigation and joint content/graph/location/time retrieval.
- Rust kinematic and constrained 2D contact simulation, separated from observed facts.
- Previewable, resumable CSV/JSON batch import through normal transaction validation.

## Unstructured content

Original bytes and derived interpretations are different records. A PDF, image, audio clip,
spreadsheet, source archive, or unknown binary may be retained without executing it.
Supported workers can derive text, tables, metadata, OCR, or transcripts; each result records
its source version, parser/model identity, limitations, and exact available locators.
Unsupported, encrypted, malformed, or partially parsed files return visible status.
Neither document instructions nor extracted code receive execution authority.

See [content ingestion and parsing](docs/content-ingestion-and-parsing.md) for the planned
format matrix, security boundary, and release gates.

## Initial scope

One machine, one database-owning process, multiple clients, an ordered commit coordinator,
and concurrent coherent readers. No mandatory network, GPU, hosted model, telemetry, or
external database. The initial platform is Linux, with Fedora Kinoite as a target test
environment; exact supported kernel/filesystem combinations must be recorded before release.

No production security or speed claim follows merely from choosing Rust. Dependencies,
unsafe code, platform interfaces, file parsers, recovery, and permissions require evidence.
No universal file-understanding, government approval, or historical-replay-after-erasure
guarantee is made.

## Documentation

| Document | Owns |
|---|---|
| [PRD](PRD.md) | Requirements and release gates |
| [Architecture](architecture.md) | Component boundaries and cross-component invariants |
| [Data model](docs/data-model.md) | Records, lifecycle, time, and constraints |
| [Time and ordering](docs/time-and-ordering.md) | UTC normalization, source timestamps, uncertainty, clock domains, and knowledge cutoffs |
| [Spatial world model](docs/spatial-world-model.md) | Frames, objects, movement, geographic queries and navigation |
| [Physics and motion](docs/physics-and-motion.md) | Kinematics, constrained contacts, numerical profiles and branch durability |
| [Ingestion and unified retrieval](docs/ingestion-and-unified-retrieval.md) | ETL batches and combined object/content/graph/space/time queries |
| [Implementation plan](docs/implementation-plan.md) | Incremental delivery, dependencies, integration readiness and scope boundaries |
| [Application use cases](docs/application-use-cases.md) | Future games, item tracking, imported prices and external-service boundaries |
| [Development session prompt](docs/development-autonomy-prompt.md) | Owner-supplied implementation instructions, gate correction and completion criteria |
| [Storage and recovery](docs/storage-and-recovery.md) | Transactions, disk artifacts, recovery, and compaction |
| [Security and privacy](docs/security-and-privacy.md) | Threats, authorization, encryption, and deletion |
| [Content ingestion and parsing](docs/content-ingestion-and-parsing.md) | Files, workers, extraction, and citations |
| [Replay and simulation](docs/replay-and-simulation.md) | Replay, branches, models, and determinism |
| [API and integration](docs/api-and-integration.md) | Consumer contract, adapters, and error semantics |
| [Verification and benchmarks](docs/verification-and-benchmarks.md) | Test suites, workloads, and required evidence |
| [Fedora Kinoite development setup](docs/development-setup.md) | Reproduce current R0 experiments and synthetic fixtures |
| [Task list](TASKS.md) | Sequenced implementation work and completion evidence |
| [Security policy](SECURITY.md) | Reporting readiness and supported versions |
| [Contributing](CONTRIBUTING.md) | Development, review, and provenance rules |

The domain specification owns the detailed contract. The PRD owns scope; decisions record
changes. A contradiction blocks the affected work until resolved, rather than allowing an
implementer to choose the most convenient interpretation.

## T-20 Linux runner

The isolated benchmark runner can create, resume and verify the encrypted BM-01 graph mapping on
x86_64 Linux/Btrfs. It is qualification-candidate tooling, not the database CLI or a benchmark
pass. Use an existing Btrfs directory and a password file owned by the current user, with one hard
link, mode `0600` (or stricter), and 1–1024 exact bytes. The file is not newline-trimmed.

~~~text
cargo run --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- \
  linux-create --root ROOT --password-file PASSWORD [--entities 100000]
cargo run --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- \
  linux-resume --root ROOT --password-file PASSWORD [--entities 100000]
cargo run --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- \
  linux-open --root ROOT --password-file PASSWORD [--entities 100000]
cargo run --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- \
  oracle-summary [--entities 100000] > ORACLE
cargo run --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- \
  linux-query --root ROOT --password-file PASSWORD --oracle-file ORACLE [--entities 100000]
~~~

Recovery testing additionally provides `linux-create-crash-probe --pause-after-revision REVISION`.
It deliberately parks after flushing a nonfinal durable frontier so an external harness can
SIGKILL that exact process and then run `linux-resume`; it is not a normal creation command.

Only the default 100,000-entity/1,000,000-relationship shape is a qualification candidate. Reports
currently set `engine_benchmark:false`: materialization/recovery phases validate storage, and the
query phase performs one correctness pass against a separately generated content-free oracle. Its
diagnostic timing is not the required repeated cold/warm benchmark and does not establish
bounded-memory behavior. The exact corpus has 299 successful outputs and 85 expected result-cap
refusals; later latency evidence must keep those populations separate.

## Delivery

1. R0: close foundational decisions and make acceptance vectors executable.
2. R1: correctness kernel with durable encrypted graph/content storage.
3. R2: local developer-alpha readiness with spatial/temporal/content retrieval, ETL, navigation and kinematics.
4. R3: local hardened-beta readiness with rich formats, disk spatial history, constrained contacts and lifecycle hardening.
5. R4: production candidate with independent review and measured operating limits.

There is not yet a general database executable. The Rust workspace, current graph/transaction
libraries, isolated T-20 Linux/Btrfs benchmark runner, R0 experiments and deterministic synthetic
fixture generator are runnable using the
[Fedora Kinoite development setup](docs/development-setup.md).
See [TASKS.md](TASKS.md) for gate status. Local production-format implementation is unblocked;
external executable distribution remains prohibited until private vulnerability reporting is
verified under T-62 and all applicable release requirements pass.

## License

Dual [MIT](LICENSE-MIT) / [Apache-2.0](LICENSE-APACHE), at your option.
Third-party components retain their own terms and require review. No upstream database
source is incorporated by this design change.
