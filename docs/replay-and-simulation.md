# Replay and simulation

Draft contract · 2026-09-16 · Not implemented

Owns FR-07, FR-08, FR-24. Three distinct uses of “simulation” must not be conflated.

## Historical replay

Reconstruct logical state from a verified retained baseline and committed normalized events.
It requires the schema/reducer versions identified by that history. It does not rerun
parsers, inference, tool calls, external fetches, OCR, transcription, or current-time queries.
Preserve accepted derivations/observations as data when they are needed for reconstruction.
Seeds cannot regenerate arbitrary real-world evidence.
Replay consumes accepted normalized timestamps and their versioned provenance; it must not
recalculate them using a newer timezone database. Clock rollback, timezone-rule changes and
source-versus-derivation availability are covered by [the time contract](time-and-ordering.md).

Replay compares canonical permitted logical state at a revision, not physical index layout
or ciphertext bytes. Unknown required operation/schema versions fail explicitly.
Reconstruction before a retention boundary is unavailable. Recovery and independent replay
must agree, with a separately implemented reference reducer detecting shared implementation
errors where practical.

## Hypothetical simulation

Create a branch with namespace, base revision/history epoch, author, model identity and
version, deterministic execution profile, assumptions, seeded inputs, quotas and expiry.
Write branch events through the same durable transaction/security boundary.

Model code receives state/input values plus explicit virtual time and deterministic random
streams. No ambient clock, network, host filesystem, credentials, or real-world effects.
Initial models are trusted reviewed Rust modules; arbitrary user native plugins are excluded.
Extensions requiring untrusted code need a separately reviewed process/runtime sandbox.

Version the algorithm, dependencies, arithmetic policy, seed derivation, ordering and schema.
Do not claim cross-platform floating-point equivalence by default. Initial demonstrations
use integer/discrete dependency scheduling and require exact logical equality.
Every returned simulated assertion is marked hypothetical with branch/model/assumption
provenance, including exports and search results.

R2 additionally requires kinematics; R3 requires the constrained contact baseline in
[physics and motion](physics-and-motion.md), with pinned numeric profiles and atomic
multi-body event groups. These are release requirements, not arbitrary physics plugins.

Branch merge is not automatic. Promotion creates fresh proposals/transactions against
current authoritative state, validates conflicts and permissions, and records approval.
A branch cannot bypass policy by inheriting an old access grant. Source purge may invalidate
its inputs or terminate the branch; it cannot indefinitely pin erased data.

## Fault simulation

The test harness controls virtual scheduling, clocks, random seeds, I/O completion, short
writes, failures, flushes, crashes and restarts. Persist the seed and event schedule needed
to reproduce a failing case. Exercise all publication paths, including blobs, checkpoints,
commit metadata, compaction, keys and restore.

The harness is a test capability, not a customer simulation model. It supplements real
process-kill and filesystem/device tests; a virtual disk cannot prove actual hardware behavior.

## Required demonstrations

- Cold replay equals reference state at each committed revision.
- Lost-response retries have one effect after restart.
- Captured parser/model outputs replay identically without those workers installed.
- Repeated dependency-delay branches yield identical results under the pinned profile.
- Main history is unchanged by branch exploration alone.
- Branch promotion under changed main state detects conflicts.
- Revoked or purged source data is not readable through replay or branch artifacts.
- A fault seed reproduces the same injected failure and recovery result.

Ordinary database operations must work when simulation workers/models are absent.
No astronomy, rendering, or GPU implementation is a release dependency.
