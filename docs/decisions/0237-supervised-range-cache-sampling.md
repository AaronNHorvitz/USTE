# Decision 0237: Supervised range-cache sampling

Date: 2026-09-22

Status: Implemented and locally verified; development observation pending.

Extend the existing parent-owned 30-second per-query sampling protocol with distinct narrow and
wide range-cache worker/parent commands and schemas. Preserve every existing page/lookup command
and JSON shape. Range workers use Decision 0236's fixed 32/16/16 MiB and 64/64/128 MiB page,
lookup and range splits and emit range fields only under the new schemas.

Record checked range-cache deltas for every empty/retained execution and warm-up. Each observation
reports budget, current accounted bytes/resident ranges/entries and cumulative hit, miss, eviction
and oversized-bypass deltas. It is explicitly neither physical I/O nor logical proof work and is
already included in total cache accounting. Counter regression, mixed absent/present partitions,
invalid residency or budget changes fail the worker.

The parent requires exact schema/profile/partition sizes, conserves page plus lookup plus range
accounting, validates every labelled pair, sums all warm-up/sample lookup and range deltas and
requires both sums to equal the terminal cache report. Missing fields, counter mutation,
over-accounting and cross-mode schemas fail before a report is accepted. The parent retains process
ownership, protocol bounds and deadline termination.

Targeted counter and parent mutation tests passed. The complete optimized standalone T-20 gate
passed **140 active tests with five unchanged opt-in ignores** and strict Clippy. Log:
`/tmp/uste-d237-native-verification.log`. It used one Cargo job, one test thread, offline
dependencies and the 4 GiB process address-space limit.

No sample ran and no performance, T-20, M1 or qualification claim follows. A subsequent observation
must use the unchanged retained fixture/oracle and the new wide supervised command, preserve all
non-timing correctness/work fields, and remain explicitly nonqualifying.
