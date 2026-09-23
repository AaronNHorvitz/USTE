# Decision 0265: Borrowed singleton selected-map development observation

Date: 2026-09-22

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0264 once with the unchanged retained 20,000-entity/200,000-relationship fixture,
oracle, query order, 96/128/32 MiB page/lookup/range split and supervised 30-second per-query
protocol. This is a sequential development observation with uncontrolled kernel/device caches, not
a causal or repeated campaign.

The parent accepted 96 warm-ups, 768 measured executions, 40 latency groups and output digest
`aec16fdc8a1630a97eace654862a192929550c5fb771aed0862a9bed67d81584`. After removing only elapsed,
percentile and RSS fields, the complete report matches Decision 0263 exactly at SHA-256
`c472dc13e72e3ed3fd5cd46776824223c89213f9c2dc9ec25bdcc653b633abd9`. Thus query outcomes,
logical results, cache counters, adapter and vault work, residency and pressure are unchanged.

The 32 MiB range partition again peaked at 29,231,596 bytes with zero evictions and zero evicted
bytes. The retained half hit all 1,074,437 ranges with zero misses and performed zero adapter reads.
The empty half performed 11,240,354 adapter reads. Terminal range accounting remained 8,789,091
bytes against the 33,554,432-byte budget, with 1,348,781 misses and no oversized refusal.

The round took 542,925 ms, 0.25% below Decision 0263. Retained all-class successful p99 by depth was
**2.634285 / 87.525892 / 61.595940 / 368.916668 ms**. The respective depth changes were -4.79%,
-4.09%, -2.49% and -3.39%. One sequential uncontrolled observation cannot attribute these timing
changes to the borrowed singleton representation. Four-hop remains 1.48 times the unchanged 250
ms target, so no latency target passes.

The release process used 265,820 KiB peak RSS, 664.74 user / 53.84 system seconds and 720.28 seconds
wall time with zero process swaps. It ran alone under the 4 GiB address-space limit and verified
enclosing 5/6 GiB high/max and 512 MiB swap caps. Cumulative cgroup peaks remained 5,373,222,912
bytes memory and 459,055,104 bytes swap. Soft-limit, maximum-limit, OOM, OOM-kill,
socket-memory-throttle and CPU-throttle counters did not increase. Commit
`ede012f55cbb567ed0cfe956f04d92e220c17fa9`, retained source certificate
`b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`, oracle bundle
`f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`692ece9bcf58ec7113b4fd29b247af75ea77d2e6839be9697a630205f9262648` were pinned. The accepted
report's SHA-256 is `97b2070d85a9ae8437afea91ea0a19df43bca7d56cacc2d1bb15f6fbaca3dcf7`.

The complete report is retained in
[`borrowed-singleton-selected-map-sampling.json`](../evidence/borrowed-singleton-selected-map-sampling.json).
Local artifacts remain under `experiments/t20-bench/target/native-d264-singleton.n8JR0V`. Keep all
qualification prerequisites and existing defaults. The next bounded work should inspect another
measured zero-I/O retained-path cost before changing implementation.
