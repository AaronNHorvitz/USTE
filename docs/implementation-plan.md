# Spatial-temporal database implementation plan

Design draft 1.6 · 2026-09-17 · R0 design ready; T-08–T-12 foundation implemented

This is the delivery guide for the [PRD](../PRD.md), not an alternative task authority.
[TASKS](../TASKS.md) owns dependencies and completion evidence. No schedule, working engine
or measured capacity is implied. Decisions 0001 and [0002](decisions/0002-spatial-world-model.md)
replace the archived simulation-first delivery assumptions.

## Product objective and application boundary

Build a native Rust database and physics kernel for queryable worlds and future game
development, agent memory, and other applications such as tracking items linked to asset
prices. The price observations are supplied records, not data the engine must acquire.
T-28/T-54/T-56 deliver headless world and local-file item/price examples under
[application use cases](application-use-cases.md), using existing types and query operators.
No native feed/broker/exchange API client, credential onboarding or live account is required.
Keep local encryption/authentication while demonstrating offline operation without provider
secrets. Decision 0011 separately governs the external executable-distribution gate.

## Implementation increments

| Increment | Tasks | Deliverable and exit demonstration |
|---|---|---|
| R0: freeze development contracts | T-01–07, T-46–47 | Resolve D-01–09 for implementation; record schemas, disk protocol, Rust dependency feasibility, coordinate/time/numeric profiles, threat model, limits, governance and literal test vectors |
| R1: durable foundation | T-08–19, T-45, T-48–49 | Rust workspace, encrypted atomic storage, graph/evidence/raw blobs, UTC types, world/observation schemas and bounded import transactions; crash/replay/reference tests |
| R2a: spatial and temporal reads | T-20–22, T-50–52 | Frame conversions, movement history, geographic/local predicates and coherent state-at-time results with explicit uncertainty |
| R2b: content and query composition | T-23–25, T-53–54 | Isolated baseline parsing, unified typed query operators, byte retrieval, source-linked CSV/JSON batch import with preview/resume |
| R2c: navigation and simulation | T-26–28, T-55–57 | Supplied-graph paths, kinematic branches, local API/CLI, generic adapter and synthetic world demonstration |
| R2 local acceptance | T-29 | Complete local alpha tests including late evidence, query budgets, restart, no-egress and source citations; non-production and non-distribution limitations visible |
| R3: complete baseline and harden | T-30–39, T-58–61 | Rich content, vectors, lifecycle, disk spatial/history indexes, region crossings, bounded 2D contacts, mixed-load and hostile-input evidence |
| R3 local acceptance | T-40 | Full local beta matrix, migrations, backup/restore, deletion and cross-feature correctness at declared bounds; no distribution authorization |
| Distribution readiness | T-62 | Owner/admin enables and harmlessly verifies private vulnerability reporting with reporter and authorized security-triage participation before any executable leaves the authorized development group |
| R4: production decision | T-41–44, T-62 | Independent security/recovery assessment, verified disclosure route, signed packages, dependency/license review, supported-platform trials and honest published limits |

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

`uste-crypto` owns only encrypted envelope admission, derivation, secret ownership, entropy use and
trusted key-adapter contracts. Storage supplies durably published writer incarnation/object IDs and
never resets an exhausted nonce session in place. Rotation, restore and writable-clone admission
remain lifecycle operations rather than crypto-library side effects.

`uste-storage` begins with Decision 0014's host-capability traits and deterministic fault adapter.
The memory durability model keeps file bytes and directory names independently synchronized and
invalidates handles on restart. T-13 adds the reviewed Linux implementation and journal; T-12's
test-only host SIGKILL scenario is not a durability or supported-filesystem claim.

## R0 design deliverables

- D-01/02/04: crash matrix, crypto/key strategy, ownership, deletion epochs and backup rules.
- D-03: named Fedora Kinoite runner, target hardware, exact dataset sizes and numeric budgets
  for p99, memory, sustained ingest, recovery, spatial candidates and simulation contention.
- D-05/07: actual dependency/license/unsafe inventory, parser coverage feasibility, release
  provenance, governance and the selected private-vulnerability-reporting procedure. Operational
  route verification is T-62, not an R0 development prerequisite. No C/C++
  engine/spatial/physics substitute.
- D-06: timestamp codec and source envelopes, schema/migration compatibility and batch retry keys.
- D-08: geographic/local geometry matrix, frames, transforms, numeric error bounds, boundary
  semantics, index candidates, time-slice path cost rules and versioned reference vectors.
- D-09: physics baseline feasibility, chosen arithmetic/integrator, contact ordering and limits,
  simulation-to-UTC mapping, checkpoint compatibility and independent analytic fixtures.

Each decision records alternatives, chosen profile, owner, security implications and measurable
acceptance evidence. Experimental spikes may inform a decision; they do not waive it.

R0 is complete at the decision/evidence scope. This authorizes dependency-ordered local
implementation only. Decision 0011 and T-62 prohibit external executable alpha/beta/candidate/
release distribution until the private reporting route is genuinely enabled and tested.

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
Imported asset-price storage and item linkage are not deferred with those connectors: they
are generic records handled by the planned local ingestion and retrieval paths.
