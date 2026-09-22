# Decision 0233: Compact expansion development observation

Date: 2026-09-21

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0232 once with the unchanged supervised 20,000-entity/200,000-relationship wide
positive-cache protocol and compare it to D0231. Preserve the retained fixture, oracle, 128/128 MiB
cache split, 30-second per-query deadline, query order, work limits and population validation. This
is a sequential cross-binary development observation with uncontrolled kernel/device caches, not a
causal or repeated campaign.

The D0232 round completed in 651,479 ms versus D0231's 655,451 ms (-0.61%). After removing only
timing, percentile and RSS fields, both reports have the same normalized SHA-256
`c848e2b1573de9733014d908a1d51a281fb5c6dc932bfdc39559ca0f0fb9fe78`: 96 warm-ups, 768
measured executions, 40 populations, output digest, successful work, cache counters, adapter I/O
and vault work all match. Retained all-class successful p99 by depth changed from 8.885070 /
576.032616 / 360.335620 / 1296.426702 ms to **8.940083 / 568.149641 / 363.751144 /
1263.669322 ms**. Depths two and four improved 1.37% and 2.53%; depths one and three regressed
0.62% and 0.95%. Four-hop remains more than five times the unchanged 250 ms target. Do not claim a
pass or causal improvement.

The release process used 265,844 KiB peak RSS, 770.55 user / 50.51 system seconds and 823.43
seconds wall time, with zero process swaps. Peak RSS is 152 KiB above D0231 and does not demonstrate
a memory reduction. The process ran alone inside the verified enclosing scope with the inherited
4 GiB process address-space limit. The shared scope's cumulative memory and swap peaks stayed at
5,372,850,176 and 286,691,328 bytes; soft-limit events stayed at 45,222, while maximum-limit/OOM
and CPU-throttle events remained zero. Shared values are not process RSS. Source certificate
`b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`, oracle bundle
`f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`ee5aceba4cb42f33b8a99fd436325581015a0f9d81df2d424b46c4ab8b31b468` stayed unchanged.

The complete report is retained in
[`compact-expansion-scan-candidates-sampling.json`](../evidence/compact-expansion-scan-candidates-sampling.json).
Keep the 64 MiB page-only default and every T-20 qualification prerequisite. Further work should
target repeated authenticated prefix I/O with explicit budget/accounting design rather than infer a
benefit from this mixed observation.
