# Decision 0249: Borrowed-authorization development observation

Date: 2026-09-22

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0248 once with the unchanged retained 20,000-entity/200,000-relationship fixture,
oracle, query order, 96/128/32 MiB page/lookup/range split and supervised 30-second per-query
protocol. This is a sequential development observation with uncontrolled kernel/device caches, not
a causal or repeated campaign.

The parent accepted 96 warm-ups, 768 measured executions, 40 latency groups and output digest
`aec16fdc8a1630a97eace654862a192929550c5fb771aed0862a9bed67d81584`. After removing only elapsed,
percentile and RSS fields, the complete report matches Decision 0247 exactly at SHA-256
`c472dc13e72e3ed3fd5cd46776824223c89213f9c2dc9ec25bdcc653b633abd9`. Thus query outcomes,
logical results, cache counters, adapter and vault work, residency and pressure are unchanged.

The 32 MiB range partition again peaked at 29,231,596 bytes with zero evictions and zero evicted
bytes. The retained half hit all 1,074,437 ranges with zero misses and performed zero adapter reads.
The empty half performed 11,240,354 adapter reads. Terminal range accounting remained 8,789,091
bytes against the 33,554,432-byte budget, with 1,348,781 misses and no oversized refusal.

The round took 595,191 ms, 2.13% above Decision 0247. Retained all-class successful p99 by depth was
**3.935008 / 120.144452 / 84.756031 / 475.754743 ms**. The respective depth changes were -0.13%,
then -5.00%, -5.53% and -2.88%. One sequential uncontrolled observation cannot attribute these
timing changes to the
borrowed evaluator. Four-hop remains 1.90 times the unchanged 250 ms target, so no latency target
passes.

The release process used 265,856 KiB peak RSS, 732.08 user / 55.71 system seconds and 790.64
seconds wall time with zero process swaps. It ran alone under the 4 GiB address-space limit and
verified enclosing 5/6 GiB high/max and 512 MiB swap caps. Cumulative cgroup peaks were
5,373,222,912 bytes memory and 286,691,328 bytes swap. Soft-limit events increased by 1,434 during
the observation; maximum-limit, OOM, OOM-kill and CPU-throttle counters remained zero. Source
certificate `b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`, oracle bundle
`f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`678a3ada77ca096ab270a5fdbb7d219c0ab4a5317c98f0cab7ae08d584a4645f` stayed pinned.

The complete report is retained in
[`borrowed-authorization-sampling.json`](../evidence/borrowed-authorization-sampling.json). Local
artifacts remain under `experiments/t20-bench/target/native-d248-authz.TXqOjg`. Keep all
qualification prerequisites and existing defaults. The next bounded work should inspect repeated
decoded-record construction and compact result-vector construction on the zero-I/O retained path
before selecting another implementation or telemetry increment.
