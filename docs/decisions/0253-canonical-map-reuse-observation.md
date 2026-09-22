# Decision 0253: Canonical-map-reuse development observation

Date: 2026-09-22

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0252 once with the unchanged retained 20,000-entity/200,000-relationship fixture,
oracle, query order, 96/128/32 MiB page/lookup/range split and supervised 30-second per-query
protocol. This is a sequential development observation with uncontrolled kernel/device caches, not
a causal or repeated campaign.

The parent accepted 96 warm-ups, 768 measured executions, 40 latency groups and output digest
`aec16fdc8a1630a97eace654862a192929550c5fb771aed0862a9bed67d81584`. After removing only elapsed,
percentile and RSS fields, the complete report matches Decision 0251 exactly at SHA-256
`c472dc13e72e3ed3fd5cd46776824223c89213f9c2dc9ec25bdcc653b633abd9`. Thus query outcomes,
logical results, cache counters, adapter and vault work, residency and pressure are unchanged.

The 32 MiB range partition again peaked at 29,231,596 bytes with zero evictions and zero evicted
bytes. The retained half hit all 1,074,437 ranges with zero misses and performed zero adapter reads.
The empty half performed 11,240,354 adapter reads. Terminal range accounting remained 8,789,091
bytes against the 33,554,432-byte budget, with 1,348,781 misses and no oversized refusal.

The round took 570,790 ms, 5.99% below Decision 0251. Retained all-class successful p99 by depth
was **3.155762 / 104.617431 / 71.272343 / 421.852798 ms**. The respective depth changes were
-23.58%, then -19.46%, -24.11% and -19.20%. One sequential uncontrolled observation cannot
attribute these timing changes to removal of the transient map nodes. Four-hop remains 1.69 times
the unchanged 250 ms target, so no latency target passes.

The release process used 265,604 KiB peak RSS, 696.14 user / 55.45 system seconds and 753.42
seconds wall time with zero process swaps. It ran alone under the 4 GiB address-space limit and
verified enclosing 5/6 GiB high/max and 512 MiB swap caps. Cumulative cgroup peaks remained
5,373,222,912 bytes memory and 286,691,328 bytes swap. Soft-limit events did not increase during the
observation; maximum-limit, OOM, OOM-kill and CPU-throttle counters remained zero. Commit
`5a90179d3239d68f00837722ecf8020896660535`, source certificate
`b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`, oracle bundle
`f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`e05315255194d6b5c5aac26cd899342dc6eba4447d8e408cfdccdd679a4ff734` were pinned. The accepted
report's SHA-256 is `4595c5c252a1c00a583a3c4809cb293aab43bbdfcf42851a313e6f6a18951f62`.

The complete report is retained in
[`canonical-map-reuse-sampling.json`](../evidence/canonical-map-reuse-sampling.json). Local
artifacts remain under `experiments/t20-bench/target/native-d252-canonical.skpZ6h`. Keep all
qualification prerequisites and existing defaults. The next bounded work should inspect the
remaining generic-value construction performed before stored-record materialization, without
adding unbudgeted decoded plaintext retention.
