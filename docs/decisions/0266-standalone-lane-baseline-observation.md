# Decision 0266: Standalone-lane baseline and retained-path profile observation

Date: 2026-09-23

Status: Development observation complete; no benchmark qualification or default change.

Reproduce the Decision 0265 observation once on a standalone clone at `2263dc5` inside a bounded
lane (5/6 GiB memory high/max, 512 MiB swap, three CPUs, 4 GiB process address-space limit) before
changing any implementation, and locate where the retained four-hop time goes. This is a
sequential development observation with uncontrolled kernel/device caches, not a causal or repeated
campaign.

The retained fixture was not copied from the reference host. It was regenerated with the pinned
`linux-packed-create --entities 20000` command under a fresh synthetic owner-only password. The
regenerated store reports `v1_state_digest`
`6880abb54859af231d877c8bbc1a9a665127a9953228c90e4a8278c048d1ecab`, the exact digest recorded for
the retained fixture in Decision 0206. Regenerated `manifest`, `oracle-summary` and
`oracle-bundle` outputs hash to the recorded `e7e0e0c6…`, `aa3e6939…` and `f667f442…` values. The
`CERTIFICATES` file hash differs from the recorded `b8fea4e2…` only because the store was encrypted
under fresh key material; logical identity is proven by the state digest. Create took 144.28 s at
347,864 KiB peak RSS.

The unchanged `linux-packed-wide-medium-range-pressure-sample` protocol (96/128/32 MiB split,
96 warm-ups, 768 measured executions, 40 latency groups, supervised 30-second deadline) accepted
output digest `aec16fdc8a1630a97eace654862a192929550c5fb771aed0862a9bed67d81584`, identical to
Decisions 0253 through 0265. Lookup hits/misses (39,002,722 / 17,960,242), range hits/misses
(1,074,437 / 1,348,781), zero evictions, range high-water 29,231,596 bytes, 11,240,354 empty-half
adapter reads and zero retained-half adapter reads are all identical to Decision 0265. The
byte-exact recipe behind the recorded `c472dc13…` comparison digest was not recoverable, so this
lane fixes its own recipe: remove only `elapsed_milliseconds`, `p50/p95/p99_nanoseconds`,
`current_rss_kib` and `process_peak_rss_kib`, serialize with sorted keys and compact separators,
then SHA-256. Under that recipe the retained Decision 0265 report and this lane's report both hash
to `0334c5cd6b50ef077ae925c457643d25c61410870cd3a762b04fbdbde0b6e4de`, so every semantic,
counter and physical-work field matches exactly.

The round took 567,391 ms, 4.51% above Decision 0265 on the reference host. Retained all-class
successful p99 by depth was **2.765 / 100.721 / 67.027 / 374.535 ms** against Decision 0265's
2.634 / 87.526 / 61.596 / 368.917 ms; the lane is a few percent slower per query, which is
consistent with its three-CPU quota and shared host. Four-hop remains 1.50 times the unchanged
250 ms target, so no latency target passes. The release process used 265,876 KiB peak RSS and
694.44 user / 53.19 system seconds over 749.71 seconds wall.

A second, otherwise identical run was sampled with `gdb` backtraces at one-second intervals (675
samples; 637 inside query execution). Samples were split by whether the stack contained physical
lookup, cursor, page-read or decrypt frames (empty half, 510 samples) or none of them (retained
half, 127 samples). In the retained subset, 78% of samples were inside `lookup_cached_with`,
63% inside `LookupCache::get_with`, and 32% inside `decode_stored_record`; the range-cache scan
path accounted for 2.4% and the client harness outside the engine read for about 12%. The
dominant non-decode cost is the positive lookup cache's ordered-map bookkeeping: each hit
performed one 175-byte identity comparison chain, one search of a ~54,000-entry `BTreeMap` whose
keys live behind `Arc` indirection, a `BTreeMap<u128>` removal and insertion for recency, and two
further `get_mut` descents, and each visit performs two such hits. The `gdb` attach pauses
perturb timing, so the profiled run's latencies are not reported as an observation.

The complete baseline report is retained in
[`standalone-lane-baseline-sampling.json`](../evidence/standalone-lane-baseline-sampling.json).
The next bounded work replaces the positive lookup cache index with a hash map plus an intrusive
exact least-recently-used list while preserving eviction order, counters and logical accounting.
