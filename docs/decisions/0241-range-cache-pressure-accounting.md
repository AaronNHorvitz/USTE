# Decision 0241: Range-cache pressure accounting

Date: 2026-09-22

Status: Implemented and locally verified; benchmark exposure pending.

Extend the privileged in-process `PackedRangeCacheReport` with two exact pressure fields after
Decision 0240 proved that terminal residency understates required capacity. `maximum_accounted_bytes`
is the greatest fixed-overhead-plus-resident charge reached during the cache lifetime.
`evicted_bytes` is the cumulative sum of exact per-range charges removed by capacity eviction. Both
survive ordinary cache clears alongside the existing counters; clears are not evictions and
oversized bypasses remain separate.

Insertion updates the high-water gauge only after checked charge admission. Every LRU removal adds
its exact stored charge to the byte counter with checked conversion and addition. Any overflow is
sticky and makes later reporting fail with the existing resource-limit error. Reports require the
high-water gauge to be at least current accounting and no greater than the declared range budget.
The sampling accumulator rejects high-water regression, invalid gauges and evicted-byte counter
regression and aggregates the new counter exactly.

Do not add the fields to an established JSON schema. Existing range query/sampling reports still
emit exactly their prior keys; focused tests explicitly assert that the new Rust fields are absent.
A subsequent separately named telemetry schema may expose both values with parent-side validation
without rewriting Decisions 0237, 0239 or their archived reports.

Focused storage tests prove exact evicted charges, high-water retention across clear and sticky byte
counter overflow. Focused cache JSON and sampling tests pass. The complete optimized workspace gate
passed **764 tests** and strict all-target/all-feature Clippy. The complete optimized standalone
T-20 gate passed **141 active tests with five unchanged opt-in ignores** and strict Clippy. Logs:
`/tmp/uste-d241-workspace-verification.log` and `/tmp/uste-d241-native-verification.log`. Both gates
used one Cargo job, one test thread, locked offline dependencies and the 4 GiB process address-space
limit under the verified enclosing 5/6 GiB high/max and 512 MiB swap caps.

No benchmark ran and no performance, T-20, M1 or qualification claim follows. Next add a distinct
supervised pressure-report schema that requires these fields in terminal configuration, warm-up and
every empty/retained ledger entry before using them to choose another cache partition.
