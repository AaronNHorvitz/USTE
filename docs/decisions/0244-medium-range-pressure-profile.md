# Decision 0244: Medium range-pressure profile

Date: 2026-09-22

Status: Implemented and locally verified; development observation pending.

Add a distinct supervised `linux-packed-wide-medium-range-pressure-sample` parent/worker command as
the next bounded capacity point after Decision 0243 proved the 16 MiB range partition saturated.
Keep the total at 256 MiB and the proven positive-lookup partition at 128 MiB; allocate 96 MiB to
pages and 32 MiB to complete ranges. This geometric step measures the pressure curve and is not a
claim that 32 MiB is sufficient or a new default.

The worker emits the distinct
`bm01-linux-packed-wide-medium-range-pressure-sampling-v1` schema and
`packed-pages-positive-lookups-medium-ranges-pressure-256m-v1` cache profile. Terminal, warm-up and
every empty/retained range observation retain Decision 0242's exact high-water and evicted-byte
fields. Parent validation requires the exact 96/128/32 MiB split, total conservation, pressure and
counter reconciliation, terminal high-water agreement, owned-child lifetime and the 30-second
per-query deadline. Small/medium schema substitution and budget relabelling refuse. Existing
commands, profiles, schemas and defaults are unchanged.

Focused cache-profile and supervisor suites passed. The complete optimized standalone T-20 gate
passed **142 active tests with five unchanged opt-in ignores** and strict all-target Clippy. Log:
`/tmp/uste-d244-native-verification.log`. It used one Cargo job, one test thread, locked offline
dependencies and the 4 GiB process address-space limit under the verified enclosing 5/6 GiB
high/max and 512 MiB swap caps.

No sample ran and no performance, T-20, M1 or qualification claim follows. A subsequent observation
must use the unchanged retained fixture/oracle and separately compare semantic output, page work,
range pressure and latency to Decisions 0233 and 0243.
