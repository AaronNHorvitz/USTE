# Decision 0211: Native single-pass/cache comparison

Date: 2026-09-20

Status: Read-only correctness and one changed-binary development sample completed.

Measure Decisions 0209 and 0210 together at pushed commit
`d3302394743d558a1f9e29dce7875effe446717d`, release binary SHA-256
`2577c8ab7635f49d472aaf88f6f44de04d13ea19c12b46242089790e0f3d22d7`.
Use Decision 0206's retained synthetic 20,000-entity/200,000-relationship store, independent
oracle summary/bundle and unchanged 64 MiB caches. Keep separate reports and the original
source-certificate identity. No materialization or admission-cap change is part of this run.

First run the complete 384-query correctness comparison. Require identical outcomes, digests,
logical work, adapter/vault work and cache counters before interpreting timing differences.
Only after inspecting a successful result and actual improvement, plus a fresh host admission,
consider one changed-binary supervised sampling attempt. Keep the original 1,800-second command
deadline, 30-second per-query supervisor, 3 GiB soft/4 GiB hard memory and 512 MiB swap limits.
No competing compiler/test workload and no unchanged retry of Decision 0206's timeout.

The correctness pass exited zero with exact D0208 outcomes, digests, logical work, adapter/
vault work and cache counters. Query time was 699,843 ms versus 793,165 ms (11.8% lower),
setup 53,535 ms, total wall 753.54 s and scope peak 276,152,320 bytes/zero swap. Retained
source certificate and binary hashes were unchanged. Fresh headroom admitted one sample at
the same limits; it exited zero in 1,631.14 seconds, peak RSS 265,896 KiB, scope peak
278,847,488 bytes/zero swap. The supervisor checked 96 warm-ups (75 successes/21 expected
result limits) and 768 measured paired executions (313 successes/71 expected result limits
per cache state), with no visit-limit outcomes. The empty-cache measured work exactly matches
the correctness pass. Source identity was unchanged afterward, and both owned processes exited.
Decisions 0212/0213 source-only work is excluded from both measurements; no compiler competed.

Retained-cache successful all-class p99 was 21.106773 ms at depth one and 5,093.321509 ms at
depth four. These do not establish the frozen 20/250 ms qualifying budgets: the smaller
development fixture already shows a substantial four-hop gap. Only one development round ran,
not the five qualifying samples. Complete authenticated I/O remains unmeasured. Preserve the
prior timeout rather than recasting it as success. Raw reports, commands, versions, exact work,
latency populations and resource evidence are archived in
[the comparison record](../evidence/single-pass-cache-native-comparison.json).

Record exact commands, versions, raw reports, resource observations and any timeout/error.
Host/device cache state and background activity remain uncontrolled. This is a combined-change
development comparison, not isolated attribution to either optimization, a 24 GiB qualifying
reservation, a qualifying profile or T-20 completion. Preserve all frozen benchmark targets,
M1's pinned handoff and the remaining full implementation/release roadmap.
