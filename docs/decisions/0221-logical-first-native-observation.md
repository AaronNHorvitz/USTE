# Decision 0221: Logical-first positive-cache development observation

Date: 2026-09-20

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0220 once with the existing supervised 20,000-entity/200,000-relationship wide
positive-cache command. Keep the retained D0219 report as the historical identity-first comparison,
but do not call the cross-binary, post-restart observation causal, a repeated campaign or a target
pass. Preserve the exact fixture, oracle, cache partition, query order, work limits, deadline and
population validation. Archive the complete new report even if it regresses.

The D0220 run completed one round in 707,800 ms versus D0219's 736,625 ms, a 3.91% decrease. After
removing only elapsed-time, latency-percentile and RSS fields, the reports are structurally equal:
96 warm-ups, 768 measured executions, 40 population groups, output digest, successful work, page
and lookup counters, adapter I/O and vault work all match exactly. This is useful consistency
evidence, not a causal speedup claim; the host and kernel/device caches were uncontrolled and the
runs used different binaries across a restart.

Retained all-class successful p99 by depth changed from 9.455773 / 675.691962 / 444.438316 /
1525.282292 ms to 8.923552 / 641.845829 / 382.309391 / 1560.853940 ms. Four-hop worsened 2.33%
and remains far above the unchanged 250 ms target. The key-order change therefore does not close
T-20's latency gap. Keep the 64 MiB page-only default, all qualification requirements and the
failed-target evidence. The next bounded implementation should remove avoidable positive-cache
hit allocation/copy work or measure another explicit bottleneck before any new capacity campaign.

The release process used 266,008 KiB peak RSS, 844.89 user / 45.50 system seconds and 894.23
seconds wall time. It ran inside the verified enclosing systemd scope with a 4 GiB inherited
virtual-address limit and recorded zero swap or cgroup pressure/OOM events. The enclosing scope's
4,246,384,640-byte shared peak includes session/file-cache state and is not workload RSS. The
source certificate and executable hashes remained unchanged. The full report is retained in
[`logical-first-positive-cache-sampling.json`](../evidence/logical-first-positive-cache-sampling.json).
