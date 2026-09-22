# Decision 0251: Fallible-range-mapping development observation

Date: 2026-09-22

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0250 once with the unchanged retained 20,000-entity/200,000-relationship fixture,
oracle, query order, 96/128/32 MiB page/lookup/range split and supervised 30-second per-query
protocol. This is a sequential development observation with uncontrolled kernel/device caches, not
a causal or repeated campaign.

The parent accepted 96 warm-ups, 768 measured executions, 40 latency groups and output digest
`aec16fdc8a1630a97eace654862a192929550c5fb771aed0862a9bed67d81584`. After removing only elapsed,
percentile and RSS fields, the complete report matches Decision 0249 exactly at SHA-256
`c472dc13e72e3ed3fd5cd46776824223c89213f9c2dc9ec25bdcc653b633abd9`. Thus query outcomes,
logical results, cache counters, adapter and vault work, residency and pressure are unchanged.

The 32 MiB range partition again peaked at 29,231,596 bytes with zero evictions and zero evicted
bytes. The retained half hit all 1,074,437 ranges with zero misses and performed zero adapter reads.
The empty half performed 11,240,354 adapter reads. Terminal range accounting remained 8,789,091
bytes against the 33,554,432-byte budget, with 1,348,781 misses and no oversized refusal.

The round took 607,136 ms, 2.01% above Decision 0249. Retained all-class successful p99 by depth was
**4.129358 / 129.888535 / 93.914371 / 522.109698 ms**. The respective depth changes were +4.94%,
then +8.11%, +10.81% and +9.74%. One sequential uncontrolled observation cannot attribute these
timing changes to the removed vector allocation. Four-hop remains 2.09 times the unchanged 250 ms
target, so no latency target passes.

The release process used 265,684 KiB peak RSS, 737.94 user / 55.81 system seconds and 795.56
seconds wall time with zero process swaps. It ran alone under the 4 GiB address-space limit and
verified enclosing 5/6 GiB high/max and 512 MiB swap caps. Cumulative cgroup peaks remained
5,373,222,912 bytes memory and 286,691,328 bytes swap. Soft-limit events increased by 965 during the
observation; maximum-limit, OOM, OOM-kill and CPU-throttle counters remained zero. Commit
`1f43ceeb75fbffb413e04463026bbf39572d8a84`, source certificate
`b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`, oracle bundle
`f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`74e8775a3a6b65acd518bf89b257f7e84d32f1d836837f677b2520ba6ff9c5c6` were pinned. The accepted
report's SHA-256 is `92b2d36faeaf4e10bdb91bf29b0d19927a9ffda7909851cfc9ae521a672e4177`.

The complete report is retained in
[`fallible-range-mapping-sampling.json`](../evidence/fallible-range-mapping-sampling.json). Local
artifacts remain under `experiments/t20-bench/target/native-d250-fallible.6rs555`. An earlier
0.41-second launch supplied the internal engine directory instead of the benchmark root, failed
preflight with `USTE_BM01_SAMPLE_WORKER`, produced no report and is retained separately under
`native-d250-fallible.4eF5YO`; it is not an observation. Keep all qualification prerequisites and
existing defaults. The next bounded work should inspect repeated decoded-record construction on
the zero-I/O retained path before selecting another implementation or telemetry increment.
