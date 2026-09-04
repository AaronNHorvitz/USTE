# USTE: Universal Spatial-Temporal Engine

The whole universe is a seed, a rulebook, and a clock. USTE is the kernel that turns them into worlds: a deterministic, multiscale, event-sourced simulation engine in Rust that computes only what matters at this moment and stores only what deviates.

USTE represents an enormous simulated space with compact rules, and computes only what matters at the present moment. It is not a database and not a rendering engine. It is the kernel underneath both: the part that decides what exists, what changes, and how any past state can be reconstructed exactly.

```
Universe = seed + rules + simulation time + sparse deviations
```

Nothing that can be regenerated needs to be **stored** — regenerable state is canonical by *derivation*, not by storage. Given a hierarchical address and a seed, the engine reproduces a body's stable properties on demand, identically, every time. The truth on disk is a small immutable **world manifest** (format version, numerical profile, seed, rules) plus the deviation log — modifications, exceptional events; checkpoints merely *cache* regenerable state to bound recovery time, and can always be discarded and rebuilt.

---

## Status

**Design complete; implementation has not started.** Earlier revisions of this repository described a GPU-accelerated graph database built on TensorFlow, TimescaleDB, and ArangoDB. That framing has been retired: it described a storage layer, not an engine, and it put databases on the simulation's critical path. The old design documents have been removed so they cannot be mistaken for the build plan; they remain in git history. The design below, together with [architecture.md](./architecture.md) (normative contracts) and [PRD.md](./PRD.md) (numbered requirements, milestones, and exit criteria), replaces them.

---

## Lineage

The core idea is not new. It is the founding trick of a specific generation of software, and modern hardware makes it dramatically more capable rather than less relevant.

**Elite (1984)** fit eight galaxies of 256 star systems each — names, positions, economies, prices — into a machine with 32 KB of RAM. The galaxy was never stored. It was a pure function of a seed, regenerated identically on every visit. Systems existed because the arithmetic said so.

**Starflight (1986)** carried that idea to hundreds of explorable planets with fractal terrain, streamed from floppy disks, on hardware that could not possibly have held them.

**Gravitar (1982)** and **Thrust (1986)** did something different and equally instructive: they made real physics the substance of play. Gravity wells, orbital insertion, momentum, tethered mass — none of it decorative. On hardware measured in kilobytes, physics was cheaper than content, so physics *became* the content.

The shared insight, stated plainly: **determinism is compression.** Store the cause, not the effect. A world that can be recomputed exactly does not need to be remembered.

### Why now is different

The BBC Micro ran at roughly one million instructions per second with 32 KB of memory. A current desktop CPU executes on the order of a hundred billion instructions per second, and its L3 cache alone exceeds that machine's entire memory by a factor of a thousand.

The same philosophy, given roughly a million-fold increase in budget, does not merely produce a bigger version of Elite. It allows the budget to be spent on *fidelity where the observer is standing* rather than on scope — real numerical integration in the active region, real terrain under an avatar, real ephemeris-grade celestial mechanics — while everything beyond remains implied by the same compact arithmetic that served in 1984.

### Why Rust

The discipline those programs required — cache-conscious data layout, no wasted byte, predictable timing — has a modern name: data-oriented design. Rust is the language where that discipline is idiomatic rather than heroic.

- **Controllable memory layout.** Contiguous arrays, `#[repr(C)]` where layout matters, and structure-of-arrays as a deliberate design choice. Rust does not produce cache-friendly layout automatically — no language does — but it makes the layout you design the layout you get.
- **No garbage collector.** Not a throughput argument — a *determinism* argument. A fixed-timestep simulation cannot tolerate unpredictable pauses in its loop.
- **Data-race freedom at compile time.** This guarantees the absence of a class of nondeterminism bugs; it does not guarantee deterministic scheduling or reduction order. Those are supplied by policy (below), and Rust's ownership model is what makes the policy enforceable rather than aspirational.
- **Control over arithmetic.** No fast-math by default, explicit SIMD, no silent reassociation — which matters enormously when bit-reproducibility is a project requirement rather than a nicety.
- **Zero-cost abstraction.** Layered, readable structure that compiles down to the tight loops the 1980s wrote by hand.

---

## Architecture

Deeper contracts — sleep/wake reconciliation, numerical profiles, durability semantics, implementation order — live in [architecture.md](./architecture.md). This section is the shape of the system.

### The invariant

The engine guarantees exactly one thing, and every test is written against it:

```
state(t) = f(world_format_version, numerical_profile, seed, rules, ordered_event_log)
```

