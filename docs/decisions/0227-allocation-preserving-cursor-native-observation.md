# Decision 0227: Allocation-preserving cursor handoff development observation

Date: 2026-09-21

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0226 once with the unchanged supervised 20,000-entity/200,000-relationship wide
positive-cache protocol and compare it to D0225. Preserve the retained fixture, oracle, 128/128 MiB
cache split, 30-second per-query deadline, query order, work limits and population validation. This
is a sequential cross-binary development observation with uncontrolled kernel/device caches, not a
causal or repeated campaign.

The D0226 round completed in 638,508 ms versus D0225's 656,099 ms (-2.68%). After removing only
timing, percentile and RSS fields, both reports have the same normalized SHA-256
`c848e2b1573de9733014d908a1d51a281fb5c6dc932bfdc39559ca0f0fb9fe78`: 96 warm-ups, 768
measured executions, 40 populations, output digest, successful work, cache counters, adapter I/O
and vault work all match. Retained all-class successful p99 by depth changed from 8.814519 /
603.351627 / 373.695491 / 1260.417563 ms to **8.855924 / 563.377184 / 364.157448 /
1257.307212 ms**. Depth one regressed 0.47%; depths two through four improved 6.63%, 2.55% and
0.25%. Four-hop remains more than five times the unchanged 250 ms target. Do not claim a pass or
causal speedup.

The release process used 265,836 KiB peak RSS, 759.11 user / 46.39 system seconds and 808.22
seconds wall time, with zero process swaps. It ran alone inside the verified enclosing scope with
the inherited 4 GiB process address-space limit. The shared scope's memory and swap peaks stayed at
5,372,850,176 and 143,224,832 bytes; soft-limit events increased from 22,368 to 24,833, while
maximum-limit/OOM and CPU-throttle events remained zero. Shared values are not process RSS. Source
certificate `b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`, oracle bundle
`f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`f6d1e71fb724aaac5ff63605763b8d460b4e37089554f985fbded9d669cd6d24` stayed unchanged.

The complete report is retained in
[`allocation-preserving-cursor-handoff-sampling.json`](../evidence/allocation-preserving-cursor-handoff-sampling.json).
Keep the 64 MiB page-only default and every T-20 qualification prerequisite. The remaining latency
gap requires removing or measuring a higher-level repeated traversal cost before another capacity
campaign.
