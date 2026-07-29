# USTE — Product Requirements Document

| | |
|---|---|
| **Product** | USTE — Universal Spatial-Temporal Engine |
| **Version** | Draft v0.1 |
| **Author** | Aaron N. Horvitz |
| **Date** | 2026-07-29 |
| **Status** | For review — design complete, implementation not started |
| **License** | Dual MIT / Apache-2.0 |
| **Companion documents** | [README.md](./README.md) (shape and lineage) · [architecture.md](./architecture.md) (normative contracts) |

This PRD restates the design as **numbered, testable requirements** and defines what "done" means for each milestone. Where this document and `architecture.md` conflict, `architecture.md` is normative and this document has a bug.

---

## 1. Summary

USTE is a deterministic, multiscale, event-sourced simulation kernel written in Rust. It represents an enormous simulated space with compact rules and computes only what matters at the present moment, under one invariant:

```
state(t) = f(world_format_version, numerical_profile, seed, rules, ordered_event_log)
```

The product is the **kernel itself**: a set of Rust crates, their documented contracts, and the test harnesses that prove the contracts hold. USTE is domain-agnostic by design — it does not know what its entities mean. Anything domain-specific lives above it and is out of scope for this document.

## 2. Product definition

### 2.1 Deliverables

| Deliverable | Form |
|---|---|
| Kernel crates | Cargo workspace: `uste-core`, `uste-time`, `uste-gen`, `uste-orbits`, `uste-sim`, `uste-log` |
| Contracts | `architecture.md`, kept normative and current |
| Proof of contracts | Property-test suites, golden replay fixtures, crash-injection suite |
| Performance evidence | Criterion benchmark suite with recorded results |
| Reference binary | A minimal CLI driving Milestone 0/1 scenarios (create world, run, crash, recover, replay, hash) |

### 2.2 Consumers

1. **The kernel developer** — needs every contract enforceable by a test, so regressions are caught by CI rather than by users.
2. **Downstream application developers** — embed the kernel; need stable, documented APIs, a versioned world format, and the reproducibility guarantees stated per numerical profile.
3. **Auditors/reviewers** — need to verify any world's history independently: manifest + log in, bit-identical state hash out, on commodity hardware.

### 2.3 Explicit non-goals (v1)

- No storage engine on the frame path; no database dependencies.
- No rendering in the authoritative loop; no renderer deliverable.
- No physics library ambitions — existing integrators/engines are dependencies where used.
- No application semantics of any kind.
- No networking, no multi-process authority, no distributed consensus.

---

## 3. Functional requirements

Requirement IDs are stable and cited by tests. Each requirement cites its normative source.

### FR-1 — Determinism and the invariant *(architecture §1)*

- **FR-1.1** Given identical `(world_format_version, numerical_profile, seed, rules, ordered_event_log)`, the kernel SHALL produce bit-identical state at every tick.
- **FR-1.2** The baseline numerical profile SHALL guarantee per-binary determinism; a `portable` profile guaranteeing cross-platform bit-equality MAY be provided later and SHALL be separately named and versioned.
- **FR-1.3** The numerical profile SHALL bind: integrator selections and step sizes, floating-point mode (FMA policy, no reassociation), math-library versions, target triple, allowed CPU features, dependency lock hash, and canonical serialization version.
- **FR-1.4** State iterated during simulation SHALL never use randomized iteration order (no default `HashMap` iteration over state).
- **FR-1.5** Parallel stages SHALL be pure maps with deterministic merges or deterministically scheduled; results SHALL be identical across thread counts.
- **FR-1.6** Hierarchical PRNG streams SHALL be keyed by address path; regenerating any node SHALL NOT require or disturb its siblings.

### FR-2 — Addressing and procedural generation *(architecture §2, §3)*

- **FR-2.1** Every body SHALL have a versioned hierarchical address; `(seed, rules, address)` SHALL determine its stable generated properties.
- **FR-2.2** Generation SHALL be lazy: querying a body SHALL NOT require generating unrelated regions.
- **FR-2.3** The generator SHALL be closed under natural consequences: it SHALL NOT emit a state whose own baseline implies an un-reflected past encounter. v0 satisfies this by generating baseline-stable systems by construction; deterministic rejection/adjustment SHALL itself be a pure function of `(seed, rules, address)`.
- **FR-2.4** Natural encounters implied by `(seed, rules)` SHALL be treated as regenerable baseline history and SHALL NOT be logged.

