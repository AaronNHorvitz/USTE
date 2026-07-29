# USTE — Design Decisions

Companion to [README.md](./README.md). The README states the shape of the system; this document specifies the contracts that make it hold. Implementation has not started; everything here is normative for when it does.

---

## 1. The invariant, versioned

```
state(t) = f(world_format_version, numerical_profile, seed, rules, ordered_event_log)
```

- **`world_format_version`** — the schema of state, addresses, and events. Any breaking change bumps it; replay across versions is migration, never reinterpretation.
- **`numerical_profile`** — a named, versioned bundle: integrator selections and step sizes, floating-point mode (FMA policy, no reassociation), math-library implementations and versions, SIMD width assumptions. The baseline profile guarantees **per-binary determinism**. A stricter `portable` profile (fixed-point or strict-arithmetic authoritative layer) guarantees cross-platform bit-equality and is adopted only where required.
- **`ordered_event_log`** — the total order over events is part of the state definition. Two logs with the same events in different order are different histories.

Every property test in the project is a restatement of this equation.

## 2. Observer independence

**Rule: fidelity is presentation; deviations require causation.** Merely simulating a region at higher fidelity must be side-effect-free on canonical state.

Mechanism — reference-relative integration (Encke's method):

- The analytical baseline (Keplerian elements per body) is canonical at all times for untouched bodies.
- The active layer integrates the *deviation* from that reference, not the absolute trajectory. Awake behavior is `baseline(t) + δ(t)`.
- **Sleep with no interaction:** `δ` is discarded. The canonical trajectory was never perturbed. No event is emitted. Two runs that differ only in who looked at what produce identical canonical histories.
- **Sleep after interaction:** the interaction *committed* a deviation. New osculating elements are fitted from the final numerical state at a recorded epoch, and the fit becomes a deviation event in the log. The body's baseline is now the new elements from that epoch forward.

Consequence: wake/sleep transitions themselves are not events. Only commits are.

## 3. Sleep/wake reconciliation contract

**Analytical → active (wake):**
- Initialize numerical state exactly from the analytical elements evaluated at the wake tick. The initialization is a pure function of `(elements, tick)` — no accumulated integrator state survives dormancy.

**Active → analytical (sleep):**
- No committed deviation → discard `δ`, resume canonical elements. Nothing written.
- Committed deviation → fit osculating elements from final numerical state at the sleep tick; write one deviation event `(address, epoch_tick, new_elements, cause)`.

**Continuity requirements at commit:**
- Position and velocity are continuous by construction (the fit is exact at epoch).
- Energy and angular momentum of the fitted conic must match the numerical state at epoch to within the profile's stated tolerance; the residual is recorded in the event for drift auditing.

**Hysteresis:**
- Minimum dwell ticks in each state before a transition is permitted, and a wake-radius / sleep-radius pair with sleep_radius > wake_radius, so boundary-hovering observers cannot oscillate a system. Thrash-rate is a monitored metric.

## 4. Time and event ordering

- Integer master clock. All region rates are integer divisors/multiples of it.
- Every event carries `(tick, region_id, sequence_within_tick)`; the triple is the total order.
- Cross-region events (a contact-region outcome affecting an active-region body) are scheduled onto the master clock, never applied mid-step.

## 5. Coordinate frames

- Nested reference frames (root → system → body-local), 64-bit positions, frame transitions at defined boundaries with hysteresis (same pattern as sleep/wake).
- **Authoritative state is frame-local and observer-independent.** Floating-origin recentering exists only in render projection, per client. No simulation-side quantity may depend on any observer's position except through explicit, logged interaction.

## 6. Persistence and durability

Append-only event log + periodic snapshots, written asynchronously.

- **Event record:** `(sequence_number, tick, region, payload, schema_version, checksum)`. Serialization is canonical (deterministic byte layout) so logs are comparable across runs.
- **Snapshots:** written to a temp file, fsynced, atomically renamed. A snapshot names the log sequence number it covers. Publication is all-or-nothing.
- **Flush policy:** explicit and configurable; the maximum crash-loss window is a stated number of ticks, not an accident of buffering. Backpressure: if the log writer falls behind its bound, the simulation *slows* rather than drops events — losing history is worse than losing frame rate.
- **Recovery:** last durable snapshot + replay of the log suffix. Recovery is the same code path as the replay test, so it is exercised constantly rather than only in disasters.

## 7. Implementation order

The replay kernel precedes the universe generator. A tiny world that survives replay perfectly proves more of the thesis than a billion generated stars.

1. Repository cleanup; Rust workspace (`uste-core`, `uste-time`, `uste-gen`, `uste-orbits`, `uste-sim`, `uste-log`).
2. Versioned hierarchical addresses and deterministic seed derivation.
3. Integer simulation time and canonical event ordering.
4. Deterministic event serialization, replay, and state hashing.
5. Two-body analytical propagation.
6. One numerical integrator with the full sleep/wake reconciliation contract (§3).
7. Property tests across replays, thread counts, and transition schedules.
8. Only then: the procedural galaxy generator and visualization (Milestone 1).

## 8. Non-goals of the kernel

- No storage engine on the frame path.
- No rendering in the authoritative loop.
- No application semantics: the kernel does not know what its entities mean. Anything domain-specific lives above it.
