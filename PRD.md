# the engine — Product Requirements Document

| | |
|---|---|
| **Product** | the engine — Universal Spatial-Temporal Engine |
| **Version** | Draft v0.8 |
| **Author** | Aaron N. Horvitz |
| **Date** | 2026-07-30 |
| **Status** | For review — design complete, implementation not started |
| **License** | Dual MIT / Apache-2.0 |
| **Companion documents** | [README.md](./README.md) (shape and lineage) · [architecture.md](./architecture.md) (normative contracts) |

This PRD restates the design as **numbered, testable requirements** and defines what "done" means for each milestone. Where this document and `architecture.md` conflict, `architecture.md` is normative and this document has a bug.

### Versioning vocabulary

The word "version" is overloaded; these terms are distinct and never interchangeable:

| Term | Means | Example |
|---|---|---|
| Document version | Revision of a design document only | this PRD, Draft v0.2 |
| `world_format_version` | On-disk schema of state, addresses, events; part of the invariant | starts at `1` |
| Numerical profile | Named, versioned numerics/execution bundle; part of the invariant | `baseline-1`, `portable-1` |
| "v0 decision" | An initial normative choice in `architecture.md`, revisable by explicit commit until the first release tag | quantum = 1/3600 s |
| Milestone (M0, M1, …) | Engineering gate with exit criteria; not a release | M0 = replay kernel |
| Release version | Crate semver; `1.0.0` requires M0 and M1 exits green | — |
| AS version | Version of Appendix A's acceptance/benchmark vectors | `AS-v0` |

---

## 1. Summary

The engine is a deterministic, multiscale, event-sourced simulation kernel written in Rust. It represents an enormous simulated space with compact rules and computes only what matters at the present moment, under one invariant:

```
state(t) = f(world_format_version, numerical_profile, seed, rules, ordered_event_log)
```

The product is the **kernel itself**: a set of Rust crates, their documented contracts, and the test harnesses that prove the contracts hold.

**Scope honesty:** the engine's *core contracts* are application-agnostic — `uste-core`, `uste-time`, and `uste-log` know entities, baselines, deviations, events, and replay, and nothing celestial. But v1 ships exactly one baseline-model family, and it is celestial: the tiered orbital models in `uste-orbits` (FR-3). The engine v1 is therefore accurately described as an **application-agnostic simulation core with celestial mechanics as its first and only baseline-model family** — genericity is enforced at the crate boundary (model families implement core traits), not claimed as a v1 deliverable. Application semantics above the model families remain out of scope entirely.

## 2. Product definition

### 2.1 Deliverables

| Deliverable | Form |
|---|---|
| Kernel crates | Cargo workspace: `uste-core`, `uste-time`, `uste-gen`, `uste-orbits`, `uste-sim`, `uste-log` |
| Contracts | `architecture.md`, kept normative and current |
| Proof of contracts | Property-test suites, golden replay fixtures, crash-injection suite |
| Performance evidence | Criterion benchmark suite with recorded results |
| Reference binary | A minimal CLI driving Milestone 0/1 scenarios (create world, run, crash, recover, replay, hash, scrub) |
| Demo viewer | `uste-view` (Bevy + `bevy_egui`, dual MIT/Apache): read-only 3D consumer — bodies, orbit conics, trajectory display, zoom, fly camera, and a minimal HUD (labels, time control, selection panel). Outside the kernel workspace's gates; Milestone V. |

### 2.2 Consumers

1. **The kernel developer** — needs every contract enforceable by a test, so regressions are caught by CI rather than by users.
2. **Downstream application developers** — embed the kernel; need stable, documented APIs, a versioned world format, and the reproducibility guarantees stated per numerical profile.
3. **Auditors/reviewers** — need to verify any world's history independently: manifest + log in, bit-identical state hash out. Under the baseline profile this requires **the exact binary the profile identifies by build hash** (or a bit-identical rebuild from the pinned toolchain, flags, features, and lockfile); hardware-independent verification on any commodity machine is a *portable-profile* capability, not a baseline one.

