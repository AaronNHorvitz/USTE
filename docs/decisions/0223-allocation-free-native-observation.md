# Decision 0223: Allocation-free positive-cache development observation

Date: 2026-09-20

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0222 once with the unchanged supervised 20,000-entity/200,000-relationship wide
positive-cache protocol and compare it to D0221. Preserve the exact fixture, oracle, cache split,
deadline, query order, work limits and population validation. This is a sequential cross-binary
development observation with uncontrolled kernel/device caches, not a causal or repeated campaign.

The D0222 round completed in 673,948 ms versus D0221's 707,800 ms (-4.78%). After removing only
timing, percentile and RSS fields, both reports are structurally equal: 96 warm-ups, 768 measured
executions, 40 populations, the output digest, successful work, cache counters, adapter I/O and
vault work all match. Retained all-class successful p99 by depth changed from 8.923552 /
641.845829 / 382.309391 / 1560.853940 ms to 9.397449 / 605.082078 / 387.319532 /
1302.698520 ms. One-hop and three-hop worsened 5.31% and 1.31%; four-hop improved 16.54% but still
misses the unchanged 250 ms target by more than five times. Do not claim a pass or causal speedup.

The release process used 265,616 KiB peak RSS, 809.16 user / 46.55 system seconds and 859.28
seconds wall time, with zero process swaps. It ran alone under the established enclosing systemd
scope and inherited 4 GiB virtual-address limit. The shared scope briefly crossed its 5 GiB soft
watermark but stayed below maximum with zero cgroup swap/OOM; that shared peak is not process RSS.
Source certificate and executable hashes remained unchanged. The complete report is retained in
[`allocation-free-positive-cache-sampling.json`](../evidence/allocation-free-positive-cache-sampling.json).
Keep the 64 MiB page-only default and all T-20 qualification requirements.
