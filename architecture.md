# the engine — Design Decisions

Companion to [README.md](./README.md). The README states the shape of the system; this document specifies the contracts that make it hold. Implementation has not started; everything here is normative for when it does.

---

## 1. The invariant, versioned

```
state(t) = f(world_format_version, numerical_profile, seed, rules, ordered_event_log)
```

- **`world_format_version`** — the schema of state, addresses, and events. Any breaking change bumps it; replay across versions is migration, never reinterpretation.
- **`numerical_profile`** — a named, versioned bundle covering *numerics and execution compatibility*:
  - integrator selections and step sizes
  - floating-point mode: FMA policy, no reassociation
  - math-library implementations and versions
  - target triple and allowed CPU feature set (e.g., `x86-64-v3`)
  - compiler version, build flags, and enabled Cargo features
  - dependency lock state (`Cargo.lock` hash)
  - the build hash of the binary the profile identifies
  - canonical serialization version (§8)

  The baseline profile guarantees **per-binary determinism**. A stricter `portable` profile (fixed-point or strict-arithmetic authoritative layer) guarantees cross-platform bit-equality and is adopted only where required.
- **`ordered_event_log`** — the total order over events is part of the state definition. Two logs with the same events in different order are different histories.

Every property test in the project is a restatement of this equation.

## 2. Canonical force model

**Rule: a body's model-segment history is a pure function of `(seed, rules, address, ordered_event_log)`. The first segment is determined by `(seed, rules, address)`; every later segment arises only at a committed event boundary (§5). Activation, fidelity level, and observation never decide which canonical forces exist.**

The canonical baseline is not synonymous with two-body Kepler. It is assigned per body at generation, in tiers:

1. **Conic** — pure Keplerian elements, for bodies whose perturbations are below the rules' significance threshold.
2. **Precessing conic** — Keplerian elements plus secular rates (node regression, apsidal precession, mean-motion correction), capturing dominant N-body effects analytically. This is how real approximate ephemerides are published.
3. **Generated ephemeris table** — for strongly interacting cases (resonances, close binaries), a one-time deterministic numerical integration performed *at system generation*, producing a tabulated canonical trajectory. Derived data: cacheable, discardable, regenerable, observation-independent — because generation is.

