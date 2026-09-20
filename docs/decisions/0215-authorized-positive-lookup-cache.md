# Decision 0215: Authorized positive-lookup cache configuration

Date: 2026-09-20

Status: Implemented and locally verified; no benchmark or consumer migration.

Expose Decision 0214 through a new trusted `AuthorizedPackedReader` constructor accepting a
total cache budget and an included positive-lookup partition. Preserve both existing uncached
and page-only constructors and their behavior. Consumer requests cannot choose partitions or
change read-work limits. This is configuration of the existing private cache, not a new read API.

Use the same metadata admission, immutable coordinator borrow, per-request current-policy and
readiness checks, target authorization, cancellation and domain filtering. A retained value is
authenticated immutable storage content, not permission or a successful consumer result. Keep
typed decoding and hidden-reference filtering on every use. The reader cannot span a write.

Keep diagnostics and clearing behind current ManageSchema authorization. Reports distinguish
page and positive-result observations and include both within the admitted total; clearing
drops both partitions and binding while retaining counters. Do not claim disk freshness,
physical erasure, complete I/O or a process-RSS bound.

Run the existing graph point/history, expansion, visibility, cancellation, maintenance, small
cache, late-mutation and exhaustive read-fault fixtures in both cache modes. Require exact
uncached/reference outputs and unchanged logical admission, including failed-query retries
and cold recovery. Record independent page/result eviction evidence rather than assuming that
every larger cache must evict. Verify these boundaries before selecting a native measurement
configuration. M1's pinned interfaces and qualification, accepted benchmark targets and the
full roadmap remain unchanged.

Eight new tests pass, including seven shared dual-mode groups and direct constructor/budget
checks. The full workspace passes 754 tests across 47 executables, strict Clippy and
warnings-denied documentation. Both cache partitions evict in the bounded trace without changing
reference results. Exact commands, selected/full coverage and resource observations are in PROGRESS.md.
