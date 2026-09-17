# Decision 0002 — Spatial-temporal world model and Rust simulation

Date: 2026-09-16

Status: owner-requested scope update; implementation and detailed design gates remain open.
Amends Decision 0001 and design draft 1.1; does not restore the archived simulation contracts.

## Decision

The product is a Rust-native spatial-temporal graph and content database with a deterministic
simulation kernel. A world contains identifiable items, relationships, optional locations and
movement histories, and attached original/derived content. Nonphysical knowledge remains valid.

Spatial lookup, bounded graph navigation, geographic and local coordinate frames, historical
movement queries, unified retrieval, batch import, and a constrained physics baseline are
release obligations, not indefinite extensions. Their exact release allocation is in the PRD.
The engine returns records and authorized source bytes, not only model-generated answers.

The strict engine profile requires Rust implementations of storage, graph, spatial, query,
and simulation components and their algorithmic dependencies. C/C++ engines hidden behind
bindings are excluded. OS/standard-library boundaries remain explicit; dependency and unsafe
audits are still necessary. Optional external parser/model profiles cannot be advertised as
the strict engine or silently become required dependencies.

## Boundaries preserved

- Real observations cannot be regenerated from a seed. Imported files retain original bytes.
- Observed, interpolated and simulated state remain distinct through queries and exports.
- Durable commit revision, UTC world time and branch ticks have different meanings.
- Physics writes branch results through controlled transactions, never directly into storage.
- Querying or viewing a world does not advance its authoritative physics or alter observations.
- Identity is independent of location; an object changing position remains the same object.
- Authorization, deletion, original-engine ownership and existing licensing remain intact.
- Storage/retrieval work without a model, physics worker, renderer or external service.

The old celestial-specific force models, sparse infinite procedural universe assumptions,
accepted-before-durable publication and proposed dual-slot recovery algorithm are not revived.
No universal physics, photorealistic game engine, real-world autonomous navigation or
high-frequency trading execution is promised.

## Consequences and implementation

Add D-08 (space/navigation) and D-09 (physics/numerics), FR-27 through FR-34, VT-18 through
VT-23 and BM-10 through BM-13. Preserve all earlier IDs and append tasks T-46 through T-61.
The [implementation plan](../implementation-plan.md) sequences this work with the existing
database foundation. Scope additions are substantial; no completion date or performance claim
follows from this document. The R0 decisions must resolve dependency feasibility and numeric
budgets before their affected implementations can pass acceptance.
