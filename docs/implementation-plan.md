# Spatial-temporal database implementation plan

Design draft 1.2 · 2026-09-16 · All implementation remains open

This is the delivery guide for the [PRD](../PRD.md), not an alternative task authority.
[TASKS](../TASKS.md) owns dependencies and completion evidence. No schedule, working engine
or measured capacity is implied. Decisions 0001 and [0002](decisions/0002-spatial-world-model.md)
replace the archived simulation-first delivery assumptions.

## Implementation increments

| Increment | Tasks | Deliverable and exit demonstration |
|---|---|---|
| R0: freeze contracts | T-01–07, T-46–47 | Resolve D-01–09; record schemas, disk protocol, Rust dependency feasibility, coordinate/time/numeric profiles, threat model, limits and literal test vectors |
| R1: durable foundation | T-08–19, T-45, T-48–49 | Rust workspace, encrypted atomic storage, graph/evidence/raw blobs, UTC types, world/observation schemas and bounded import transactions; crash/replay/reference tests |
| R2a: spatial and temporal reads | T-20–22, T-50–52 | Frame conversions, movement history, geographic/local predicates and coherent state-at-time results with explicit uncertainty |
| R2b: content and query composition | T-23–25, T-53–54 | Isolated baseline parsing, unified typed query operators, byte retrieval, source-linked CSV/JSON batch import with preview/resume |
| R2c: navigation and simulation | T-26–28, T-55–57 | Supplied-graph paths, kinematic branches, local API/CLI, generic adapter and synthetic world demonstration |
| R2 acceptance | T-29 | Complete alpha tests including late evidence, query budgets, restart, no-egress and source citations; non-production limitations visible |
| R3: complete baseline and harden | T-30–39, T-58–61 | Rich content, vectors, lifecycle, disk spatial/history indexes, region crossings, bounded 2D contacts, mixed-load and hostile-input evidence |
| R3 acceptance | T-40 | Full beta matrix, migrations, backup/restore, deletion and cross-feature correctness at declared bounds |
| R4: production decision | T-41–44 | Independent security/recovery assessment, signed packages, dependency/license review, supported-platform trials and honest published limits |

Task IDs are stable, not execution order. New tasks inserted in earlier gates must complete
before those gates close. No green documentation check marks an implementation row complete.

## Architectural decomposition

Use separate Rust crates/modules for stable types, storage, transaction coordination, policy,
graph, content, spatial geometry/indexes, movement history, query planning, replay and physics.
All writes converge on one transaction authority. Geometry/temporal/text indexes are derived
from revisioned records; they are not independent write authorities. Physics has no storage,
filesystem, credential or network handle. Its supervisor submits bounded branch transactions.

Implement simple reference models before optimized indexes. Start spatial correctness with
bounded scans, then add the D-08-selected native disk index without changing query semantics.
Do not claim speed until optimized paths pass the same reference fixtures and BM workloads.
Hot object lookup, temporal history, blob reads and graph traversal need distinct access paths.

## R0 design deliverables

- D-01/02/04: crash matrix, crypto/key strategy, ownership, deletion epochs and backup rules.
- D-03: named Fedora Kinoite runner, target hardware, exact dataset sizes and numeric budgets
  for p99, memory, sustained ingest, recovery, spatial candidates and simulation contention.
- D-05/07: actual dependency/license/unsafe inventory, parser coverage feasibility, release
  provenance and private vulnerability reporting. No C/C++ engine/spatial/physics substitute.
- D-06: timestamp codec and source envelopes, schema/migration compatibility and batch retry keys.
- D-08: geographic/local geometry matrix, frames, transforms, numeric error bounds, boundary
  semantics, index candidates, time-slice path cost rules and versioned reference vectors.
- D-09: physics baseline feasibility, chosen arithmetic/integrator, contact ordering and limits,
  simulation-to-UTC mapping, checkpoint compatibility and independent analytic fixtures.

Each decision records alternatives, chosen profile, owner, security implications and measurable
acceptance evidence. Experimental spikes may inform a decision; they do not waive it.

## Test-first work packages

For each task: pin fixtures and failure cases; implement the reference behavior; implement the
production path; compare results; inject cancellation, quota, permission and crash failures;
measure relevant workloads; document limitations; attach reviewed evidence before checking off.
Preserve exact source/tree, toolchain, dependency versions and test commands in the result.

Required end-to-end slices are raw binary round-trip; source-backed temporal assertion;
moving object with attached document; combined location/content/relationship query; navigation
under constraints; isolated kinematic/contact simulation; batch resume; and revocation/purge
across every derived index, trajectory and branch. Each includes restart and negative cases.

## Integration readiness, not automatic cutover

A consumer may begin an experimental rebuildable index adapter once R2 contracts pass, while
its existing memory remains authoritative. Authoritative storage replacement requires R3
migration/lifecycle tests and that consumer's own approval/security gates; production claims
also require R4. A pinned version, feature negotiation, bounded queries, namespace mapping,
idempotent change delivery and explicit reconciliation are mandatory.

The consumer owns memory admission, prompt budgets, source permissions and tool execution.
USTE cannot approve its own facts or execute stored procedures. Physics/geography are optional
per workload and must not slow or block a nonspatial memory request by requiring simulation.
No downstream application identity is embedded in this repository.

## Deferred capabilities

An interactive 3D renderer, infinite procedural universe, global GIS projection catalog,
general 3D physics, robotics, trading execution, distributed consensus, textual query language,
automatic web/exchange connectors and Python SDK require separate scope decisions. They are
not substitutes for the explicit spatial/physics/ingest baseline required above.
