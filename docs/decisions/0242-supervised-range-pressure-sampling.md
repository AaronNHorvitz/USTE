# Decision 0242: Supervised range-pressure sampling

Date: 2026-09-22

Status: Implemented and locally verified; development observation pending.

Expose Decision 0241's exact pressure accounting only through a distinct supervised
`linux-packed-wide-small-range-pressure-sample` parent/worker command. Preserve the D0239
112/128/16 MiB page/lookup/range split, but emit the new
`bm01-linux-packed-wide-small-range-pressure-sampling-v1` schema and
`packed-pages-positive-lookups-small-ranges-pressure-256m-v1` cache profile. Every existing
command, schema, profile and JSON shape remains unchanged.

The pressure schema requires `maximum_accounted_bytes` and `evicted_bytes` in terminal range
configuration, warm-up range work and both empty/retained range entries in every sample. The parent
requires high-water accounting to cover current accounting without exceeding the 16 MiB budget,
sums checked evicted-byte deltas across warm-up and all measured states to the terminal counter, and
requires the latest retained high-water to equal the terminal gauge. Missing fields, over-budget or
regressed gauges, byte-counter mismatch, old-schema relabelling and pressure fields added to an old
schema all refuse. Existing counter, residency, total-conservation and owned-child 30-second
deadline checks remain mandatory.

Focused tests passed four cache-profile cases, two range-accumulator cases and seven supervisor
cases. The complete optimized standalone T-20 gate passed **142 active tests with five unchanged
opt-in ignores** and strict all-target Clippy. Log: `/tmp/uste-d242-native-verification.log`. It used
one Cargo job, one test thread, locked offline dependencies and the 4 GiB process address-space
limit under the verified enclosing 5/6 GiB high/max and 512 MiB swap caps.

No sample ran and no performance, T-20, M1 or qualification claim follows. A subsequent observation
must use the unchanged retained fixture/oracle and this pressure command. Its semantic and physical
results should match D0240 absent uncontrolled-cache noise because the cache split and behavior are
unchanged; only the newly authenticated telemetry shape differs.
