# Decision 0206: Native 20,000-entity development measurement

Date: 2026-09-20

Status: Create/open/correctness passed; supervised sampling timed out. Nonqualifying development only.

Exercise the existing native packed BM-01 development ceiling of 20,000 entities and 200,000
relationships without changing that ceiling or the frozen qualifying profile. Use the normal
encrypted/durable/authorized implementation at `18a45e41d754b914b735fe413c15b09fb6d3b812`, with
Decision 0203 command owner accounting and the unchanged 64 MiB bounded caches. Generate the
independent manifest, query summary and warm-up/measured oracle bundle before materialization.

Run one workload at a time with fresh headroom checks, 3 GiB soft/4 GiB hard process-group memory
and 512 MiB swap limits. Fixture generation has a 600-second per-command deadline. Admit native
create, cold open, independent correctness-query and supervised development sampling separately,
with 1,800-second command deadlines; retain the existing sampler's 30-second owned query deadline.
Inspect each outcome before advancing. Preserve the synthetic store and partial evidence on
failure, without resetting nonces, weakening thresholds or repeatedly relaunching unchanged
failures. No compiler/test workload competes with these measurements.

Record exact binary/commit/locks/features, fixture hashes, commands, phase reports, owner-work
scope, process/group resource observations, source-certificate hashes and actual cache evictions.
Cold open means an empty USTE process/cache, not controlled OS/filesystem/device cache state.
Do not infer cache pressure from configured capacity or label missing measurements successful.
No oracle or database materialization result is a qualifying campaign by itself.

The [retained report archive](../evidence/native-packed-20000-development.json) pins successful
creation, cold open and all 384 independent oracle outcomes. Correctness queries recorded
17,781,965 actual cache evictions and 870,407 ms query-only time. Supervised sampling exited
124 at its unchanged 1,800-second command deadline with no final report. Do not report missing
latency populations as passing or rerun this unchanged failure. Its scope peaked at 277,757,952
bytes with zero swap; the last observed memory-event counters were all zero. GNU time's final
parent-only RSS/CPU does not represent the terminated worker; the scope records 1,796.394 CPU
seconds. The source-certificate hash remained unchanged. Preserve the fixture and continue
implementation/performance work before another separately justified measurement.

The 100,000-entity/1,000,000-relationship BM-01 profile, reserved runner, required sampling windows
and all BM-06 larger-than-memory/recovery targets remain unchanged. Complete authenticated I/O,
nonce/rotation lifecycle and other accepted roadmap/release work remain open. M1 stays pinned to
its separate accepted implementation and qualification; no consumer migration is involved.
