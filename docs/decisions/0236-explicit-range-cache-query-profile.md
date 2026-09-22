# Decision 0236: Explicit range-cache query profile

Date: 2026-09-22

Status: Implemented and locally verified; supervised sampling remains pending.

Expose Decision 0235 only through two new nonqualifying native correctness commands. The ordinary
profile uses one unchanged 64 MiB total split into 32 MiB pages, 16 MiB positive lookups and 16 MiB
complete ranges. The wide profile uses the unchanged 256 MiB maximum split into 64 MiB pages,
64 MiB positive lookups and 128 MiB complete ranges. These are reallocations within one total, not
additional hidden memory. Existing page-only and positive-lookup commands, partitions, profile names
and report shapes remain unchanged.

`linux-packed-range-query` and `linux-packed-wide-range-query` emit the distinct
`bm01-linux-packed-range-query-v1` correctness schema. Their cache profiles are
`packed-pages-positive-lookups-ranges-v1` and
`packed-pages-positive-lookups-ranges-256m-v1`. Range reports include their independent budget,
accounted bytes, resident ranges/entries, hits, misses, evictions and oversized bypasses. Validation
requires exact total/page/lookup/range budgets, each partition's accounting bound, total accounting
and exact cross-size/cross-mode identity. Range keys are emitted only for the new mode, preserving
the existing JSON shape for page and lookup profiles.

Configuration tests cover both sizes, all three modes, cross-size/mode refusal, exact partition
arithmetic, range counters and malformed/missing/over-budget reports. An initial compile caught the
two new query functions missing from the packed runner's public re-export; the export was corrected
before verification. The optimized standalone T-20 gate passed **137 active tests with five
unchanged opt-in ignores** and strict Clippy. Log: `/tmp/uste-d236-native-verification.log`. It used
one Cargo job, one test thread, offline dependencies and the 4 GiB process address-space limit.

No workload was executed, and no performance, T-20, M1 or qualification claim follows. Add
separate deadline-supervised worker/parent commands and strict range-schema validation before one
retained-fixture development observation. Do not compare an unsupervised correctness command to the
prior supervised positive-cache samples.
