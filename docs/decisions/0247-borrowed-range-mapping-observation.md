# Decision 0247: Borrowed-range-mapping development observation

Date: 2026-09-22

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0246 once with the unchanged retained 20,000-entity/200,000-relationship fixture,
oracle, query order, 96/128/32 MiB page/lookup/range split and supervised 30-second per-query
protocol. This is a sequential development observation with uncontrolled kernel/device caches, not
a causal or repeated campaign.

The parent accepted 96 warm-ups, 768 measured executions, 40 latency groups and output digest
`aec16fdc8a1630a97eace654862a192929550c5fb771aed0862a9bed67d81584`. After removing only elapsed,
percentile and RSS fields, the complete report matches Decision 0245 exactly at SHA-256
`c472dc13e72e3ed3fd5cd46776824223c89213f9c2dc9ec25bdcc653b633abd9`. The independently selected
semantic subset also remains
`b80069dd6e102490fcf979457db87f97bb8762c21f169c52a0602a8e1305a1a9`. Thus query outcomes,
logical results, cache counters, adapter and vault work, residency and pressure are unchanged.

The 32 MiB range partition again peaked at 29,231,596 bytes with zero evictions and zero evicted
bytes. The empty half missed 1,074,437 ranges; the retained half hit all 1,074,437 with zero misses
and performed zero adapter reads. Empty reads remained 11,240,354. Decision 0246 therefore did not
change externally reported work or the previously observed page/range tradeoff.

The round took 582,786 ms, 0.13% below Decision 0245. Retained all-class successful p99 by depth was
**3.940148 / 126.474289 / 89.712790 / 489.870038 ms**, changing by -0.76%, +2.96%, +4.38% and
-10.52%. One sequential observation cannot attribute these mixed timing changes to the removed
intermediary copies. Four-hop remains 1.96 times the unchanged 250 ms target, so no latency target
passes.

The release process used 266,096 KiB peak RSS, 711.62 user / 54.42 system seconds and 768.81
seconds wall time with zero process swaps. It ran alone under the 4 GiB address-space limit and
verified enclosing 5/6 GiB high/max and 512 MiB swap caps. Cumulative cgroup peaks remained
5,373,222,912 bytes memory and 286,691,328 bytes swap. Soft-limit events increased by 1,623 during
the observation; maximum-limit, OOM, OOM-kill and CPU-throttle counters remained zero. Source
certificate `b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`, oracle bundle
`f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`792aea102771613a3e8283bd0f38de99c08d86aebc571a2530742fd9515a72b0` stayed unchanged.

The complete report is retained in
[`borrowed-range-mapping-sampling.json`](../evidence/borrowed-range-mapping-sampling.json). Keep all
qualification prerequisites and existing defaults. The next bounded work should inspect repeated
compact-result construction, record decoding and authorization on the zero-I/O retained four-hop
path before selecting another implementation or telemetry increment.
