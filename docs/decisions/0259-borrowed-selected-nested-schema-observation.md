# Decision 0259: Borrowed-selected-nested-schema development observation

Date: 2026-09-22

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0258 once with the unchanged retained 20,000-entity/200,000-relationship fixture,
oracle, query order, 96/128/32 MiB page/lookup/range split and supervised 30-second per-query
protocol. This is a sequential development observation with uncontrolled kernel/device caches, not
a causal or repeated campaign.

The parent accepted 96 warm-ups, 768 measured executions, 40 latency groups and output digest
`aec16fdc8a1630a97eace654862a192929550c5fb771aed0862a9bed67d81584`. After removing only elapsed,
percentile and RSS fields, the complete report matches Decision 0257 exactly at SHA-256
`c472dc13e72e3ed3fd5cd46776824223c89213f9c2dc9ec25bdcc653b633abd9`. Thus query outcomes,
logical results, cache counters, adapter and vault work, residency and pressure are unchanged.

The 32 MiB range partition again peaked at 29,231,596 bytes with zero evictions and zero evicted
bytes. The retained half hit all 1,074,437 ranges with zero misses and performed zero adapter reads.
The empty half performed 11,240,354 adapter reads. Terminal range accounting remained 8,789,091
bytes against the 33,554,432-byte budget, with 1,348,781 misses and no oversized refusal.

The round took 546,001 ms, 0.84% above Decision 0257. Retained all-class successful p99 by depth
was **2.705347 / 91.887128 / 61.900717 / 379.813938 ms**. The respective depth changes were
+1.70%, +4.79%, +2.74% and +3.04%. One sequential uncontrolled observation cannot attribute these
timing changes to borrowing selected nested schema. Four-hop remains 1.52 times the unchanged 250
ms target, so no latency target passes.

The release process used 265,612 KiB peak RSS, 668.75 user / 53.96 system seconds and 724.77
seconds wall time with zero process swaps. It ran alone under the 4 GiB address-space limit and
verified enclosing 5/6 GiB high/max and 512 MiB swap caps. Cumulative cgroup peaks remained
5,373,222,912 bytes memory and 286,691,328 bytes swap. Soft-limit events increased by 1,510;
maximum-limit, OOM, OOM-kill, socket-memory-throttle and CPU-throttle counters did not increase.
Commit `56db221a86e3f777705ce3c30afd5182bf328e8a`, retained source certificate
`b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`, oracle bundle
`f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`16cdc9591370f87dba9a9cbecbe44c194bc80d9e4073e8b2d9867e974392fa06` were pinned. The accepted
report's SHA-256 is `e7ccea8b3cc1e2db91e898381c6e40a5d10426bb25dfd09c17b02af05f259d1e`.

The complete report is retained in
[`borrowed-selected-nested-schema-sampling.json`](../evidence/borrowed-selected-nested-schema-sampling.json).
Local artifacts remain under `experiments/t20-bench/target/native-d258-nested.TKcFsg`. Keep all
qualification prerequisites and existing defaults. The next bounded work should inspect another
measured zero-I/O retained-path cost before changing implementation; dynamic properties and opaque
objects remain deliberately owned.
