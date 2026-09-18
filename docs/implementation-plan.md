# Spatial-temporal database implementation plan

Design draft 1.11 · 2026-09-17 · R1 implementation through T-49 complete; T-19 acceptance open

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

T-48's accepted R1 catalog is a bounded in-memory correctness implementation, not the native disk
index or BM-10 capacity result. T-49 owns authorized atomic graph/import reference closure; T-50
turns T-48's opaque transform bindings into validated transform records and algorithms.

Decision 0023 implements T-49 as one capability-free composite reducer over graph and spatial
state. It binds finalized source/mapping blobs, composes authorization, rejects dangling external
references and retains a private restartable job ledger. This is the typed R1 transaction contract;
T-54 still owns CSV/JSON parsing, mapping execution, rejected-row reports and operator tooling.

Task IDs are stable, not execution order. New tasks inserted in earlier gates must complete
before those gates close. No green documentation check marks an implementation row complete.

Decision 0024 corrects one semantic ordering contradiction without weakening acceptance: T-20's
disk-index/cache work must precede T-19 because T-19 requires BM-01/BM-06 results that T-20 owns.
T-19 remains open and still requires all applicable R1 VT/BM results, including the currently
failed BM-04 target. T-29 directly depends on T-19, so the reorder cannot bypass R1 acceptance.

Decision 0025 fixes T-20's initial encrypted immutable-run/root and bounded-page-cache profile plus
a current graph projection. Consumer current-graph reads now traverse that projection through the
mandatory authorization facade. This is still not task closure: streaming larger-than-memory
checkpoint/state construction and qualifying BM-01/BM-06 runs are mandatory. T-35 retains
compaction, authoritative baselines and garbage
collection, so an optional derived root never becomes a second commit authority.

Decision 0026 adds borrow-aware replay/checkpoint access, one-new-payload-chunk checkpoint
publication, visitor-based index scans and the exact `bm01-materialization-v1` fixture/oracle
contract. Its content-free
manifest explicitly is not engine benchmark evidence. Opaque checkpoint candidates can now be
authenticated and streamed without a complete transport buffer, but reducer decoding and
Decision 0027 replaces graph prepared snapshots with ordered before/after deltas, changed-record
validation and incremental derived-index publication. Decision 0028 adds the general in-memory
reverse dependencies and removes deletion's full-record scan. Decision 0034 later removes the
remaining ordinary spatial/composite ingest candidate clones with request-sized overlays and
preflighted component deltas. Reducer decoding and current maps remain full-memory. T-20 next
requires new versioned state profiles
that persist those reverse references plus a genuine
state-larger-than-memory BM-06 path before running the qualifying BM-01/BM-06 protocols.

Decision 0029 freezes the complete `graph-state-v1` family/key/value contract and publishes fully
scrubbed certificate-bound roots from the current snapshot. This is format/root groundwork, not a
disk-backed reducer: construction and semantic admission still traverse full in-memory state and no
root recovery reader exists yet.

Decision 0030 supplies that bounded root reader and semantic reconstruction candidate. It removes
the contiguous checkpoint payload from this path but still constructs a complete in-memory
`GraphState` and reads candidates twice. Decision 0031 pairs it with a versioned coordinator
metadata root through a temporary authenticated recovery owner; seeded open rechecks the prefix and
replays the suffix. Decision 0032 adds the bounded authenticated storage-level base/delta scratch
merge without changing `index-v1`. Decision 0033 connects exact bounded graph changes to all eight
terminal families through an opaque base/outcome-bound plan, independently compares the merged
descriptors with the postcommit graph and publishes one complete root. It still scans and retains
the full in-memory reducer. Decision 0034 bounds ordinary composite write preparation without
changing that live-state boundary. T-20 next needs a live disk-backed base/overlay state, explicit-
I/O streaming semantic validation and qualifying scale evidence. Decision 0035 supplies the first
explicit-I/O current-record proof/preparation slice, including exact negative proofs and a pure
post-load phase. Decision 0036 adds complete bounded reverse/history proof buckets. Live persistent
overlays and benchmarks remain. Decision 0037 authenticates the base metadata/counts in that proof
and derives the existing bounded terminal-root plan from the partial result without complete-
snapshot access. Decision 0038 lets that proof-backed result enter the authoritative coordinator
only after reducer verification binds exact request bytes, target revision and current base. Live
publication, postcommit validation and recovery remain full-memory.