Bit-reproducible under a fixed *numerical profile* — a named, versioned bundle of integrator choices, floating-point mode, and math-library versions. Per-binary determinism is the baseline profile; cross-platform bit-equality is a separate, stricter profile that must be purchased deliberately (fixed-point or strict-arithmetic discipline in the authoritative layer), not assumed.

### Layered simulation

Fidelity is a function of relevance, not of distance alone. Five layers, each cheaper than the one above it. Costs shown are **budgets, not measurements** — they become claims only when Criterion benchmarks back them.

| Layer | Role | Budget |
|---|---|---|
| **Procedural** | Generates galaxies, systems, and bodies deterministically from hierarchical address and seed | On-demand; cached; near-free |
| **Analytical** | Dormant bodies advance by closed-form propagation (Keplerian orbits; iterative Kepler solves and transcendentals included) | Sub-microsecond per body, evaluated lazily |
| **Active** | Nearby bodies use numerical integration and interact physically | Milliseconds for thousands of bodies |
| **Contact** | Collisions, vehicles, terrain, immediate phenomena — higher frequency | Bounded by bubble radius, not world size |
| **Historical** | Append-only event log recording every committed deviation from the generated baseline | Off the frame path entirely |

Objects sleep when they cannot affect anything. On revisit, the engine either advances them analytically or processes only the events that mattered while they were dormant. **The apparent universe is enormous; the actively simulated universe stays small.** That is the entire reason this is achievable on one machine.

### Observer independence: fidelity is presentation, deviations require causation

Switching a system between analytical and numerical physics must not, by itself, change the universe — otherwise two observers visiting different systems would fork physical history merely by looking.

USTE resolves this with two rules:

