# Decision 0228: Ordered adjacency scan merge

Date: 2026-09-21

Status: Implemented and locally verified; native performance observation pending.

D0227 leaves the retained traversal latency gate open. Both authenticated adjacency scans already
return entries in strict key order, and their fixed namespace/entity prefix leaves the 16-byte
relationship suffix in `RecordRef` order. Shared expansion nevertheless inserted every candidate
into a new `BTreeMap` before consuming that same order. This allocated one tree node per candidate
and repeated ordered comparisons on every adjacency request.

Consume outgoing-only and incoming-only scans directly. For `Either`, merge the two ordered scan
vectors in one pass. Equal relationship suffixes are the self-loop/dual-direction case: require the
same scoped relationship and neighbor, combine both direction bits, and emit the candidate once.
Keep key/value shape and entity-prefix validation before reference construction. Keep relationship
status/endpoints, reference authorization, neighbor type, result limits and result ordering
unchanged. Authenticated cursor order remains the authority; no sorting, work limit, cache,
accounting, on-disk format or public API changes.

Focused verification passed the shared legacy expansion reference/work-budget test; packed
direction/self-loop/reference equivalence; all five exact aggregate limits; late corruption; and
page-only plus positive-cache exact-work/narrow-limit cases. Strict affected-crate Clippy passed.
The complete workspace gate then passed **757 tests**, zero failures/ignores, strict Clippy and
warnings-denied docs. Its long suites were graph disk 128/21,927.68 s, replay checkpoint
50/4,448.54 s, storage unit 244/1,349.76 s and transaction integration 118/4,836.75 s. The first
silent execution transport was lost without terminal status and is excluded; the complete rerun
used a 30-second transport heartbeat. Log: `/tmp/uste-d228-workspace-verification.log`.

The standalone native release gate passed **136 active tests with five unchanged opt-in ignores**
and strict Clippy; log `/tmp/uste-d228-native-verification.log`. Both final gates used one Cargo
job, one test thread and one heavy workload at a time inside the verified enclosing scope, with the
inherited 4 GiB process address-space limit. The cumulative shared scope peaks stayed at
5,372,850,176 bytes memory and 143,224,832 bytes swap; soft-limit events stayed at 26,549, and
maximum-limit/OOM/CPU-throttle events stayed zero. Shared values are not workload RSS.

No performance, T-20 or M1 gate claim follows until the unchanged supervised retained-fixture
protocol measures this binary. Keep the 64 MiB page-only default and every qualification
prerequisite.