Every force in the simulation is classified as **canonical** (present in the assigned baseline model) or **presentational** (existing only in the active layer's richer integration). The presentational residual is *always* discarded. There is no third category.

**Interactions commit state changes; model changes occur only as committed segment boundaries.** An interaction is a coupling exceeding the rules-defined significance threshold ε (momentum/energy transfer); below ε, influence is presentational by definition. ε is part of `rules`, hence part of the invariant.

**Model segments.** A body's canonical model is *piecewise*: a sequence of model segments, each valid from an epoch. The first segment is assigned at generation; every subsequent segment boundary is a committed event. When an interaction invalidates a tier assignment (a conic that no longer describes the body; a tier-3 table made stale), the committing event re-invokes the *same pure assignment function* on the post-interaction state, producing new segments for **all coupled bodies atomically in a single event group** (§5). A tier-3 rebuild is deterministic regeneration from the epoch state vector. At no point does activation, fidelity, or observation decide a segment boundary — only generation and committed events do.

## 3. Observer independence

**Rule: fidelity is presentation; deviations require causation.** Merely simulating a region at higher fidelity is side-effect-free on canonical state.

Mechanism — reference-relative integration (Encke's method) *against the canonical model of §2*:

- The canonical baseline is the truth, at all times, for every body that nothing has touched.
- The active layer integrates the *deviation* from that baseline, not the absolute trajectory. Awake behavior is `baseline(t) + δ(t)`. With tiered baselines, δ contains only sub-threshold residuals and genuine interactions — not the dominant physics.
- **Sleep with no committed interaction:** δ is discarded. No event. Two runs differing only in who observed what produce identical canonical histories.
- **Sleep after a committed interaction:** the commit records the new canonical state (§4). The body's baseline forks from that epoch forward.

**Canonical events are detected from canonical state — δ is never a cause.** Event-acceptance predicates (ε-crossings, encounters, collisions) evaluate over baselines plus committed deviations only. Presentational residuals are invisible to them: no matter what the active layer's richer physics appears to show, an interaction is canonical if and only if canonical state predicts it.

**Natural encounters are baseline history, not events.** If `(seed, rules)` implies it, it is not an event: an encounter between untouched bodies is a deterministic consequence of the seed, hence regenerable, hence part of generated baseline history — computed lazily during generation like everything else procedural, and never logged. The log records exactly what the seed cannot imply. This is what keeps the log sparse at universe scale by definition rather than by hope.

This imposes a closure obligation on the generator: it may not emit a state whose own baseline implies an un-reflected past encounter. Two mechanisms, v0 adopting the first: (a) **generate baseline-stable systems by construction** — configurations whose baselines self-intersect are deterministically rejected or adjusted at generation, which is physically defensible since observed long-lived systems are the survivors; (b) generated natural-event history as derived, regenerable data (the tier-3 pattern), reserved for later.

**Causal waking is observer-independent and scoped to the deviation frontier.** Only committed deviations can produce futures the seed does not imply, so encounter detection watches only deviated bodies and their coupling neighborhoods: a deterministic encounter queue populated at commit time, plus a spatial broad phase over the deviation set. Ungenerated, untouched space is never scanned — its entire history is implied. Systems whose deviated trajectories predict an interaction wake and commit whether or not any observer is present. Observers trigger *presentational* wakes only; causality triggers *canonical* wakes; deviations beget deviations, and the seed's universe never does.

**Observers act through presence, not through observation.** An observer that physically enters a system does so as a canonical entity whose arrival is itself committed state — presence is an interaction; looking is not.

Wake/sleep transitions themselves are not events. Only commits are.

## 4. Sleep/wake reconciliation contract

**Analytical → active (wake):**
- Initialize numerical state exactly from the canonical model evaluated at the wake tick. Pure function of `(model, tick)`; no integrator state survives dormancy.

**Active → analytical (sleep):**
- No committed deviation → discard δ, resume canonical model. Nothing written.
- Committed deviation → write **one atomic event group with one deviation member per affected body** (a single-body commit is a group of size one, §5), each member containing:
  - `address`, `epoch_tick`, `cause`
  - **epoch state vector** (position, velocity in the frame of record)
  - **reference-model identifier** — which baseline tier and parameters apply from this epoch
  - **model-specific invariant audit** (see below)

**Invariant audit, scoped honestly:**
- *Two-body contract (Milestone 0):* the fitted conic's energy and angular momentum relative to the primary must match the numerical state at epoch within the profile's tolerance; the residual is recorded in the event.
- *General contract:* independent conics cannot preserve total N-body invariants. For tier-2/3 models the audit records the model's own conserved or slowly-varying quantities and their residuals at epoch. What is audited is part of the model definition, not an afterthought.

**Hysteresis:**
- Minimum dwell ticks in each state; wake radius strictly inside sleep radius, so boundary-hovering observers cannot oscillate a system. Thrash rate is a monitored metric.

## 5. Event lifecycle

Three states, strictly ordered. **"Commit" throughout this document means the pending → accepted transition.** Simulation state, encounter queues, and model segments advance at acceptance; durability governs only externally visible permanence, never internal progress.

1. **Pending** — produced within a frame, not yet ordered. Invisible to canonical history.
2. **Accepted** — assigned its `(tick, region, sequence)` position in the total order. Part of canonical history; replay includes it; simulation proceeds on it. Multi-body consequences (coupled model-segment updates, §2) are accepted as one **atomic event group**: replay observes all of it or none of it, never an interleaving.
3. **Durable** — flushed to storage per the explicit flush policy. **The unit of the log is the event group**: a group is written as one checksummed record (a plain event is a group of size one), and the durable frontier advances only past complete groups. A crash can therefore truncate the log only at a group boundary — never inside a multi-body update.

**Rule: nothing is presented externally as permanent until durable.** Internal simulation may proceed on accepted events; any externally visible guarantee of permanence waits for durability.

**Crash semantics:** recovery rolls back to the durable frontier — the newest snapshot whose covered endpoint is at or before `F` (§9), plus the durable log suffix from that endpoint to `F`. The crash-loss window is therefore *precisely* the accepted-but-not-durable set, bounded by the flush policy's stated maximum. Recovery is the same code path as the replay test.

## 6. Time

- **Representation:** `u64` ticks. Checked arithmetic; overflow is a deterministic panic, not a wrap.
- **Quantum (v0):** 1/3600 second, chosen so all region rates are integer divisors (240 Hz = 15 ticks/step, 120 Hz = 30, 60 Hz = 60, 10 Hz = 360, 1 Hz = 3600). Changing the quantum is a `world_format_version` bump.
- **Epoch:** tick 0 is world creation. No external calendar in the kernel.
- **Maximum duration:** `u64` at 3600 ticks/s ≈ 1.6 × 10⁸ years. Stated, tested, and absurdly sufficient.
- Region clocks are integer divisors/multiples of the master clock; cross-region events are scheduled onto master ticks, never applied mid-step.

## 7. Space, units, and frames

- **Positions (baseline profile):** `f64`, frame-local, SI meters. Frame extents are bounded (≤ ~10⁹ m from origin) so `f64` resolution stays below one micrometer everywhere in-frame.
- **Positions (portable profile):** `i64` fixed-point micrometers (±9.2 × 10¹² m ≈ 61 AU per frame) where bit-equality across platforms is required.
- **Units:** SI throughout, enforced by dimensional newtypes (`Meters`, `MetersPerTick`, `Kilograms`) — unit errors are compile errors.
- **Frames:** identified by hierarchical address; nesting root → system → body-local. Transforms are deterministic functions of the canonical models of the bodies involved. Frame transitions occur at defined boundaries with hysteresis (same pattern as sleep/wake).
- **Authoritative state is frame-local and observer-independent.** Floating-origin recentering exists only in render projection, per client. No simulation-side quantity may depend on any observer's position except through explicit, logged interaction.

## 8. Canonical serialization

- Little-endian, field-by-field defined encoding. In-memory `struct` layout (including padding) never touches the wire or the hash.
- **NaN is forbidden in canonical state.** Producing one is a checked, deterministic failure — a bug surfaced, not a value stored.
- State hashes are computed over canonical bytes only.
- The serialization rules carry their own version, referenced by the numerical profile.

## 9. Persistence and durability

Four artifacts: an append-only event log, periodic snapshots, one **world manifest**, and the dual-slot **durable-frontier record** (below). Asynchrony is a *steady-state* property of log and snapshot writes — creation is not asynchronous (below).

- **World creation is durable-or-absent, via atomic directory publication.** Creation builds the world in a temporary sibling directory on the same filesystem: manifest, empty log, and initial frontier record (`F` = log start, both slots) are written and fsynced, the directory itself is fsynced, the directory is atomically renamed to its final name, and the parent directory is fsynced — only then does creation report success. **Existence at the final name therefore *means* creation completed.** A crash mid-creation leaves only a temporary-named directory, which is always safe to delete and retry; it can never be confused with a damaged legal world, because sequential writes into the final path — where that ambiguity would arise — never occur. Corollary: **every legal world has a valid frontier from birth**, so an invalid or missing frontier at the final name always indicates corruption and never a fresh world — which is what makes the hard-error rule below sound.
- **World manifest:** immutable, checksummed, written once at creation: `world_format_version`, the full numerical profile (including target triple, CPU feature set, dependency lock hash, serialization version), `seed`, and `rules`. The invariant names four inputs beyond the log; the manifest is where they live. **Truth about the world on disk is the manifest plus the log**; the frontier record is the fourth artifact and is truth about *durability* — it asserts nothing about the world, only which prefix of the log is acknowledged, and the recovery rules below are undefined without it. Snapshots and log segments reference the manifest's checksum; a log without its manifest is not a world.
- **Group record — the unit of the log:** length-prefixed and footer-terminated: `(length, sequence_number, tick, region, member_events[], schema_version, checksum, footer)` in canonical serialization. A plain event is a group of size one; one checksum covers the whole group.
- **The durable frontier is persisted independently of the data it protects.** Framing alone cannot distinguish a crash tail from corruption: a corrupted length or footer in the final acknowledged record would masquerade as a structurally incomplete tail and be silently discarded. Therefore the frontier — the byte offset up to which durability has been acknowledged — is its own record, written **dual-slot** (two alternating slots, each independently checksummed, highest valid slot wins), so a torn write of the frontier itself is survivable.
- **Durability ordering:** fsync the log through offset `F` → write and fsync the frontier record `F` → only then acknowledge durability externally. The acknowledgment is never earlier than the frontier; the frontier is never earlier than the flushed bytes it names.
- **Recovery rules, in terms of the frontier `F`:**
  - Any structural malformation or checksum failure **at or before `F`** — corruption of acknowledged history. **Hard recovery error.** The world refuses to open; repair is an explicit operator action against backups, never automatic.
  - Bytes **after `F`**: **discarded unconditionally — the log is truncated at `F`,** without inspection. This is strict rollback, and it is the cheap choice *because* the kernel is deterministic: everything endogenous past `F` (causal wakes, autonomous evolution) is re-derived bit-identically when simulation resumes from `F` — replay is the recovery. Divergence can enter only through exogenous inputs, which were by definition never acknowledged: the producer's contract is that an unacknowledged submission may be lost and must be retried, which keeps retry/idempotency machinery out of the kernel entirely. Salvaging valid groups past `F` was considered and rejected — it would redefine the crash-loss window, require a recovered-frontier commit step, and force deduplication of retried events into the kernel, to save only recompute time.
  - **Both frontier slots invalid** — the frontier itself cannot be established. Hard recovery error, same rule as corrupted history.

  The recovery point is therefore always exactly `F` — a verified group boundary by construction. The crash-loss window is *precisely* the accepted-but-not-durable set, matching §5 verbatim; no failure at or before an acknowledged offset is ever repaired silently; and the two failure modes are distinguished positionally by the frontier, not structurally by framing.
- **Snapshots** are an optimization: they *cache* regenerable and replayable state to bound recovery time. They are never the truth — the manifest and log are. Written to a temp file, fsynced, atomically renamed. Each names its covered endpoint `E` as **both a group sequence number and a log byte offset** landing on a verified group boundary — the offset is what makes comparison against `F` unambiguous — and records the **canonical state hash** of the state it contains. Recovery SHALL recompute the hash of the loaded state and compare it against the recorded hash before use; mismatch means the snapshot is declined and flagged, and recovery proceeds from an older snapshot or from genesis replay — declining is always safe, because snapshots are cache. A hash that is internally consistent but was never equal to `f(manifest, log ≤ E)` indicates a broken derivation pipeline; the **scrub audit** — an explicit kernel operation (library call, and `uste scrub` in the reference CLI) that re-derives published snapshots, compares hashes, and emits a machine-readable report with nonzero exit on any mismatch — exists to catch that class, since no recovery-time check can. Scheduling cadence belongs to the consumer.
- **Snapshot contents must equal the label.** Eligibility checks `E ≤ F`, but a snapshotter that copies newer live state and stamps it `E` would pass that check while smuggling post-`E` history — the same resurrection bug behind a legal label. The requirement is exact: a snapshot's state must equal `f(manifest, log ≤ E)` bit-for-bit. v0 satisfies it by construction: **snapshots are derived, not captured** — produced by replaying the durable artifacts through `E` outside the live process, which cannot observe live state at all and reduces snapshot correctness to replay correctness, the most-tested path in the system. A live copy-on-write view frozen exactly at a group boundary is a permitted later optimization, subject to the same acceptance test: the snapshot's state hash must match the replay-derived hash for the same `E`.
- **Snapshot eligibility is subordinated to the frontier.** Live state contains accepted-but-not-durable events, so any *capture-based* snapshotter reading it could cover history beyond `F` — and restoring that after strict rollback would resurrect exactly what the rollback discarded. v0 snapshots are replay-derived from durable artifacts and cannot exceed `F` by construction; the rules below remain normative regardless, as defense-in-depth and as the contract any future capture-based (COW) snapshotter must meet:
  - **Publication:** a snapshot's covered endpoint must be at or before the durable frontier *at publication time* — snapshots capture only durable history. Since `F` is monotone, every legally published snapshot remains valid forever.
  - **Recovery:** select the newest snapshot with covered endpoint ≤ `F`. A snapshot claiming coverage beyond `F` can only mean a publication bug or corruption: it is never used, and its presence is surfaced as an integrity warning — declining a snapshot is always safe, because snapshots are cache and never truth.
- **Flush policy:** explicit and configurable; the maximum crash-loss window is a stated number of ticks. Backpressure: if the log writer falls behind its bound, the simulation *slows* rather than drops events — losing history is worse than losing frame rate.

## 10. Implementation order

The replay kernel precedes the universe generator. A tiny world that survives replay perfectly proves more of the thesis than a billion generated stars.

1. Repository cleanup; Rust workspace (`uste-core`, `uste-time`, `uste-gen`, `uste-orbits`, `uste-sim`, `uste-log`).
2. Versioned hierarchical addresses and deterministic seed derivation.
3. Integer simulation time (§6) and canonical event ordering (§5).
4. Canonical serialization (§8), replay, and state hashing — with crash-injection tests at every group-record boundary, against the frontier record itself (torn frontier write, single-slot corruption, dual-slot loss), and against snapshot eligibility (a planted snapshot claiming coverage beyond `F` must be declined and flagged), distinguishing tail-discard from hard corruption error (§9).
5. Two-body analytical propagation (tier-1 canonical model).
6. One numerical integrator with the full reconciliation contract (§4), two-body scope.
7. Property tests across replays, thread counts, and transition schedules; golden event logs as committed fixtures under `tests/fixtures/`.
8. The public read API (PRD FR-11) and the CLI `inspect` command exercising every query — the final Milestone 0 step.
9. After M0: the V1 read-only demo viewer — a separate crate consuming only the read API; never a kernel gate.
10. Milestone 1: the procedural galaxy generator and tier-1 assignment at scale. After M1: the V2 viewer increment (scale traversal, dual-trajectory rendering, BENCH-V). Tier-2/3 canonical models follow later milestones.

## 11. Non-goals of the kernel

- No storage engine on the frame path.
- No rendering in the authoritative loop. (A read-only demo viewer exists as a separate consumer crate — it exercises the public read API and the render-side floating origin, and can never gate the kernel.)
- No application semantics: the *core* does not know what its entities mean — model families (v1: the celestial models of `uste-orbits`) plug into core traits, and anything above the model families lives outside the kernel entirely.