### 2.3 Explicit non-goals (v1)

- No storage engine on the frame path; no database dependencies.
- No rendering in the authoritative loop — absolute and permanent. (A read-only **demo viewer** is a deliverable as of v0.5 — Milestone V — but it consumes the public read API only, lives outside the kernel's gates, and can never block or alter a kernel milestone.)
- No physics library ambitions — existing integrators/engines are dependencies where used.
- No application semantics of any kind.
- No networking, no multi-process authority, no distributed consensus.

---

## 3. Functional requirements

Requirement IDs are stable and cited by tests. Each requirement cites its normative source. Requirements tagged **[Post-1.0]** are normative for their feature when it is built, but are not required for release 1.0.0 — which requires exactly the M0 and M1 exit criteria.

### FR-1 — Determinism and the invariant *(architecture §1)*

- **FR-1.1** Given identical `(world_format_version, numerical_profile, seed, rules, ordered_event_log)`, the kernel SHALL produce bit-identical state at every tick.
- **FR-1.2** The baseline numerical profile SHALL guarantee per-binary determinism; a `portable` profile guaranteeing cross-platform bit-equality MAY be provided later and SHALL be separately named and versioned.
- **FR-1.3** The numerical profile SHALL bind: integrator selections and step sizes, floating-point mode (FMA policy, no reassociation), math-library versions, target triple, allowed CPU features, **compiler version, build flags, enabled Cargo features,** dependency lock hash, canonical serialization version, **and the build hash of the released binary it identifies**.
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
- **FR-3.3** The celestial model family defines three baseline tiers: (1) conic, (2) precessing conic with secular rates, (3) generated ephemeris table produced deterministically at generation. Release 1.0.0 SHALL implement tier 1 only; tiers 2 and 3 are **[Post-1.0]**.
- **FR-3.4** Every force SHALL be classified canonical or presentational; no third category SHALL exist.
- **FR-3.5** Coupled model-segment updates SHALL be committed atomically in a single event group.

### FR-4 — Layered simulation and sleep/wake *(architecture §3, §4)*

- **FR-4.1a** The five layer *boundaries* — procedural, analytical, active, contact, historical — SHALL exist as interfaces from Milestone 0, so no later milestone changes the kernel's shape.
- **FR-4.1b** Layer *implementations* are milestone-scoped: analytical + active (two-body) and historical in M0; procedural generation in M1; contact physics in M2+. A boundary whose implementation is absent SHALL fail explicitly, never silently no-op.
- **FR-4.2** Wake SHALL initialize numerical state as a pure function of `(model, tick)`; no integrator state survives dormancy.
- **FR-4.3** Sleep with no committed interaction SHALL discard the deviation and write nothing.
- **FR-4.4** Sleep after a committed interaction SHALL write **one atomic event group containing one deviation member per affected body**, each member carrying: address, epoch tick, cause, epoch state vector, reference-model identifier, and the model-specific invariant audit. A single-body commit is a group of size one (FR-6.3).
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
- **FR-7.10** Snapshots SHALL record the canonical state hash of their contents. Recovery SHALL recompute and compare before use; on mismatch the snapshot SHALL be declined and flagged, and recovery SHALL fall back to an older snapshot or genesis replay. The **scrub audit** SHALL exist as an explicit product surface — a library operation and `uste scrub` in the reference CLI — re-deriving published snapshots, comparing hashes, emitting a machine-readable report, and exiting nonzero on any mismatch. Scheduling cadence belongs to the consumer; M0's CI SHALL run scrub as part of the snapshot suite (TV-E).
- **FR-7.11** Log-writer backpressure SHALL slow the simulation rather than drop events (BENCH-C2).

### FR-8 — Time *(architecture §6)*

- **FR-8.1** Simulation time SHALL be `u64` ticks with checked arithmetic; overflow SHALL be a deterministic panic.
- **FR-8.2** The v0 quantum SHALL be 1/3600 s; all region rates SHALL be integer divisors of the master clock. Changing the quantum SHALL bump `world_format_version`.
- **FR-8.3** Epoch SHALL be tick 0 at world creation; the kernel SHALL carry no external calendar.

### FR-9 — Space, units, frames *(architecture §7)*

- **FR-9.1** Baseline positions SHALL be `f64`, frame-local, SI meters, with frame extents bounded (≤ ~10⁹ m) to keep resolution sub-micrometer in-frame.
- **FR-9.2 [Post-1.0]** When the portable profile is introduced, it SHALL use `i64` fixed-point micrometers.
- **FR-9.3** Quantities SHALL use dimensional newtypes; unit errors SHALL be compile errors.
- **FR-9.4** Authoritative state SHALL be frame-local and observer-independent; floating-origin recentering SHALL exist only in render projection, per client.
- **FR-9.5** Frame transitions SHALL occur at defined boundaries with hysteresis; transforms SHALL be deterministic functions of the canonical models involved.

### FR-10 — Canonical serialization *(architecture §8)*

- **FR-10.1** Serialization SHALL be little-endian, field-by-field defined; in-memory layout and padding SHALL never touch the wire or the hash.
- **FR-10.2** NaN SHALL be forbidden in canonical state; producing one SHALL be a checked, deterministic failure.
- **FR-10.3** State hashes SHALL be computed over canonical bytes only.
- **FR-10.4** Serialization rules SHALL carry their own version, referenced by the numerical profile.

### FR-11 — Public read API *(M0 deliverable)*

- **FR-11.1** The kernel SHALL expose an immutable read-only view API in `uste-core`, sufficient for external consumers without private access: world identity (manifest data) and current tick; the frame tree; body enumeration by address; **a display label per body** (the canonically rendered address, plus an optional deterministic generated name where the generator provides one — M1); each body's current model segment (tier, elements, epoch); canonical state evaluation `state_at(address, tick)`; active-region status with any current deviation δ, marked presentational; and committed-event metadata (tick, cause) for a body's segment boundaries.
- **FR-11.2** The view API SHALL offer no mutation path, enforced structurally by the borrow system (shared references / snapshot views), not by convention.
- **FR-11.3** The reference CLI SHALL include an `inspect` command exercising every FR-11.1 query, so the API is proven sufficient before any viewer exists — the viewer consumes an API that already has a test, rather than designing one by accident.
- **FR-11.4 (Coherent snapshots.)** Every consumer frame receives one immutable snapshot view at a single tick; all queries within that view answer from the same state — no torn reads.
- **FR-11.5 (Visibility.)** A view attached to a live kernel sees **accepted** state (commit advances simulation; durability is a persistence property), with the durable frontier available as an explicit query. A view opened **offline from durable artifacts** — V1's mode — sees exactly durable history.
- **FR-11.6 (`state_at` domain.)** Valid ticks are `[0, latest tick of the view]` — for offline views, the end of durable history. Out-of-range is a deterministic checked error, never a panic. Within range, canonical state is always well-defined from `(manifest, log)`.
- **FR-11.7 (Playback.)** The viewer's time control is an **offline replay clock** over recorded history, bounded by the durable log. Pause, speed, and scrub manipulate the viewer's clock only; nothing they do reaches the kernel.

---

## 4. Non-functional requirements

- **NFR-1 (Budgets, not claims).** Every performance budget is defined as an exact, gateable scenario in **Appendix A (AS-v0)** — entity counts, timestep counts, thread counts, statistic, and pass/fail threshold on the pinned runner: BENCH-A (analytical), BENCH-B (active region), BENCH-C/C2 (log path and backpressure), BENCH-M (memory footprint and scale-independence), BENCH-V (viewer graphics, V2-gating only). Prose figures anywhere in these documents are informal restatements of those vectors. **Benchmarks gate the build only on pinned CI hardware** — a dedicated runner with a fixed CPU model, pinned frequency governor, and recorded machine identity in the baseline. On shared or unpinned runners benchmarks run informationally and SHALL NOT gate, because a performance gate on noisy hardware is a flakiness generator, not a guarantee.
- **NFR-2 (Reproducibility).** The replay guarantee SHALL hold across runs and across thread counts on the same binary/profile. Cross-platform equality is out of scope until the portable profile exists.
- **NFR-3 (Crash safety).** The crash-injection matrix (§6.3, TV-CRASH) SHALL pass at every group-record boundary, against the frontier record (torn write, single-slot corruption, dual-slot loss), against planted ineligible snapshots (`E > F`), **and against a planted snapshot with legal `E ≤ F` but incorrect contents** — which SHALL be declined via state-hash mismatch with recovery falling back cleanly.
- **NFR-4 (Versioning discipline).** Any change to state schema, addresses, events, quantum, or serialization SHALL bump `world_format_version`. Any change to integrators, FP behavior, or dependencies affecting arithmetic SHALL produce a new numerical profile.
- **NFR-5 (Licensing).** All dependencies SHALL be compatible with dual MIT/Apache-2.0 distribution; a CI license check SHALL enforce this.
- **NFR-6 (Single-machine scope).** All v1 targets assume one commodity desktop; nothing in the design may *require* more.

---

## 5. Milestones

### Milestone 0 — the replay kernel

Scope: `uste-core`, `uste-time`, `uste-log`, `uste-orbits` (tier-1 only), minimal `uste-sim`, and the reference CLI. **No galaxy generation. No rendering.**

Build order *(architecture §10)*: workspace → addresses/seeds → time and event ordering → serialization, replay, hashing (with crash-injection) → two-body propagation → one numerical integrator with the full §4 reconciliation contract → property tests.

**Exit criteria (all required):**

1. **The replay test — TV-REPLAY (Appendix A).** The specified world (64 two-body systems, 1,000,000 ticks, seeded wake/sleep schedule, committed deviations at fixed ticks) replays cold from `(manifest, log)` to a bit-identical state hash — across runs and across thread counts {1, 2, 8}. *(FR-1.1, FR-1.5)*
2. **The observation test — TV-OBS (Appendix A).** Two runs differing only in wake/sleep schedule, with no committed interactions, produce bit-identical canonical histories at every checkpoint. *(FR-5.1)*
3. **Crash-injection matrix green — TV-CRASH (Appendix A).** Every enumerated case lands in its specified outcome, including durable-or-absent creation (kill at every step of the publication sequence). *(FR-7.x)*
4. **Snapshot equivalence — TV-E (Appendix A).** Derived snapshots at every endpoint in the TV-E matrix hash identically to direct replay; the planted beyond-`F` and legal-`E`/wrong-contents snapshots are declined and flagged with clean fallback. *(FR-7.8, FR-7.9, FR-7.10)*
5. **Reconciliation audit — TV-KEPLER (Appendix A).** Two-body commit events carry correct epoch state vectors, with relative energy and angular-momentum residuals ≤ 1e-9 across the AS-v0 eccentricity sweep. *(FR-4.4, FR-4.5)*
6. Golden fixtures committed under `tests/fixtures/`; CI runs the full suite plus license check.
7. **Read-API proof.** The FR-11 view API exists and `uste inspect` exercises every FR-11.1 query against the TV-REPLAY world. *(FR-11)*

### Milestone 1 — the galaxy demonstration

Scope: `uste-gen` (hierarchical galaxy/system/body generation, baseline-stable by construction), tier-1 assignments at generation, fidelity transitions at scale.

**Exit criteria:**

1. **TV-GEN (Appendix A) passes in full**: generation at the specified scale, the laziness check, regeneration identity across fresh processes, and the sparse-storage check. *(FR-2.1, FR-2.2, FR-2.4, FR-7.x)*
2. A selected system runs the full procedural → analytical → active cycle; leaving and revisiting reproduces identical properties; unvisited systems are provably untouched (TV-GEN regeneration-identity check). *(FR-4.x, FR-5.1)*
3. Numerical drift of a woken system against its tier-1 baseline stays within the TV-GEN drift threshold. *(FR-5.2)*
4. Criterion evidence recorded against every Appendix A benchmark vector (BENCH-A/B/C/C2/M) on the pinned runner.

### Milestone V — the demo viewer *(incremental: V1 gated on M0, V2 gated on M1)*

Scope: `uste-view` (Bevy + `bevy_egui`), a read-only 3D consumer. Not part of the kernel workspace; viewer defects never gate kernel milestones. Purpose: demonstration, plus three engineering proofs — the FR-11 read API suffices for a real consumer, the render-side floating origin works as specified, and the canonical/presentational split is visible.

**V1 — the basic viewer (after M0; this is the quick MVP):**

1. **Read-only proof.** The viewer consumes only the FR-11 API; the kernel workspace builds, tests, and gates with the viewer crate absent. *(FR-9.4, FR-11)*
2. **Renders the TV-REPLAY world** — an M0 artifact, so V1 has no M1 dependency: bodies at analytical positions; orbit curves **derived directly from canonical elements with bounded screen-space error ≤ 1 pixel** (provenance from the math is required; tessellation strategy is the implementer's).
3. **Fly camera.** Free flight plus focus-on-body, frame-rate-independent controls.
4. **Minimal HUD** (`bevy_egui`, read-only): body labels per FR-11.1 (canonical address, generated name when the world provides one); time control — pause, speed multiplier, scrub, current-tick readout — as the FR-11.7 offline replay clock, bounded by recorded history; selection panel showing a clicked body's elements and model tier, its **latest committed segment boundary** (tick, cause) when one exists, and, separately labeled, any **current presentational deviation** — which is by definition uncommitted and is never conflated with committed history. The HUD displays kernel truth; it never mutates it.

**V2 — scale and deviation (after M1):**

5. **TV-GEN system view.**
6. **Scale traversal.** Continuous zoom ≥ 6 orders of magnitude via render-side floating-origin recentering only; authoritative state untouched and observer-independent throughout. *(FR-9.4)*
7. **Dual-trajectory rendering.** A deviated body draws its canonical baseline and its deviated trajectory as two visually distinct curves — the Encke split, on screen.
8. **Performance — BENCH-V (Appendix A).** Viewer-gating only, on the pinned runner, HUD visible.

### Beyond M1 and V (listed, not committed)

Tier-2/3 canonical models and their invariant audits · contact-layer physics integration · portable numerical profile · COW snapshotter (gated on FR-7.8's equivalence test) · multi-observer support.

---

## 6. Test strategy

- **6.1 Property tests** — every FR with a SHALL is cited by at least one test; the invariant (FR-1.1) is exercised by randomized schedules of wake/sleep/commit under fixed seeds.
- **6.2 Golden fixtures** — small committed `(manifest, log, expected-hash)` triples under `tests/fixtures/`; any hash change is a reviewed, deliberate `world_format_version` or profile event, never a drive-by.
- **6.3 Crash injection** — kill/truncate/corrupt at: every group-record boundary; mid-group; each frontier slot; both slots; each step of creation's publication sequence; snapshot temp files; planted ineligible snapshots (`E > F`); planted snapshots with legal `E ≤ F` but contents failing their recorded state hash.
- **6.4 Benchmarks** — Criterion with stored baselines. Gating runs execute only on the pinned runner (fixed CPU, pinned governor, machine identity recorded alongside the baseline); regressions there fail the build. Runs on any other hardware are informational only.
- **6.5 Concurrency** — the full replay suite runs at 1, 2, N threads and compares hashes.

---

## 7. Risks

| Risk | Exposure | Mitigation |
|---|---|---|
| Floating-point drift across dependency/toolchain upgrades silently breaks replay | High — it's the core promise | Profile binds lock hash and toolchain; golden fixtures catch any change; upgrades are deliberate profile bumps |
| Kepler solver edge cases (near-parabolic, hyperbolic, high-eccentricity) | Medium | Property-test the solver across eccentricity sweep; define supported domain in tier-1 model spec |
| Hysteresis tuning produces wake thrash or stale physics | Medium | Thrash rate is a monitored metric from M0; dwell/radius are `rules` parameters, not constants |
| Scope creep toward renderer/applications before M0 exits | High — historical pattern | The viewer is now *sanctioned but caged*: Milestone V is read-only, gated on M0 completion, outside the kernel workspace, and can never block a kernel gate. Everything else remains a non-goal. |
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

## 9. Appendix A — Acceptance and benchmark vectors (AS-v0)

This appendix is the objective content behind every qualitative phrase in the gates. It is versioned independently (**AS-v0**); any change is an explicit commit bumping the AS version — a failing gate is revised by commit, never by silently editing the threshold. Gates cite these IDs.

**Vector files.** Each row is completed by a canonical, committed vector file set under `tests/vectors/<id>/` containing its full literal inputs — seeds, rules and profile identities, orbital elements, schedule algorithms, state grids, payload definitions. This table records each file set's SHA-256 once committed (M0 build-order step 7 for TV-REPLAY/OBS/E/KEPLER/CRASH and the BENCH vectors; M1 for TV-GEN). A directory has no portable hash, so **the file-set hash is defined canonically**: SHA-256 over a manifest of UTF-8 lines `<hex sha256 of file> <relative path>`, paths forward-slashed, sorted bytewise by path, LF-terminated. **Until its hash is recorded here, a vector is DRAFT and cannot gate.** The parameters below are normative constraints on the files' construction; the files are normative for everything else.

### Test vectors

| ID | Scenario (literal constraints) | Pass condition | Vector hash |
|---|---|---|---|
| **TV-REPLAY** | Seed `0x5EED0001`; profile `baseline-1`; rules `rules-tv1`. 64 tier-1 two-body systems, elements enumerated in the vector file (e ≤ 0.9, a ∈ [10⁷, 10⁹] m). 1,000,000 ticks (≈ 277.8 s at quantum 1/3600 s). Wake/sleep schedule generated by the hierarchical PRNG stream keyed `("tv-replay-sched", system_address)`, algorithm defined in the vector file. Committed deviations at ticks 250,000 / 500,000 / 750,000 touching 1, 2, 3 bodies. | Cold replay from `(manifest, log)` yields a bit-identical state hash across runs and thread counts {1, 2, 8} | DRAFT |
| **TV-OBS** | The TV-REPLAY world; two runs: schedule A (no wakes) vs. schedule B (every system woken/slept 10× at ticks from stream `("tv-obs-sched", system_address)`); zero committed interactions | Bit-identical canonical state hashes at every 100,000-tick checkpoint | DRAFT |
| **TV-E** | Snapshot endpoints over the TV-REPLAY log: E ∈ {genesis, first group, ⌊n/2⌋ group, last durable group = F}; two adversarial plants: `E > F`, and legal `E` with wrong contents | Each derived snapshot's hash equals the direct-replay-to-E hash; both plants declined and flagged; recovery falls back cleanly | DRAFT |
| **TV-KEPLER** | State grid: e ∈ {0, 0.1, 0.5, 0.9, 0.99} × mean anomaly M ∈ {0°, 45°, 90°, 135°, 180°, 225°, 270°, 315°} × a ∈ {10⁷ m, 10⁹ m} = 80 cases, enumerated in the vector file. M0 domain is elliptical 0 ≤ e ≤ 0.99; out-of-domain input is a checked error. | Kepler-equation residual ≤ 1e-12 rad on all 80 cases; reconciliation-audit residuals (relative energy and angular momentum at epoch) ≤ 1e-9 | DRAFT |
| **TV-CRASH** | The §6.3 injection matrix, enumerated per artifact and per publication step in the vector file | Every case lands in its specified outcome — tail-discard, hard error, or decline-and-flag; no third outcome observed | DRAFT |
| **TV-GEN** *(M1)* | Seed `0x5EED0002`; profile `baseline-1`; rules `rules-tv1`. Galaxy of 2²⁰ addressable systems; K = 1,000 addresses sampled by the stream `("tv-gen-sample")`. | (a) **Laziness:** generating the samples performs zero generation work outside the sampled subtrees (instrumented generation counter). (b) **Regeneration identity:** each sampled address regenerated in a fresh process yields identical canonical bytes. (c) **Sparse storage:** after exactly one committed modification, on-disk artifacts are exactly manifest + log (one event group) + frontier (+ eligible snapshots), log < 4 KiB. (d) **Drift:** a woken sampled system with no committed interaction tracks its tier-1 baseline within relative position deviation ≤ 1e-9 over 10,000 ticks. | DRAFT |

### Benchmark vectors (gating only on the pinned runner)

| ID | Scenario (literal constraints) | Statistic | Threshold | Vector hash |
|---|---|---|---|---|
| **BENCH-A** analytical | 100,000 tier-1 bodies (elements from the vector file, seed `0x5EEDB001`) evaluated at 100 distinct ticks (10⁷ element→state evaluations), single thread | mean per evaluation; p95 per-tick batch vs. median batch | mean ≤ 1.0 µs; p95 ≤ 1.5 × median | DRAFT |
| **BENCH-B** active | 5,000 active bodies as **100 independent systems × 50 bodies**, Encke deviations integrated against each system's tier-1 primary baseline, no cross-system coupling; 10,000 steps at 60 Hz region rate, 8 threads | p95 and p99.9 step wall time | p95 ≤ 8 ms; p99.9 ≤ 16 ms | DRAFT |
| **BENCH-C** log | 50,000 events/s sustained 60 s; 256-byte member payloads, groups of size one; writer batches ≤ 4 MiB or ≤ 10 ms, one fsync per batch | p99 frame-path enqueue stall; event loss | stall ≤ 50 µs; loss = 0 | DRAFT |
| **BENCH-C2** backpressure | BENCH-C load with the writer throttled to 10 MB/s | steady-state behavior within 5 s of throttle onset | accepted **byte** rate equals drained **byte** rate ± 5% (expected events/s derived from the canonical encoded group size, recorded in the vector file); accepted-but-not-durable backlog remains ≤ its configured bound thereafter — no unbounded growth; loss = 0 (FR-7.11) | DRAFT |
| **BENCH-M** memory | TV-GEN world (2²⁰ systems) opened idle; the same world with 100 systems woken (BENCH-B topology, 5,000 active bodies); control: an otherwise identical 2¹⁰-system world opened idle. Measured via instrumented global allocator plus OS-reported peak RSS at defined points. | peak RSS; allocator live bytes | idle open ≤ 128 MiB **and within 10% of the 2¹⁰ control** — memory must not scale with apparent universe size; active scenario ≤ 512 MiB | DRAFT |
| **BENCH-V** viewer *(V2 gate)* | TV-GEN system scene at 1920×1080, vsync off, wgpu Vulkan backend, HUD visible; seeded 60 s camera path defined in the vector file (system sweep → dive to a moon → selection with panel open) | median and p5 fps over the path | median ≥ 60; p5 ≥ 30 | DRAFT |

Thresholds are **initial calibration targets** — chosen to be falsifiable, not certified achievable; a miss triggers an explicit AS revision with rationale. The pinned runner's machine identity — CPU model, frequency governor, memory configuration, **filesystem, and storage device** — is recorded alongside every baseline; BENCH-V baselines additionally record GPU model, driver version, render backend, resolution, and vsync policy. Changing any of it is an AS-version event.

## 10. Change log

| Version | Date | Change |
|---|---|---|
| 0.1 | 2026-07-29 | Initial PRD: requirements FR-1…FR-10, NFR-1…6, milestones M0/M1 with exit criteria, test strategy, risks, open questions. Derived from README.md and architecture.md after five external design-review rounds converged. |
| 0.2 | 2026-07-30 | Versioning vocabulary added. FR-7.10/7.11 reordered: snapshot content-hash + scrub surface (`uste scrub`) is FR-7.10, backpressure FR-7.11. All qualitative gates bound to Appendix A (AS-v0) exact vectors and thresholds; NFR-1 defined by BENCH IDs. README slogan reversal fixed (canonical by derivation, not by storage) and status lines reconciled — tracked here for traceability. |
| 0.8 | 2026-07-30 | Architecture build order updated to match V1/V2 (read API as final M0 step; V1 after M0; V2 after M1) — the normative document no longer contradicts the PRD it governs. FR-11.4–11.7 define coherent per-frame snapshots, accepted-vs-durable visibility with offline views seeing durable history, the `state_at` domain with checked out-of-range errors, and playback as an offline replay clock. FR-11.1 adds display labels; the HUD separates latest committed segment boundary from current presentational deviation. Benchmark table repaired (orphaned BENCH-M/V rows rejoined); NFR-1 lists BENCH-V. |
| 0.7 | 2026-07-30 | Viewer review applied: Milestone V split into V1 (after M0, views the TV-REPLAY world — removing the TV-GEN/M1 contradiction) and V2 (after M1: TV-GEN view, six-order zoom, dual-trajectory, performance). FR-11 public read API added as an M0 deliverable with `uste inspect` proof and a new M0 exit criterion. "Exact conics" relaxed to element-derived curves with ≤ 1 px screen-space error. BENCH-V graphics acceptance vector added (resolution, backend, vsync, seeded camera path) with GPU/driver identity recorded in baselines. |
| 0.6 | 2026-07-30 | Milestone V gains exit criterion 6: minimal HUD via `bevy_egui` — body labels, time control (pause / speed / tick readout), and a selection panel showing elements, model tier, and deviation status. Performance criterion measured with HUD visible. |
| 0.5 | 2026-07-30 | Milestone V added: `uste-view` demo viewer (Bevy, read-only, gated on M0, parallel to M1, never gating the kernel) with five exit criteria including the Encke-split visualization and a ≥ 6-orders-of-magnitude floating-origin zoom. Renderer non-goal amended accordingly; scope-creep mitigation updated. |
| 0.4 | 2026-07-30 | BENCH-M added: memory footprint with a scale-independence gate (2²⁰-system world within 10% of a 2¹⁰ control at idle) plus active-scenario limit. BENCH-C2 units reconciled to byte rates with events/s derived from canonical group size, and a bounded-backlog requirement added. File-set SHA-256 defined canonically over a sorted path/hash manifest. |
| 0.3 | 2026-07-30 | Appendix A vectors made reproducible: committed vector files under `tests/vectors/` with SHA-256 recorded per row, DRAFT-cannot-gate rule, literal seeds and profile/rules identities, TV-KEPLER state grid, BENCH-B force topology, BENCH-C payload/batching/fsync policy, BENCH-C2 measurable slowdown condition, runner identity extended to filesystem and storage. Release scope reconciled: [Post-1.0] tag introduced; FR-3.3 tiers 2–3 and FR-9.2 portable profile marked Post-1.0. TV-GEN added and M1 exits bound to it. Risk-table wording: the M0 exit criteria, plural, are the definition of done. |
