# Decision 0001 — Independent temporal graph and content database

Date: 2026-09-16

Status: product direction accepted by project owner; detailed contracts are drafts pending
the R0 design gate. This is not acceptance of implementation, security, or performance.

Spatial/physics release scope is subsequently expanded by
[Decision 0002](0002-spatial-world-model.md); the durable database boundaries below remain.

## Context

The original concept combined graph and time-series services. Its successor became a
deterministic spatial simulation kernel. The current objective is an independent local
database for agent workloads, including unstructured files and controlled parsing.

The pre-change documents at commit 0f96428 are preserved under
[history/simulation](../history/simulation/README.md), with archival banners and repaired
relative links. The still-earlier service-based schema remains in Git history, including
commit 3372b1e. Neither history defines current requirements.

## Decision

- Build original Rust storage, transaction, temporal-graph, and event/replay components.
- Make original file bytes, versioned artifacts, and evidence first-class data.
- Separate database operations from untrusted parsing and optional model inference.
- Use generic namespaces, principals, assertions, artifacts, and adapters; no downstream
  application identity or policy is embedded in the engine.
- Retain event ordering, explicit durability, canonical serialization, verified snapshots,
  deterministic reducers, and isolated simulation.
- Preserve existing MIT/Apache licensing; review every additional dependency separately.
- Favor safe Rust, with no C/C++ database engine dependency. The scope and exceptions for
  platform/cryptographic/parser dependencies are explicit design decisions, not silent additions.

## Superseded assumptions

- The product is now a database; “not a database” and “no storage engine” no longer apply.
- Real observations and file contents cannot be regenerated from seeds.
- Accepted/staged changes are not durable database commits.
- Client retries require idempotency; unacknowledged operations may have committed.
- Historical replay is bounded by retention; erased history is not promised recoverable.
- Checkpoints may become explicit recovery baselines after validated compaction.
- Simulation clocks are not substitutes for database revision order or real-world valid time.
- GPU analytics, orbital models, and a viewer are not initial database dependencies.

## Alternatives and consequences

Embedding an existing Rust database would reduce storage implementation work, but would not
meet the owner's present goal of an original engine. Splicing upstream engines would add
maintenance and consistency risks. Original implementation is chosen knowingly: durability,
indexing, concurrency, recovery, performance, and security must now be demonstrated here.

“Original” describes implementation intent, not patent novelty or freedom-to-operate.
Conceptual influences must be distinguished from copied source. No security equivalence to
established databases or procurement certification is claimed.

Arbitrary-byte storage is required; universal semantic understanding is not promised.
Parser coverage must be named, tested, versioned, and restricted by the selected security profile.

## Acceptance and changes

R0 resolves the open decisions in [architecture](../../architecture.md). Later changes to
authority, durability, deletion, or dependency policy require a new decision and updated tests.
Implementation tasks cannot be marked complete merely because a design document exists.
