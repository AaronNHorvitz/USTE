# Decision 0229: Ordered adjacency merge development observation

Date: 2026-09-21

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0228 once with the unchanged supervised 20,000-entity/200,000-relationship wide
positive-cache protocol and compare it to D0227. Preserve the retained fixture, oracle, 128/128 MiB
cache split, 30-second per-query deadline, query order, work limits and population validation. This
is a sequential cross-binary development observation with uncontrolled kernel/device caches, not a
causal or repeated campaign.

The D0228 round completed in 668,628 ms versus D0227's 638,508 ms (+4.72%). After removing only
timing, percentile and RSS fields, both reports have the same normalized SHA-256
`c848e2b1573de9733014d908a1d51a281fb5c6dc932bfdc39559ca0f0fb9fe78`: 96 warm-ups, 768
measured executions, 40 populations, output digest, successful work, cache counters, adapter I/O
and vault work all match. Retained all-class successful p99 by depth changed from 8.855924 /
563.377184 / 364.157448 / 1257.307212 ms to **8.777650 / 575.710303 / 365.319741 /
1270.350394 ms**. Depth one improved 0.88%; depths two through four regressed 2.19%, 0.32% and
1.04%. Four-hop remains more than five times the unchanged 250 ms target. Do not claim a pass or
causal regression.

The release process used 265,836 KiB peak RSS, 792.29 user / 50.36 system seconds and 846.57
seconds wall time, with zero process swaps. It ran alone inside the verified enclosing scope with
the inherited 4 GiB process address-space limit. The shared scope's cumulative memory and swap
peaks stayed at 5,372,850,176 and 143,224,832 bytes; soft-limit events stayed at 26,549, while
maximum-limit/OOM and CPU-throttle events remained zero. Shared values are not process RSS. Source
certificate `b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`, oracle bundle
`f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`fd6a056ef4f0f782d44d422de7df44a60e1058f09d058b3e6274522950781e23` stayed unchanged.

The complete report is retained in
[`ordered-adjacency-scan-merge-sampling.json`](../evidence/ordered-adjacency-scan-merge-sampling.json).
Keep the 64 MiB page-only default and every T-20 qualification prerequisite. This mixed single
observation does not resolve the latency gap; remove or explicitly measure another higher-level
repeated traversal cost before any capacity campaign.
