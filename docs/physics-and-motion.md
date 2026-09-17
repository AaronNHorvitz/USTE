# Physics and motion simulation

Draft contract · 2026-09-16 · Not implemented

Owns FR-31/32. Recording motion is database work; calculating hypothetical motion is model
work. The simulation kernel is part of the planned product but not a mandatory process on
ordinary ingest/read paths. Decision 0010 freezes physical scope and the numerical execution profile.

## Required baseline and non-goals

R2 implements fixed-step local Cartesian 2D/3D kinematics: stationary, constant velocity and
constant acceleration under bounded inputs. R3 adds a constrained 2D rigid-body demonstration:
finite positive-mass discs, static plane/box boundaries, gravity, bounded restitution,
frictionless contacts and deterministic contact ordering. Unsupported shapes/forces return
typed errors, not plausible-looking approximations. Decision 0010 defines overlap resolution,
collision/tunneling limits, substep policy, tolerances and maximum supported velocities.

This is not a full game engine: general 3D contacts, joints, friction, deformables, fluids,
celestial dynamics, relativistic time, GPU simulation and arbitrary native plugins remain
outside the initial release. No safety-critical physical accuracy claim is made.

## State and execution profile

Each body references a world/entity, frame, geometry/shape version, mass, pose, optional
velocity, initial conditions and evidence or explicit assumptions. Units are typed; numeric
overflow, nonfinite values, unsupported scales and invalid mass fail before state publication.

Every run pins model/version, algorithm/integrator, tick quantum, step size, arithmetic and
rounding policy, tolerances, contact ordering, seed/stream identities, schema, build/toolchain,
dependency lock and supported hardware profile. Decision 0010 deliberately chooses fixed-point
arithmetic. The baseline promises repeatability only within its tested
execution profile; cross-platform bit equality is a separate capability requiring evidence.
Changing worker count or iteration order must not silently change the named profile's result.

Simulation ticks are checked integers. An optional mapping to UTC specifies origin, rational
tick duration, rounding, time-scale policy and supported range. Virtual future/past instants
are labeled hypothetical. UI local time is a projection of a resolved UTC mapping; unanchored
virtual time stays virtual. Pausing or faster-than-real-time execution does not alter that mapping.

## Controlled run lifecycle

1. Authorize and pin a coherent base revision, exact source versions and bounded input region.
2. Validate geometry, frames, units, model admission, quotas and simulation-time mapping.
3. Create an isolated branch/run identity; record assumptions and the numerical profile.
4. Advance pure model steps under CPU/RSS/step/output/collision-pair budgets.
5. Commit complete multi-body event groups and checkpoints through the transaction coordinator.
6. Return durable result references, uncertainty/limitations, profile and source provenance.

Worker-internal tentative steps are not durable database state. Backpressure slows or pauses
the run; it never drops committed consequences. Cancellation reports the last durable step
and whether a transaction outcome is unknown. Resume from a verified checkpoint plus ordered
events under the same profile, or refuse incompatibility. No new inference during replay.

Model effects are branch-local. Promotion can submit an authorized hypothetical assertion;
it cannot turn a forecast into an observed measurement or authorize a physical action.
Source revocation/purge invalidates dependent runs and results under retention policy.
Merely inspecting a world does not advance it. A viewer is an external read-only consumer,
not part of scheduling, collision acceptance or durability.

## Verification

VT-20 checks analytic constant-velocity/acceleration cases, units, step convergence where
applicable, range boundaries, clock mapping, restart and reference replay. VT-21 checks the
declared collision cases: head-on contacts, simultaneous contacts, walls, overlap, maximum
velocity, budget refusal, deterministic ordering, and conservation/error tolerances appropriate
to each model (do not assert energy conservation for inelastic impacts).
Crash each multi-body publication boundary; either the whole durable group appears or none.
Compare independent reference calculations, not two calls to the same integrator.
BM-12 measures run cost, contact-pair growth, durable output rate and cancellation/recovery;
BM-13 measures impact on ordinary readers. A physics feature cannot pass by showing animation.
