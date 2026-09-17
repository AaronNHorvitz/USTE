# Decision 0024 — R1 benchmark-enabling dependency order

Date: 2026-09-17

Status: accepted implementation-sequencing correction. No test, workload, budget or acceptance
requirement is removed or lowered.

## Problem

T-19 requires the applicable R1 verification suites and BM-01/02/04/06 budgets to pass. BM-04 is
currently failed, and BM-01/02/06 have not run. At the same time, T-17/T-18 and the implementation
plan correctly assign the disk index, bounded cache and BM-01/BM-06 enabling work to T-20, while
T-20 depended on T-19. The textual ownership therefore made honest T-19 completion impossible:
the acceptance gate preceded work required by that gate.

This was an acyclic task graph but a semantic dependency contradiction. Treating failed or absent
benchmarks as passed would violate Decision 0007 and the owner instruction not to lower budgets.

## Correction

T-20 now depends directly on its implemented foundations T-17, T-18 and T-49, and T-19 depends on
T-20. T-19 retains its complete R1 VT/BM acceptance requirement, including the unchanged Decision
0007 thresholds. T-19 remains open while T-20 and the remaining T-19 benchmark/qualification work
proceed. T-29 gains an explicit T-19 dependency so no local developer-alpha acceptance can bypass
the R1 gate after this reorder.

Task identifiers and implementation scope remain stable. T-20 being implemented before T-19 is
benchmark-enabling work, not an R2 acceptance claim. T-50 and the R2 gate remain downstream of
T-19. T-62 remains solely the external executable-distribution prerequisite and is unaffected.

## Current evidence status

- BM-04 remains failed at 95.923 MiB/s versus the 250 MiB/s target; its mixed-small-object portion
  is also absent.
- BM-01, BM-02 and BM-06 have no qualifying result.
- T-19 also must finish its clause-level VT audit, runnable R1 instructions and any missing
  phantom-sensitive/fuzz coverage before closure.

The next permitted implementation task is T-20. Later evidence must report benchmark failures and
cannot infer production scalability from the current bounded in-memory correctness kernels.
