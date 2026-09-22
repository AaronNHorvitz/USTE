# Decision 0257: Borrowed-direct-string development observation

Date: 2026-09-22

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0256 once with the unchanged retained 20,000-entity/200,000-relationship fixture,
oracle, query order, 96/128/32 MiB page/lookup/range split and supervised 30-second per-query
protocol. This is a sequential development observation with uncontrolled kernel/device caches, not
a causal or repeated campaign.

The parent accepted 96 warm-ups, 768 measured executions, 40 latency groups and output digest
`aec16fdc8a1630a97eace654862a192929550c5fb771aed0862a9bed67d81584`. After removing only elapsed,
percentile and RSS fields, the complete report matches Decision 0255 exactly at SHA-256
`c472dc13e72e3ed3fd5cd46776824223c89213f9c2dc9ec25bdcc653b633abd9`. Thus query outcomes,
logical results, cache counters, adapter and vault work, residency and pressure are unchanged.

The 32 MiB range partition again peaked at 29,231,596 bytes with zero evictions and zero evicted
bytes. The retained half hit all 1,074,437 ranges with zero misses and performed zero adapter reads.
The empty half performed 11,240,354 adapter reads. Terminal range accounting remained 8,789,091
bytes against the 33,554,432-byte budget, with 1,348,781 misses and no oversized refusal.

The round took 541,442 ms, 0.01% above Decision 0255. Retained all-class successful p99 by depth
was **2.660155 / 87.684055 / 60.252556 / 368.605719 ms**. The respective depth changes were
+2.17%, +1.28%, -0.02% and +1.40%. One sequential uncontrolled observation cannot attribute these
timing changes to borrowing fixed direct strings. Four-hop remains 1.47 times the unchanged 250 ms
target, so no latency target passes.

The release process used 265,984 KiB peak RSS, 662.81 user / 53.74 system seconds and 719.17
seconds wall time with zero process swaps. It ran alone under the 4 GiB address-space limit and
verified enclosing 5/6 GiB high/max and 512 MiB swap caps. Cumulative cgroup peaks remained
5,373,222,912 bytes memory and 286,691,328 bytes swap. Soft-limit events increased by 1,213;
maximum-limit, OOM, OOM-kill and CPU-throttle counters remained zero. Commit
`691fa0a8046dbb992d942f5c8117491546fc1d1e`, source certificate
`b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`, oracle bundle
`f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`160eb9d7e13024e65baa9f5f3127d2ff8ad97e636145f14f8365fc14a0012c0e` were pinned. The accepted
report's SHA-256 is `941df748efa45854dbad75642693185ce5fbff093f133a1fc5359770ad7d5f6d`.

The complete report is retained in
[`borrowed-direct-string-sampling.json`](../evidence/borrowed-direct-string-sampling.json). Local
artifacts remain under `experiments/t20-bench/target/native-d256-direct.Ene44N`. Keep all
qualification prerequisites and existing defaults. The next bounded work should inspect nested
fixed-schema map construction in repeated stored-record decoding or select another measured
zero-I/O retained-path cost without introducing unbudgeted plaintext retention.
