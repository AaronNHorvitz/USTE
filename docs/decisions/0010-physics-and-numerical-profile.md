# Decision 0010 — Deterministic kinematics and contact profile

Date: 2026-09-16

Status: accepted for profile `physics-fixed-v1`.

Closes D-09 and is the decision artifact for T-47.

## Arithmetic and execution

The strict baseline is a first-party safe-Rust fixed-point implementation with no physics or
linear-algebra dependency. A scalar is signed `i128` Q64.32 (32 fractional bits) in its named
SI unit. Add/subtract/multiply/divide use checked widened/decomposed arithmetic, round nearest
ties-to-even, and fail before publication on overflow. Integer square root has a fixed
bit-by-bit algorithm. Inputs are admitted only within ±1e9 m position, 1,000 m/s velocity,
100 m/s² acceleration, mass `(0, 1e9] kg`, radius `[1e-6, 1e6] m`; quantization error is
reported. No NaN/infinity exists in the profile.

Ticks are checked `u64`. Tick duration is an integer number of nanoseconds in `[1,1e9]` and
is fixed for a run. The run order is entity ID byte order. Parallel calculation may be used
only if it commits byte-identical ordered results. Profile compatibility includes arithmetic,
algorithm, constants, schema and build major; a mismatched checkpoint is refused.

## R2 kinematics

Stationary, constant-velocity and constant-acceleration 2D/3D use the analytic per-tick
equations `p = p0 + v0*t + a*t²/2`, `v = v0 + a*t`, evaluated from the checkpoint origin so
rounding does not accumulate differently after resume. Output is quantized once per requested
durable tick. The engine records assumptions separately from observations and never writes a
simulated value into an observed-state record.

An optional UTC mapping records a resolved `posix-utc-v1` origin and rational nanoseconds per
tick (positive numerator/denominator, reduced). Mapping uses checked integer arithmetic and
ties-to-even; it does not consult a clock or timezone. Unanchored runs expose virtual ticks only.

## R3 constrained contacts

The admitted model is frictionless 2D dynamic discs plus static axis-aligned box/plane
boundaries and uniform gravity. Restitution is Q0.32 in `[0,1]`. Each step uses semi-implicit
Euler for external acceleration, broad-phase uniform grid cells sized to the maximum diameter,
and deterministic narrow-phase pairs ordered by `(min body ID, max body ID, boundary ID)`.
Grid cells and collision pairs are capped before solving.

Discrete collision is admitted only when maximum per-substep displacement is <= one quarter
of the smallest participating radius. The supervisor selects the least substep count meeting
that bound, up to 16; otherwise `UnsupportedModel` reports tunnelling risk. Contact normal for
coincident centres is the deterministic +x axis ordered by IDs. One normal impulse resolves
approaching velocity using inverse masses and restitution; separating contacts receive none.
Penetration correction removes 80% beyond 1 micrometre, mass weighted, and is re-evaluated for
four fixed solver passes. Static boundaries have zero inverse mass. Friction, rotation, joints,
continuous collision and general polygons are unsupported.

All body results for a durable tick are one branch-local transaction group. Budget refusal,
arithmetic failure, cancellation before the commit certificate or solver failure publishes no
partial body state. Resume uses the last verified whole checkpoint plus committed tick groups.

## Accuracy, determinism and alternatives

Kinematic vectors tolerate at most one Q64.32 unit from the independently calculated rational
answer. Elastic two-body contacts require momentum error <= 8 units and kinetic-energy error
<= 32 units for admitted non-overflow cases; inelastic cases check the analytic restitution
result and non-increasing energy. Penetration after four passes must be <= initial penetration
and the documented residual; this baseline makes no safety-critical or continuous-collision
claim.

Floating point was rejected because the requirement prioritizes restart/profile repeatability
over peak speed and cross-target contraction/extended precision is easy to misstate. A third-
party physics engine was rejected by the strict Rust ownership boundary. Fixed point narrows
dynamic range and requires explicit refusal, but is independently testable.

`acceptance/r0/physics.tsv` contains exact rational kinematics, UTC mapping, overflow,
head-on/wall/overlap/order and budget vectors. `tests/r0_vectors.rs` executes kinematic and
ordering cases. VT-20/21 must add an independently implemented rational/contact oracle and
checkpoint/crash tests before release gates can pass.