Decision 0039 adds the measurement seam needed before connecting the fixture: the authorized
coordinator exposes cumulative cache/page/fragment/result-byte work and explicit page zeroization
only to a currently `ManageSchema`-authorized operator with an issuer-bound root capability. The
telemetry is cardinality-sensitive. Clearing establishes only an empty USTE userspace cache;
qualifying reports must separately describe process and host storage-cache conditions. It is
instrumentation, not BM-01 evidence.

Decision 0040 performs the first connection at a hard-capped development scale. It durably maps,
accepts, restarts and queries the production graph/index, then requires exact equality with the
independent oracle for all measured query shapes. The remaining qualifying driver must replace the
memory/test-key environment with Linux/Btrfs and portable recovery, batch the exact 100k/1m profile,
separate oracle generation, and collect the accepted repeated latency/RSS/environment evidence.

Decision 0041 completes the exact-profile transaction batching portion: construction streams at
most 10,000 operations at a time and the shared Evidence record makes the qualifying plan exactly
212 durable revisions. Linux/Btrfs execution, portable recovery, independent oracle generation and
the accepted repeated measurements remain before BM-01 can pass.

Decision 0042 supplies Linux/Btrfs creation, deterministic resume and authenticated open with OS
entropy and portable recovery. A 20/200 real-filesystem smoke preserves revision 4 across reopen and
resume. Decision 0043 adds a separately generated content-free oracle summary and a one-pass Linux
correctness phase with exact output/typed-limit matching, RSS and authenticated index counters.
Decision 0044 adds real process-loss/resume coverage at every incomplete 20/200 materialization
phase; exact-scale interruption and BM-06 remain unclaimed.
Decision 0045 adds a bounded separately generated oracle bundle for the 96 warm-up and 384 measured
queries. Decision 0046 adds repeated authorized-query sampling that excludes expected refusals
from success percentiles, fixes five minimum-60-second samples at exact scale and pairs empty with
retained USTE-cache executions. Decision 0047 adds the external 30-second worker supervisor while
preserving retained-cache state. Next run the exact campaign under the accepted host reservation
and remove the full-memory reducer/recovery
boundary; accepted host/RSS/cache evidence remains mandatory.

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
invalidates handles on restart. Decision 0015's T-13 slice adds the reviewed Linux implementation,
exclusive ownership, encrypted opaque groups, fixed commit certificates and streaming fail-closed
recovery. Btrfs and ext4 process-loss evidence does not establish controller-cache or power-loss
behavior.

Decision 0016 layers `uste-txn` over that journal. Domain reducers consume bounded canonical bytes,
produce an owned prepared change and result digest without mutating live state; the coordinator
durably binds request, principal-scoped idempotency and transaction outcomes before publication.
Decision 0017 adds bounded encrypted blob chunks and canonical inventories under the same owner;
blob bytes and inventory are durable and verified before their transaction certificate publishes.
Its T-15 qualification injects every modeled publication failure class, rejects authenticated
malformed/replayed objects, caps live upload buffers per database and measures a 12 GiB encrypted
restart/round-trip at bounded RSS.
Decision 0018 adds the storage-independent `uste-policy` kernel and `uste-txn` authorized facade.
Trusted authentication supplies the principal; consumer commit requests cannot spoof it. Every
implemented revision-view/outcome/blob/commit path authorizes before existence access, version
changes invalidate active handles, and exact committed-byte ownership rebuilds across restart.
Because uncommitted reservations are not enumerable yet, reopened coordinators deny new upload
starts while allowing evidenced-token reconciliation; T-35 owns removing that limitation. Decision
0019/T-17 adds durable graph policy records, complete pre-state record requirements and
reference-safe authorized graph projections. Its indexes remain in-memory until T-20.

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

Decision 0020 completes T-18 with capability-free cold replay, canonical reducer/coordinator
checkpoints and alternating encrypted cache slots bound to exact journal certificates. Recovery
authenticates the journal before accepting an opaque candidate, verifies prefix coordinator state
and replays the reducer suffix; damaged or unsupported candidates fall back to the other slot or
cold replay. The current double-open handoff and 256 MiB in-memory limit are explicit correctness
constraints pending T-20/BM-06 rather than capacity claims.

Decision 0021 completes T-45's R1 kernel with the safe-Rust `uste-time` crate, strict explicit
normalization, embedded/hash-verified TZDB 2026c rules, bounded canonical source envelopes and
replay-stable graph values. It does not pull timezone code into `uste-types`, consult host zone
configuration or implement the T-21 temporal index/T-24 adapter layers.

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
