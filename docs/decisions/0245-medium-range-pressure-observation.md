# Decision 0245: Medium-range-pressure development observation

Date: 2026-09-22

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0244 once with the unchanged retained 20,000-entity/200,000-relationship fixture,
oracle, query order and supervised 30-second per-query protocol. Preserve the 256 MiB total as
96 MiB pages, 128 MiB positive lookups and 32 MiB complete ranges. This is a sequential
development observation with uncontrolled kernel/device caches, not a causal or repeated campaign.

The parent accepted the complete report: 96 warm-ups, 768 measured executions, 40 latency groups
and output digest `aec16fdc8a1630a97eace654862a192929550c5fb771aed0862a9bed67d81584`.
An independently selected semantic subset containing fixture/oracle identity, setup state,
warm-up outcomes, query classifications/counts, logical results and output digests matches both
Decisions 0233 and 0243 at SHA-256
`b80069dd6e102490fcf979457db87f97bb8762c21f169c52a0602a8e1305a1a9`. Changed cache and physical
work are deliberately outside that comparison.

The 32 MiB range partition retained this measured working set without pressure eviction. Its
maximum accounted size was 29,231,596 bytes, 4,322,836 bytes below the 33,554,432-byte budget.
Warm-up, empty and retained observations all recorded zero evictions and zero evicted bytes; the
terminal cumulative counters also remained zero. The empty half missed 1,074,437 ranges and the
retained half hit all 1,074,437 with zero misses. Consequently, the retained half performed zero
filesystem-adapter reads, compared with 1,557,673 under Decision 0243's saturated 16 MiB range
partition.

Reducing pages from 112 MiB to 96 MiB increased empty-half adapter reads from 9,600,066 to
11,240,354. Total empty-plus-retained reads were 11,240,354, 0.74% above Decision 0243 and 5.43%
above Decision 0233. The result therefore establishes sufficiency only for the observed range
working set; it does not establish that the 96/128/32 MiB split is a better default.

The round took 583,539 ms, 8.88% below Decision 0243 and 10.43% below Decision 0233. Retained
all-class successful p99 by depth was **3.970467 / 122.834907 / 85.951997 / 547.486501 ms**.
Compared with Decision 0243, those values changed by -1.12%, -1.17%, +0.78% and -58.38%.
Sequential uncontrolled timing cannot establish causality. Four-hop remains 2.19 times the
unchanged 250 ms target, so no latency target passes.

The release process used 265,592 KiB peak RSS, 716.53 user / 54.47 system seconds and 772.74
seconds wall time with zero process swaps. It ran alone under the 4 GiB address-space limit and
verified enclosing 5/6 GiB high/max and 512 MiB swap caps. Cumulative cgroup peaks remained
5,372,850,176 bytes memory and 286,691,328 bytes swap; soft-limit events stayed at 106,607 and
maximum-limit, OOM, OOM-kill and CPU-throttle counters remained zero. Source certificate
`b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`, oracle bundle
`f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`7d39e33978005018b1eb85620bb150d58e20ff3680b20c205f3de070092148b0` stayed unchanged.

The complete report is retained in
[`medium-range-pressure-sampling.json`](../evidence/medium-range-pressure-sampling.json). Keep all
qualification prerequisites and existing defaults. The next bounded work should inspect the
zero-adapter-I/O retained four-hop path before choosing an implementation or telemetry increment;
do not infer that a larger range partition or a qualifying campaign is warranted.
