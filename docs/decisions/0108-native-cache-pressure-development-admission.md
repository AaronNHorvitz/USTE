# Decision 0108 — Native cache-pressure development admission

Date: 2026-09-19

Status: implemented and locally verified, with nonqualifying cache-pressure evidence. T-20 remains open.

Extend only the native development ceiling from 10,000 to 20,000 entities (200,000 relationships).
The exact-version 10,000-entity observations used 57,228,288 accounted cache bytes and had zero
evictions under the accepted 64 MiB cache. They therefore did not establish native cache-pressure
behavior. A larger bounded development fixture is the next diagnostic step, not a qualifying run.

Keep both memory-adapter commands capped at 1,000 entities. Native create/resume/open, crash probe,
query and supervised sampling share the new ceiling before filesystem or child-process access;
20,001 and the qualifying 100,000 remain refused. Reports retain the development limit and explicit
nonqualifying labels. Do not enlarge the page cache, per-transaction 10,000-operation/16 MiB request
caps, query budgets, qualified resource reservation or acceptance thresholds. Profile-derived
limits already validate through exact scale; that validation is not execution qualification.

The new ceiling streams 420,001 operations across 44 certified revisions. Extend the batch test
to encode every batch at both the old and new ceilings and check ordered revisions, exact totals
and existing per-request bounds. Keep native oracle, process-loss, deadline and admission tests.
Before a diagnostic, check RAM/swap, free disk space and competing workloads. Run one heavy
process group with MemoryHigh=3G, MemoryMax=4G and MemorySwapMax=512M, plus a wall timeout. Retain
partial synthetic fixtures on failure for recovery inspection. Do not rerun an unchanged failure.

Measure rather than assume evictions, bounded residency or correct outcomes. Record exact commit,
binary/lock hashes, command, resource controls, adapter/cache work and oracle result. Single-pass
uncontrolled-host-cache observations are not warm-cache latency qualification, a larger-than-RAM
proof, the 24 GiB reservation, BM-06's recovery campaign, or T-20 completion. Storage construction
rewrite amplification and principal quota accounting remain separate implementation work.

Native release regression passes all 51 active unit tests and three CLI/process tests, with the
two unchanged exact-profile oracle ignores. Strict native all-target Clippy and format/docs/task
checks pass. PROGRESS.md records commands and resource limits.

The [exact-version 20,000-entity observation](../evidence/native-disk-20000-development.json) on
`0d5eeeb` builds revision 44 and matches all 384 query expectations (313 results, 71 expected
result-limit refusals). It records 7,577,807 cache evictions with 67,098,624 accounted bytes under
the unchanged 64 MiB budget. Query command wall time is 435.30s, peak RSS 265,384 KiB, zero swaps.
This establishes that this native development run exercised eviction, not benchmark qualification.
