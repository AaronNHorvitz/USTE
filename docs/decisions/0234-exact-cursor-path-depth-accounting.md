# Decision 0234: Exact cursor path-depth accounting

Date: 2026-09-22

Status: Implemented and locally verified; prefix-result retention remains pending.

Decision 0233 identifies repeated authenticated adjacency-prefix traversal as the next material
read cost, but a retained result must pass every later caller limit using the work actually needed
to produce that result. `TreeCursorReport` already records candidates, returned entries and bytes,
pages, encoded bytes and value chunks. It did not record the greatest branch depth reached. Reusing
a result under only those fields could bypass a narrower `maximum_path_branches`; retaining the
original configured limit instead would be safe but needlessly refuse callers whose limit covers
the actual traversal.

Add `path_branches` to `TreeCursorReport` and update it to the greatest live traversal-stack depth
after each structurally valid branch is accepted. Empty trees and ranges that perform no traversal
continue to report zero. Successful forward and reverse scans now rerun with that observed value as
their exact path limit and produce the identical result/report; one-less depth fails with the same
sticky `ResourceLimit` behavior as every other cursor limit. Page and encoded-byte proof work,
candidate and returned-byte accounting, traversal order, authorization, cache behavior, formats
and persisted data are unchanged.

This field is a prerequisite, not a prefix cache. Any later result cache still needs a separately
declared partition in the total cache budget, full tree/direction/bounds identity, owner and unlock-
session binding, complete-success-only admission, bounded plaintext retention, clear/eviction
tests, counters, and replay checks against every `TreeCursorLimits` field. Existing cache
constructors and the page-only benchmark default remain unchanged.

Focused forward and reverse exact-limit tests passed. The first full invocation used default-debug
test code and reached the exhaustive packed expansion read-fault case after all preceding cases
passed, but that known disproportionately slow configuration was interrupted and excluded; its
partial log is `/tmp/uste-d234-workspace-verification.log`. The complete optimized gate used one
Cargo job, one test thread, offline dependencies and the 4 GiB process address-space limit. It
passed **758 workspace tests**, strict workspace Clippy, warnings-denied documentation, the
documentation/task validators, vector/publication checks and isolated tooling. The isolated T-20
driver passed **140 active tests with five unchanged opt-in ignores** and strict Clippy. Complete
log: `/tmp/uste-d234-workspace-verification-optimized.log`.

The enclosing shared scope retained its earlier 5,372,850,176-byte memory and 286,691,328-byte swap
peaks. Soft-limit events reached 73,298; maximum-limit, OOM and CPU-throttle events remained zero.
These cumulative shared values are not process RSS. No performance, T-20, M1 or qualification gate
claim follows.