1. **Every body's canonical force model is assigned at generation** — a pure function of `(seed, rules, address)`, never of activation or observation. Baselines come in tiers (pure conic; precessing conic with secular rates; deterministic ephemeris table generated with the system), so the *dominant* physics is canonical rather than an artifact of who is watching. Every force is classified canonical or presentational; there is no third category.
2. **The active layer integrates deviations *relative to* that canonical baseline** (Encke's method, rather than absolute integration). Awake behavior is baseline-plus-deviation; the deviation holds only sub-threshold residuals and genuine interactions. If no interaction exceeds the rules-defined significance threshold, the deviation is discarded on sleep and the canonical trajectory was never perturbed — observation leaves no fingerprints. Only a committed interaction forks a body's canonical trajectory, by exactly the recorded delta. **Interactions commit state changes; model changes occur only as committed segment boundaries** — a body's model history is itself a pure function of `(seed, rules, address, event_log)`.

Three corollaries complete the rule. **Canonical events are detected from canonical state** — presentational residuals are invisible to event predicates, so richer active physics can never reveal-and-commit an interaction the canonical model does not predict. **Natural encounters implied by the seed are baseline history, not events** — regenerated on demand like everything procedural, never logged; the log records exactly what the seed cannot imply, which is what keeps it sparse at universe scale by definition. And **causal waking is observer-independent and scoped to the deviation frontier**: only committed deviations can produce futures the seed does not imply, so the kernel's encounter detection watches deviated bodies and their neighborhoods — ungenerated, untouched space is never scanned — and wakes them whether or not anyone is watching. Observers participate through *presence* — a committed canonical fact — never through observation itself.

Replay reproduces committed deviations; observation alone produces none. The force classification, tiered canonical models with piecewise model segments, the sleep/wake reconciliation contract, and the event lifecycle (pending → accepted → durable, with external permanence requiring durability) are specified in [architecture.md](./architecture.md).

### Determinism policy

Reproducibility is the project's central claim, so the known hazards are policy, not afterthought:

- **No randomized iteration order.** Rust's `HashMap` randomizes iteration per process. Ordered or seeded-hash containers only, anywhere state is iterated.
- **Order-independent parallelism.** Parallel stages must be pure maps with deterministic merges, or deterministically scheduled. Reduction order is not permitted to vary between runs or thread counts.
- **Numerical profiles, not vibes.** Every build declares its profile; the invariant is scoped to it.
- **Integer time.** Simulation time is integer ticks. Never accumulated floating-point seconds.
- **Hierarchical seed derivation.** PRNG streams are keyed by address path, so any node in the hierarchy regenerates independently without touching its siblings.
- **Symplectic integrators** in the orbital layer, where long-horizon energy drift is the quantity being controlled.

### Multi-rate time

The contact region, the active region, and the analytical layer advance at different frequencies, and the procedural layer is timeless. Coordinating those clocks reproducibly is a first-class subsystem: an integer master clock, nested fixed timesteps, and a total ordering of events across regions.

### Coordinate frames

Human-scale precision and astronomical-scale extent cannot coexist in one flat coordinate system; single-precision error at astronomical distance is catastrophic. USTE uses nested reference frames with 64-bit positions.

**Authoritative state lives in stable, frame-local coordinates and never moves for anyone's convenience.** Floating-origin recentering is strictly a rendering-side projection, applied per client — with multiple observers, each render context recenters independently and none of it touches simulation state. Rounding must never depend on viewpoint.

### CPU versus GPU

The authoritative simulation is **CPU-resident**. This is a design conclusion, not a limitation:

- Dynamics are small-N sequential ODE integration — a CPU-shaped workload. Where the CPU/GPU crossover sits depends on force complexity and hardware, and is a benchmark question, not a slogan.
- Event scheduling, sparse graphs, branching, and shifting workloads are CPU-shaped problems.
- Reproducibility is materially easier to guarantee on CPU.
- Compact layout, SIMD, and cache-conscious design do more here than raw arithmetic throughput.

The GPU is retained for what it is actually built for: **rendering**, particles, atmospheres, fields, and — when batch sizes justify it — large sets of independent orbital evaluations and other embarrassingly parallel numerics.

### Persistence

Storage is four artifacts: an immutable **world manifest** carrying the invariant's non-log inputs; an append-only **event log**; periodic **snapshots**; and a small dual-slot **durable-frontier record** naming the acknowledged byte offset. **World creation is durable-or-absent** — the world is built in a temporary directory, fsynced, and atomically renamed into place before creation reports success, so existence at the final name *means* creation completed and every legal world has a valid frontier from birth. In steady state, the log and snapshots are written **asynchronously, off the critical path**. Manifest and log are truth about the world; snapshots are cache; the frontier is truth about *durability* — it says nothing about the world, only how much of the log is acknowledged, and recovery semantics are undefined without it. Durability is contractual, not incidental — group-framed, checksummed, schema-versioned records; an independently persisted durable frontier; atomic checkpoint publication; an explicit flush policy defining the maximum crash-loss window; recovery by replaying the log suffix over the last durable checkpoint, with corruption of acknowledged history a hard error rather than a silent truncation. Details in [architecture.md](./architecture.md). Databases are a later concern, appropriate for historical analysis and exploration tooling rather than for participating in a simulation frame.

---

## Milestones

**Milestone 0 — the replay kernel.** Before any galaxy exists: hierarchical addresses and seed derivation, integer time and canonical event ordering, deterministic serialization, state hashing, a two-body analytical propagator, one numerical integrator with the full sleep/wake reconciliation contract, and property tests across replays, thread counts, and transition schedules. The exit test:

> Simulate a trivial world through a long interval with mixed sleep/wake transitions and at least one committed deviation. Hash the state. Replay cold from `(world_format, numerical_profile, seed, rules, event_log)`. The hashes must be bit-identical — across runs, and across thread counts.

A tiny world that survives replay perfectly proves more of USTE's thesis than a billion generated stars. **This milestone precedes the universe generator on purpose.**

**Milestone 1 — the galaxy demonstration.**

1. Generate a stable galaxy from a single seed.
2. Select a generated star system.
3. Numerically simulate its primary/planet/satellite system.
4. Move seamlessly between procedural, analytical, and active fidelity levels.
5. Leave the system and return; regenerated properties must be identical, and unvisited systems must be provably untouched.
6. Persist one committed modification without storing the untouched remainder.
7. Benchmark per PRD Appendix A (BENCH-A/B/C/C2/M): throughput, memory footprint, and numerical drift against the analytical baseline.

---

**Milestone V — the demo viewer** *(incremental: V1 after M0, V2 after M1)*: `uste-view`, a read-only 3D consumer. **V1, the quick MVP:** the TV-REPLAY world rendered from canonical elements (≤ 1 px screen-space error), a fly camera, and a minimal HUD — body labels, a time control with tick readout, and a selection panel showing a body's elements and deviation status. **V2:** the TV-GEN system view, continuous zoom across six orders of magnitude on the render-side floating origin, and dual-trajectory rendering — a deviated body shows its canonical baseline and actual path as two distinct curves, making the kernel's central design decision visible on screen. The viewer consumes the public read API (FR-11) only and can never gate the kernel.

## What this is not

- **Not a database.** Storage is a consequence of the design, not the substance of it.
- **Not a game engine.** Rendering consumes the kernel's state; it does not define it.
- **Not a physics library.** Existing integrators and rigid-body engines are dependencies, not the contribution. The contribution is the multiscale, deterministic, event-sourced structure that coordinates them.

---

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](./LICENSE-APACHE))
- MIT license ([LICENSE-MIT](./LICENSE-MIT))

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions. Contributions must carry a Developer Certificate of Origin sign-off (`git commit -s`).
