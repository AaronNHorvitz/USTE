# Decision 0218: Explicit bounded query-cache capacity comparison

Date: 2026-09-20

Status: Implemented and locally verified; larger capacity comparison unmeasured.

D0216's same-binary development pair records a regression, not a speedup, for the 48/16 MiB
split. Preserve that evidence and the 64 MiB page-only default. Add separately named correctness
commands for a 256 MiB page-only control and a 256 MiB total split equally between pages and
positive lookups. This stays within storage's existing 256 MiB trusted cache ceiling; it does
not raise the storage maximum, query/proof/scratch budgets, fixture cap or benchmark thresholds.
Larger capacity is a declared independent variable, not a hidden replacement for the failed split.

Both commands cold-admit the same source and execute the complete unchanged 384-case oracle,
clearing all enabled USTE partitions before each query. Report distinct versioned cache profiles
and exact included partition accounting. Reject cross-size/cross-mode report substitution.
Use the existing bounded authorized reader; no new cache algorithm, authority, consumer API,
source migration or format is introduced. Existing sampling remains at its pinned 64 MiB settings.

Require native fixture and separate-process tests for all four configurations, exact oracle
equivalence, unchanged source bytes and pre-I/O qualifying-size refusal. A later resource-admitted
same-binary measurement must compare both 256 MiB configurations and retain failures/regressions.
Development measurements do not qualify latency, full authenticated I/O or larger-than-memory
operation; no claim follows that this capacity is optimal. Qualifying dimensions, reserved host,
five-sample/30-trial protocols and all T-20/T-19/R2–R4 acceptance gates remain unchanged.

The full native release regression passes 132 active tests with five unchanged opt-in ignores,
strict Clippy and format/docs/task checks. Four configurations match the independent native
fixture oracle; separate CLI runs confirm exact budgets/profiles, all outcomes, no query writes
and unchanged certificate bytes. Cross-size/mode substitution and qualifying-size missing-path
refusal pass. PROGRESS records exact commands, binary/source baseline and resource usage.