### FR-3 — Canonical models *(architecture §2)*

- **FR-3.1** A body's model-segment history SHALL be a pure function of `(seed, rules, address, ordered_event_log)`.
- **FR-3.2** The first segment SHALL be assigned at generation; every later segment boundary SHALL be a committed event.
- **FR-3.3** Supported baseline tiers: (1) conic, (2) precessing conic with secular rates, (3) generated ephemeris table produced deterministically at generation. Milestone 0 SHALL implement tier 1 only.
- **FR-3.4** Every force SHALL be classified canonical or presentational; no third category SHALL exist.
- **FR-3.5** Coupled model-segment updates SHALL be committed atomically in a single event group.

### FR-4 — Layered simulation and sleep/wake *(architecture §3, §4)*

- **FR-4.1** The kernel SHALL implement the five layers: procedural, analytical, active, contact, historical. (Contact-layer physics is Milestone ≥ 2; the layer boundary SHALL exist from Milestone 0.)
- **FR-4.2** Wake SHALL initialize numerical state as a pure function of `(model, tick)`; no integrator state survives dormancy.
- **FR-4.3** Sleep with no committed interaction SHALL discard the deviation and write nothing.
- **FR-4.4** Sleep after a committed interaction SHALL write one deviation event carrying: address, epoch tick, cause, epoch state vector, reference-model identifier, and the model-specific invariant audit.
- **FR-4.5** The two-body invariant audit SHALL verify energy and angular momentum of the fitted conic against the numerical state at epoch within profile tolerance, recording the residual in the event.
- **FR-4.6** Transitions SHALL be governed by hysteresis: minimum dwell ticks and wake radius strictly inside sleep radius; thrash rate SHALL be a monitored metric.

### FR-5 — Observer independence *(architecture §3)*

- **FR-5.1** Two runs differing only in observation (wake/sleep schedules with no committed interactions) SHALL produce bit-identical canonical histories.
- **FR-5.2** The active layer SHALL integrate deviations relative to the canonical baseline (Encke), never absolute trajectories.
- **FR-5.3** Canonical event predicates SHALL evaluate over canonical state only; presentational residuals SHALL be invisible to them.
- **FR-5.4** Causal waking SHALL be observer-independent and scoped to the deviation frontier: a deterministic encounter queue populated at commit time plus a spatial broad phase over deviated bodies. Untouched, ungenerated space SHALL never be scanned.
- **FR-5.5** Observers SHALL participate only through presence — a committed canonical fact. The interaction significance threshold ε SHALL be part of `rules`.

### FR-6 — Events *(architecture §5)*

- **FR-6.1** Event lifecycle SHALL be pending → accepted → durable. **Commit** SHALL mean the pending → accepted transition; simulation state, encounter queues, and model segments SHALL advance at acceptance.
- **FR-6.2** Total order SHALL be `(tick, region, sequence)`; two logs with the same events in different order are different histories.
- **FR-6.3** The event group SHALL be the unit of acceptance and of the log; replay SHALL observe all of a group or none of it.
- **FR-6.4** Cross-region events SHALL be scheduled onto master ticks, never applied mid-step.

### FR-7 — Persistence and recovery *(architecture §9)*

- **FR-7.1** Storage SHALL comprise exactly four artifacts: world manifest, append-only event log, snapshots, and the dual-slot durable-frontier record.
- **FR-7.2** World creation SHALL be durable-or-absent via atomic directory publication: build in a temp sibling, fsync files and directory, atomic rename, fsync parent, then report success. Existence at the final name SHALL mean creation completed.
- **FR-7.3** The manifest SHALL be immutable and checksummed, carrying `world_format_version`, the full numerical profile, `seed`, and `rules`.
- **FR-7.4** Group records SHALL be length-prefixed, footer-terminated, and checksummed as a unit, in canonical serialization.
- **FR-7.5** The durable frontier SHALL be persisted dual-slot, each slot independently checksummed, highest valid slot winning; a torn frontier write SHALL be survivable.
- **FR-7.6** Durability ordering SHALL be: fsync log through `F` → write and fsync frontier `F` → external acknowledgment. Never earlier.
- **FR-7.7** Recovery SHALL be strict rollback: truncate at `F` unconditionally. Malformation at or before `F`, or loss of both frontier slots, SHALL be a hard recovery error — the world refuses to open; repair is an explicit operator action.
- **FR-7.8** Snapshots SHALL be derived, not captured: produced by replaying durable artifacts through endpoint `E` outside the live process. Snapshot state SHALL equal `f(manifest, log ≤ E)` bit-for-bit. A live COW capture MAY replace this later only if its state hash matches the replay-derived hash for the same `E`.
- **FR-7.9** Snapshot endpoints SHALL be recorded as both group sequence number and log byte offset on a verified group boundary; eligibility SHALL be `E ≤ F` at publication time; recovery SHALL select the newest snapshot with `E ≤ F` and SHALL decline and flag any snapshot claiming coverage beyond `F`.
- **FR-7.10** Log-writer backpressure SHALL slow the simulation rather than drop events.

