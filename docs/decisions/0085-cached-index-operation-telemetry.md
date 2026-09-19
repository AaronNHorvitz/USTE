# Decision 0085 — Cached index operation telemetry

Date: 2026-09-18

Status: T-20 partial implementation; benchmark qualification remains open.

Retain fixed-size cumulative telemetry in `PageCache` for the cached exact-key, predecessor
and prefix-scan primitives. Count completed and failed operations and their existing
`IndexReadStats`, including work observed before resource limits, invalid inputs, corruption,
adapter errors and visitor failures. Aggregate once at the primitive boundary; delegating
wrappers do not double-count. A successful primitive does not imply its enclosing authorized
query succeeded. Failed scans may have delivered provisional visitor bytes, never accepted
partial query results.

Preserve existing read ordering, authorization, authentication, work admission, returned errors
and per-call statistics. Checked cumulative overflow atomically rejects the affected aggregate
and permanently invalidates the report for that cache lifetime without changing the storage
result. Clearing cached pages does not reset telemetry or rehabilitate invalid counters.
Telemetry retains no keys, names, values, principals or per-operation history.

Expose `AuthorizedDiskReader::index_read_report` only after current `ManageSchema`
authorization, as with existing cache diagnostics. Consumer result types and authorization
semantics are unchanged. The storage-layer report is a trusted low-level capability, not
permission to expose candidate-sensitive work through an unprivileged consumer interface.

Scope is deliberately narrower than complete I/O accounting: uncached cursors, publication,
scrubs and rejection before entry into a cached primitive are excluded. Page authentication
failures do not count as authenticated pages. Existing fragment counters cover their existing
enumeration sites, not every parser action. Filesystem-adapter traffic remains separately
measured under Decision 0084. No physical-device, complete-recovery, larger-than-memory or
qualifying BM-01/BM-06 claim follows from these counters. Native query totals, warm-up totals and
paired sample deltas report this scope explicitly as `partial-cached-primitives`, preserving
false completeness/device flags and all nonqualification disclosures. Counter subtraction and
aggregation are checked outside timed query execution; actual primitive observation remains
inside the timed read. Cache eviction scalability remains subsequent work.

Tests compare cumulative work to exact successful primitive statistics, preserve failed bounded
read work and visitor-byte accounting, reject each cumulative field's overflow atomically,
preserve actual return values after telemetry overflow, retain counters through cache clearing,
and require maintenance authorization for reports. Existing encrypted recovery, corruption,
graph reference and adapter-fault tests continue to apply.
Native tests preserve oracle/deadline checks and compare empty/retained logical primitive work
while verifying zero newly authenticated pages for the retained small fixture.
