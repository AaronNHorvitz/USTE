# Decision 0224: Scoped positive-cache value decoding

Date: 2026-09-21

Status: Implemented and locally verified; native performance observation pending.

D0223 shows that allocation-free cache probes alone do not close the retained four-hop latency
gap. A successful positive-cache hit still allocates and copies the complete retained plaintext
before the graph layer immediately decodes that temporary value. Add a scoped mapper to the
positive lookup path so workspace consumers can convert resident plaintext while it is borrowed.
The mapper result cannot borrow from the cache, and the ordinary owned-value lookup remains
available and unchanged.

Preserve validation and authority ordering. Key and lookup limits, commitment context, canonical
tree proof, owner/session binding and exact cache identity are checked before resident plaintext is
made available. Recheck stored proof work and value length against the caller's limits. Complete
the cache's exact-LRU integrity check and stamp update before invoking the mapper. A miss is counted
once, falls through to the existing authenticated lookup and cache admission, and maps the owned
result only after that lookup succeeds. Absence is never retained.

Use the scoped path only for cached graph current-record point reads and expansion record fetches.
Cold and cache-disabled paths keep the existing owned lookup. Expansion captures the resident
encoded length with the decoded record and charges the same pages, encoded bytes and returned bytes
as before. Authorization, corruption checks, work limits, cache budgets/counters, zeroization,
on-disk formats and the page-only benchmark default do not change. This is an additive workspace
API, not permission for callers to retain cache-owned plaintext or bypass proof work.

Four focused tests verify the same resident allocation is observed across hits, a mapped warm hit
performs no adapter read, graph point/history warmth remains correct, and positive-cache expansion
keeps exact work and narrow-limit behavior. The first focused end-to-end expectation was updated to
include the newly added mapped hit; strict Clippy also identified and removed one unnecessary type
qualification. No product assertion or limit was weakened.

The final full workspace command passed 757 tests, strict Clippy and warnings-denied documentation.
The standalone native release gate passed 136 active tests with five unchanged opt-in ignores and
strict Clippy. Formatting for all manifests, documentation validation and the task graph passed.
Both heavy gates ran serially with one Cargo job/thread, the inherited 4 GiB process address-space
limit and the verified enclosing 5 GiB high / 6 GiB maximum / 512 MiB swap-maximum scope. The shared
scope peaked at 5,371,727,872 bytes and 143,224,832 bytes of swap, recorded 14,528 soft-limit events,
and recorded zero maximum-limit or OOM events. Those cumulative shared-scope figures are not
workload RSS. The workspace tests ran much more slowly than D0222 without cgroup CPU throttling;
retain that anomaly rather than treating elapsed time as performance evidence.

No latency improvement or T-20 gate follows from implementation verification. Measure this change
separately with the unchanged supervised retained-fixture protocol before making any performance
claim, and keep all reserved-host qualification prerequisites open.
