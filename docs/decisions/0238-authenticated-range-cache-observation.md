# Decision 0238: Authenticated range-cache development observation

Date: 2026-09-22

Status: Development observation complete; cache split rejected; no benchmark qualification.

Measure Decision 0237 once with the unchanged retained 20,000-entity/200,000-relationship fixture,
oracle, query order and supervised 30-second per-query protocol. The new wide range profile preserves
the 256 MiB total but splits it into 64 MiB pages, 64 MiB positive lookups and 128 MiB complete
ranges. This is a sequential development observation with uncontrolled kernel/device caches, not a
causal or repeated campaign.

The parent accepted the complete `bm01-linux-packed-wide-range-sampling-v1` report: 96 warm-ups,
768 measured executions, 40 latency groups and output digest
`aec16fdc8a1630a97eace654862a192929550c5fb771aed0862a9bed67d81584`. A semantic subset containing
the oracle/result profile, warm-up outcomes, group shape, output, execution count, successful visits
and successful logical bytes matches Decision 0233 with SHA-256
`1ee6e2472d2cf96e80492d72ad1588c9133eb3be2014e8f02562bbaec51f76e1`. Physical work cannot match
because the schema and page/lookup/range partitions intentionally differ.

The range partition retained 3,041 ranges / 37,388 entries in 8,789,091 bytes. Its measured empty
half recorded 1,074,437 misses; the identical retained half recorded exactly 1,074,437 hits and zero
misses, evictions or oversized bypasses. Warm-up plus measured deltas reconcile to terminal range
counters. Thus complete-range reuse works for this workload, but the fixed 128 MiB allocation is far
larger than observed residency.

The displaced page/lookup capacity dominates the mixed result. Positive-lookups evicted 5,633,779
times in the empty half and 10,187,643 times in the retained half; total adapter reads were
17,216,275 empty and 10,104,149 retained, 108% and 322% above Decision 0233. The round took
1,074,832 ms versus 651,479 ms (+64.98%). Retained all-class successful p99 by depth changed from
8.940083 / 568.149641 / 363.751144 / 1263.669322 ms to **4.062156 / 126.562816 / 88.816068 /
4172.023623 ms**: depths one through three improved 54.56%, 77.72% and 75.58%, while depth four
regressed 230.15% and remains far above the unchanged 250 ms target. Reject this 64/64/128 split;
one observation does not establish causality or a default.

The release process used 265,912 KiB peak RSS, 1,179.72 user / 115.78 system seconds and 1,299.82
seconds wall time with zero process swaps. It ran alone under the inherited 4 GiB address-space
limit and verified enclosing 5/6 GiB high/max and 512 MiB swap caps. Post-run cumulative cgroup
peaks were 5,372,850,176 bytes memory and 286,691,328 bytes swap; maximum-limit, OOM, OOM-kill and
CPU-throttle counters remained zero. Source certificate
`b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`, oracle bundle
`f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`c36113295e0cd01bfa29269e80867ef37b46dae7fd51c33b70222517259d248b` stayed unchanged.

The complete report is retained in
[`authenticated-range-cache-sampling.json`](../evidence/authenticated-range-cache-sampling.json).
Keep every T-20 qualification prerequisite and the existing defaults. The next bounded experiment
may add a distinct, explicitly named development profile that restores the prior 128 MiB lookup
partition and gives ranges a small evidence-backed partition; it must preserve total accounting,
strict schema validation and supervision before another observation.
