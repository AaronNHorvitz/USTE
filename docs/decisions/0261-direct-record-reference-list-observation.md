# Decision 0261: Direct record-reference list development observation

Date: 2026-09-22

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0260 once with the unchanged retained 20,000-entity/200,000-relationship fixture,
oracle, query order, 96/128/32 MiB page/lookup/range split and supervised 30-second per-query
protocol. This is a sequential development observation with uncontrolled kernel/device caches, not
a causal or repeated campaign.

The parent accepted 96 warm-ups, 768 measured executions, 40 latency groups and output digest
`aec16fdc8a1630a97eace654862a192929550c5fb771aed0862a9bed67d81584`. After removing only elapsed,
percentile and RSS fields, the complete report matches Decision 0259 exactly at SHA-256
`c472dc13e72e3ed3fd5cd46776824223c89213f9c2dc9ec25bdcc653b633abd9`. Thus query outcomes,
logical results, cache counters, adapter and vault work, residency and pressure are unchanged.

The 32 MiB range partition again peaked at 29,231,596 bytes with zero evictions and zero evicted
bytes. The retained half hit all 1,074,437 ranges with zero misses and performed zero adapter reads.
The empty half performed 11,240,354 adapter reads. Terminal range accounting remained 8,789,091
bytes against the 33,554,432-byte budget, with 1,348,781 misses and no oversized refusal.

The round took 545,588 ms, 0.08% below Decision 0259. Retained all-class successful p99 by depth was
**2.682137 / 91.463877 / 64.022884 / 386.697762 ms**. The respective depth changes were -0.86%,
-0.46%, +3.43% and +1.81%. One sequential uncontrolled observation cannot attribute these timing
changes to direct record-reference list decoding. Four-hop remains 1.55 times the unchanged 250 ms
target, so no latency target passes.

The release process used 265,908 KiB peak RSS, 666.42 user / 54.24 system seconds and 723.35 seconds
wall time with zero process swaps. It ran alone under the 4 GiB address-space limit and verified
enclosing 5/6 GiB high/max and 512 MiB swap caps. Cumulative cgroup peaks remained 5,373,222,912
bytes memory and 286,691,328 bytes swap. Soft-limit events increased by 1,599; maximum-limit, OOM,
OOM-kill, socket-memory-throttle and CPU-throttle counters did not increase. Commit
`9096e69bc6ec784390ba4969f35c521465699783`, retained source certificate
`b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`, oracle bundle
`f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`a416773bb03d5b415b472a90891a9452d98b56f28c9f77e8517eb28f2d0c3c84` were pinned. The accepted
report's SHA-256 is `9f81ac25c84215c813e5691b0462c4eed8f76aa6f7d677934e9dda63da2efed2`.

The complete report is retained in
[`direct-record-reference-list-sampling.json`](../evidence/direct-record-reference-list-sampling.json).
Local artifacts remain under `experiments/t20-bench/target/native-d260-refs.16gJC0`. Keep all
qualification prerequisites and existing defaults. The next bounded work should inspect another
measured zero-I/O retained-path cost before changing implementation.
