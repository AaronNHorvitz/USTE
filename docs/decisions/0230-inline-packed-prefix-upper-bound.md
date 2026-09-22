# Decision 0230: Inline packed prefix upper bound

Date: 2026-09-21

Status: Implemented and locally verified; native performance observation pending.

Every packed adjacency or provenance scan constructed its 16-byte exclusive prefix upper bound in
a temporary `Vec`, then the cursor copied that bound into its own zeroizing storage. Replace only
the temporary caller allocation with fixed-size stack scratch plus an explicit used length. Preserve
the shortest-successor rule, trailing-`0xff` carry and the unbounded range when every prefix byte is
`0xff`. The cursor still owns and zeroizes its copied bound. Cursor selection, authenticated proof
work, candidate/result accounting, graph validation, authorization, formats and APIs are unchanged.

A focused unit test covers ordinary increment, one- and two-byte carry and the all-`0xff` boundary.
Packed expansion direction/duplicate/reference equivalence and both page-only and positive-cache
exact-work/narrow-limit tests passed. The first focused command used the wrong integration target
name after its library filter selected zero tests; the corrected exact filters passed. Initial
strict Clippy then rejected the test module preceding a production item; moving the module to the
file end resolved that lint without changing product code.

The final workspace gate passed **758 tests**, zero failures/ignores, strict workspace Clippy and
warnings-denied documentation. Its longest suites were graph disk 128/526.88 s, transaction
metadata 50/106.07 s, storage unit 244/31.46 s and transaction integration 118/107.58 s. Log:
`/tmp/uste-d230-workspace-verification.log`.

The standalone native release gate passed **136 active tests with five unchanged opt-in ignores**
and strict Clippy; log `/tmp/uste-d230-native-verification.log`. Both gates used one Cargo job, one
test thread and one heavy workload at a time with the inherited 4 GiB process address-space limit.
The enclosing shared scope's cumulative memory peak stayed at 5,372,850,176 bytes; swap peak rose
to 286,691,328 bytes within the 536,870,912-byte cap. Soft-limit events reached 36,739, with zero
maximum-limit, OOM or CPU-throttle events. Shared values are not process RSS.

No performance, T-20 or M1 gate claim follows until the unchanged supervised retained-fixture
protocol measures this binary. Keep the 64 MiB page-only default and every qualification
prerequisite.
