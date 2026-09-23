# Decision 0263: Fixed stored-record field-table development observation

Date: 2026-09-22

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0262 once with the unchanged retained 20,000-entity/200,000-relationship fixture,
oracle, query order, 96/128/32 MiB page/lookup/range split and supervised 30-second per-query
protocol. This is a sequential development observation with uncontrolled kernel/device caches, not
a causal or repeated campaign.

The parent accepted 96 warm-ups, 768 measured executions, 40 latency groups and output digest
`aec16fdc8a1630a97eace654862a192929550c5fb771aed0862a9bed67d81584`. After removing only elapsed,
percentile and RSS fields, the complete report matches Decision 0261 exactly at SHA-256
`c472dc13e72e3ed3fd5cd46776824223c89213f9c2dc9ec25bdcc653b633abd9`. Thus query outcomes,
logical results, cache counters, adapter and vault work, residency and pressure are unchanged.

The 32 MiB range partition again peaked at 29,231,596 bytes with zero evictions and zero evicted
bytes. The retained half hit all 1,074,437 ranges with zero misses and performed zero adapter reads.
The empty half performed 11,240,354 adapter reads. Terminal range accounting remained 8,789,091
bytes against the 33,554,432-byte budget, with 1,348,781 misses and no oversized refusal.

The round took 544,273 ms, 0.24% below Decision 0261. Retained all-class successful p99 by depth was
**2.766824 / 91.255523 / 63.166153 / 381.871066 ms**. The respective depth changes were +3.16%,
-0.23%, -1.34% and -1.25%. One sequential uncontrolled observation cannot attribute these timing
changes to the fixed root field table. Four-hop remains 1.53 times the unchanged 250 ms target, so
no latency target passes.

The release process used 265,544 KiB peak RSS, 664.45 user / 54.65 system seconds and 720.79 seconds
wall time with zero process swaps. It ran alone under the 4 GiB address-space limit and verified
enclosing 5/6 GiB high/max and 512 MiB swap caps. Cumulative cgroup peaks remained 5,373,222,912
bytes memory and 459,055,104 bytes swap. Soft-limit events increased by 22; maximum-limit, OOM,
OOM-kill, socket-memory-throttle and CPU-throttle counters did not increase. Commit
`108b882d5a674562d3ea92a3aecec7cc926629eb`, retained source certificate
`b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`, oracle bundle
`f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`d3d20bdadf742b8514ceb2149d1d3bc054335e9fca3728dc7bd18f3affdc0f82` were pinned. The accepted
report's SHA-256 is `db0ad43518bb07e5619736bfa87fdb637643ba9c4d35e7d85e072b8649a46ecd`.

The complete report is retained in
[`fixed-stored-record-field-table-sampling.json`](../evidence/fixed-stored-record-field-table-sampling.json).
Local artifacts remain under `experiments/t20-bench/target/native-d262-fields.RIbtaG`. Keep all
qualification prerequisites and existing defaults. The next bounded work should inspect another
measured zero-I/O retained-path cost before changing implementation.
