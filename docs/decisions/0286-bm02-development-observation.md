# Decision 0286: BM-02 development observation, 2,000 commits and 64,000 batched events

Date: 2026-09-26

Status: Development observation; no benchmark qualification, target or protocol change.

The [Decision 0285](0285-bm02-development-runner.md) runner was run three times in the
standalone lane (6 GiB, three CPUs, shared heavy-work reservation). Each run used a fresh store
on local Btrfs, `--single 2000 --batches 64 --batch-events 1000`, and the release binary
`6c353b85cb6440d07563f17c388f7382c9b8c753a42c929e3b11592f0a31c826`. Kernel, filesystem and
device caches were uncontrolled. All three runs produced outcome digest
`3b6113504bab85bc81cfa9982c853de887b0cf7574bcec675e2880fc7725df2f`, reached final revision
2,065, created 65,500 entities and committed 11,675,736 request bytes.

| Measure (ms unless noted) | Run 1 | Run 2 | Run 3 |
|---|---|---|---|
| single commit p50 / p99 / max | 1.186 / 1.782 / 40.645 | 1.183 / 1.514 / 40.306 | 1.180 / 1.756 / 37.132 |
| correction commit p99 | 2.172 | 1.514 | 1.677 |
| interleaved read p50 / p99 (view + point read) | 0.073 / 0.372 | 0.072 / 0.197 | 0.071 / 0.248 |
| 1,000-event batch commit p50 / p99 | 5.915 / 7.048 | 5.893 / 7.003 | 5.879 / 6.124 |
| batch events/s, commit time | 167,389 | 169,148 | 170,250 |
| batch events/s, phase wall-clock | 142,053 | 143,255 | 144,037 |
| single transactions/s, commit time | 784 | 795 | 776 |
| current / peak RSS (KiB) | 171,264 / 264,876 | 171,300 / 264,848 | 171,252 / 264,804 |

The 37–41 ms single-commit maxima are one-off outliers that were not attributed. Peak RSS is set
by the portable Argon2id key wrap during store creation, which is why it matches the Decision
0283 probe. It does not reflect the commit workload.

On this development plan, durable single-commit p99 is about 1.5–2.2 ms against the 50 ms target,
and batched ingestion is about 70 times the 2,000 events/s target. These figures are for a
single-threaded writer with same-thread reads, the in-memory graph reducer, a small store, and
an unpinned workload shape. They are not the qualifying BM-02 profile, host or protocol. BM-02
remains unqualified until the owner pins its manifest (counts, reader concurrency, duration)
and it is run on the reserved host with concurrent readers and queue/backpressure reporting.
