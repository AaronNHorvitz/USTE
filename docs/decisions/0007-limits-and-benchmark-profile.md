# Decision 0007 — Limits, benchmark budgets and reference runner

Date: 2026-09-16

Status: accepted as profile `limits-v1`; budgets are pass/fail targets, not measurements.

Closes D-03 and is the decision artifact for T-05. Actual results must distinguish cold/warm
caches and cannot lower these values without a versioned decision.

## Reference runner

`kinoite-reference-2026-09` is x86_64 Fedora 44 (Kinoite-family immutable workstation),
kernel 7.1.10, Btrfs 7.1 on local NVMe with compression `zstd:1`, Intel Core i9-13900KF
(24 cores/32 threads), 64 GiB RAM, 8 GiB swap, Rust/Cargo 1.95.0. Benchmarks reserve 16
logical CPUs, 24 GiB RAM and 200 GiB free disk, use a release build, run with encryption,
authorization and durable flushes enabled, and record firmware/device and tree/lock hashes.
CPU governor and background load are recorded, not silently changed. Each latency workload
has one warm-up and at least five 60-second samples; recovery has 30 independent trials.

## Hard admission/resource caps

| Resource | `limits-v1` cap |
|---|---:|
| transaction encoded request / operations / new references | 16 MiB / 10,000 / 100,000 |
| inline value / nesting / collection entries | 1 MiB / 32 / 65,536 |
| single blob / namespace logical bytes | 16 GiB / 1 TiB default |
| upload, blob and IPC buffer | 8 MiB / 1 MiB / 1 MiB |
| writer queue / concurrent uploads / pinned readers | 1,024 / 32 / 256 |
| reader pin / query wall time / query scratch / result bytes | 60 s / 30 s / 256 MiB / 64 MiB |
| traversal visits/depth/results | 1,000,000 / 32 / 100,000 |
| spatial candidates/nearest k/frame depth | 1,000,000 / 10,000 / 32 |
| observations per entity response / interpolation gap | 100,000 / 24 h |
| worker elapsed/CPU/RSS/output | 300 s / 240 s / 2 GiB / 512 MiB |
| archive depth/entries/expanded bytes/ratio | 4 / 10,000 / 4 GiB / 100:1 |
| image pixels/dimension | 100,000,000 / 32,768 |
| audio/video duration/frames | 6 h / 100,000 sampled frames |
| branches per namespace / run ticks / emitted events | 1,000 / 10,000,000 / 10,000,000 |
| contact bodies/pairs per tick/substeps | 10,000 / 5,000,000 / 16 |
| batch rows / rows per atomic batch / error report | 1 billion / 10,000 / 10,000 entries |
| idempotency outcomes / active subscriptions per namespace | 10 million / 1,024 |

All byte counts are exact octets and all queues backpressure. Per-principal/namespace policies
may lower caps. Requests cannot raise them.

## Performance budgets

On the reserved reference runner: BM-01 warm 1-hop p99 <= 20 ms and 4-hop p99 <= 250 ms;
BM-02 single durable commit p99 <= 50 ms and >= 2,000 events/s in batches; BM-03 as-of p99
<= 500 ms; BM-04 streaming ingest >= 250 MiB/s with peak RSS <= 1 GiB above cache budget;
BM-06 recovery of 10 million events from a checkpoint <= 120 s. BM-10 warm radius/box p99
<= 100 ms, nearest p99 <= 250 ms, and exact-final-predicate recall 100%. BM-11 state-at-time
p99 <= 100 ms and bounded history p99 <= 500 ms. BM-12 runs 1,000 kinematic bodies at >=
100,000 body-steps/s and refuses over-budget dense contacts before publication. BM-13 keeps
ordinary point-read p99 <= 2x its isolated p99, writer queue within cap and RSS <= 24 GiB.

Cold results, p50/p95, disk/index amplification, candidates, cancellation latency and
background impact are always reported even where no release threshold is yet set. A workload
larger than available cache/RAM is mandatory for R3 scalability claims.

## Fixture manifests

R0 freezes deterministic generators rather than committing huge corpora. Generator
`synthetic-v1` uses BLAKE3 counter streams from the published 32-byte seed, stable IDs and no
wall clock. Exact sizes are BM-01 100k/1m; BM-03 10m events; BM-04 100k 4 KiB blobs plus one
20 GiB stream; BM-10 1m points (half uniform, half 100 clusters); BM-11 10m observations with
5% late, 1% corrected, 1% duplicates; BM-12 100/1k/10k bodies; BM-13 the documented world
with 100k moving objects and simultaneous 10k-row batches.

`acceptance/r0/benchmark-manifest.tsv` is the machine-readable registry. Initial values are
targets based on the intended local workstation class, not achieved results. Bench reports
must include failures; no marketing claim follows from this decision.
