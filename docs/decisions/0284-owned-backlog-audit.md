# Decision 0284: Owned ready-backlog audit under the lane's resource cap

Date: 2026-09-26

Status: Accepted audit; records blockers and next operations; changes no task status or target.

The owner asked for an audit of the owned, dependency-ready backlog under this lane's cap (6 GiB
memory, three CPUs, shared heavy-work lock). An unavailable 24 GiB qualification run blocks that
measurement only, not runner code, fixes or honestly labelled smaller development fixtures.

## Dependency graph

A mechanical pass over TASKS.md finds two open rows whose dependencies are all complete. T-20 is
in progress. T-62 is the private vulnerability-reporting verification, which needs the owner's
repository-administration account, so it is not performable by USTE. Every other open R2–R4 row
depends on T-20 directly or through T-19. The owned critical path is therefore T-20's remaining
acceptance and the R1 benchmark evidence T-19 requires (BM-01, BM-02, BM-04, BM-06). Among the
capability packages:

- **DB-R02 and DB-R03** are implemented, apart from T-34 purge, and await independent review.
- **DB-R04** has its candidate contract pinned (Decision 0282) and awaits consumer review and
  integration.
- **DB-R01** is the T-20/T-19 work below.
- **DB-R05** is the original roadmap and follows it.

## Item by item

| Item | State | Performable here | Exact next operation |
|---|---|---|---|
| BM-01 warm four-hop at 100k/1m, five samples | 20k development observations pass (199.6–233.3 ms, Decisions 0268–0274); qualification unperformed | Code and development runs yes; qualification no (24 GiB reserved host) | Owner runs the commands in `docs/evidence/r1-acceptance-report.md`. The packed engine first needs a versioned decision raising `MAX_NATIVE_DEVELOPMENT_ENTITIES` with construction and memory evidence, for which a development construction at an intermediate size is performable here. |
| T-20 retained-path decode cost | 37% of retained samples in `decode_stored_record` (Decision 0266 profile) | Yes | A decoded-record cache would change cache residency and counters, so it is a behaviour change. It needs its own decision, vectors and a new semantic baseline; it is not an optimisation under the frozen protocol. |
| BM-02 commit p99 ≤ 50 ms, ≥ 2,000 events/s | No runner exists | Runner and development runs yes; qualification no | Implement a development runner over the production authorized write path. It should report single-commit latency percentiles and batched events per second with durable receipts and interleaved readers, labelled non-qualifying. |
| BM-04 ≥ 250 MiB/s ingest | 95.9 MiB/s recorded; 105.9 MiB/s lane development observation; barrier-bound (Decision 0283) | Yes | A new staging-protocol decision that amortizes per-chunk barriers (grouped progress witnesses or staged chunks without per-chunk renames). Then rerun the complete T-15 publication fault matrix and resume/abort/corruption vectors, and remeasure. Also add a runner for the 100,000-small-object half. |
| BM-06 10M events ≤ 120 s | Native development recovery controls pass at ≤ 8,192 records | Larger development constructions yes; 30 reserved-host trials no | A versioned admission decision above 8,192 records with construction evidence, then an intermediate development recovery observation. |
| T-20 VT-05/VT-14 matrix | Rebuild, visibility and cache-pressure cases exist per the T-20 evidence files | Yes | Map each VT-05/VT-14 case to an existing test and add any missing case before T-20 acceptance. |
| T-19 report | Drafted (`docs/evidence/r1-acceptance-report.md`) | Yes (drafting only) | Refresh after each benchmark change; acceptance is the owner's. |
| T-62 | Open | No | Owner/admin verification of the reporting route. |

Nothing in this audit lowers or reinterprets a target. The full gate on this lane still carries
the legacy BM-06 process-test deadline failure (Decision 0273), which fails on unchanged code
too and is environmental.