### FR-8 — Time *(architecture §6)*

- **FR-8.1** Simulation time SHALL be `u64` ticks with checked arithmetic; overflow SHALL be a deterministic panic.
- **FR-8.2** The v0 quantum SHALL be 1/3600 s; all region rates SHALL be integer divisors of the master clock. Changing the quantum SHALL bump `world_format_version`.
- **FR-8.3** Epoch SHALL be tick 0 at world creation; the kernel SHALL carry no external calendar.

### FR-9 — Space, units, frames *(architecture §7)*

- **FR-9.1** Baseline positions SHALL be `f64`, frame-local, SI meters, with frame extents bounded (≤ ~10⁹ m) to keep resolution sub-micrometer in-frame.
- **FR-9.2** The portable profile SHALL use `i64` fixed-point micrometers.
- **FR-9.3** Quantities SHALL use dimensional newtypes; unit errors SHALL be compile errors.
- **FR-9.4** Authoritative state SHALL be frame-local and observer-independent; floating-origin recentering SHALL exist only in render projection, per client.
- **FR-9.5** Frame transitions SHALL occur at defined boundaries with hysteresis; transforms SHALL be deterministic functions of the canonical models involved.

### FR-10 — Canonical serialization *(architecture §8)*

- **FR-10.1** Serialization SHALL be little-endian, field-by-field defined; in-memory layout and padding SHALL never touch the wire or the hash.
- **FR-10.2** NaN SHALL be forbidden in canonical state; producing one SHALL be a checked, deterministic failure.
- **FR-10.3** State hashes SHALL be computed over canonical bytes only.
- **FR-10.4** Serialization rules SHALL carry their own version, referenced by the numerical profile.

---

## 4. Non-functional requirements

- **NFR-1 (Budgets, not claims).** Performance figures are budgets until Criterion evidence exists: analytical propagation sub-microsecond per body; active region milliseconds for thousands of bodies; log writes off the frame path. Each budget SHALL have a benchmark; benchmarks SHALL run in CI with recorded baselines.
- **NFR-2 (Reproducibility).** The replay guarantee SHALL hold across runs and across thread counts on the same binary/profile. Cross-platform equality is out of scope until the portable profile exists.
- **NFR-3 (Crash safety).** The crash-injection matrix (§6.3 below) SHALL pass at every group-record boundary, against the frontier record (torn write, single-slot corruption, dual-slot loss), and against planted ineligible snapshots.
- **NFR-4 (Versioning discipline).** Any change to state schema, addresses, events, quantum, or serialization SHALL bump `world_format_version`. Any change to integrators, FP behavior, or dependencies affecting arithmetic SHALL produce a new numerical profile.
- **NFR-5 (Licensing).** All dependencies SHALL be compatible with dual MIT/Apache-2.0 distribution; a CI license check SHALL enforce this.
- **NFR-6 (Single-machine scope).** All v1 targets assume one commodity desktop; nothing in the design may *require* more.

---

## 5. Milestones

### Milestone 0 — the replay kernel

Scope: `uste-core`, `uste-time`, `uste-log`, `uste-orbits` (tier-1 only), minimal `uste-sim`, and the reference CLI. **No galaxy generation. No rendering.**

Build order *(architecture §10)*: workspace → addresses/seeds → time and event ordering → serialization, replay, hashing (with crash-injection) → two-body propagation → one numerical integrator with the full §4 reconciliation contract → property tests.

**Exit criteria (all required):**

