# USTE: Universal Spatial-Temporal Engine

A deterministic, multiscale, event-sourced simulation kernel written in Rust.

USTE represents an enormous simulated space with compact rules, and computes only what matters at the present moment. It is not a database and not a rendering engine. It is the kernel underneath both: the part that decides what exists, what changes, and how any past state can be reconstructed exactly.

```
Universe = seed + rules + simulation time + sparse deviations
```

Nothing that can be regenerated is stored. Given a hierarchical address and a seed, the engine reproduces a body's stable properties on demand, identically, every time. Persistent storage holds only what deviates from that generated baseline — discoveries, modifications, exceptional events — plus periodic checkpoints.

---

## Status

**Redesign in progress.** Earlier revisions of this repository described a GPU-accelerated graph database built on TensorFlow, TimescaleDB, and ArangoDB. That framing has been retired: it described a storage layer, not an engine, and it put databases on the simulation's critical path. The design below replaces it. Implementation has not started.

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

- **Predictable memory layout.** Structs and arrays are compact and contiguous by default. Cache behavior is something you design, not something you hope for.
- **No garbage collector.** Not a throughput argument — a *determinism* argument. A fixed-timestep simulation cannot tolerate unpredictable pauses in its loop.
- **Fearless parallelism.** Data-race freedom at compile time is what makes it safe to parallelize a simulation without silently sacrificing reproducibility.
- **SIMD and control over arithmetic.** No fast-math by default, which matters enormously when bit-reproducibility is a project requirement rather than a nicety.
- **Zero-cost abstraction.** Layered, readable structure that compiles down to the tight loops the 1980s wrote by hand.

---

## Architecture

### Layered simulation

Fidelity is a function of relevance, not of distance alone. Five layers, each cheaper than the one above it:

| Layer | Role | Cost |
|---|---|---|
| **Procedural** | Generates galaxies, systems, and bodies deterministically from hierarchical address and seed | Evaluated on demand; effectively free |
| **Analytical** | Dormant bodies advance by closed-form propagation (Keplerian orbits) | Nanoseconds per body, evaluated lazily |
| **Active** | Nearby bodies use numerical integration and interact physically | Milliseconds for thousands of bodies |
| **Contact** | Collisions, vehicles, terrain, immediate phenomena — higher frequency | Bounded by bubble radius, not world size |
| **Historical** | Append-only event log recording every deviation from the generated baseline | Off the frame path entirely |

Objects sleep when they cannot affect anything. On revisit, the engine either advances them analytically or processes only the events that mattered while they were dormant. **The apparent universe is enormous; the actively simulated universe stays small.** That is the entire reason this is achievable on one machine.

### Canonicality: the analytical layer is the truth

A sleeping system woken into numerical integration will not match what continuous numerical integration from the origin would have produced. Left unaddressed, this makes state depend on *when a thing was observed* rather than on time alone — observation would perturb the universe.

USTE resolves this by rule rather than by chasing agreement between layers:

**Analytical propagation is canonical for dormant intervals. It is not an approximation of a truer numerical trajectory — it is the truth for that interval. Wake events are recorded in the event log like any other deviation.**

The contract the engine actually guarantees is therefore:

```
state(t) = f(seed, rules, event_log)     — bit-reproducible, always
```

A pure function of seed *and history*. Every test in the project is written against that invariant.

### Determinism

Reproducibility is the project's central claim, so the known hazards are policy, not afterthought:

- **No randomized iteration order.** Rust's `HashMap` randomizes iteration per process. Ordered or seeded-hash containers only, anywhere state is iterated.
- **Order-independent parallelism.** Parallel stages must be pure maps with deterministic merges, or deterministically scheduled. Reduction order is not permitted to vary between runs.
- **Explicit floating-point contract.** Per-binary determinism is the baseline commitment. Cross-architecture bit-equality, where required, is bought with fixed-point or strict-arithmetic discipline in the authoritative layer.
- **Integer time.** Simulation time is integer ticks. Never accumulated floating-point seconds.
- **Hierarchical seed derivation.** PRNG streams are keyed by address path, so any node in the hierarchy regenerates independently without touching its siblings.
- **Symplectic integrators** in the orbital layer, where long-horizon energy drift is the quantity being controlled.

### Multi-rate time

The contact region, the active region, and the analytical layer advance at different frequencies, and the procedural layer is timeless. Coordinating those clocks reproducibly is a first-class subsystem: an integer master clock, nested fixed timesteps, and a total ordering of events across regions.

### Coordinate frames

Human-scale precision and astronomical-scale extent cannot coexist in one flat coordinate system; single-precision error at astronomical distance is catastrophic. USTE uses nested reference frames with 64-bit positions, a floating origin that recenters on the observer, and 32-bit local coordinates handed to the renderer. This is the load-bearing architectural decision — every other capability composes cleanly on top of it, or fails to.

### CPU versus GPU

The authoritative simulation is **CPU-resident**. This is a design conclusion, not a limitation:

- Dynamics are small-N sequential ODE integration — a workload CPUs win outright until batches reach the hundred-thousand range.
- Event scheduling, sparse graphs, branching, and shifting workloads are CPU-shaped problems.
- Reproducibility is materially easier to guarantee on CPU.
- Compact `struct`s, SIMD, and cache-conscious layout do more here than raw arithmetic throughput.

The GPU is retained for what it is actually built for: **rendering**, particles, atmospheres, fields, and — when batch sizes justify it — large sets of independent orbital evaluations and other embarrassingly parallel numerics.

### Persistence

Storage is **asynchronous and off the critical path**. An append-only event log plus periodic snapshots is the initial and sufficient design. Databases are a later concern, appropriate for historical analysis and exploration tooling rather than for participating in a simulation frame.

---

## Proof of concept

The first milestone demonstrates the entire thesis on one machine:

1. Generate a stable galaxy from a single seed.
2. Select a generated star system.
3. Numerically simulate its primary/planet/satellite system.
4. Move seamlessly between procedural, analytical, and active fidelity levels.
5. Leave the system and return; regenerated properties must be identical.
6. Persist one artificial modification without storing the untouched remainder.
7. **The replay test:** simulate a long interval with mixed sleep/wake transitions and at least one modification, hash the resulting state, then replay cold from `(seed, rules, event_log)` and assert the hashes are bit-identical.
8. Benchmark entities processed per second, memory footprint, and numerical drift.

Step 7 is the project's claim. Everything else is features.

---

## What this is not

- **Not a database.** Storage is a consequence of the design, not the substance of it.
- **Not a game engine.** Rendering consumes the kernel's state; it does not define it.
- **Not a physics library.** Existing integrators and rigid-body engines are dependencies, not the contribution. The contribution is the multiscale, deterministic, event-sourced structure that coordinates them.

---

## License

MIT. See [LICENSE](LICENSE).
