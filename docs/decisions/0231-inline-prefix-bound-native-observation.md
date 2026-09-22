# Decision 0231: Inline prefix-bound development observation

Date: 2026-09-21

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0230 once with the unchanged supervised 20,000-entity/200,000-relationship wide
positive-cache protocol and compare it to D0229. Preserve the retained fixture, oracle, 128/128 MiB
cache split, 30-second per-query deadline, query order, work limits and population validation. This
is a sequential cross-binary development observation with uncontrolled kernel/device caches, not a
causal or repeated campaign.

The D0230 round completed in 655,451 ms versus D0229's 668,628 ms (-1.97%). After removing only
timing, percentile and RSS fields, both reports have the same normalized SHA-256
`c848e2b1573de9733014d908a1d51a281fb5c6dc932bfdc39559ca0f0fb9fe78`: 96 warm-ups, 768
measured executions, 40 populations, output digest, successful work, cache counters, adapter I/O
and vault work all match. Retained all-class successful p99 by depth changed from 8.777650 /
575.710303 / 365.319741 / 1270.350394 ms to **8.885070 / 576.032616 / 360.335620 /
1296.426702 ms**. Depths one, two and four regressed 1.22%, 0.06% and 2.05%; depth three improved
1.36%. Four-hop remains more than five times the unchanged 250 ms target. Do not claim a pass or
causal speedup.

The release process used 265,692 KiB peak RSS, 774.27 user / 49.83 system seconds and 827.20
seconds wall time, with zero process swaps. It ran alone inside the verified enclosing scope with
the inherited 4 GiB process address-space limit. The shared scope's cumulative memory and swap
peaks stayed at 5,372,850,176 and 286,691,328 bytes; soft-limit events increased from 36,739 to
38,703, while maximum-limit/OOM and CPU-throttle events remained zero. Shared values are not
process RSS. Source certificate `b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`,
oracle bundle `f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`75b558fd0b22377ac29d4fd18e3b41fda79d9951f8f3f7c3397095aa214b28b4` stayed unchanged.

The complete report is retained in
[`inline-packed-prefix-upper-bound-sampling.json`](../evidence/inline-packed-prefix-upper-bound-sampling.json).
Keep the 64 MiB page-only default and every T-20 qualification prerequisite. The dominant retained
cost remains authenticated adjacency scanning; evaluate a bounded proof-preserving way to avoid
repeated identical prefix work before another capacity campaign.