1. **The replay test.** A trivial world simulated through a long interval with mixed sleep/wake transitions and ≥ 1 committed deviation replays cold from `(manifest, log)` to a bit-identical state hash — across runs and across thread counts. *(FR-1.1, FR-1.5)*
2. **The observation test.** Two runs differing only in wake/sleep schedule, with no committed interactions, produce bit-identical canonical histories. *(FR-5.1)*
3. **Crash-injection matrix green.** Every case in NFR-3, including durable-or-absent creation (kill at every step of the publication sequence). *(FR-7.x)*
4. **Snapshot equivalence.** Derived snapshot at every tested `E` hashes identically to direct replay to `E`; planted beyond-`F` snapshot is declined and flagged. *(FR-7.8, FR-7.9)*
5. **Reconciliation audit.** Two-body commit events carry correct epoch state vectors and invariant residuals within tolerance. *(FR-4.4, FR-4.5)*
6. Golden fixtures committed under `tests/fixtures/`; CI runs the full suite plus license check.

### Milestone 1 — the galaxy demonstration

Scope: `uste-gen` (hierarchical galaxy/system/body generation, baseline-stable by construction), tier-1 assignments at generation, fidelity transitions at scale.

**Exit criteria:**

1. A stable galaxy generates from one seed; any address regenerates identically and lazily. *(FR-2.1, FR-2.2)*
2. A selected system runs the full procedural → analytical → active cycle; leaving and revisiting reproduces identical properties; unvisited systems are provably untouched (state hash of their regenerated baseline unchanged). *(FR-4.x, FR-5.1)*
3. One committed modification persists while the untouched remainder stores nothing. *(FR-2.4, FR-7.x)*
4. Criterion evidence recorded against every NFR-1 budget; numerical drift vs. the analytical baseline measured and published.

### Beyond M1 (listed, not committed)

Tier-2/3 canonical models and their invariant audits · contact-layer physics integration · portable numerical profile · COW snapshotter (gated on FR-7.8's equivalence test) · rendering consumer · multi-observer support.

---

## 6. Test strategy

- **6.1 Property tests** — every FR with a SHALL is cited by at least one test; the invariant (FR-1.1) is exercised by randomized schedules of wake/sleep/commit under fixed seeds.
- **6.2 Golden fixtures** — small committed `(manifest, log, expected-hash)` triples under `tests/fixtures/`; any hash change is a reviewed, deliberate `world_format_version` or profile event, never a drive-by.
- **6.3 Crash injection** — kill/truncate/corrupt at: every group-record boundary; mid-group; each frontier slot; both slots; each step of creation's publication sequence; snapshot temp files; planted ineligible snapshots.
- **6.4 Benchmarks** — Criterion, in CI, with stored baselines; regressions fail the build.
- **6.5 Concurrency** — the full replay suite runs at 1, 2, N threads and compares hashes.

---

## 7. Risks

| Risk | Exposure | Mitigation |
|---|---|---|
| Floating-point drift across dependency/toolchain upgrades silently breaks replay | High — it's the core promise | Profile binds lock hash and toolchain; golden fixtures catch any change; upgrades are deliberate profile bumps |
| Kepler solver edge cases (near-parabolic, hyperbolic, high-eccentricity) | Medium | Property-test the solver across eccentricity sweep; define supported domain in tier-1 model spec |
| Hysteresis tuning produces wake thrash or stale physics | Medium | Thrash rate is a monitored metric from M0; dwell/radius are `rules` parameters, not constants |
| Scope creep toward renderer/applications before M0 exits | High — historical pattern | PRD non-goals; milestone gates; the replay test as the only M0 definition of done |
| Single-maintainer bandwidth | High | Milestones sized to evenings; M0 has no research unknowns — every contract is already specified |

---

## 8. Open questions (deferred deliberately)

1. Tier-2 secular-rate model: which perturbation theory, and its supported domain.
2. Tier-3 table format, interpolation scheme, and regeneration cost bounds.
3. Contact-layer engine choice and its determinism story (candidate: Rapier's deterministic mode) — Milestone ≥ 2.
4. ECS or hand-rolled stores for `uste-sim` hot loops — decide at M0 implementation, benchmark-driven.
5. Portable profile arithmetic: fixed-point everywhere vs. strict-float subset.
6. State-hash algorithm selection (speed vs. collision comfort) — decide at M0, recorded in serialization version.

---

## 9. Change log

| Version | Date | Change |
|---|---|---|
| 0.1 | 2026-07-29 | Initial PRD: requirements FR-1…FR-10, NFR-1…6, milestones M0/M1 with exit criteria, test strategy, risks, open questions. Derived from README.md and architecture.md after five external design-review rounds converged. |
