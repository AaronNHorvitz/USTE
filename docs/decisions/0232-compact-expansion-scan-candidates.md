# Decision 0232: Compact expansion scan candidates

Date: 2026-09-21

Status: Implemented and locally verified; native performance observation pending.

Decision 0231 leaves authenticated adjacency scanning as the dominant retained cost. A persistent
prefix-result cache is not yet admissible: it needs a separately reported owner/session/root-bound
budget partition and exact replay of cursor work limits. Do not borrow hidden capacity from either
existing cache partition.

First remove avoidable retained scan storage. Packed expansion previously transferred every
cursor candidate's heap-backed key and value into an `IndexScanEntry` and retained all of those
buffers until shared graph semantics consumed the complete scan. Instead, validate each packed
candidate and immediately retain only its fixed 16-byte record identifier plus optional fixed
16-byte neighbor identifier. Cursor-owned buffers are dropped after each iteration. Shared
semantics reconstructs scoped references at the same consumption point, preserves ordered merge,
self-loop checks and output order, and retains identical authorization and record validation.

The legacy disk reader converts its already collected authenticated scan into the same compact
representation before shared semantics. This can detect malformed secondary key/value shape before
candidate record lookups rather than while consuming candidates; either path fails closed and
returns no partial output. Successful scan proof work, cache counters, aggregate page/entry/byte/
lookup limits, formats and public APIs are unchanged.

Focused legacy reference/shared-budget, packed direction/duplicate/reference, five exact aggregate
limits, permission/cancellation, late corruption and page-only plus positive-cache exact-work tests
passed. A default-debug invocation grouped the exhaustive packed read-fault test and ran much
slower than the standard optimized profile; it was interrupted and excluded. The same exhaustive
test passed in the final optimized full gate.

The final workspace gate passed **758 tests**, zero failures/ignores, strict workspace Clippy and
warnings-denied documentation. Its longest suites were graph disk 128/536.44 s, transaction
metadata 50/104.63 s, storage unit 244/31.21 s and transaction integration 118/107.84 s. Log:
`/tmp/uste-d232-workspace-verification.log`.

The standalone native release gate passed **136 active tests with five unchanged opt-in ignores**
and strict Clippy; log `/tmp/uste-d232-native-verification.log`. Both gates used one Cargo job, one
test thread and one heavy workload at a time with the inherited 4 GiB process address-space limit.
The enclosing shared scope's cumulative memory and swap peaks stayed at 5,372,850,176 and
286,691,328 bytes; soft-limit events reached 45,222, with zero maximum-limit, OOM or CPU-throttle
events. Shared values are not process RSS.

No performance, T-20 or M1 gate claim follows until the unchanged supervised retained-fixture
protocol measures this binary. Keep the 64 MiB page-only default and every qualification
prerequisite.
