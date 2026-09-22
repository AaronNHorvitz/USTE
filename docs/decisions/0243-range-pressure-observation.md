# Decision 0243: Range-pressure development observation

Date: 2026-09-22

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0242 once with the unchanged retained 20,000-entity/200,000-relationship fixture,
oracle, query order, 112/128/16 MiB page/lookup/range split and supervised 30-second per-query
protocol. This is a sequential development observation with uncontrolled kernel/device caches, not
a causal or repeated campaign.

The parent accepted the complete pressure report: 96 warm-ups, 768 measured executions, 40 latency
groups and output digest `aec16fdc8a1630a97eace654862a192929550c5fb771aed0862a9bed67d81584`.
After removing only timing/RSS, schema/profile labels and the two newly exposed pressure fields, the
report matches Decision 0240 exactly at SHA-256
`7184584f3799a82bef31af0617654937b4ba820e3801e4cdcf6562d3849151cd`. Thus all existing semantic,
cache-counter, adapter-I/O and vault-work evidence is unchanged; the instrumentation did not alter
observed cache behavior.

The exact pressure fields confirm that 16 MiB is saturated, not merely busy. Terminal, warm-up and
both measured states all report a 16,777,216-byte high-water equal to the full range budget.
Evicted charges were 239,953,583 bytes during warm-up, 888,794,394 bytes in the empty half and
2,381,225,814 bytes in the retained half, summing exactly to the terminal 3,509,973,791 bytes. The
existing 70,636 / 257,206 / 868,946 eviction counts reconcile independently. Terminal residency
remained only 8,789,091 bytes, reaffirming that terminal occupancy cannot size this partition.

The round took 640,426 ms, 0.30% above D0240. Retained all-class successful p99 by depth was
**4.015284 / 124.291078 / 85.287004 / 1315.392045 ms**, 0.17%, 0.86%, 1.47% and 1.62% above D0240.
These small sequential timing changes are uncontrolled noise, not evidence of telemetry overhead.
Four-hop remains more than five times the unchanged 250 ms target; no target passes.

The release process used 265,904 KiB peak RSS, 761.24 user / 52.69 system seconds and 815.67
seconds wall time with zero process swaps. It ran alone under the 4 GiB address-space limit and
verified enclosing 5/6 GiB high/max and 512 MiB swap caps. Cumulative cgroup peaks remained
5,372,850,176 bytes memory and 286,691,328 bytes swap; soft-limit events stayed at 106,607 and
maximum-limit, OOM, OOM-kill and CPU-throttle counters remained zero. Source certificate
`b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`, oracle bundle
`f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`5654844710f1a263ca8e3deb8711cc9519c6ea7884e45c8525f59c9dfb4cd2cc` stayed unchanged.

The complete report is retained in
[`range-pressure-sampling.json`](../evidence/range-pressure-sampling.json). Keep all qualification
prerequisites and existing defaults. The next bounded capacity point should be a distinct
96/128/32 MiB page/lookup/range pressure profile: it preserves the proven lookup budget and doubles
the saturated range partition while keeping the total fixed, without claiming that 32 MiB is
sufficient.
