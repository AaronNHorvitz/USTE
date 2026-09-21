# Decision 0225: Scoped positive-cache decoding development observation

Date: 2026-09-21

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0224 once with the unchanged supervised 20,000-entity/200,000-relationship wide
positive-cache protocol and compare it to D0223. Preserve the retained fixture, oracle, 128/128 MiB
cache split, 30-second per-query deadline, query order, work limits and population validation. This
is a sequential cross-binary development observation with uncontrolled kernel/device caches, not a
causal or repeated campaign.

The D0224 round completed in 656,099 ms versus D0223's 673,948 ms (-2.65%). After removing only
timing, percentile and RSS fields, both reports have the same normalized SHA-256
`c848e2b1573de9733014d908a1d51a281fb5c6dc932bfdc39559ca0f0fb9fe78`: 96 warm-ups, 768
measured executions, 40 populations, output digest, successful work, cache counters, adapter I/O
and vault work all match. Retained all-class successful p99 by depth changed from 9.397449 /
605.082078 / 387.319532 / 1302.698520 ms to **8.814519 / 603.351627 / 373.695491 /
1260.417563 ms**. The single observation improved each depth, but four-hop remains more than five
times the unchanged 250 ms target. Do not claim a pass or causal speedup.

The release process used 265,372 KiB peak RSS, 780.75 user / 45.96 system seconds and 830.22
seconds wall time, with zero process swaps. It ran alone inside the verified enclosing scope with
the inherited 4 GiB process address-space limit. The shared scope's prior high-event, peak-memory
and peak-swap counters did not increase during this sample, and maximum-limit/OOM events remained
zero; shared values are not process RSS. Source certificate
`b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`, oracle bundle
`f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`a4141186de339271fc138e6f3b447a0d999f4aa7bd2e59d2ba5ea02450937224` stayed unchanged.

The complete report is retained in
[`scoped-positive-cache-value-decoding-sampling.json`](../evidence/scoped-positive-cache-value-decoding-sampling.json).
Keep the 64 MiB page-only default and every T-20 qualification prerequisite. The remaining latency
gap requires profiling or removal of a higher-level repeated traversal cost before another capacity
campaign.
