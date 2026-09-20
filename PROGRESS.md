# Implementation progress and handoff

Updated: 2026-09-20 · Branch: `codex/uste-implementation`

## Latest verified core increment — vault encryption accounting (Decision 0212)

D0211 completed below without a competing compiler. After confirming both sample processes
exited, unchanged source certificate, 25 GiB available RAM/2.0 GiB free swap/952 GiB disk,
focused crypto/coordinator verification started in session 50678 / `uste-d212-focused.scope`,
invocation `c2d18bbf447044ff94d0ec702e2758fa`. One job/thread, 3G/4G/512M group,
assertion/overflow-enabled test opt-level 1. **164 tests passed** in five executables, no failures
or ignores: crypto unit 9/0.00s, crypto boundary 14/2.22s, transaction unit 10/0.00s, authorization
13/0.03s, coordinator 118/106.28s, after 30.01s compile. Final scope peak 859,082,752 bytes,
zero swap, CPU 138,802,534,000 ns. Exact command:

```sh
systemd-run --user --scope --unit=uste-d212-focused.scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc 'set -o pipefail; CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-crypto -p uste-txn --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d212-focused-verification.log; verification_status=$?; systemctl --user show uste-d212-focused.scope -p MemoryHigh -p MemoryMax -p MemorySwapMax -p MemoryPeak -p MemorySwapPeak -p CPUUsageNSec; exit "$verification_status"'
```

After fresh 25 GiB available RAM/2.0 GiB free swap/952 GiB disk and no competing workload,
full workspace verification passed in session 54222 / `uste-d212-workspace.scope`, invocation
`ea32544adc434cc4832ba0045bf894a8`, with the same limits/job/thread/test profile. **736 tests
passed in 47 executables, none failed/ignored**. Compile 1m45s; graph disk 120/478.48s,
checkpoint 50/109.72s, storage 231/33.90s, coordinator 118/106.02s. Strict all-feature Clippy
passed in 11.85s and warnings-denied docs in 15.88s. Final scope peak 3,221,434,368 bytes,
zero swap, CPU 869,086,515,000 ns. This includes D0210 and D0212, not standalone D0213.
Exact command:

```sh
systemd-run --user --scope --unit=uste-d212-workspace.scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc 'set -o pipefail; { CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 && CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings && CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked --offline; } 2>&1 | tee /tmp/uste-d212-workspace-verification.log; verification_status=$?; systemctl --user show uste-d212-workspace.scope -p MemoryHigh -p MemoryMax -p MemorySwapMax -p MemoryPeak -p MemorySwapPeak -p CPUUsageNSec; exit "$verification_status"'
```

Next commit/push the reviewed core boundary, then verify and commit the separate native wiring.
D0213 includes paired atomic owner admission and per-owner encryption output totals across all
native phase/process paths; its release tests and strict lint are still pending. D0211 is pushed
as `21b2bda`; no large measurement will be rerun unchanged. T-20/T-19 remain incomplete.

## Completed development measurement — single-pass/cache comparison (Decision 0211)

Pushed/clean capability baseline `d3302394743d558a1f9e29dce7875effe446717d`; release binary
SHA-256 `2577c8ab7635f49d472aaf88f6f44de04d13ea19c12b46242089790e0f3d22d7`.
D0209 single-pass authentication and D0210 newest-slot cache lookup are measured together,
not attributed individually. Root/standalone locks, features and all fixture/oracle/cache/
query limits remain unchanged from D0206/D0208. No qualifying profile is admitted.

Session 96516 / `uste-d211-query.scope`, invocation `ac8e603fba7945749f036c2a2c3d8af1`, exited
zero after the complete independent correctness query pass on retained
`experiments/t20-bench/target/native-packed20000.ggoHSe`; separate report root
`experiments/t20-bench/target/native-lookup-fast20000.pG8YCJ`. Fresh preflight 25 GiB available
RAM/2.0 GiB free swap/952 GiB free disk, no competing workload. Same 3G/4G/512M group,
1,800-second command timeout/10-second TERM grace. Exact command:

```sh
systemd-run --user --scope --unit=uste-d211-query.scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '/usr/bin/time -v -o experiments/t20-bench/target/native-lookup-fast20000.pG8YCJ/query.time timeout --signal=TERM --kill-after=10s 1800s experiments/t20-bench/target/release/uste-t20-bench linux-packed-query --root /var/home/aaronnhorvitz/dev/01_repos/USTE/experiments/t20-bench/target/native-packed20000.ggoHSe --password-file /var/home/aaronnhorvitz/dev/01_repos/USTE/experiments/t20-bench/target/native-pressure20000.ya99oO/password --entities 20000 --oracle-file /var/home/aaronnhorvitz/dev/01_repos/USTE/experiments/t20-bench/target/native-packed20000.ggoHSe/oracle-summary > experiments/t20-bench/target/native-lookup-fast20000.pG8YCJ/query.json 2> experiments/t20-bench/target/native-lookup-fast20000.pG8YCJ/query.stderr; measurement_status=$?; systemctl --user show uste-d211-query.scope -p MemoryHigh -p MemoryMax -p MemorySwapMax -p MemoryCurrent -p MemoryPeak -p MemorySwapCurrent -p MemorySwapPeak -p CPUUsageNSec > experiments/t20-bench/target/native-lookup-fast20000.pG8YCJ/query.scope; exit "$measurement_status"'
```

All 384 outcomes (313 successful, 71 expected result-limit, zero visit-limit), output/oracle
digests, logical work, cache counters, query adapter/vault work and setup owner work exactly
match D0208. Query 699,843 ms versus 793,165 ms (11.8% lower); setup 53,535 ms versus 61,895 ms.
Wall 753.54s, user 679.81s/system 72.01s, peak RSS 265,864 KiB; scope peak 276,152,320 bytes,
zero swap, CPU 751,961,695,000 ns. Source certificate SHA-256 remains
`b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`; binary identity unchanged.

After fresh 25 GiB available RAM/2.0 GiB free swap/952 GiB disk and no competing workload,
one changed-binary supervised sample was admitted with the original deadlines and scope limits.
Session 57280 / `uste-d211-sample.scope`, invocation `b0ecd85c5c4c464ba8ff407967c4c532`,
exited zero. Exact command is the query command above with scope `uste-d211-sample.scope`,
output basenames `sample`, command `linux-packed-sample` and oracle `oracle-bundle`.
No unchanged retry or deadline extension. Wall 1,631.14s (27m11.14s), user 1,469.29s/system
158.12s; peak RSS 265,896 KiB, scope peak 278,847,488 bytes/zero swap, CPU 1,627,542,770,000 ns.
Supervisor verified 96 warm-ups (75 successful/21 expected result limits) and one 768-execution
paired round, 313 successes/71 expected result limits per cache state; no visit limits. Measured
round 1,397,258 ms. Empty-cache work exactly matches the query pass. Retained-cache misses
18,352,026/read bytes 377,042,374,170 versus empty 18,779,324/385,821,211,580; both report
4,095,526 successful visits and 46,474,984 logical result bytes. Retained successful all-class
p99 is 21.106773 ms at depth one and 5,093.321509 ms at depth four: a substantial development
performance gap, not a qualifying pass or permission to lower 20/250 ms targets.
Source certificate unchanged after completion and both processes exited before the build.
Raw reports, every latency population, exact commands, locks/features and resource provenance
are archived in [the D0211 record](docs/evidence/single-pass-cache-native-comparison.json).
D0206's timeout and all earlier artifacts remain intact; no qualifying campaign was run.

Decision 0212 is source-only work prepared during this run and explicitly excluded from its
binary: separate fixed-size vault encryption-work diagnostics, trusted journal/coordinator/
recovery/packed getters preserving poison/uncertainty and no-snapshot/no-I/O guards, plus
overflow/poison, nonce/error, exact framing, lock/unlock and owner-handoff tests. These edits
passed core verification above. Decision 0213 separately drafts native
paired owner admission with a sibling encryption ledger; unchanged decrypt output and explicit
partial-accounting flags remain. No consumer facade changes; complete authenticated I/O stays
false. Review corrected copied getter comments and the repeat-resume zero-encryption test
expectation before verification. Both source increments are excluded from the pinned measurement.

## Prior verified increment — newest page-cache fast path (Decision 0210)

Pushed `ef4c1a9` records the verified single-pass lookup; its release binary SHA-256 is
`2fb5eb87c10f771a42d61e6d59a9e4b021fe2914becc718bc69cc79c20057338`.
The next internal cache change compares the complete newest slot key before the ordered map.
It adds no state/allocation, preserves all enclosing context/owner/session checks, and leaves
clock, counters, exact LRU order and resource admission unchanged. Two new tests cover
10,000 comparison-free newest hits, non-newest reorder/eviction, complete-key mismatch,
empty/single-slot behavior and unchanged metadata capacity. Review corrected an immutable
loop-variable borrow in the new assertion before compilation; no runtime behavior changed.

Storage verification plus strict all-feature workspace Clippy/docs passed in session 20712 /
`uste-d210-storage.scope`, invocation `6bbec15b3cab49df8964837307d7c04d`, after fresh 25 GiB
RAM/2.0 GiB swap/952 GiB disk preflight and no competing workload. One job/thread,
assertion/overflow-enabled opt-level 1 storage tests, 3G/4G/512M group. **231 storage tests
passed** in 33.17s after 29.61s compile; strict Clippy 8.07s and warnings-denied docs 13.01s
passed. Final scope peak 1,588,989,952 bytes/zero swap. Exact command:

```sh
systemd-run --user --scope --unit=uste-d210-storage.scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc 'set -o pipefail; { CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-storage --lib --all-features --locked --offline -- --test-threads=1 && CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings && CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked --offline; } 2>&1 | tee /tmp/uste-d210-storage-verification.log; verification_status=$?; systemctl --user show uste-d210-storage.scope -p MemoryHigh -p MemoryMax -p MemorySwapMax -p MemoryPeak -p MemorySwapPeak -p CPUUsageNSec; exit "$verification_status"'
```

Native regression passed in session 13838 / `uste-d210-native.scope`, invocation
`718a287764c2453d81521f5831509db1`, after fresh 25 GiB RAM/2.0 GiB swap/952 GiB disk preflight
and no competing workload. **122 active tests passed, five unchanged opt-in ignores**:
release compile 1m05s, library 90/128.03s, legacy process 3/11.79s, packed history 11/59.70s,
packed terminal 7/31.40s, manifest 3/0.91s, legacy recovery 8/72.90s. Strict native Clippy
passed in 2.14s; final scope peak 568,975,360 bytes/zero swap. Same group limits and one
job/thread. Formatting, documentation and task graph checks passed. Exact command:

```sh
systemd-run --user --scope --unit=uste-d210-native.scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc 'set -o pipefail; { CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1 && CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings; } 2>&1 | tee /tmp/uste-d210-native-verification.log; verification_status=$?; systemctl --user show uste-d210-native.scope -p MemoryHigh -p MemoryMax -p MemorySwapMax -p MemoryPeak -p MemorySwapPeak -p CPUUsageNSec; exit "$verification_status"'
```

The capability is pushed as `d330239`; D0211 above records the completed native comparison. The prior
730-test full workspace result belongs to D0209 and excludes this later
cache change. A subsequent native comparison must pin both increments, preserve D0206/D0208
artifacts and keep qualifying targets/reservations unchanged. T-20/T-19 remain open.

## Prior verified increment — single-pass packed lookup authentication (Decision 0209)

Pushed baseline `b4a26f6` records D0208's complete read-only comparison. The follow-up uses the
already root-bound per-node chain instead of rehashing it at the leaf. Identical structural
routing/proof-input admission and all encrypted node, value, owner, budget and authorization
checks remain. The public full proof verifier retains its hash fold; every successful packed
unit-test traversal cross-checks that verifier. Four new reference/admission tests cover 768
logical and 768 packed byte/prefix queries, exact/minus-one limits and malformed routing.

Focused storage session 56438 / `uste-d209-storage.scope` exited zero: 229 passed, none failed/
ignored, 31.31 s after 27.99 s compile. Final scope peak 955,846,656 bytes/zero swap.
Exact command, one job/thread and assertion/overflow-enabled test opt-level 1:

```sh
systemd-run --user --scope --unit=uste-d209-storage.scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc 'set -o pipefail; CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-storage --lib --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d209-storage-verification.log; verification_status=$?; systemctl --user show uste-d209-storage.scope -p MemoryHigh -p MemoryMax -p MemorySwapMax -p MemoryPeak -p MemorySwapPeak -p CPUUsageNSec; exit "$verification_status"'
```

Full workspace verification passed in session 96340 / `uste-d209-workspace.scope`,
invocation `3e6168bc3e81417d92b6e2946cf6ce6b`. Fresh preflight 26 GiB RAM/2.0 GiB swap/952 GiB
disk, no competing workload, same 3G/4G/512M scope. **730 tests passed across 47 executables,
zero failures/ignores**; compile 14.74s, graph disk 120/440.45s, checkpoint recovery
50/103.74s, storage 229/30.89s, transaction coordinator 118/106.98s. Strict Clippy 7.33s
and warnings-denied docs 12.25s passed. Final scope peak 2,973,106,176 bytes/zero swap.
Exact command:

```sh
systemd-run --user --scope --unit=uste-d209-workspace.scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc 'set -o pipefail; { CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 && CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings && CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked --offline; } 2>&1 | tee /tmp/uste-d209-workspace-verification.log; verification_status=$?; systemctl --user show uste-d209-workspace.scope -p MemoryHigh -p MemoryMax -p MemorySwapMax -p MemoryPeak -p MemorySwapPeak -p CPUUsageNSec; exit "$verification_status"'
```

Native regression passed in session 70267 / `uste-d209-native.scope`, invocation
`48d3a09c7640497caca910515e40ca84`, after fresh 25 GiB RAM/2.0 GiB swap/952 GiB disk preflight
and confirmation of no competing workload. **122 active tests passed, five unchanged opt-in
ignores**; release build 1m05s, library 90/128.39s, legacy process 3/11.86s, packed history
11/59.61s, packed terminal 7/31.47s, manifest 3/0.91s, legacy recovery 8/72.83s. Strict native
Clippy passed in 2.12s. Final scope peak 565,596,160 bytes/zero swap. Same memory group bounds,
one job/thread. Root/standalone formatting, documentation and task graph checks pass. Command:

```sh
systemd-run --user --scope --unit=uste-d209-native.scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc 'set -o pipefail; { CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1 && CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings; } 2>&1 | tee /tmp/uste-d209-native-verification.log; verification_status=$?; systemctl --user show uste-d209-native.scope -p MemoryHigh -p MemoryMax -p MemorySwapMax -p MemoryPeak -p MemorySwapPeak -p CPUUsageNSec; exit "$verification_status"'
```

The change is pushed as `ef4c1a9`; D0210 checks the cache's newest-slot fast path before a
separately pinned comparison. No sampling or qualification pass is claimed. Preserve D0206's
timeout, D0208's exact older binary evidence,
all retained fixtures and M1's separate pinned handoff. T-20 and T-19 remain open.

## Prior measurement — retained-commitment native comparison (Decision 0208)

Verified capability `184b54bc7f508e8668e6850febabb766ef1442c4` is committed and pushed;
release binary SHA-256 `b35e6349961fc3885ff3a85745f0f67e0ce8082ccb2fbaefe884ae5fe158268d`.
Root/standalone lockfiles, release features, retained store, oracle summary, 64 MiB caches
and query budgets were unchanged from D0206. No build or competing workload ran during measurement.

Session 30062 / `uste-d208-query.scope`, invocation `90b640edb34542bfb692c1c31484d6ef`,
passed the independent correctness queries against
`experiments/t20-bench/target/native-packed20000.ggoHSe`. Separate reports are retained in
`experiments/t20-bench/target/native-read-reuse20000.02ohmt`. Fresh headroom 26 GiB available
RAM/2.0 GiB free swap/952 GiB free disk; 3G/4G/512M group and unchanged 1,800-second timeout.
Exact command:

```sh
systemd-run --user --scope --unit=uste-d208-query.scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '/usr/bin/time -v -o experiments/t20-bench/target/native-read-reuse20000.02ohmt/query.time timeout --signal=TERM --kill-after=10s 1800s experiments/t20-bench/target/release/uste-t20-bench linux-packed-query --root /var/home/aaronnhorvitz/dev/01_repos/USTE/experiments/t20-bench/target/native-packed20000.ggoHSe --password-file /var/home/aaronnhorvitz/dev/01_repos/USTE/experiments/t20-bench/target/native-pressure20000.ya99oO/password --entities 20000 --oracle-file /var/home/aaronnhorvitz/dev/01_repos/USTE/experiments/t20-bench/target/native-packed20000.ggoHSe/oracle-summary > experiments/t20-bench/target/native-read-reuse20000.02ohmt/query.json 2> experiments/t20-bench/target/native-read-reuse20000.02ohmt/query.stderr; measurement_status=$?; systemctl --user show uste-d208-query.scope -p MemoryHigh -p MemoryMax -p MemorySwapMax -p MemoryCurrent -p MemoryPeak -p MemorySwapCurrent -p MemorySwapPeak -p CPUUsageNSec > experiments/t20-bench/target/native-read-reuse20000.02ohmt/query.scope; exit "$measurement_status"'
```

Exit zero: all 384 outcomes, output/state/certificate hashes, visits/result bytes, cache
counters, query/setup adapter I/O and owner/vault work match D0206 exactly. Query-only
793,165 ms versus 870,407 ms (8.9% lower); setup 61,895 versus 69,958 ms. Total wall
855.22 s/RSS 265,676 KiB, final scope peak 277,741,568 bytes/zero swap. The exact report is
`docs/evidence/retained-commitment-native-comparison.json`. Host/device caches are uncontrolled;
these are development measurements, not qualifying latency. D0206's sampling timeout remains
incomplete; this modest improvement does not by itself prove a sample will finish. No sampling
retry ran. T-20/T-19 remain open; M1's exact-version result remains separate.

While D0208 ran, Decision 0209 prepared a source-only follow-up: reuse the already
root-bound node chain instead of rehashing its complete path at the leaf, retaining the
identical structural route and proof-input admission checks. The public full verifier is
unchanged semantically and remains a test-only cross-check for successful packed traversals.
Four new reference/admission regressions and the existing corruption/fault matrix passed
focused storage verification. These edits are NOT in D0208's binary. After its exit and fresh 26 GiB RAM/
2.0 GiB swap/952 GiB disk admission with no competing process, storage verification started
in session 56438 / `uste-d209-storage.scope` (invocation `ba19e4e35f1c4cc183871d66ffc6a69b`).
Command: `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-storage --lib --all-features
--locked --offline -- --test-threads=1`, in the established 3G/4G/512M scope, output
`/tmp/uste-d209-storage-verification.log`. Current full-workspace status and the next action
are recorded in the D0209 section above.

## Prior verified increment — retain validated packed-record commitments (Decision 0207)

Pushed `b4a72b4` preserves D0206's exact correctness/cache-pressure evidence and sampling
timeout. On that baseline, reuse the commitment computed during mandatory packed-node decode
for the immediately following root/parent comparison in point lookup, cursor traversal,
full-tree validation and batch inspection. Public decode and all proof, routing, value, owner,
authorization, I/O and resource checks remain unchanged. No format/API or M1 handoff change.

Workspace session 97961 / `uste-d207-workspace.scope` exited zero: **726 passed, zero failed or
ignored across 47 executables**. Build 1m09s; graph disk 120/440.96s, checkpoint recovery
50/104.03s, storage 225/33.05s (including the new mutation/context commitment test), transaction
coordinator 118/117.31s. Strict all-feature Clippy 7.31s and warnings-denied docs 11.92s passed.
Final scope peak 3,221,450,752 bytes, zero swap; one build job/thread, assertion/overflow-enabled
test opt-level 1. Preflight 26 GiB RAM/2.0 GiB swap/952 GiB disk. Exact command:

```sh
systemd-run --user --scope --unit=uste-d207-workspace.scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc 'set -o pipefail; { CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 && CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings && CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked --offline; } 2>&1 | tee /tmp/uste-d207-workspace-verification.log; verification_status=$?; systemctl --user show uste-d207-workspace.scope -p MemoryHigh -p MemoryMax -p MemorySwapMax -p MemoryPeak -p MemorySwapPeak -p CPUUsageNSec; exit "$verification_status"'
```

Native regression passed in session 8066 / `uste-d207-native.scope`, invocation
`25763b646af14d639714d8503b4e3d49`, after fresh 25 GiB RAM/2.0 GiB swap/952 GiB disk preflight
and no competing workload. Release build 1m05s; **122 active tests passed, five existing opt-in
cases ignored**: library 90/128.94s, legacy process 3/11.85s, packed history 11/60.03s,
packed terminal 7/33.06s, manifest 3/0.93s, legacy recovery 8/72.92s. Strict native Clippy
passed in 2.13s. Final scope peak 596,852,736 bytes/zero swap. Release binary SHA-256
`b35e6349961fc3885ff3a85745f0f67e0ce8082ccb2fbaefe884ae5fe158268d`.
Root/standalone formatting, documentation and 68-task dependency checks passed. Exact command:

```sh
systemd-run --user --scope --unit=uste-d207-native.scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc 'set -o pipefail; { CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1 && CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings; } 2>&1 | tee /tmp/uste-d207-native-verification.log; verification_status=$?; systemctl --user show uste-d207-native.scope -p MemoryHigh -p MemoryMax -p MemorySwapMax -p MemoryPeak -p MemorySwapPeak -p CPUUsageNSec; exit "$verification_status"'
```

The capability is now pushed as `184b54b`; D0208 above records a separately versioned
read-only comparison on the retained fixture before deciding whether changed performance
justifies another bounded sampling attempt. Do not relabel the D0206 timeout or raise its
deadline/benchmark targets. T-20/T-19 and full qualification remain open.

## Prior measurement — native 20,000 entities; sampling incomplete (Decision 0206)

Pushed/clean baseline `18a45e41d754b914b735fe413c15b09fb6d3b812`; release binary SHA-256
`0f7a7ee6c5172d59b3998fc5e1d2a4d48b74eda39f246d5c72f8c163801315ce`. Root/standalone lock hashes
remain `7ed2b533b3c801250a89e008b4ca26c48f16400e38224d8a63447c0393aaa97b` /
`4d348b4b13db7e0834d3cc998166d2eddf9e4fe4f96765a8ad7a172efbf21dd4`; standalone manifest defaults
and declared dependency features, release profile. No model/native/qualifying admission change.

Retain `experiments/t20-bench/target/native-packed20000.ggoHSe` on either success or failure.
Fixture session 71356 / `uste-d206-fixture.scope` exited zero, serial 600-second command limits:
manifest 0.16 s/2,776 KiB RSS; independent oracle summary 2.67 s/16,760 KiB; oracle bundle
3.45 s/17,488 KiB. Post-workload scope peak 22,675,456 bytes/zero swap. Manifest pins 20,000
entities/200,000 relationships, 44 durable revisions, 384 measured/96 warm-up query shapes.
SHA-256: manifest `e7e0e0c6229dded778948f9f1de71876e7c671ea32ae3554f90eef2c38a2f946`,
summary `aa3e6939c372351d6dfff0a8eaae989411c4a10d34244f2298114da34194af2c`,
bundle `f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b`.
Each command invoked the pinned binary with `manifest`, `oracle-summary` or `oracle-bundle`
and `--entities 20000`; outputs and GNU time files are retained in that directory.

Fresh preflight before generation and create: 26 GiB available RAM/2.0 GiB free swap/954 GiB
disk, no competing compiler/test/measurement. Same Fedora 44 Btrfs/NVMe zstd:1 profile, kernel
7.1.10-200.fc44.x86_64, i9-13900KF/32 logical CPUs; host caches/background applications are
uncontrolled. One heavy workload, 3G/4G/512M scope. The existing synthetic password file is
regular, 0600, one link/80 bytes; its contents are not printed or distributed.

Native create session 99976 passed under `uste-d206-create.scope`, 1,800-second timeout with 10-second
TERM grace. Report/error/time/post-scope paths are `create.json`, `create.stderr`, `create.time`,
`create.scope` in the retained root. It reached revision 44 and reports the complete fixture;
elapsed 161,373 ms (wall 161.37 s), peak RSS 345,200 KiB, post-workload scope peak 2,680,999,936
bytes/zero swap. Adapter reads 30,591,846,347 bytes/writes 2,542,755,209 bytes; three owner
lifetimes account for 1,462,683 completed decrypts/30,592,387,163 encoded bytes. These are
distinct measurement boundaries, not device traffic. State digest
`6880abb54859af231d877c8bbc1a9a665127a9953228c90e4a8278c048d1ecab`; certificate SHA-256
`b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`.

Fresh-process open session 13044 passed under `uste-d206-open.scope`, with the same command/limits
below substituting `open` for `create` in the command, scope and four result paths. Fresh
headroom 26 GiB RAM/2.0 GiB swap/952 GiB disk, no competing workload and unused scope name.
It reproduced the state digest and source-certificate hash, with zero adapter writes. Elapsed
69,508 ms/wall 69.51 s, RSS 265,348 KiB, scope peak 277,151,744 bytes/zero swap. Adapter reads
21,647,766,637 bytes; one owner reports 1,032,459 decrypts/21,647,762,379 encoded bytes.

Independent correctness queries passed under `uste-d206-query.scope` (session 22945, exit zero). Same fresh
26 GiB RAM/2.0 GiB swap/952 GiB disk and no competing workload; same 1,800-second/3G/4G/512M
bounds. Command substitutes `query` for `create` and adds `--oracle-file` with the retained
root's absolute `oracle-summary` path. Reports use `query.*` files. Expected independent corpus
is 384 queries: all 313 successes, 71 result-limit refusals and zero visit-limit refusals matched.
Output digest `0c978f102932e082ed1f1012e037a299658cb8d8ff26c5051438c67750b8ee42`,
4,095,526 successful visits/46,474,984 logical result bytes match the independent oracle.
Query-only 870,407 ms; total wall 940.53 s/RSS 265,588 KiB; scope peak 277,086,208 bytes,
zero swap. Actual cache evictions 17,781,965, hits 545,294,528/misses 18,779,324;
385,821,211,580 query adapter/decrypt encoded bytes and zero writes. Cache pressure is now
observed, but this is neither a qualifying sample nor evidence that latency targets pass.

Supervised sampling session 47650 exited 124 under `uste-d206-sample.scope`, invocation
`4fe26fd6d97f4b4f960c6fe7559c2d2e`, after fresh 26 GiB RAM/2.0 GiB swap/952 GiB disk preflight.
The same command below substitutes `sample` and uses `--oracle-file` with the retained root's
absolute `oracle-bundle` path. The original 1,800-second command/30-second query limits remain.
It reached the 1,800-second command deadline with empty stdout/stderr and no latency report;
no sample/qualification pass is claimed and no unchanged retry is admitted. Scope peak
277,757,952 bytes/zero swap, aggregate CPU 1,796.394 s; last worker sample at 29:45 was
89,340 KiB RSS/29:41 CPU and last memory-event sample at 27:38 was all zero. GNU time's
2,644 KiB/0.04 CPU-second parent-only result omits the terminated worker and is not its peak.
Both owned processes were gone before the next workload. Certificate hash still matches.
All raw reports, commands and provenance are in
`docs/evidence/native-packed-20000-development.json`; the synthetic store remains retained.
Qualification remains withheld and T-20/T-19 remain open; M1's pinned result is unchanged.

Decision 0207's record-commitment reuse change is excluded from the pinned D0206 binary.
Its full workspace verification subsequently passed in session 97961 / `uste-d207-workspace.scope`
(invocation `972604c7dada401dbe79265343b6a3ff`), after fresh 26 GiB RAM/2.0 GiB swap/952 GiB
disk preflight and confirmation that sampling left no competing worker. One Cargo job/test
thread, test opt-level 1 with debug assertions/overflow checks, 3G/4G/512M scope. The pipeline
is workspace/all-target/all-feature locked offline tests, strict workspace Clippy and
warnings-denied no-deps docs; output `/tmp/uste-d207-workspace-verification.log`. The current
verification totals and next action are recorded in the D0207 section above.

```sh
systemd-run --user --scope --unit=uste-d206-create.scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '/usr/bin/time -v -o experiments/t20-bench/target/native-packed20000.ggoHSe/create.time timeout --signal=TERM --kill-after=10s 1800s experiments/t20-bench/target/release/uste-t20-bench linux-packed-create --root /var/home/aaronnhorvitz/dev/01_repos/USTE/experiments/t20-bench/target/native-packed20000.ggoHSe --password-file /var/home/aaronnhorvitz/dev/01_repos/USTE/experiments/t20-bench/target/native-pressure20000.ya99oO/password --entities 20000 > experiments/t20-bench/target/native-packed20000.ggoHSe/create.json 2> experiments/t20-bench/target/native-packed20000.ggoHSe/create.stderr; measurement_status=$?; systemctl --user show uste-d206-create.scope -p MemoryHigh -p MemoryMax -p MemorySwapMax -p MemoryCurrent -p MemoryPeak -p MemorySwapCurrent -p MemorySwapPeak -p CPUUsageNSec > experiments/t20-bench/target/native-packed20000.ggoHSe/create.scope; exit "$measurement_status"'
```

## Prior verified increment — bounded packed process output (Decision 0205)

Pushed `2245a0a` records verified BM-06 owner accounting. The separately reviewed BM-01 process
test supervisor now drains stdout and stderr concurrently with 256 KiB + one sentinel byte per
stream. Normal 90-second deadline, owned-child kill/reap, exact source assertions and nonzero
exit diagnostics remain intact. New tests cover empty/large/exact-bound output, either overflow
stream, nonzero exit output and the expected deadline panic/cleanup. No production code or
benchmark limit changed.

Verified in the corrected Decision 0204 invocation below: all seven packed process tests passed
in 35.22 s, strict standalone Clippy passed in 1.66 s. This file was compiled in session 82243,
not the earlier session 87818. Tested runtime is the Decision 0204 source, now `2245a0a`, with
this harness patch; core diagnostics remain at `ef70c0b`. Exact commands, initial report-helper
failure, corrected results and resource limits are retained below. No additional large run was
performed for this harness change.

Next T-20: fresh host/binary/fixture admission, then a bounded native BM-01 development run at
the existing 20,000-entity ceiling. Generate independent oracle inputs, measure encrypted
create/open/query/sampling as safely permitted, retain artifacts and report actual cache pressure
and owner work. The ceiling is not measured capacity and cannot substitute for the frozen
100,000-entity/1,000,000-relationship qualification or BM-06's larger-than-memory campaign.
T-20/T-19/full roadmap and external release gates remain open; M1 remains pinned unchanged.

## Prior verified increment — native history owner accounting (Decision 0204)

Pushed `2b939e6` contains verified BM-01 lifetime accounting. The verified BM-06 integration
records bootstrap/resume, construction/rebuild, history validation and fresh terminal digest
admission owners before drop. Tail-only commands omit the terminal owner. It reuses checked
fixed slots without double-counting consuming handoffs; existing BM-01 scope and incomplete-I/O
flags remain unchanged. Tests extend every history command/prefix/process path's report checks.

Session 87818 / `run-p1186476-i22149293.scope` ran the full standalone release suite and
strict standalone Clippy, log `/tmp/uste-d204-native-suite.log`. Fresh preflight 26 GiB available
RAM/2.0 GiB free swap/954 GiB disk, no competing heavy workload. One Cargo job/test thread,
3G/4G/512M limits; exact command is Decision 0203's invocation below with this log path.
It exited 101 after the new report helper omitted valid `create-prefix`/`resume-prefix` labels:
two prefix tests failed with `unexpected completed phase`. Library 90/130.04 s and legacy
BM-01 process 3/11.84 s passed; history CLI had 9 pass/2 fail/3 ignored in 51.49 s. Compile
55.14 s. Remaining tests and Clippy did not run. Corrected only the new helper's phase mapping,
retaining exact owner-set/sum/source assertions; no runtime or acceptance relaxation.

Decision 0205's separate pending BM-01 harness fix was written after those binaries compiled:
bounded concurrent stdout/stderr drains, unchanged default deadline, exact bound/overflow and
owned-child cleanup controls. Session 82243 / `run-p1188337-i22146299.scope` verified the
corrected history and remaining process suites including that new harness, then strict Clippy.
Log `/tmp/uste-d204-d205-native-fixed.log`; same fresh 26/2/954 GiB headroom and process limits.
It exited zero: history 11 passed/3 ignored/59.52 s, packed terminal 7/35.22 s (including the
two separate Decision 0205 controls), manifest 3/0.91 s, legacy recovery 8/72.82 s. Compile
3.10 s; strict standalone Clippy 1.66 s. Combined with the preceding 90 library and 3 legacy
BM-01 process passes, all 122 active cases were exercised across the split runs; five opt-in
cases remain ignored. This is not one full-suite invocation on the final tree. Initial scope
sampled peak 417,042,432 bytes/zero swap; corrected scope 518,733,824 bytes/zero swap, neither
a final lifetime peak. No runtime behavior or acceptance thresholds were changed to fix tests.

Decision 0205 remains a separately reviewed/tested harness change for its next commit. Next
T-20: commit that output-safety prerequisite and perform resource-admitted native BM-01 scale
work. Completed decrypt attribution still omits key unwrap/encryption bytes/pre-vault decode
failures/device I/O; qualifying campaigns, larger-than-memory evidence and lifecycle work
remain outstanding. T-20/T-19 stay open; M1's exact pinned result is unchanged.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc 'set -o pipefail; { CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --test packed_history --test packed_terminal --test recovery_manifest --test recovery_process --locked --offline -- --test-threads=1 && CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings; } 2>&1 | tee /tmp/uste-d204-d205-native-fixed.log'
```

## Prior verified increment — native command owner accounting (Decision 0203)

Pushed `ef70c0b` contains the verified owner diagnostics. Native BM-01 changes now record
fixed disjoint owner lifetimes, reject duplicate slots/checked overflow, and publish exact
per-owner and total completed-decrypt reports. Existing single-owner fields and incomplete-I/O
qualification remain unchanged. Tests assert every command's slot set/sum and bootstrap
process-loss paths. BM-06 aggregation remains the next independent implementation step.

Session 48553 / `run-p1183481-i22185184.scope` passed the full standalone release suite and
strict standalone Clippy. Log `/tmp/uste-d203-native-suite.log`; fresh preflight 26 GiB available
RAM/2.0 GiB free swap/954 GiB disk, no competing heavy workload. One Cargo job/test thread and
3G/4G/512M limits. All 119 active tests passed; five opt-in cases ignored. Library 89/130.33 s,
legacy BM-01 process 3/11.96 s, native history 11/60.10 s, packed terminal 5/35.18 s, manifest
3/0.92 s, legacy recovery 8/72.82 s. Compile 1m02s; strict Clippy 2.79 s. Sampled scope peak
566,521,856 bytes/zero swap is not final lifetime peak. No failures or lowered requirements.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc 'set -o pipefail; { CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1 && CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings; } 2>&1 | tee /tmp/uste-d203-native-suite.log'
```

Next T-20: corresponding BM-06 lifetime accounting, including the currently discarded terminal
digest-admission owner and bootstrap resume. Then continue native BM-01 scale and safe larger
history/lifecycle prerequisites before reserved-host campaigns. No new large-run measurement,
complete authenticated-I/O or T-20/T-19 completion is claimed. M1 remains pinned unchanged.

## Prior verified increment — trusted owner diagnostics (Decision 0202)

Pushed `1d477d4` records the complete sixteen-batch development experiment below. Ordinary
coordinator and authenticated recovery owners now expose their existing cumulative vault decrypt
and nonce reports, without constructing reducer snapshots or doing I/O. Packed diagnostics
delegate through the same guarded path. Uncertain owners return `OutcomeUnknown`; storage/vault
failures remain fail-closed. Consuming recovery-to-packed handoff preserves exact counters.
No consumer facade, on-disk, authorization, durability or nonce-reset contract changed.

Session 56321 / `run-p1180929-i22176706.scope` exited zero. Log
`/tmp/uste-d202-owner-verification.log`; preflight 27 GiB available RAM/2.0 GiB free swap/954 GiB
disk, no competing heavy workload. One Cargo job/thread, 3G/4G/512M process-group limits.
All 141 transaction-crate tests passed: unit 10/0.00 s, policy 13/0.03 s, coordinator 118/115.90 s.
Compile 19.75 s, strict workspace Clippy 5.79 s, warning-denied workspace docs 10.49 s.
The first compile warned about one unnecessary `mut` in a new test. It was removed; the final
source's three focused `owner_work` tests passed in 0.02 s after 12.75 s compilation, session
91321 / `run-p1182767-i22149095.scope`, log `/tmp/uste-d202-owner-final.log`, preflight 26 GiB
RAM/2.0 GiB swap/954 GiB disk. No test failures or weakened assertions. No lifetime peak was
captured for these two scopes; configured caps are not measured RSS.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc 'set -o pipefail; { CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-txn --all-targets --all-features --locked --offline -- --test-threads=1 && CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings && CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked --offline; } 2>&1 | tee /tmp/uste-d202-owner-verification.log'
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc 'set -o pipefail; CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-txn --test transaction_coordinator --all-features --locked --offline owner_work -- --test-threads=1 2>&1 | tee /tmp/uste-d202-owner-final.log'
```

These tests cover only Decision 0202's core changes, not the separate pending Decision 0203
native accounting integration. Next T-20: finish and verify explicit per-command BM-01 vault
lifetime sums, then equivalent BM-06 attribution and safe scale/qualification prerequisites.
The getters alone do not establish complete authenticated I/O. T-20/T-19 remain open; pinned M1
and all accepted roadmap/release gates are unchanged.

## Prior verified increment — sixteen-batch native history check (Decision 0201)

Pushed `fb65972` preserves the verified proof-buffering result. Only packed native experimental
admission is raised to 8,192 records; the shared explicit scale test adds checkpoint 1,585,
terminal 1,601 and sixteen tail batches. Old 513/4,096 tests remain; out-of-range cases now test
8,193 plus unchanged 100,000 refusal. The 4,096-byte payload, 100 versions/record, logical proof
budgets, nonce/session cap, model/legacy caps and benchmark targets are unchanged.

Session 36786 / `run-p1174533-i22151710.scope` passed the ordinary standalone release suite
and strict Clippy. Log `/tmp/uste-d201-native-suite.log`; same command as Decision
0200 with this log path. Preflight 27 GiB available RAM/2.0 GiB free swap/969 GiB disk; one
job/thread, 3G/4G/512M process-group limits. All 118 active tests passed; five opt-in cases
ignored. Library 88/130.89 s, legacy BM-01 process 3/11.89 s, history CLI 11/59.66 s,
packed terminal 5/35.26 s, manifest 3/0.92 s, legacy history 8/73.12 s. Compile 54.44 s;
strict Clippy 1.46 s. Sampled peak 554,811,392 bytes/zero swap, not final lifetime peak.

The existing manifest command for 8,192 records reports 819,200 events, 3,355,443,200 payload
bytes, checkpoint 1,585/frontier 1,601, 8,192 tail events/16 revisions and synthetic stream digest
`cd07ca446292c0c1c847390e81ac9946c35a8d12d2b33794477497e8434cd1f8`. It is fixture arithmetic,
not materialization or qualification. Exact emitted JSON/newline SHA-256 is
`16c7bfff3370ca798b8e3b0947c91166dc584db090d410aa7fe18001abac712e`.

The explicit 8,192-record case passed in `uste-d201-native-history-8192.scope`, log
`/tmp/uste-d201-native-8192.log`. Fresh preflight 27 GiB available RAM/2.0 GiB swap/968 GiB disk,
no competing build/test and scope name unused. Same 1,800-second child/5,400-second overall
deadlines, one job/thread and 3G/4G/512M bounds, with post-workload scope peak capture.
Binary SHA-256 `1c524d7de77c101996dfe771598af83d6f8c4d06f3e239a83cb97fca6cd59fce`, based on
`fb6597224c1ed4ba84604f0adee0d60ab5a916c5` plus six source/test edits identified by hash.
Decision 0202 owner-diagnostic edits were made after this binary was built and are excluded.

Session 71382 exited zero: one test passed in 1,343.09 s (wall 1,343.15 s). All five phases
passed: create 311,711 ms, resume 288,029 ms, explicit checkpoint recovery 171,298 ms,
repeated resume 288,153 ms and cold open 170,759 ms. Checkpoint 1,585 contains 811,008 versions;
terminal 1,601 contains 819,200. Explicit checkpoint recovery replayed sixteen groups. All
terminal digests equal `b9c0e5feb5d9999c6479266fe8b506e3da05e068cd01d4f5cfaff789a6926091`;
repeated resume/open write zero adapter bytes and preserve the exact certificate log.
Construction used 577,772/1,048,576 nonces; this excludes bootstrap/key adapter/other owners.

Process peak RSS 349,020 KiB; post-workload cgroup peak 3,222,265,856 bytes and zero swap.
Last sampled events high 714,996/max 0/OOM 0/OOM-kill 0 are not final lifetime event totals.
Retained root `experiments/t20-bench/target/packed-history-cli-1176977-1789893893141352207`;
CERTIFICATES 6,665,922 bytes, SHA-256
`c889a36ba96c1543f9057d4384841ac4344a2dc1e9a97e482e9ba04bbf3b8ca8`.
[Exact command, provenance, manifest and full reports](docs/evidence/native-packed-history-8192-development.json)
archive the development result. No qualifying latency, complete authenticated I/O or
larger-than-24-GiB claim follows. T-20/T-19 remain open; M1 stays pinned unchanged.

Next T-20: verify the pending Decision 0202 trusted owner diagnostics, then explicitly account
for disjoint vault lifetimes without double-counting consuming handoffs. Continue native BM-01
scale and safe larger-history/lifecycle prerequisites before reserved-host qualification.

## Prior verified increment — native proof buffering (Decision 0200)

Pushed `a6b6f47` contains the locally verified domain integration. Native packed commands now
select separate 64 MiB fresh preparation caches for writer and suffix/origin work; report fields
and configuration assertions distinguish them from staging/admission caches. No model/default
cache choice, logical proof budget, nonce cap, native scale limit or qualifying target changes.

Session 93643 / `run-p1169799-i22156525.scope` runs the full release standalone suite and
strict standalone Clippy. Log `/tmp/uste-d200-native-suite.log`; preflight 27 GiB available RAM,
2.0 GiB free swap, 976 GiB disk, no competing build/test. Same 3 GiB high/4 GiB max/512 MiB swap
scope and one Cargo job/test thread. Invocation is the full standalone command under Decision
0197 with this new log path. **All 118 active tests passed; four opt-in cases ignored.** Library
88/129.70 s, legacy BM-01 process 3/11.88 s, history CLI 11/59.65 s, packed terminal 5/35.17 s,
manifest 3/0.92 s, legacy history 8/72.69 s. Release compilation 1m00s; strict Clippy 1.61 s.
Sampled peak 408,989,696 bytes/zero swap (not final lifetime peak).

The unchanged fresh 4,096-record eight-batch process-loss test passed in session 36773 under
`uste-d200-native-history-4096.scope`, log `/tmp/uste-d200-native-4096.log`. Fresh preflight
27 GiB RAM/2.0 GiB swap/976 GiB disk, no competing build/test and new scope name unused.
Same 1,800-second child/5,400-second overall deadlines, one job/thread, 3G/4G/512M limits and
post-workload scope peak capture as Decision 0197. Exact binary SHA-256
`3153f72477580d7f68a02fc674411ce8627a74d05523bc1038510af647c4849d`, based on
`a6b6f4766c8e877676fdab8dcc69da95c9dca2d7` plus the five pending native/test source edits.
Retained root `experiments/t20-bench/target/packed-history-cli-1172398-1789892590364740922`.
One test passed/658.16 s (wall 658.23 s), all five phases and source-prefix assertions intact.
Create 152,380 ms; resume 141,389 ms; checkpoint recovery 83,948 ms; repeated resume 141,254 ms;
cold open 83,772 ms. Checkpoint 793/405,504 versions and terminal 801/409,600 versions retain
the pinned digests. Written bytes and construction nonces match Decision 0197; repeated resume
and cold open write zero bytes. Final CERTIFICATES 3,337,122 bytes, SHA-256
`8f32aeade1cd9506f6c0895f981b7fd634f0d8c6178256bd412d3dc66341c995`.

Process peak RSS 349,248 KiB; post-workload scope peak 3,222,016,000 bytes, swap peak
16,269,312 bytes (3,096,576 bytes still swapped at observation). GNU time's `Swaps: 0` does
not mean zero cgroup swap. Last sampled memory events: high 198,060/max 0/OOM 0/OOM-kill 0;
these event counts are not final lifetime totals.

[Exact invocation, provenance and all phase reports](docs/evidence/native-buffered-proof-4096-development.json)
archive the result. Relative to Decision 0197, creation adapter reads fell from
169,026,198,092 to 55,545,418,057 bytes (5,523,523 fewer 20,545-byte page loads), and creation
elapsed fell from 273,191 to 152,380 ms. Other phase times do not establish a general recovery
improvement. Both measurements have uncontrolled host/device caches and are nonqualifying.

Historical next action (now completed by Decision 0201): extend the native history test to 8,192 records/16 suffix batches,
with unchanged source/payload/version semantics, nonce limit and process limits. Require fresh
host admission and full reference/recovery assertions; do not raise model/legacy or qualifying
limits or claim larger-than-memory qualification. Native BM-01 scale, complete authenticated I/O,
nonce/rotation lifecycle and qualifying campaigns remain outstanding.

## Prior verified increment — writer and suffix proof integration (Decision 0199)

Pushed `01020ef` contains the verified primitive. Optional `proof_cache_bytes` now selects it
for authorized writer preparation and each suffix/origin revision, independently of staging.
Existing constructors select `None`; suffix cache counters use checked aggregation. Twelve new
integration tests cover authorization, content-free errors, retries/collisions/expiry, revocation,
uncertainty, invalid budget refusal without writes, origin/suffix reference recovery, cache
counter conservation and all observed suffix faults/corruption. Old 609 suffix fault cases remain.

Corrected session 18349 / `run-p1168105-i22184021.scope` passed all twelve/37.47 s;
compile 11.00 s and strict workspace Clippy 0.51 s. Initial test expectation/lint failures and
their exact disposition are recorded below. Warning-denying workspace docs passed/2.71 s in
session 70995. No default behavior, native admission or benchmark claim changed.

Broader session 95827 / `run-p1168761-i22184056.scope` passed all 53 `packed_graph_` tests
in 130.00 s, including old uncached and new buffered cases, followed by strict standalone Clippy
compatibility/1.91 s. Compile reuse 0.04 s. Sampled peak 30,330,880 bytes/zero swap, not final peak.
Log `/tmp/uste-d199-regression-compatibility.log`; preflight 27 GiB available RAM/2.0 GiB free
swap, one job/thread under the same 3G/4G/512M bounds.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
{ CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-graph --test disk_index --all-features --locked --offline packed_graph_ -- --test-threads=1 &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings; } 2>&1 | tee /tmp/uste-d199-regression-compatibility.log'
```

Next explicitly select native proof buffering, run the full standalone
suite and remeasure the unchanged 4,096-record fixture. Keep exact provenance and all qualification
limits. T-20/T-19 remain open; M1 remains complete at its original pinned qualification.

## Latest verified primitive — fresh graph proof preparation (Decision 0198)

Pushed `3acead5` preserves the native staging result below. Additive opt-in buffered graph
preparation shares existing proof/reducer logic and keeps the original API uncached. Five new
tests cover fresh residency/read reduction, all exact/minus-one proof limits, reducer successes
and rejections, current/history/reverse corruption and owner/scope checks, and all observed
read faults/crashes with recovery. The original 270 uncached fault cases remain unchanged.

Initial command selected nonexistent `--test disk_state` (exit 101, no tests); corrected target
`disk_index` then caught an unnecessary qualification exposed by the new shared cache import
(exit 101, no tests). Corrected that qualification. Logs `/tmp/uste-d198-focused.log` and
`/tmp/uste-d198-focused-fixed.log` preserve both failures. Final focused session 53721 /
`run-p1162766-i22101992.scope` passed 5 tests/0.53 s, compile 19.97 s and strict graph Clippy
2.03 s. Log `/tmp/uste-d198-focused-final.log`. Preflight 27 GiB available RAM, 2.0 GiB free
swap, 976 GiB free disk; one Cargo job/test thread, 3 GiB high/4 GiB max/512 MiB swap.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
{ CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-graph --test disk_index --all-features --locked --offline packed_graph_buffered_preparation -- --test-threads=1 &&
CARGO_BUILD_JOBS=1 cargo clippy -p uste-graph --all-targets --all-features --locked --offline -- -D warnings; } 2>&1 | tee /tmp/uste-d198-focused-final.log'
```

Full workspace session 96213 / `run-p1163449-i22144473.scope` ran the same complete
test/Clippy/warning-denying-doc command recorded under Decision 0194, with log
`/tmp/uste-d198-workspace-verification.log`. Its test executables finished compiling before
the separate Decision 0199 domain edits were written. Those new writer/suffix options and twelve
tests are not verified by this test invocation; subsequent Clippy/docs will see them. Keep
their files out of the Decision 0198 commit until separately tested. The domain compatibility
changes in `experiments/t20-bench/src/engine/packed/limits.rs` select `None`; no native selection
or measurement is part of either pending domain increment. No scale admission, qualifying
campaign, M1 or task-status changes.

Its **710 tests across 47 executables passed**, zero failed/ignored: graph disk 108/439.96 s,
replay checkpoint 50/104.59 s, storage 224/32.84 s and transaction coordinator 115/116.63 s;
compile 1.67 s. Sampled scope peak 690,524,160 bytes/zero swap, not final lifetime peak.
The following Clippy step failed on nine redundant `self::` qualifiers in the then-new Decision
0199 test helpers (overall command exit 101; docs did not run). Removed those qualifiers;
no behavioral/test expectation changes. Separate session 70995 / `run-p1167201-i22175928.scope`
passed strict workspace Clippy/1.59 s and warning-denying docs/2.71 s. Its subsequent twelve
Decision 0199 integration tests passed eleven and failed one/37.45 s (compile 20.27 s): the new
invalid-cache test expected `Authorization(Transaction(Storage(ResourceLimit)))`, but existing
writer semantics correctly return `Preparation(Storage(ResourceLimit))`. Corrected that exact
expectation, preserving zero-write/frontier and exact-retry assertions. Log
`/tmp/uste-d199-domain-verification.log`; preflight 27 GiB available RAM/2.0 GiB swap. Same
3G/4G/512M process bounds, one job/thread. Separate corrected session 18349 /
`run-p1168105-i22184021.scope` runs the twelve integration tests and strict workspace Clippy,
log `/tmp/uste-d199-domain-verification-fixed.log`; its passing result is recorded above.

The Decision 0198 primitive is independently verified by five focused and all 710 workspace
tests and committed/pushed as `01020ef`. Decision 0199 writer/recovery/test/constructor paths
are separately verified as recorded above.

## Prior verified increment — native buffered staging measurement (Decision 0197)

Pushed `9b07d0f` is the verified 705-test core baseline. Native packed commands now opt into
64 MiB per-private-batch staging, while model/default constructors remain uncached. New report
and configuration assertions passed as recorded below. No scale increase is claimed.
The first build, session 91221 / `run-p1156496-i22147849.scope`, exited 101 because
the enlarged history `serde_json::json!` macro exceeded its compile-time recursion limit.
Log `/tmp/uste-d197-native-suite.log`; no tests ran. Split the two new fields into separate
insertions, retaining the same output schema and default compiler recursion limit.

Corrected full standalone session 29056 / `run-p1156998-i22109325.scope` passed all 118 active
tests; four opt-in cases ignored. Library 88/131.26 s, legacy process 3/11.84 s, history CLI
11/60.21 s, packed terminal 5/35.73 s, manifest 3/0.93 s, legacy history 8/72.54 s.
Release compile 54.57 s; strict standalone Clippy 1.81 s. Sampled peak 398,331,904 bytes/zero
swap (not final peak). Preflight 28 GiB available RAM/2.0 GiB free swap/983 GiB disk.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
{ CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1 &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings; } 2>&1 | tee /tmp/uste-d197-native-suite-fixed.log'
```

Also backfilled explicit lock hashes/features in Decision 0193's archive, checked against its
pinned `0f17918` base; its historical measurements are unchanged.

The explicit fresh 4,096-record run passed: session 58344,
`uste-d197-native-history-4096.scope`, log `/tmp/uste-d197-native-4096.log`. Preflight 27 GiB
available RAM/2.0 GiB free swap/983 GiB disk, no competing Cargo workload. Same 1,800-second
child/5,400-second overall deadlines and 3 GiB high/4 GiB maximum/512 MiB swap bounds. Wrapper
records post-workload scope peak/swap statistics before exit and preserves the workload status.
Retained synthetic root: `experiments/t20-bench/target/packed-history-cli-1159451-1789889951482048000`.
Binary SHA-256 `fc0e14d081c7b9a91f617d4ceaa7119e6ee4baa45b81e8f3a4d6ae3597f4ea8a`, based on
`9b07d0f15f9d0021e69e6381cecfe11c11c31812` plus the pending Decision 0197 source edits.
One test passed in 780.74 s (wall 780.80 s). All five phases preserved exact digests and source
certificate-prefix assertions: create 273,191 ms; resume 142,885 ms; checkpoint recovery 84,602 ms;
repeated resume 141,231 ms; cold open 83,479 ms. Terminal history is 409,600 versions at revision
801; checkpoint revision 793 plus eight suffix groups. Repeated resume and cold open wrote zero
bytes. Process peak RSS 349,636 KiB; post-workload scope MemoryPeak 3,222,011,904 bytes and
MemorySwapPeak zero. Final CERTIFICATES 3,337,122 bytes, SHA-256
`c7fc7584318b87b4606009c9804f5a1694e9279783a418c5a3c41a656aee6020`.

[Exact invocation, provenance and all phase reports](docs/evidence/native-buffered-staging-4096-development.json)
retain the measured result. Compared with Decision 0193, create adapter reads fell from
219,650,187,522 to 169,026,198,092 bytes and elapsed time from 321,264 to 273,191 ms; writes and
nonce counts are identical. Other phase elapsed times do not show a consistent improvement.
These are uncontrolled-host development comparisons, not physical-device I/O, complete
authenticated I/O, qualifying recovery latency or larger-than-memory acceptance.

Next T-20 action: isolate remaining construction/read amplification, preserve full proof checks,
and implement bounded improvements with reference/corruption/fault coverage before increasing
native admission or running qualifying campaigns. Inspection ruled out unconditional historical
proof loading: preparation scans history only for explicit read-view predicates, while this
fixture uses version predicates. T-20/T-19 stay open; M1 qualification remains pinned unchanged.

## Prior verified increment — explicit domain staging buffers (Decision 0196)

Pushed baseline `6b35b3b`. Optional per-batch budgets now flow through packed graph genesis,
delta/live staging and coordinator primary/quota staging; all existing constructors explicitly
select `None`. New buffered reference, proof-budget, corruption and every-observed-fault variants
preserve the original uncached tests and their literal fault counts. No native cache selection
or performance claim is included in this domain increment.

Focused session 88371 / `run-p1149170-i22101447.scope` passed 11 graph/8.22 s and 15
transaction/16.67 s buffered cases; compile 38.83 s, strict workspace Clippy 6.51 s.
Preflight 28 GiB available RAM/2.0 GiB free swap; one job/thread, 3 GiB high/4 GiB maximum/
512 MiB swap. Log `/tmp/uste-d196-focused.log`.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
{ CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-graph -p uste-txn --all-targets --all-features --locked --offline buffered -- --test-threads=1 &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings; } 2>&1 | tee /tmp/uste-d196-focused.log'
```

Next verify the separate native selection and measure the unchanged development fixture.
T-20/T-19 remain open; these results imply neither M1 requalification nor benchmark qualification.

Subsequent review added checked cache-counter aggregation in graph suffix reports and two
end-to-end buffered suffix/live-rebase tests. Full workspace session 58732 /
`run-p1151257-i22147655.scope` passed, log `/tmp/uste-d196-workspace-verification.log`.
All **705 tests across 47 executables** passed, zero failed/ignored. Graph disk 103/437.63 s,
replay checkpoint 50/104.81 s, storage unit 224/32.79 s, coordinator 115/116.37 s.
Compile 29.85 s, strict workspace Clippy 2.61 s, warning-denying workspace docs 10.36 s.
It used the same full-workspace test/Clippy/warning-denying-doc
command as Decision 0194 below, with that new log path. Preflight 28 GiB available RAM/2.0 GiB
free swap; sampled peak 1,581,711,360 bytes/zero swap, not final lifetime peak.

Separate Decision 0197 experiment edits select 64 MiB per-private-batch staging in native packed
paths and add report/configuration checks. They were not part of this workspace run and were
subsequently verified separately above. The
`experiments/t20-bench/src/engine/packed/limits.rs` compatibility additions (`None`) belong to
Decision 0196; the other experiment paths belong to Decision 0197.

## Prior verified increment — buffered staging and scoped bridge (Decisions 0194–0195)

Pushed `3afb5ac` preserves the verified eight-batch development result below. Decision 0194
adds fresh bounded packed-tree staging without changing proof charges or output publication.
Focused session 88762 / `run-p1138925-i22108221.scope` passed all five new storage cases/0.27 s,
compile 35.13 s. Command: `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-storage --all-features --locked --offline buffered_staging -- --test-threads=1`
inside the usual 3 GiB high/4 GiB maximum/512 MiB swap scope; log `/tmp/uste-d194-focused.log`.

Full workspace session 82116 / `run-p1140008-i22124383.scope` passed, log
`/tmp/uste-d194-workspace-verification.log`. Preflight 28 GiB available RAM/2.0 GiB free swap/
983 GiB disk; one Cargo job/test thread. Compilation finished in 59.96 s; sampled scope peak
2,350,116,864 bytes/zero swap (sample, not final lifetime peak). All 693 tests across 47
executables passed, zero failed/ignored. Graph disk 98/400.56 s, replay checkpoint 50/103.91 s,
storage unit 224/32.67 s, coordinator 108/104.14 s. Strict workspace Clippy passed/7.72 s and
warning-denying workspace docs passed/11.76 s.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
{ CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked --offline; } 2>&1 | tee /tmp/uste-d194-workspace-verification.log'
```

Decision 0195's additive transaction maintenance bridge and two new tests were written after
the full-suite test executables finished compiling; they were **not tested by that invocation**.
Its later Clippy/doc steps included them. Separate session 53118 /
`run-p1146075-i22124832.scope` passed three maintenance tests/0.01 s, compile 14.84 s, including
both new tests and the original ordinary retry path. This verifies the bridge independently;
do not describe the full workspace invocation as 695 tests. Command below also starts the
full standalone regression run, which subsequently **passed**: 117 active tests, four explicitly
ignored development cases (including the separately verified scale tests). Library 87/131.65 s,
legacy process 3/11.85 s, packed history CLI 11/60.79 s, packed terminal 5/36.08 s,
manifest 3/0.93 s, legacy history process 8/73.11 s; release compile 63 s. These executable
tests used the `6b35b3b` implementation, built before Decision 0196's edits. Final standalone
Clippy passed/3.34 s and saw the then-current Decision 0196 library/limit additions; it does
not verify their behavior. Sampled scope peak 646,856,704 bytes/zero swap (not final peak).
Preflight 28 GiB available RAM/2.0 GiB free swap.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
{ CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-txn --test transaction_coordinator --all-features --locked --offline maintenance -- --test-threads=1 &&
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1 &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings; } 2>&1 | tee /tmp/uste-d195-bridge-and-native.log'
```

Format/diff/docs/task checks pass. Next integrate
optional bounded staging into graph/coordinator callers with unchanged proof-work limits,
reference/fault/recovery tests and actual native measurements. Neither new API is itself native
performance evidence; T-20/T-19 and all full-roadmap gates remain open. M1's pinned handoff is
unchanged: these are privileged maintenance additions, not a consumer-interface migration.

## Prior verified increment — eight-batch native development (Decision 0193)

Pushed baseline `0f17918`; uncommitted experiment raises only packed native admission to 4,096
records and retains the 513-record case. The supervisor now concurrently drains both output pipes
with a 256 KiB per-pipe content limit. Default CLI session 11566 passed ten tests/62.12 s, with
both scale cases explicitly ignored; compile 26.45 s, strict Clippy 1.50 s, sampled scope peak
347,852,800 bytes/zero swap. This does **not** verify the larger ceiling.

Explicit larger run session 66471 / `run-p1132051-i22100377.scope` failed after checkpoint creation, using one
job/test thread and 3 GiB high/4 GiB maximum/512 MiB swap bounds, 1,800-second child and
5,400-second overall deadlines. Preflight 27 GiB available RAM/2.0 GiB free swap/997 GiB disk,
no competing Cargo workload. Log `/tmp/uste-d193-native-4096.log`; retained synthetic root
`experiments/t20-bench/target/packed-history-cli-1132228-1789885187740006861`.
Binary SHA-256 `7f660ae7973bdb70f352b1499b7e78e541b422d9d0ca33af5bbd84c468df5d2a`.
Creation verified 405,504 versions at revision 793 in 323.925 s, using 288,912 construction-owner
nonces, peak process RSS 292,312 KiB. The next tail probe exited before its marker; the supervisor
lost stderr and instead reported EOF while parsing an empty marker. Total 333.95 s, exit 101;
source certificate length remains 3,303,834 bytes. Last sampled scope peak 3,222,261,760 bytes,
swap 262,144 bytes; earlier memory.events had zero max/OOM events and active high-limit reclaim.
Do not relaunch unchanged construction. The marker helper is being repaired to retain bounded
stderr on premature exit. Retained checkpoint diagnostic open is session 79978, log
`/tmp/uste-d193-retained-open.log`. Preserve this source store; no larger-ceiling verification,
qualification or task completion is implied.

The retained diagnostic open failed at `USTE_BM06_PACKED_BINDING` in 9.71 s. Binding's fixed
1 MiB cursor budget omitted the distance from revision one through the current certificate
frontier (>3 MiB here). Corrected it to add the profile-derived certificate-proof allowance,
with checked arithmetic, while retaining the original small-policy-group allowance and exact
identity checks. Session 26690 passed the new bound arithmetic test and ten CLI cases/60.24 s;
compile 53.47/0.02 s, strict Clippy 1.51 s, sampled peak 386,785,280 bytes/zero swap.
Session 73276 passed the new early-marker-exit diagnostic test, then the corrected retained open
in 76.08 s/peak RSS 345,524 KiB. Digest remains
`a9d0967ebb60c0c3fd8c43dbbd6249ce481cc2b03898b8baa0a05c30d1c222e2`; certificate SHA-256 remains
`40702c770d9abbeb467119406f8ae6345417efb223fa1b0307636f4133c1406c`, with zero adapter writes.
Corrected binary SHA-256 `cb7506e9ccc7992268edd9380d9673e84c15dd19ac63f64cdfc28f5a14f903e8`.
The full corrected fresh-fixture run is session 6582 / `run-p1136052-i22104799.scope`;
log `/tmp/uste-d193-native-4096-fixed.log`, retained root
`experiments/t20-bench/target/packed-history-cli-1136222-1789886072155584131`.
The corrected run passed: one explicit test/824.73 s, total wall 824.79 s, peak process RSS
348,556 KiB. Sampled scope peak 3,222,011,904 bytes; maximum observed swap 19,996,672 bytes
(not final lifetime peak), zero observed max/OOM events. Creation, killed-tail resume, explicit
eight-group checkpoint recovery, repeated resume and cold open all passed. Terminal revision
801 contains all 409,600 versions with matching digest and unchanged source certificate prefixes.
The first fixture remains intact. Exact source/binary hashes and all five phase reports are in
`docs/evidence/native-packed-history-4096-development.json`, including the exact scoped command.
This verifies the development ceiling only, not larger-than-memory or qualifying BM-06 behavior.

Separate unverified Decision 0194 storage changes add fresh operation-local packed-tree staging
buffers and focused tests. They were written after the corrected benchmark binary was built;
they are not linked into that run and must not be included in the Decision 0193 commit/evidence.
The scale workload has finished. Next verify and repair Decision 0194's buffered staging, then
integrate only after reference/fault/corruption checks. Full core remains 687 at `c3b7047` plus
subsequent focused coverage; full standalone remains 113 at `ed0bdd6` plus focused coverage.
T-20/T-19, pinned M1 and external qualification/release gates remain unchanged. This next action
supersedes historical next-step text below.

## Prior verified increment — packed history phase accounting (Decision 0192)

Built and tested on pushed `509feab`: successful native history reports now partition monotonic
time and adapter counters into setup/admission, final history verification, tail/exact-retry work
and terminal-digest admission. Checked deltas reject counter/clock rewind; all stage byte and
operation/failure counters sum to the final adapter snapshot. Setup includes construction and
earlier prefix verification where requested. No stage is advertised as qualifying recovery latency,
complete authenticated I/O or physical-device traffic. Existing command totals remain available.

Session 13777 / `run-p1129572-i22104462.scope` passed the new phase-partition unit test and all
nine active native history CLI cases/62.25 s; one opt-in scale case not selected here. Compile
57.07 s, second invocation 0.02 s, strict standalone Clippy 1.66 s. The CLI checks conservation
across create/open/tail/recover/checkpoint-replay/rebuild/resume and bounded construction prefixes.
Source/certificate/digest assertions remain intact. Preflight 28 GiB available RAM/2.0 GiB free
swap; sampled scope peak 377,761,792 bytes/zero swap, not final lifetime peak.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
{ CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline phase_work_partitions -- --test-threads=1 &&
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --test packed_history --locked --offline -- --test-threads=1 &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings; } 2>&1 | tee /tmp/uste-d192-phases.log'
```

Format/diff/docs/task checks pass. Full core remains 687 at `c3b7047` plus Decision 0191's focused
crypto/fault coverage; full standalone 113 at `ed0bdd6` plus subsequent focused coverage. Next
exercise a resource-admitted larger native multi-batch development fixture with bounded process
output and actual nonce/phase measurements; retain failures and require a real pass before
recording an increased development ceiling. Complete accounting, exact-size construction and
reserved-host qualification remain open. T-20/T-19, pinned M1, full roadmap and external release
gates are unchanged. Supersedes older next-step text.

## Prior verified increment — construction nonce headroom (Decision 0191)

Built and tested on pushed `ed0bdd6`: privileged per-vault nonce diagnostics expose exact issued,
limit and remaining counts without resetting or changing encryption. Lock/unlock retains counts;
journal poison/coordinator uncertainty refuse diagnostics. Native construction samples its actual
owner before dropping it, explicitly excluding bootstrap, other vaults and key-adapter work.
Other phases report null. No consumer authorization surface or ciphertext format changes.

Initial focused session 44257 failed compilation on an unnecessary qualification in the new test;
removed that qualification, without changing lint policy. Corrected session 40493 /
`run-p1124147-i22089072.scope` passed all 19 crypto tests (13 integration/2.29 s), one storage
poison/refusal case and the packed live every-observed-commit-fault sweep/1.11 s. Compile times
1.18/40.15/19.76 s; strict workspace Clippy 11.96 s. Scope peak was not captured.
Native session 52750 / `run-p1126858-i22123479.scope` passed nine active history CLI tests/62.27 s,
one explicit scale case ignored in that invocation; compile 37.52 s, strict standalone Clippy
1.62 s, sampled peak 375,693,312 bytes/zero swap. Warning-denying workspace docs session 77624
passed/14.02 s. Full historical core remains 687 at `c3b7047`; full standalone 113 at `ed0bdd6`.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
export CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true
{ cargo test -p uste-crypto --all-features --locked --offline -- --test-threads=1 &&
cargo test -p uste-storage --all-features --locked --offline packed_roots_binding_limits_and_poison -- --test-threads=1 &&
cargo test -p uste-txn --all-features --locked --offline packed_live_every_observed_commit_fault -- --test-threads=1 &&
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings; } 2>&1 | tee /tmp/uste-d191-focused.log'
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
{ CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --test packed_history --locked --offline -- --test-threads=1 &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings; } 2>&1 | tee /tmp/uste-d191-native.log'
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked --offline'
```

[Pinned native measurement](docs/evidence/native-packed-nonce-headroom-development.json) includes
exact replacement-source/binary/lock hashes, invocation and all five phase reports. Explicit scale
session 56027 / `run-p1127945-i22107643.scope` passed/90.75 s, total wall 90.81 s, peak RSS
347,652 KiB/zero swaps. Construction through checkpoint 199 used **37,127 nonces**, leaving
1,011,449; interrupted-tail resume used 370, repeated terminal resume zero. These are per-owner
observations, not extrapolated exact-size bounds or successful-encryption totals. All terminal
digests match the earlier pinned fixture. Retained synthetic root:
`experiments/t20-bench/target/packed-history-cli-1128123-1789884453940092208`.
Preflight 28 GiB available RAM/2.0 GiB free swap/998 GiB free disk; sampled scope peak
1,370,808,320 bytes/zero swap is not its final lifetime peak. Other workloads remained untouched.

Format/diff/docs/task checks pass. Next separate packed recovery/admission, history verification
and terminal-digest work before larger bounded development runs. Complete accounting and safe
exact-size construction/30-trial reserved-host qualification remain open; current headroom is not
a benchmark reservation. T-20/T-19, pinned M1 and the entire roadmap/release gates remain unchanged.
Supersedes older next-step text.

## Prior verified increment — bounded native construction (Decision 0190)

Built and tested on pushed `8cfd933`: packed native create-prefix/resume-prefix accept explicit
complete-generation targets through the checkpoint, including policy-only revision one.
They authenticate/verify existing history, preserve exact retries, refuse rewinds/overwrite,
and retain the 513-record ceiling. Missing/duplicate/mixed flags and invalid targets refuse
before I/O. Ordinary phases remain unchanged. No writer rotation or nonce reset is implied.

Focused session 35613 / `run-p1116525-i22099492.scope` passed three prefix CLI tests/11.98 s,
compile 25.87 s. Full session 28027 / `run-p1117739-i22103882.scope` passed **113 active tests**,
three ignored campaigns (the native scale case separately passed at `8cfd933`), seven executables
plus empty doctests. Library 85/135.85 s, legacy BM-01 process 3/12.12 s, packed history 9/63.17 s,
packed BM-01 process 5/36.95 s, manifest 3/0.97 s, legacy history 8/76.18 s; compile 30.04 s,
strict Clippy 4.06 s. Test executables were built before pending Decision 0191 diagnostics;
the final Clippy run also checked those additive core APIs, which are excluded from this increment.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --test packed_history --locked --offline bounded_prefix -- --test-threads=1 2>&1 | tee /tmp/uste-d190-prefix.log'
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d190-full-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Preflight 21 GiB available RAM/2.0 GiB free swap; sampled scope peak 390,266,880 bytes/zero swap,
not final lifetime peak. Format/diff/docs/task checks pass. Core baseline remains 687 at
`c3b7047`; no qualifying campaign ran. Next verify actual construction-owner nonce headroom,
then complete accounting and safe larger construction before reserved-host BM-01/BM-06.
T-20/T-19 remain open; pinned M1 and all roadmap/release requirements are preserved.
Supersedes older next-step text.

## Prior verified increment — native multi-batch history (Decision 0189)

Built and tested on pushed `b892f12`: packed native BM-06 admits at most 513 records; model and
legacy native caps remain two. A new owned-child tail-prefix probe stops after intermediate
graph publication before metadata rebase. Qualifying dimensions still refuse before I/O.
The opt-in scale case constructs 50,787 versions at checkpoint 199, SIGKILLs its child at 200,
resumes the remaining batch through 201/all 51,300 versions, explicitly replays both groups
from 199 despite newer roots, and verifies repeated resume/open without certificate appends.
The expanded native cap is backed by an actual run, not just prefix arithmetic.

[Pinned evidence](docs/evidence/native-packed-history-513-development.json) binds base commit,
exact replacement-source SHA-256s, binary SHA-256, lockfiles and all five phase reports. Runtime
binary SHA-256 is `8b52a7403d4baf7dd992b1e2c4792e304435eda5c5698a15cb8b4a238055a202`.
Checkpoint v1 digest `a1f9fa3f9ce93320dc63866673150f257602276c4b2980f4067b279b9abdd2e8`;
terminal `f192f7c3a8f5efb74972869875736fe67ed0068c6abc11d71cb00c7f596d0b3c` agrees across
resume, explicit checkpoint replay, repeated resume and open.

Session 62048 passed four model cases/2.48 s and six CLI cases/50.58 s, with the new scale case
explicitly ignored in that normal invocation; compile 25.63/25.46 s, strict Clippy 1.58 s.
First explicit scale session 10694 failed a test-only byte-count assertion after constructing
checkpoint 199 and killing the owned child at 200: expected 832,200 bytes instead of 836,361,
omitting the encrypted certificate-header frame. Fixed the fixture assertion; no guard relaxed.
Its database remains at `experiments/t20-bench/target/packed-history-cli-1102869-1789882494172776422`
(926 MiB apparent size), with original failure/binary details in the evidence. Wall 44.59 s,
peak RSS 277,624 KiB/zero swaps. No unchanged failing workload was relaunched.

Corrected scale session 12260 / `run-p1106364-i21948294.scope` exited 0: one explicit test/90.02 s,
compile 25.33 s, total wall 115.37 s, peak RSS including compilation 430,908 KiB, zero swaps.
Successful retained root `experiments/t20-bench/target/packed-history-cli-1107106-1789882647354994945`
is 940 MiB apparent size. Phase elapsed/peak RSS KiB: create 38,576 ms/278,384; resume
14,343/347,264; explicit checkpoint replay 8,943/346,076; repeated resume 13,987/346,544;
open 8,685/346,624. These durations include historical verification, not recovery-only latency.
Preflight 20–21 GiB available RAM/2.0 GiB free swap/~1000 GiB free disk; other host workloads
were present and untouched. Sampled corrected scope peak 905,940,992 bytes/zero swap is not its
final lifetime peak. Both source stores were retained, not removed or rewritten into a migration.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
/usr/bin/time -v timeout --signal=TERM --kill-after=10s 5400s env CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --test packed_history --locked --offline packed_history_native_513_record_intermediate_tail_sigkill_resumes -- --ignored --nocapture --test-threads=1 2>&1 | tee /tmp/uste-d189-native-513-final.log'
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d189-full-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Full session 91910 / `run-p1108784-i22088176.scope` exited 0: **110 active tests**, two pre-existing
ignored campaigns plus the separately passed opt-in scale case, seven executables plus empty
doctests. Library 85/133.91 s, legacy BM-01 process 3/11.97 s, packed history 6/49.58 s,
packed BM-01 process 5/36.46 s, manifest 3/0.92 s, legacy history process 8/74.31 s. Compile
30.83 s, strict Clippy 1.50 s; sampled scope peak 400,658,432 bytes/zero swap, not final lifetime
peak. Format/diff/docs/task checks pass; full core remains 687 at `c3b7047`, root lock unchanged.
Next define bounded native construction steps and verify writer/session resource constraints
before further scale increases, then complete accounting and reserved-host campaigns. Never
clear/bypass nonce tracking or claim writer-incarnation rotation from an ordinary reopen.
T-20/T-19, pinned M1, the full roadmap and release gates remain unchanged. Supersedes older next steps.

## Prior verified increment — native coordinator buffering (Decision 0188)

Built and tested on pushed `c3b7047`: the packed development engine selects fresh buffered
primary and quota admission with sequential 64 MiB phase budgets. Native BM-01/BM-06 reports
declare the exact budget/scope; the sampler supervisor refuses missing/null/wrong declarations
and unbuffered setup. Query caches remain independent, proof limits and all workload caps unchanged.
Archived measurements retain their original binary attribution; no new timing claim is made.

Session 21696 / `run-p1079242-i22086805.scope` exited 0: **109 active tests**, two existing ignored
campaigns, seven executables plus empty doctests. Library 85/134.80 s, legacy BM-01 process 3/12.09 s,
packed BM-06 CLI 5/50.47 s, packed BM-01 CLI 5/36.73 s, manifest 3/0.95 s, legacy BM-06 process
8/73.72 s. Compile 1m 04s; strict standalone Clippy 3.37 s. No failures.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d188-native-integration.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Preflight 22 GiB available RAM/2.0 GiB free swap; sampled scope peak 644,067,328 bytes/zero swap,
not final lifetime peak. Formatting/diff/docs/task checks pass. Full core is **687 tests at
`c3b7047`**; root lockfile unchanged. Next enable and verify a bounded native multi-batch BM-06
development profile and intermediate-tail process interruption, with fresh resource admission;
do not infer qualification from arithmetic or the existing two-record runs. Complete accounting,
safe larger construction and reserved-host campaigns remain required. T-20/T-19, pinned M1,
the preserved full roadmap and external release gates are unchanged. Supersedes older next steps.

## Prior verified increment — buffered quota admission (Decision 0187)

Built and tested on pushed baseline `b51a037`: opt-in quota admission now uses fresh sequential
canonical-family caches followed by a separate correspondence cache for metadata/principals,
owner traversal and primary-owner lookups. Uncached APIs, exact proof charges, checked totals,
owner pairing and late-corruption refusal remain intact. Reports are fixed per-phase diagnostics,
not concurrent memory sums or complete I/O measurements. Native integration is not included yet.

Initial session 78800 failed to compile a new test's call to the private `paired` helper. Corrected
the test to compare public anchors and commitments; the existing wrong-pair refusal cases remain.
Session 71977 / `run-p1061304-i22061752.scope` passed all nine focused quota-admission cases/17.38 s,
compile 26.09 s and strict workspace Clippy/8.01 s. Sampled peak 626,294,784 bytes/zero swap.
Focused command selected `cargo test -p uste-txn --all-features --locked --offline packed_quota_admission
-- --test-threads=1` with the optimized/asserting profile and memory scope below.

Full session 16264 / `run-p1062326-i22019197.scope` exited 0: **687 tests across 47 executables**,
zero failures/ignored. Graph disk-index 98/439.85 s, replay 50/107.30 s, storage library 219/32.71 s,
transaction coordinator 108/101.19 s. Compile 1m 03s; strict Clippy 0.05 s and rustdoc 10.64 s.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d187-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked --offline'
```

Preflight 21 GiB available RAM/2.2 GiB free swap. Sampled full scope peak 2,039,599,104 bytes/
zero swap, not final lifetime peak. Format/diff/docs/task checks pass. Root lockfile unchanged.
This full core run includes Decision 0186 primary buffering; pending standalone integration is
excluded and not verified by it. Last full standalone harness remains 109 at `98bcd86`. Next
verify/commit that integration, then perform resource-safe larger native construction and
accounting work. No larger-than-memory or qualifying campaign pass is claimed. T-20/T-19,
pinned M1, the full roadmap and external release gates remain unchanged. Supersedes older next steps.

## Prior verified increment — buffered primary coordinator admission (Decision 0186)

Implemented on pushed `98bcd86`: opt-in primary admission creates fresh sequential canonical
family caches, then a fresh correspondence cache for streamed retry/transaction/owner/witness
checks. Uncached APIs and decoding remain shared; logical proof charges and exact limits do not
change on hits. Fixed per-phase reports expose bounded residency without implying simultaneous
allocation, complete I/O or consumer authority. Caller-warmed caches are never accepted.

Session 21965 / `run-p1049456-i22010254.scope` exited 0: 11 focused primary-admission cases/16.22 s,
compile 27.28 s, strict workspace Clippy 8.06 s. Five original cases also run buffered, retaining
the original uncached 798-fault assertion and sweeping every actually observed buffered read
fault. A sixth new case checks one-page/larger budgets, identical proof work, fewer adapter reads,
fresh repeated admission, invalid budgets before I/O and late ciphertext mutation. No failures.

Session 50827 / `run-p1050587-i22049627.scope` exited 0: all **126 transaction-crate tests**
(10 library, 13 authorization, 103 coordinator; coordinator 102.84 s), compile 0.04 s and strict
workspace rustdoc 10.41 s. Commands:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-txn --all-features --locked --offline packed_admission -- --test-threads=1 2>&1 | tee /tmp/uste-d186-primary-admission.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings'
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-txn --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d186-txn-verification.log &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked --offline'
```

Preflight 22 GiB available RAM/2.5 GiB free swap. Sampled transaction scope peak 102,121,472 bytes/
zero swap (not final lifetime peak; first compile scope peak unavailable). Formatting, diff,
documentation and task checks pass; root lockfile unchanged. This is focused/full-crate coverage,
not a new full-workspace run: full core remains 676 at `0322067`; full harness 109 at `98bcd86`.
Next apply the same fresh bounded buffering to quota admission, then verify the full core and
integrate both into the native harness. Safe larger construction, complete accounting and
reserved-host qualification still precede T-20/T-19 completion. Pinned M1, the full roadmap and
external release gates are unchanged. This supersedes older next-step text.

## Prior verified increment — multi-batch continuation (Decision 0185)

Implemented on pushed `b7e664e`: native construction resume still ends at the checkpoint,
but an already-started final generation now completes the full frontier. A shared streaming
helper validates complete-generation targets/budgets, verifies existing history before appending,
and preserves exact identities, authorization and per-batch publication/rebase. The 513-record
model exercises partial-generation continuation and interruption before metadata rebase, cold
suffix recovery, exact retries, all 1,026 payload versions and the independent reference digest.
Both full-history caps remain two. Literal qualifying frontier arithmetic is not materialization.

Initial session 23823 passed three model cases but failed continuation. Diagnostic session 42466
identified a test-only one-shot clock assumption: authorized retries also sample the clock.
Fixed to four observations per four attempts, without changing authorization/retention. One
compile under `run-p1039895-i22017848.scope` caught a misspelled test ClockObservation field;
corrected to `monotonic_ticks`. Session 73265 / `run-p1040189-i21994528.scope` passed four model
cases/2.70 s, continuation arithmetic/0.00 s and strict Clippy/1.57 s (compile 25.55 s).
Commands used `--lib packed_bm06` and `--lib bm06_continuation` with the settings below.

Full session 72489 / `run-p1040776-i21994548.scope` exited 0: **109 active tests**, two existing
ignored campaigns, seven executables plus empty doctests. Library 85/139.85 s, legacy BM-01
process 3/12.24 s, packed BM-06 CLI 5/53.27 s, packed BM-01 CLI 5/37.06 s, manifest 3/1.09 s,
legacy BM-06 process 8/75.01 s. Compile 30.32 s; final strict Clippy 3.32 s.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d185-full-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Preflight 20 GiB available RAM/2.8 GiB free swap; sampled full scope peak 384,872,448 bytes/
zero swap, not final lifetime peak. Full test executables were built before pending coordinator
buffering core edits; those edits are excluded from this increment and not covered by this run.
Focused strict Clippy also passed before those edits; the final Clippy included the pending API.
Format/diff/docs/task checks pass. Full core remains 676 at `0322067`. Next implement and verify
fresh bounded coordinator-admission buffering, then safe larger native construction/accounting
before reserved-host campaigns. T-20/T-19, pinned M1, full roadmap and release gates remain open
or complete exactly as recorded in TASKS; no qualification claim. This supersedes older next steps.

## Prior verified increment — explicit native checkpoint recovery (Decision 0184)

Implemented on pushed `bcdb8b7`: `bm06-packed-linux-recover-checkpoint` pins the profile
checkpoint and checks the full generation's suffix group count, even when newer complete roots
exist. Ordinary recovery retains latest-root/no-op behavior. Regression tests verify exact retry,
history/digest equivalence, source preservation, pre-I/O admission and corrupt old manifests
refusing explicit recovery while valid terminal roots still open. Both record caps remain two.

Session 51117 / `run-p1036259-i22017659.scope` exited 0: seven native history tests/44.30 s,
five separate-process history tests/53.44 s, three manifest tests/1.09 s; native compile 24.56 s,
CLI compile 25.53 s, strict Clippy 1.50 s. Commands:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --lib linux_runner::packed::history --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d184-native.log &&
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --test packed_history --test recovery_manifest --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d184-cli.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Preflight 21 GiB available RAM/2.8 GiB free swap, no competing USTE workload. Sampled scope
peak 362,213,376 bytes/zero swap (not final lifetime peak). Format/diff/docs/task checks pass.
Full core remains 676 at `0322067`; last full harness 106 active at `15ab0bb`. No new full-suite,
larger-than-memory or qualifying campaign claim. Next fix and exercise interrupted multi-batch
tail continuation before increasing native scale; complete accounting, safe construction and
reserved-host campaigns remain required. T-20/T-19, pinned M1, full roadmap and release gates
are unchanged. This supersedes older next-step text.

## Prior verified increment — generation-wide packed tails (Decision 0183)

Implemented on pushed `15ab0bb`: model/native packed tail construction streams every batch of
the selected update generation. Intermediate batches retain normal authorized publication/rebase;
only the final batch deliberately leaves acknowledged publication pending. Generation/base/frontier
and batch sequence checks precede their writes. Native recovery exact-retries every tail batch.
Both full-history CLI caps remain two. The 513-record model proves two-group recovery from an
explicit earlier checkpoint despite a newer intermediate root, all 1,026 payload versions, exact
terminal retry, zero retry overlays and the cold reference v1 digest.

Initial session 83225 passed three cases but failed the new two-batch case. Diagnostic session
73025 identified `Authorization(Transaction(RetryableUnavailable))`: the test supplied a one-shot
scripted clock to two fresh transactions. Fixed the fixture to supply exactly two observations;
the final test checks they were consumed. No authorization or publication limit was relaxed.
Session 67417 then passed all four model cases/2.50 s and Clippy/1.62 s (compile 25.94 s), sampled
peak 366,714,880 bytes/zero swap. Added an explicit frontier-budget refusal before final verification.
Final session 41941 / `run-p1022141-i21982224.scope` exited 0: four model cases/2.55 s,
six native history cases/37.37 s, five separate-process history cases/53.13 s and three manifest
cases/1.11 s. Compilation 25.81 s model and 26.02 s CLI; strict Clippy 0.65 s.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --lib packed_bm06 --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d183-model-final.log &&
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --lib linux_runner::packed::history --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d183-native-final.log &&
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --test packed_history --test recovery_manifest --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d183-cli-final.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Preflight 19 GiB available RAM/2.7 GiB free swap; sampled final scope peak 520,888,320 bytes/
zero swap. One job/thread/workload; format/diff/docs/task checks pass. Full core remains 676 tests
at `0322067`, last full harness 106 active at `15ab0bb`; this increment has focused verification,
not a claimed new full-suite run. Next add explicit native checkpoint selection and interrupted
multi-batch-tail continuation before increasing native scale. Native latest-root recovery does
not yet guarantee replay of the entire qualifying tail. Complete accounting, safe construction
and reserved-host campaigns remain open; M1, TASKS, T-19/full roadmap and release gates unchanged.
This supersedes older next-step text.

## Prior verified increment — BM-06 partial-generation prefixes (Decision 0182)

Implemented on pushed `5adf5a9`: exact prefix counts and streaming historical verification no
longer equate one revision with one generation. Native history verification/continuation uses
the exact frontier; full-generation callers retain their previous contract. Both CLI record
caps remain two. A bounded 513-record model test executes three real data batches (512 creates,
one create, 512 updates), cold-admits/retries each prefix, verifies every committed payload and
matches the full reference v1 digest. Independent event scans check all prefixes across seven
small batch-boundary profiles; qualifying-size literal arithmetic is not materialization evidence.

Initial focused compile under `run-p986300-i21984133.scope` failed due to a missing test-only
CheckpointState trait import; fixed. Session 62995 / `run-p987326-i21941247.scope` then passed
the partial-generation case/0.59 s and independent counts/0.00 s (compile 25.32 s); sampled peak
374,050,816 bytes/zero swap. Focused commands selected `--lib bm06_partial_generations` and
`--lib bm06_prefix_counts` with the same release/offline/locked/serial settings below.
After wiring all native phase verification, full session 55693 /
`run-p989138-i21980454.scope` exited 0: **106 active tests**, two existing ignored campaigns,
seven executables plus empty doctests. Library 82/128.21 s, legacy BM-01 process 3/12.12 s,
packed BM-06 CLI 5/52.46 s, packed BM-01 CLI 5/36.96 s, BM-06 manifest 3/1.09 s,
legacy BM-06 process 8/76.93 s. Compile 55.10 s, strict Clippy 1.57 s.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d182-prefix-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Preflight 24 GiB available RAM/2.7 GiB free swap; sampled full-scope peak 603,316,224 bytes/
zero swap. One job/thread and one heavy workload. Format/diff/docs/task checks pass. Core remains
676 tests at `0322067`; the Decision 0181 measured binary remains pinned to `edc0845`, not this
new build. Next implement/test generation-wide packed tail materialization and bounded recovery
from its selected checkpoint, preserving exact retries before raising any native cap. Complete
accounting, safe larger construction and qualifying campaigns remain T-20 prerequisites.
M1, TASKS, T-19/full roadmap and release gates are unchanged. This supersedes older next steps.

## Prior verified increment — buffered native comparison (Decision 0181)

Measured pushed `edc0845`, binary SHA-256
`0a5b2fa572647ac0c1bb2ad8ada7814daf3a8a05d85383878c0c4d28f36c6579`, against the retained
Decision 0177 fixture; all original artifacts preserved. Reports are archived in
`docs/evidence/native-packed-buffered-1000-development.json`. Open/query/sample exited 0:
wall 2.53/72.63/155.83 s, peak RSS 265,272/265,380/265,888 KiB, zero process swaps.
Query cold setup is 2,389 ms versus 51,261 ms; last-owner decrypts 6,381 versus 2,378,825,
authenticated encoded bytes 153,801,773 versus 48,895,663,753. This is not physical I/O.
Query-only execution is 70,228 versus 69,953 ms; paired execution 135,278 versus 134,026 ms.
No query throughput improvement is claimed. All 384 outcomes, 96 warm-ups and 768 timed
executions pass, with identical state/query/paired digests and query cache/decrypt work.
Certificate SHA-256 before/after remains
`820626386caa307fae632e8270238c96db43333704ccfba48a5ed39576827c47`.

Commands used the Decision 0177 scope/time/1800-second timeout template and the same root,
credential, entity count and oracle files. Exact substitutions:
`buffered-open` / `linux-packed-open` / no oracle argument;
`buffered-query` / `linux-packed-query` / absolute artifact `oracle-summary`;
`buffered-sample` / `linux-packed-sample` / absolute artifact `oracle-bundle`.
Each output uses that phase's `.json` and `.time` file in
`experiments/t20-bench/target/native-packed1000.QGIZQZ`. The unchanged prebuilt binary was
invoked directly; no compilation is included. Open session 94771/scope
`run-p978543-i21638521.scope`; query 2681/`run-p978826-i21957020.scope`; sample
2749/`run-p981536-i21957137.scope`. Preflight 25 GiB available RAM/2.8 GiB free swap;
sampled sampling-scope peak 277,987,328 bytes/zero swap; other scope peaks not captured.
One workload at a time under 3G/4G/512M limits. Original and new report fields were compared
exactly, separately respecting each digest domain. Host caches remain uncontrolled and query
evictions zero. This does not qualify cache pressure, larger-than-memory or benchmark targets.

Core remains 676 tests at `0322067`, harness 104 active at `edc0845`. New BM-06 prefix arithmetic
and partial-history verification edits are uncompiled/unverified and excluded from this evidence
commit and measurement binary. Next verify that bounded 513-record partial-generation work,
then continue multi-batch tails, coordinator accounting and qualification prerequisites. M1,
TASKS, the full roadmap and release gates remain unchanged. This supersedes older next steps.

## Prior verified increment — native buffered graph admission (Decision 0180)

Implemented on pushed `0322067`: packed model/native BM-01 and BM-06 select fresh 64 MiB logical
graph-admission caches, sequential canonical families then semantic validation. No retained
admission pages warm consumer queries. Reports disclose budget/scope and unbuffered coordinator
admission; supervisor rejects missing/mismatched fields or a false coordinator-buffering claim.
Fixture, limits, digest domains, deadlines and qualification thresholds remain unchanged.

Session 4514 / `run-p962353-i21971835.scope` exited 0: 104 active tests across seven executables
plus empty doctests; two existing ignored campaigns. Library 80/128.14 s, legacy BM-01 process
3/11.99 s, packed BM-06 CLI 5/51.99 s, packed BM-01 CLI 5/36.81 s, BM-06 manifest 3/1.08 s,
legacy BM-06 process 8/76.10 s. Compile 1m06s, strict Clippy 3.19 s. Tests preserve exact oracle
outcomes and digests, tail/prefix semantics, source certificates, real process-loss recovery and
unchanged query cache/decrypt accounting. Exact command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d180-native-buffering-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Preflight 24 GiB available RAM/3.7 GiB free swap; sampled scope peak 636,878,848 bytes/zero swap.
One job/thread and one heavy workload retained. Root/standalone format, diff, docs and task checks
pass. Full core baseline remains 676 tests at `0322067`; no new full-core run is claimed here.
Next remeasure the retained native 1,000-entity fixture with this exact committed binary, preserving
Decision 0177's baseline artifacts. Then continue coordinator accounting and BM-06 multi-batch
prefix/scale prerequisites. No T-20/T-19, larger-than-memory or benchmark qualification claim;
M1 pinned handoff and full roadmap/release gates remain unchanged. This supersedes older next steps.

## Prior verified increment — buffered cold graph admission (Decision 0179)

Implemented on pushed `ec6ba8e`: namespace/target-bound maintenance exposes fresh canonical
buffering; opt-in graph cold admission uses sequential fresh canonical caches and then a fresh
semantic cache. No caller-supplied warm cache or graph-wide map. Complete canonical, history,
relationship, policy, count and v1 digest validation remains shared with the uncached path.
Separate fixed-size cache reports preserve identical logical proof counters and ceilings.
Seven additional tests cover all existing cold-admission cases, 24 limit refusals, eight
authenticated false graphs, later corruption, all actual read faults/restart, one-page/larger
budgets and repeated fresh admissions. No writes occur during admission.

Check session 96238 passed/2.69 s. Focused session 41919 failed compilation due to two unnecessary
test type qualifications; corrected. Session 88642 then passed seven tests/2.07 s, compile
21.03 s and workspace strict Clippy/8.50 s. No focused scope peak was captured.
Final full session 32768 / `run-p950395-i21971194.scope` exited 0: **676 tests across 47
executables, zero failed/ignored**. Graph disk-index 98/441.47 s, replay 50/107.69 s,
storage library 219/32.96 s, transaction integration 97/99.48 s. Compile 45.71 s, strict Clippy
2.23 s and warning-denied rustdoc 10.50 s. Exact command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d179-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked --offline'
```

Preflight 26 GiB available RAM/3.7 GiB free swap; sampled scope peak 2,592,628,736 bytes/zero swap.
One job/thread, one heavy workload retained. Format/diff/docs/task checks pass; Cargo.lock SHA-256
remains `7ed2b533b3c801250a89e008b4ca26c48f16400e38224d8a63447c0393aaa97b`.
Decision 0180's native harness wiring is prepared but not yet verified; excluded from this core
commit. Next verify and commit that wiring, then remeasure the retained 1,000-entity fixture at
an exact version. Coordinator buffering/accounting, scale construction and qualifying campaigns
remain T-20 work. T-19/full roadmap, M1 pinned handoff and external release gates are unchanged.
This supersedes older next-step text.

## Prior verified increment — fresh buffered canonical admission (Decision 0178)

Implemented on pushed `b3df3b8`: opt-in storage canonical admission creates, binds and destroys
its own bounded cache. All canonical/value-hash validation and logical proof ceilings remain;
cache hits do not buy additional proof budget. No prewarmed cache can be supplied. Five new
tests cover exact results/counters, every proof ceiling, one-page/larger budgets, empty/missing/
foreign/stale/locked cases, authenticated noncanonical branches, later corruption and every
actual read fault with fresh-owner recovery. Existing uncached behavior remains unchanged.

Focused session 43102 passed four initial tests/0.08 s after 26.03 s compile; sampled scope peak
1,113,026,560 bytes/zero swap. Session 42028 failed compilation in the added false-tree fixture
(incorrect physical context field); corrected by deriving the complete physical context from
a staged root. Final session 46483 / `run-p936965-i21927139.scope` exited 0: all 238 storage
tests across eight executables (including one zero-test example), 219 library tests/33.02 s,
compile 0.95 s. Workspace strict Clippy passed/7.74 s. Exact command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-storage --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d178-storage-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings'
```

Preflight 24 GiB available RAM/3.5 GiB free swap; final scope sampled peak 365,899,776 bytes/
zero swap. Format/diff/docs/task checks pass. No full-workspace test rerun is claimed for this
storage-only slice; previous full core baseline is 664 tests at `e2ca489`, standalone 104 active
at `9b9fccc`. Next integrate bounded buffering into graph cold admission and prove unchanged
semantic/source binding, limits and fault behavior before new native measurements. Coordinator
correspondence buffering and qualification prerequisites remain open. M1, TASKS and gates are
unchanged. This supersedes older next-step text.

## Prior verified increment — native packed 1,000-entity measurement (Decision 0177)

Pushed implementation `9b9fccc`, release binary SHA-256
`8edc933d0b4be898a998da4ccbe4f8ac13ec3892c1972bfbfd351e38afe8934e`:
native create, query and supervised sampling all exited 0. Full machine-readable reports are in
`docs/evidence/native-packed-1000-development.json`; retained artifacts are under
`experiments/t20-bench/target/native-packed1000.QGIZQZ`. Create wall 62.23 s / peak RSS 319,612 KiB;
query 121.23 s / 266,168 KiB; sampling 203.67 s / 265,944 KiB; all zero process swaps.
Frontier 4, 1,000 entities/10,000 relationships, independent v1 digest matches Decision 0166.
All 384 query outcomes, 96 warm-ups and 768 paired timed executions succeeded. Certificate
SHA-256 before/after is unchanged. Query cache evictions are zero: this is not a pressure test.
Cold-open last-owner accounting reports 48,895,663,753 authenticated encoded bytes, not physical
device traffic; complete authenticated accounting remains false. No BM-01/BM-06 pass is claimed.

Exact command template (each workload separately under the scope, sequentially):

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
/usr/bin/time -v -o experiments/t20-bench/target/native-packed1000.QGIZQZ/PHASE.time \
timeout --signal=TERM --kill-after=10s 1800 experiments/t20-bench/target/release/uste-t20-bench COMMAND \
--root /var/home/aaronnhorvitz/dev/01_repos/USTE/experiments/t20-bench/target/native-packed1000.QGIZQZ \
--password-file /var/home/aaronnhorvitz/dev/01_repos/USTE/experiments/t20-bench/target/native-pressure20000.ya99oO/password \
--entities 1000 ORACLE_ARGUMENTS > experiments/t20-bench/target/native-packed1000.QGIZQZ/PHASE.json'
```

Substitutions: `create`/`linux-packed-create`/no oracle argument;
`query`/`linux-packed-query`/`--oracle-file` absolute artifact path plus `/oracle-summary`;
`sample`/`linux-packed-sample`/`--oracle-file` absolute artifact path plus `/oracle-bundle`.
Both oracle files were generated by the same binary's `oracle-summary --entities 1000` and
`oracle-bundle --entities 1000` commands before construction. Existing synthetic credential was
reused read-only. Create session 76977/scope `run-p907197-i21867829.scope`; query session
6257/scope `run-p912084-i21828798.scope`. Sampling's session identifier was lost in context
recovery; its completed time file records exit 0 and full report validates the supervised run.
Preflight 27–28 GiB available RAM/3.7 GiB free swap; no scope peak sampled for these runs.
One heavy workload at a time; unrelated workloads untouched. Full core/harness tests remain
the 664/104-active-test baselines from Decisions 0175–0176. M1 and task statuses unchanged.
Next implement fresh bounded cold-admission caching without weakening proof limits or allowing
warm caches to hide later corruption, then repeat pinned development measurements. T-20 remains
open; larger-scale construction/accounting and qualifying campaigns still precede T-19.
This supersedes all older next-step text below.

## Prior verified increment — packed BM-01 vault-work reporting (Decision 0176)

Implemented on pushed `e2ca489` plus this increment. The trusted native operator reports checked
per-owner decrypt deltas for warm-up and empty/retained samples, alongside cache and adapter work.
Terminal setup totals explicitly cover only the last cold-open owner; query-only work is separate.
Consumers receive no vault-wide report API. Supervisor validation requires the counter shape and
partial-accounting disclosures, rejecting fabricated physical-I/O/key-unwrap/other-owner claims.
The unchanged oracle digest, query plan, cache size, deadlines, admission caps and thresholds remain.

Full session 20025 / `run-p888943-i21877630.scope` exited 0: 104 active tests across seven
executables, two existing ignored campaigns, zero failures. Library 80/153.70 s; legacy BM-01
process 3/12.31 s; packed BM-06 CLI 5/53.92 s; packed BM-01 CLI 5/59.44 s; BM-06 manifest
3/1.15 s; legacy BM-06 process 8/76.39 s. Compile 1m05s, strict Clippy 4.13 s. CLI tests prove
small-fixture cache misses equal successful decrypts with exactly 20,545 encoded/16,384 plaintext
bytes per packed page, retained queries add zero decrypts, setup/warm-up work is positive, and
source certificates plus independent result digests remain unchanged. Every counter's rollback/
overflow and parent report-field refusals pass.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d176-vault-reporting-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Preflight 28 GiB available RAM/3.7 GiB free swap; sampled scope peak 645,009,408 bytes/zero swap.
One job/thread and 3G/4G/512M limits retained. Format/diff/docs/task checks pass. Core remains
the 664-test baseline at `e2ca489`; M1/handoff, full roadmap, lockfile and release gates unchanged.
Next run resource-capped native packed 1,000-entity development measurements against the independent
oracle (fresh empty artifact root `experiments/t20-bench/target/native-packed1000.QGIZQZ` is prepared),
then continue complete accounting and scale prerequisites. No qualifying campaign, complete I/O,
larger-than-memory or T-20/T-19 completion is claimed. This supersedes older next-step text.

## Prior verified increment — trusted per-vault authentication work (Decision 0175)

Pushed baseline is `394512a` (supervised packed sampling). Decision 0175 adds fixed-size checked
vault decrypt diagnostics on raw trusted maintenance handles only. Successful/failed calls,
encoded authenticated bytes and returned plaintext bytes remain separate from physical I/O and
semantic admission. Diagnostic overflow/poison cannot change decrypt outcomes or reset nonces.
Decision 0176 benchmark wiring is separate pending work, excluded from the core verification.

Initial crypto session 32168 failed compilation because an existing test-only vault literal needed
the new field; repaired. Session 95370 passed 17 then-current crypto tests but its graph target
selected zero cases, so that was not counted as graph verification. Corrected session 71201 passed
all 18 crypto tests (six unit/12 integration; 2.26 s integration) and the actual disk-index graph
measurement test/0.13 s. Session 1562 / `run-p846180-i21849062.scope` passed 24 authorized packed
tests/270.65 s, compile 13.29 s and workspace strict Clippy/11.89 s. Sampled peak 397,250,560 bytes/
zero swap. Exact focused commands used one job/thread and test opt-level 1 with debug assertions
and overflow checks retained:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-crypto --locked --offline -- --test-threads=1 &&
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-graph --locked --offline authorized_packed_vault_work -- --test-threads=1'
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-graph --test disk_index --locked --offline authorized_packed -- --test-threads=1 &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked --offline -- -D warnings'
```

Full core session 44463 / `run-p857704-i21794771.scope` was deliberately interrupted with SIGINT
(exit 130) during graph tests after review found that namespace ManageSchema cannot authorize
vault-wide historical totals. Removed the uncommitted consumer report method; benchmark operators
use their existing raw coordinator. Tests now prove denied/revoked reads incur no decrypt work.
This was a design correction, not memory pressure or a passing full gate. Preflight
29 GiB available RAM/3.7 GiB free swap; compile 1m47s, sampled peak 2,708,295,680 bytes/zero swap.
The final uncertainty-report assertion is included in this full build. Command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d175-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked --offline'
```

Corrected session 47444 / `run-p863452-i21839197.scope` passed both actual vault-work/denied-read
tests in 0.24 s after 14.11 s compilation. Consumer API has no vault report method. Final full
session 49479 / `run-p864363-i21876844.scope` exited 0 using the same full command with log
`/tmp/uste-d175-workspace-final.log`: 664 tests/47 executables, zero failures/ignored tests.
Graph disk/index 91/436.36 s, checkpoint/replay 50/106.74 s, transaction coordinator 97/103.94 s.
Compile 4.18 s; all-features Clippy 5.11 s; warning-denying documentation 13.50 s. Preflight
28 GiB available RAM/3.7 GiB free swap; sampled scope peak 1,744,416,768 bytes/zero swap.
All resource safeguards retained. Format/diff/docs/task checks pass; lockfile unchanged.

The separate Decision 0176 benchmark counter wiring is unverified and excluded from this increment.
Next verify that wiring, then continue complete accounting and resource-safe
scale work toward T-20. No qualification or task completion is claimed; all earlier roadmap and
external gates remain. Core changes alter no consumer interface, persisted format or M1 handoff.

## Prior verified increment — supervised packed BM-01 sampling (Decision 0174)

Implemented on pushed `c372c6a` plus this increment: native packed sampling cold-admits the
complete triple and runs the unchanged bounded oracle/warm-up/paired-query plan with a 64 MiB
authorized cache. The existing owned-worker protocol enforces the fixed 30-second deadline;
parent finalization binds packed schema, fixture frontier, counts and nonqualification disclosures.
The unchanged 20,000-entity pre-I/O cap still refuses qualifying dimensions. Actual cache and
adapter counters are reported separately; authenticated work is explicitly unmeasured.

Initial session 93668 failed compilation on two unqualified names in the new supervisor test;
qualified them without changing behavior. Focused session 35164 /
`run-p833248-i21837283.scope` then passed 21 packed library tests/104.98 s, five packed CLI
tests/58.52 s, compile 49.06 s and strict Clippy/1.40 s. The new CLI case independently verifies
the paired digest, 96 warm-ups, 768 measured executions, 32 latency groups, retained cache hits/
zero retained reads on the small fixture and unchanged source certificates. Sampled scope peak
365,637,632 bytes/zero swap; preflight 30 GiB available RAM/3.7 GiB free swap.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --lib --test packed_terminal --locked --offline packed -- --test-threads=1 &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Full session 5852 / `run-p835216-i21785549.scope` exited 0: 103 active tests across seven test
executables, two existing ignored campaigns, zero failures. Library 79/150.57 s; legacy BM-01
process 3/11.90 s; packed BM-06 CLI 5/53.05 s; packed BM-01 CLI 5/58.53 s; BM-06 manifest
3/1.11 s; legacy BM-06 process 8/74.26 s. Compile 4.96 s, Clippy 0.04 s. Preflight 29 GiB
available RAM/3.7 GiB free swap; sampled scope peak 525,119,488 bytes/zero swap. One job/thread,
one heavy workload and 3G/4G/512M limits retained. Full command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d174-sampling-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Format/diff/docs/task checks pass. No core source is included in this increment; the
unreferenced next-increment vault measurement module is excluded. Core retains its 658-test gate
at `6bb43a4`. Next implement privileged authentication-work diagnostics, then resource-safe scale
prerequisites before qualifying campaigns. T-20/T-19 remain unchecked; pinned M1, full roadmap,
lockfile and release gates are unchanged. This section supersedes older next-step text; neither
sampling plumbing nor capped correctness tests are performance/larger-than-memory qualification.

## Prior verified increment — packed BM-01 partial-origin rebuild (Decision 0173)

Implemented on pushed `885ca5a` plus this increment: explicit BM-01 rebuild restores the actual
authenticated prefix, cold-admits exact prefix counts and reports `complete_fixture`. It does not
append batches. Data-bearing prefixes require the evidence marker; policy-only prefixes require the
exact canonical policy and profile-bound first transaction through a fully finished bounded cursor.
This binding is read-only, not an expiry-dependent retry. Empty and legacy unbound policy prefixes
still refuse. Open/query still require complete materialization; resume retains its exact-retry rules.

Tests remove every packed manifest at frontiers two and three, refuse ordinary resume/wrong-profile
rebuild, prove repeated reconstruction preserves source bytes and prefix digest, then resume to the
reference terminal state. The five-boundary BM-01 SIGKILL test now additionally checks empty-store
refusal and policy-only reconstruction with zero source append. Session 59056 /
`run-p811573-i21796517.scope` exited 0: 20 packed library cases/105.81 s, four packed CLI cases/
52.61 s, compile 47.75 s, Clippy 1.40 s; zero failures. Preflight 30 GiB available RAM/3.6 GiB
free swap; sampled scope peak 367,181,824 bytes/zero swap. One job/thread and 3G/4G/512M caps.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --lib --test packed_terminal --locked --offline packed -- --test-threads=1 &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Format/diff/docs/task checks pass. Other library tests retain the 77-active/two-ignored gate at
`885ca5a`; other process suites retain the 96-active full gate at `33d7588`. Core source remains
unchanged with 658 tests at `6bb43a4`. Pending packed sampler work is excluded. Next implement
supervised packed BM-01 sampling with the frozen plan/oracle, then complete I/O accounting and
resource-safe scale prerequisites before qualifying campaigns. T-20/T-19 remain unchecked; pinned
M1, full roadmap, lockfile and release gates remain unchanged. This section supersedes earlier
next steps; no performance or larger-than-memory qualification follows from these tests.

## Prior verified increment — packed BM-06 prefix/process recovery (Decision 0172)

Implemented on pushed `33d7588` plus this increment: native packed BM-06 resume reconstructs only
zero/one-revision bootstrap (one outcome/zero owners/1 MiB replay), rejects foreign principal/key/
request and otherwise uses admitted packed triples with authenticated suffix streaming. It verifies
existing history before fresh writes and exact-retries every original batch. Incomplete construction
finishes at checkpoint 100; an already certified 101 stays 101. Explicit rebuild now reconstructs
only the actual prefix, including policy-only and incomplete data-bearing prefixes, without appending
events. Complete cache loss remains a resume refusal until explicit reconstruction.

Construction/tail probes park after durable creation, policy acknowledgement, graph publication
before metadata rebase, or the final certified-but-unpublished tail. Tests kill/reap only owned
children. Focused session 22299 / `run-p805286-i21799970.scope` passed five CLI tests/51.54 s,
compile 24.33 s and Clippy/0.13 s. Eight actual SIGKILL cases cover construction 0/1/2/50/99/100,
partial cache-loss rebuild at 50 and final tail 101. Wrong certified dimensions, three-byte tail
repair, exact frontier/history/digest preservation and no duplicate commits pass.

Session 41548 / `run-p806174-i21765885.scope` exited 0: 77 active library tests/136.06 s with
two pre-existing ignored campaigns, plus five CLI cases/52.35 s, including policy-only explicit
reconstruction and foreign bootstrap refusals. Compile 25.54 s, Clippy 0.67 s; zero failures.
Preflight 30 GiB available RAM/3.6 GiB free swap; sampled scope peak 373,899,264 bytes/zero swap.
One job/thread and 3G/4G/512M caps retained; format/diff/docs/task graph checks pass.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --test packed_history --locked --offline -- --test-threads=1 &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --lib --test packed_history --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d172-history-resume-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Other process suites retain their 96-test full gate at `33d7588`, not a new run. Core source remains
unchanged with 658 tests at `6bb43a4`; pending BM-01 partial-rebuild helper work is excluded. Next
close the corresponding explicit partial-cache rebuild gap in the packed BM-01 runner, then add
packed sampling/complete I/O evidence and resource-safe scale work before qualifying campaigns.
The native BM-06 two-record cap and all benchmark thresholds are unchanged. T-20/T-19 remain
unchecked; pinned M1, full roadmap, lockfile and release gates remain unchanged. This section
supersedes earlier next steps; process-loss correctness is not power-loss or performance qualification.

## Prior verified increment — native packed BM-06 terminal pipeline (Decision 0171)

Implemented on pushed `2819afd` plus this increment: separate native packed BM-06 commands create
checkpoint 100, open, certify a pending tail at 101, recover and explicitly rebuild. Real credential
guards, OS entropy, real clock and disk certificate/blob metadata are retained. Source policy and
profile-bound bootstrap identity authenticate before derived output or a fresh tail. The shared
bounded triple selector is mechanically factored for both fixture families; selected corruption
still fails without trying an older admitted base. No consumer or M1 interface changed.

Tail reports only checkpoint history verification and no terminal digest. Recovery verifies all
200 historical versions and exact-retries the certified tail; repeated recovery is a no-op with no
root/source mutation. Explicit origin rebuild works at checkpoint or terminal without authority
changes. Missing all roots, wrong key/profile, committed-certificate and selected-pack corruption
are tested. The native cap remains two records; incomplete construction resume/process-loss
controls are not yet implemented. Pending bootstrap helper work is excluded from this increment.

Focused session 75615 / `run-p783866-i21749268.scope` exited 0: four native history tests/26.36 s,
compile 23.72 s, Clippy/1.32 s. The subsequently added selected-pack corruption case and two
separate-process CLI tests are in full session 20136 / `run-p786737-i21783196.scope`, which exited 0:
96 active tests across seven executables, two pre-existing ignored campaigns, zero failures. Library
76/135.11 s; BM-01 process 3/12.02 s; packed history CLI 2/8.36 s; packed BM-01 CLI 4/51.35 s;
BM-06 CLI 3/1.14 s; BM-06 process 8/74.08 s. Compile 52.41 s, Clippy 0.65 s. Preflight 29 GiB
available RAM/3.9 GiB free swap; sampled scope peak 552,792,064 bytes/zero swap. One job/thread
and 3G/4G/512M caps retained. Format/diff/docs/task graph checks pass.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d171-native-history-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Core source is unchanged; its gate remains 658 tests at `6bb43a4`. Next implement native packed
BM-06 incomplete-prefix resume, bounded bootstrap and owned-child process-loss controls, including
explicit prefix reconstruction after cache loss. Then complete packed sampling/I/O evidence and
resource-safe scale work before reserved-host qualification. T-20/T-19 remain unchecked; pinned M1,
full roadmap, lockfile and release prerequisites are unchanged. This section supersedes earlier next
steps; elapsed development-command timings are not benchmark results.

## Prior verified increment — packed BM-06 history equivalence (Decision 0170)

Implemented on pushed `269c7ba` plus this increment: `bm06-packed-check --records 1|2` constructs
the unchanged 100-version/4096-byte event fixture with authorized packed writes and metadata rebase.
It certifies the final batch while deliberately refusing derived publication with ResourceLimit,
restarts from checkpoint 100 to frontier 101, exact-retries that outcome, cold-admits all families
and verifies every historical payload/identity/version/revision. Explicit zero-overlay origin rebuild
streams 100 groups with 64-certificate windows, verifies history again, exact-retries every data batch
at the terminal clock and cold-compares the full v1 digest. No request/outcome map or v1 roots are used.

Packed limit construction now accepts fixture shape without changing BM-01 values. BM-06 semantic
history groups allow 100 versions/100 × 16 KiB; reverse-predecessor point reads stay bounded. Shape
arithmetic is tested through 100,000 records without database allocation; actual verifier cap remains
two records before key/filesystem allocation. The CLI explicitly reports memory-model storage,
zero qualifying trials and incomplete authenticated I/O. Native packed BM-06 work is not included.

Initial compilation found a nonexistent `PackedGraphBase::revision` accessor, corrected to its
authenticated anchor. Focused session 72638 / `run-p776857-i21764388.scope` passed two tests/1.08 s,
compile 23.77 s and Clippy/1.45 s. Full session 75663 / `run-p777918-i21740861.scope` exited 0:
89 active tests across six executables, two pre-existing ignored campaigns, zero failures. Library
71/101.83 s; BM-01 process 3/11.89 s; packed CLI 4/50.62 s; BM-06 CLI 3/1.11 s; BM-06 process
8/73.95 s. Compile 52.29 s, Clippy 1.30 s. Preflight 28 GiB available RAM/4.0 GiB free swap;
sampled scope peak 358,195,200 bytes/zero swap, not a final peak. One job/thread and 3G/4G/512M caps.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d170-packed-history-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Format/diff/docs/task graph checks pass. Core source is unchanged; its gate remains 658 tests at
`6bb43a4`. Next connect native packed BM-06 checkpoint/tail/recovery/rebuild and failure controls.
Complete authenticated I/O, exact-scale construction, host reservation and qualifying campaigns
remain open. T-20/T-19 remain unchecked; pinned M1, full roadmap, lockfile and release prerequisites
are unchanged. This section supersedes earlier next steps.

## Prior verified increment — packed bootstrap and process loss (Decision 0169)

Implemented on pushed `d72db81` plus this increment: new packed policy identities bind the frozen
fixture version and entity count. Explicit resume initializes verified empty stores or reconstructs
only one bounded, correctly bound policy transaction (one outcome/zero owners/1 MiB replay).
Legacy unbound policy-only prefixes still refuse; existing data-bearing fixtures remain supported.
Actual child-process probes park after durable creation, policy acknowledgement or successful graph
publication before metadata rebase. Only owned children are killed/reaped, including timeout cleanup.
Recovery reports now preserve the first opener's repair counts through subsequent cold admission.

Session 57394 / `run-p770048-i21748472.scope` exited 0: four packed CLI tests/50.53 s, compile
23.20 s, strict Clippy/1.31 s. Five actual SIGKILL cases at revisions 0–4 each recover the exact
20/200 reference digest, preserve the certified prefix, report a three-byte injected incomplete
certificate-tail repair, avoid duplicate commits and pass all 384 oracle queries. Certified wrong
profiles refuse without changing certificates. Future pause revisions refuse before I/O.

Session 44178 / `run-p771160-i21732459.scope` exited 0: all 68 active library tests/101.30 s,
two pre-existing ignored campaigns, compile 23.51 s, Clippy/0.05 s. This includes legacy-prefix,
wrong-key/profile, committed corruption, explicit rebuild and v1/model regression. Preflight
29–30 GiB available RAM/4.2–4.1 GiB free swap; one job/thread and 3G/4G/512M caps. No final cgroup
peak was captured. Other process suites retain their full 83-test gate at `d72db81`, not a new run;
core source remains unchanged with 658 tests at `6bb43a4`. Pending packed BM-06 work is excluded.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --test packed_terminal --locked --offline -- --test-threads=1 &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --lib --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d169-library-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Next integrate packed BM-06 history and checkpoint/suffix/origin equivalence, then its native
pipeline. Complete authenticated I/O, exact-scale construction and reserved-host BM-01/BM-06
qualification remain open. These process controls do not emulate power loss. T-20/T-19 remain
unchecked; pinned M1, full roadmap, lockfile and release gates are unchanged. This section
supersedes earlier next steps.

## Prior verified increment — native packed data-prefix resume (Decision 0168)

Implemented on pushed `79c4c6d` plus this increment: explicit `linux-packed-resume` authenticates
the data-bearing fixture marker before derived output/fresh append, selects the newest complete
same-revision packed triple with bounded discovery, independently admits it, streams any certified
suffix, then exact-retries/continues the unchanged batches. A replayed base digest is never reported
as the terminal digest; final cold admission provides that digest. Missing all caches is a refusal,
not implicit origin rebuild. Legacy empty/policy-only resume remains deliberately unsupported.

New tests compare arithmetic counts to every fixture batch at eight boundary profiles through
100,000 entities (fixture generation only), recover all four selected prefixes to the same native
20/200 terminal digest, resume incomplete prefixes with older complete/newer unpaired manifests,
refuse wrong profiles/cache loss/committed corruption, and preserve exact terminal retries. The
existing CLI test also exercises no-op resume; the qualifying-size pre-I/O cap remains unchanged.
Focused session 86399 / `run-p764399-i21551002.scope` passed 10 tests/56.76 s and Clippy/1.38 s
(compile 24.08 s); the subsequent stronger unpaired-manifest test is included in the full run.

Full session 28280 / `run-p765164-i21763465.scope` exited 0: 83 active tests across six executables,
two pre-existing ignored campaigns, zero failures. Library 68/99.92 s, BM-01 process 3/11.96 s,
packed CLI 2/11.76 s, BM-06 CLI 2/0.51 s, BM-06 process 8/74.03 s; compile 48.83 s and
Clippy 0.06 s. Preflight 30 GiB available RAM/4.3 GiB free swap; sampled scope peak 546,213,888
bytes/zero swap. One job/thread, 3G/4G/512M caps; format/diff/docs/task graph checks pass. Core source
is unchanged and the 658-test gate remains `6bb43a4`. Pending profile-bound bootstrap code is
excluded from this increment.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d168-prefix-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Next bind new bootstrap retry/transaction identities to fixture dimensions, retain refusal of legacy
unbound policy-only prefixes, and test actual owned-child interruption before bootstrap and before
metadata rebase. Then integrate packed BM-06 history. Complete authenticated I/O, exact-scale
construction and reserved-host qualification remain open; T-20/T-19 remain unchecked. Pinned M1,
full spatial/lifecycle roadmap, lockfile and release prerequisites remain unchanged. This section
supersedes earlier next steps.

## Prior verified increment — native packed terminal pipeline (Decision 0167)

Implemented on pushed `f8bc0cf` plus this increment: separate native packed create/open/rebuild/query
commands use real Linux/Btrfs storage, OS entropy, guarded credentials and the shared packed engine.
Only policy bootstrap uses the ordinary reducer; recovery uses disk certificate/blob metadata.
Open independently admits the terminal triple. Explicit rebuild authenticates the revision-two
fixture marker before derived output, reconstructs with zero overlays, then cold-admits again.
Queries consume a separate bounded oracle summary and preserve every exact output/typed refusal.
Existing v1 commands/artifacts and M1 consumer interfaces are unchanged.

Four native library tests and two separate-process CLI tests cover 20 entities/200 relationships,
all 384 queries, every data-batch exact retry without source/root mutation, create replacement
refusal, wrong credentials/profile/oracle, absent/corrupt derived roots, explicit rebuild and
committed-source corruption. Initial focused session 52701 failed two test-harness assertions:
incorrect packed filename prefix and comparison of optional caches as authoritative files. Fixed
the fixtures to select `p-` roots and `KEY`, `MANIFEST`, `CERTIFICATES`, `j-*` authority, and avoid
ciphertext dumps on assertion failure. No production checks weakened. Focused session 71170 passed
four tests/21.94 s and Clippy/1.41 s; final strengthened retry/authority assertions are in the full run.

Full session 25680 / `run-p757305-i21747995.scope` exited 0: 80 active tests across six executables,
two pre-existing ignored campaigns, zero failures. Library 65/73.43 s; BM-01 process 3/11.98 s;
new packed CLI 2/9.59 s; BM-06 CLI 2/0.52 s; BM-06 process 8/74.87 s. Compile 49.19 s;
strict Clippy 0.58 s. Preflight 30 GiB available RAM/5.7 GiB free swap; observed scope peak sample
359,600,128 bytes/zero swap (not a final peak). One job/thread and 3G/4G/512M caps retained.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d167-native-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Core source is unchanged; the core gate remains 658 tests at `6bb43a4`. The prior 1,000-entity
memory-model timing remains pinned to Decision 0166, not remeasured here. Terminal-only commands
do not implement incomplete-prefix resume or process-loss qualification. Next implement bounded
native prefix selection/recovery and owned-child interruption controls, then packed BM-06 history.
Complete authenticated I/O, exact-scale construction and reserved-host campaigns remain open.
The native 20,000-entity ceiling is only admission, not measured packed capacity. T-20/T-19 remain
unchecked; pinned M1, full roadmap and release prerequisites are unchanged. This section supersedes
earlier next steps.

## Prior verified increment — packed benchmark engine equivalence (Decision 0166)

Implemented on pushed `6bb43a4` plus this increment: `packed-engine-check` constructs the unchanged
BM-01 mapping through authorized packed writes, one-outcome live overlays and per-batch metadata
rebase. Bootstrap reconstructs policy only, with no v1 graph/coordinator roots. Disk certificate/blob
metadata recovery, independent cold triple admission, bounded query caching and explicit zero-overlay
origin rebuild are all connected. Cold/origin exact retries preserve outcomes without adding overlays.
Both paths match every oracle query; a second cold admission matches the complete v1 state digest.
The existing v1/native commands and artifacts are unchanged. No consumer or M1 interface changed.

Focused release session 38957 / `run-p752634-i21715994.scope` passed three tests/4.19 s and strict
Clippy/1.62 s (compile 28.25 s). The 20/200 case preserves the frozen 384-query digest
`46f1bdb3138d6325e4c0f56b5fd3bbf5ff092d816e8a0f6c23acd15687b910b5`. Bounds/read-limit arithmetic
is checked for every accepted profile, while the actual memory-model verifier still refuses over
1,000 entities before filesystem/key allocation.

Session 17719 / `run-p753212-i21731658.scope` passed the actual 1,000-entity/10,000-relationship
command: frontier 4, three origin suffix groups and all 384 queries matched. Whole-command wall
250.81 s includes 18.91 s compilation, construction, admission, queries/oracle and origin rebuild;
it is not isolated query/recovery timing. Maximum RSS 431,496 KiB, zero swaps; sampled scope peak
449,994,752 bytes/zero swap. Query cache accounted 17,677,312 of 67,108,864 bytes, with 85,879,413
hits, 1,015 misses and zero evictions. This does not establish cache-pressure acceptance. Exact
digests and observations are retained in `docs/evidence/packed-engine-1000-development.json`.

Final benchmark regression session 64228 / `run-p754109-i21762814.scope` exited 0: 74 active tests,
two pre-existing ignored exact-oracle campaigns, zero failures; library 61/49.59 s, BM-01 process
3/11.80 s, BM-06 CLI 2/0.49 s and BM-06 process 8/72.74 s. Compile 3.67 s; Clippy/0.04 s.
Preflight 30 GiB available RAM/5.7 GiB free swap; sampled scope peak 522,870,784 bytes/zero swap.
One job/thread, one heavy workload and 3G/4G/512M caps were retained. Core regression is the
658-test Decision 0165 gate at `6bb43a4`; core source did not change in this experiment increment.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --lib --locked --offline engine::packed -- --test-threads=1 --nocapture &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 /usr/bin/time -v cargo run --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- packed-engine-check --entities 1000 2>&1 | tee /tmp/uste-d166-packed1000-development.log'
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d166-native-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Format/diff/docs/task graph and evidence JSON checks pass. Next connect the packed engine to the
native construction/recovery/query runner, preserving profile binding, explicit rebuild and failure
controls, then integrate BM-06 packed history. The memory-adapter/oracle measurement does not
qualify any benchmark. Complete authenticated I/O accounting, exact-scale construction, host
reservation and qualifying BM-01/BM-06 campaigns remain open. T-20/T-19 remain unchecked; pinned
M1, full spatial/lifecycle roadmap and distribution gates remain unchanged. This section supersedes
earlier next steps.

## Prior verified increment — terminal-only packed origin recovery (Decision 0165)

Implemented on pushed `92e5e8c` plus this increment: explicit journal-origin reconstruction needs
no derived graph/primary/quota root, stages only bounded genesis, releases that reducer, then
streams the authenticated suffix through the shared packed recovery implementation. Publication
occurs only at the actual authenticated frontier. No live state escapes an error or partial
three-manifest publication. Ordinary open still requires independently admitted roots; there is
no silent fallback, rollback or atomic-three-file claim. The caller still selects storage's
certificate/blob recovery mode. Reports are bounded staging/proof work, not complete adapter I/O.

Seven new tests cover first-only and multi-revision root-free reconstruction, full-reducer digest
equality, every exact retry including genesis, zero outcome/owner overlays, cold admission and
continued writes, eight limit refusals, retained/corrupt derived roots, late committed-certificate
corruption, authenticated retry/transaction collisions and false results. All 909 observed I/O
error/crash cases restart successfully without intermediate publication. The graph-only path
refuses authenticated inventories at genesis or suffix; generic first-owner/quota behavior remains
unchanged. An initial fixture incorrectly tried to certify an empty inventory, already rejected
by coordinator admission. It was corrected to assert that refusal separately and certify an actual
synthetic blob inventory; no production guard changed.

Final full gate session 88698 / `run-p748201-i21762360.scope` exited 0: 658 tests across 47
executables, zero ignored/failures; graph integration 89/414.66 s, transaction integration
97/96.16 s, Clippy/2.28 s and workspace docs/9.12 s. Preflight 30 GiB available RAM/5.7 GiB free
swap; sampled scope peak 694,444,032 bytes/zero swap. A final test-only review restored the original
explicit restart in the shared genesis fixture; session 94343 / `run-p751801-i21762617.scope`
passed all four genesis tests/1.59 s and seven origin tests/27.48 s afterward (compile 12.74 s).
Earlier session 72172 failed only the malformed empty-inventory fixture; focused correction 12132
passed before the full gate was rerun. All workloads used one build job/thread and 3G/4G/512M limits.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d165-workspace-final.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked --offline'
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-graph --test disk_index --locked --offline packed_genesis -- --test-threads=1 &&
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-graph --test disk_index --locked --offline packed_graph_origin -- --test-threads=1 --nocapture'
```

Format/diff/docs/task graph checks pass. Native regression remains 71 active tests at `35bc482`,
not a new run here. Pending benchmark integration is deliberately excluded from this core increment.
Next execute/review the packed BM-01 development oracle integration, then connect the native
construction/recovery/query runner and BM-06 packed history path. Full authenticated I/O accounting,
exact-scale construction and reserved-host qualification remain open; no benchmark target changed.
T-20/T-19 remain unchecked. Pinned M1 interfaces/evidence and the lockfile are unchanged. This section
supersedes earlier next steps; full-project and distribution prerequisites remain as recorded below.

## Prior verified increment — private packed graph genesis (Decision 0164)

Implemented on pushed `a32693d` plus this increment: stage all eight packed graph families directly
from an opaque authenticated first-transaction reconstruction, without requiring v1 roots. Borrow
the genesis snapshot; preflight entry/byte/batch totals; retain one bounded batch and fixed family
handles. Explicit aggregate read/write limits remain separate from per-batch limits. Receipt/owner
binding precedes output. Staging publishes no root manifest and changes no journal authority;
failed work can leave unreachable immutable scratch packs, not discoverable partial state.

Four integration tests pass (session 95133 / `run-p736623-i21730652.scope`, 1.73 s), including 219
observed staging I/O error/crash cases with restart, policy-only/record-bearing first transactions,
a multi-page value, 1/2/512-entry partition equality, five exact/minus-one aggregate budgets,
foreign owner/reopened-handle rejection, corruption refusal and independent cold semantic admission
after explicit terminal publication. Workspace Clippy passed/3.38 s. Session 1056 /
`run-p737353-i21738839.scope` passed 16 graph library tests/0.01 s, Clippy/0.05 s and graph docs/1.21 s.
Both sessions exited 0 with one build job/test thread and the 3G/4G/512M cgroup limits below.
Preflight 30 GiB available RAM/5.7 GiB free swap; no scope peak sample was retained for these short
checks. This additive API does not change existing callers. Full workspace regression remains
647 tests at `a32693d`; native regression remains 71 active tests at `35bc482`, not new runs here.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-graph --test disk_index --locked --offline packed_genesis -- --test-threads=1 --nocapture &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings'
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-graph --lib --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d164-library-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-graph --no-deps --locked --offline'
```

Format/diff/docs/task graph checks pass. Next implement explicit packed origin recovery: privately
stage genesis graph/primary/quota, stream every suffix transaction with bounded certificate windows,
then publish only the fully validated terminal triple. Ordinary open must not silently rebuild or
roll back. Native integration and qualifying campaigns follow; T-20/T-19 and the full roadmap remain
open. M1 interfaces/evidence and the lockfile are unchanged. This section supersedes earlier next steps.

## Prior verified increment — authorized packed graph cache configuration (Decision 0163)

Implemented on pushed `35bc482` plus this increment: trusted adapters can select a bounded packed
cache for point, historical, adjacency and evidence-support queries. The existing constructor
remains uncached. Every read still checks current policy/readiness, scope, typed permissions and
cancellation; warm plaintext does not authorize hidden references. ManageSchema is required for
cache diagnostics/clear, including disabled-cache diagnostics. Proof-work admission is unchanged
by warmth. The trusted experimental packed-read implementation trait gains an optional cache;
ordinary consumer request/output types, v1 APIs and pinned M1 interfaces are unchanged.

Six new tests cover uncached/cold/warm full-replay equality, minimum (25,600-byte) and 64 KiB
eviction, maintenance-only diagnostics, foreign/absent/denied callers, sticky warm cancellation,
all exact/minus-one point/history and aggregate expansion budgets, late ciphertext mutations in
five graph families, and 279 observed read-error/crash cases with cold recovery. Ordinary read
errors also retry through the same partially warmed cache without returning partial results.
Focused session 64856 / `run-p729130-i21719177.scope` passed the first five tests/52.57 s and
Clippy/5.20 s; the added point/history limit test passed session 73842/0.12 s.

Full gate session 88149 / `run-p731046-i21714978.scope` exited 0: 647 tests across 47 executables,
zero failed/ignored (graph disk 78/416.30 s, transaction integration 97/98.24 s), Clippy 0.06 s,
graph/transaction docs 2.70 s. Preflight 30 GiB available RAM/5.7 GiB free swap; sampled scope peak
1,827,446,784 bytes/zero swap. Exact command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d163-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-graph -p uste-txn --no-deps --locked --offline'
```

Format, diff, docs and task graph checks pass; lockfile digest is unchanged. Native regression
remains pinned to `35bc482`; this optional packed facade is not yet wired into native fixtures,
and no new native or qualifying campaign is claimed. Next implement private packed graph genesis
staging (Decision 0164), then explicit terminal-only origin recovery and native integration.
The unregistered genesis source/tests and Decision 0164 draft are excluded from this commit.
T-20/T-19 and the preserved full roadmap/release gates remain open. M1 qualification and handoff
are unchanged. This section supersedes earlier next steps.

## Prior verified increment — owner/session-bound packed caching (Decision 0162)

Implemented on pushed `983a5db` plus this increment: optional packed lookup and forward/reverse
cursor caching binds every page to its complete physical context, exact journal owner and unlocked
vault session. A different owner refuses until clear; key lock/unlock or replacement cannot serve
old plaintext. Existing nonce tracking is never reset. Authenticated immutable pages use the same
slot-addressed LRU policy as v1 through a private generic representation. Uncached APIs, cold
canonical admission, persisted formats and M1 interfaces are unchanged. Cached proof-work admission
charges the same page/encoded-byte units as uncached execution; cache counters are not physical I/O.
Key locking invalidates access, not synchronous erasure of an independently owned cache. Clear/drop
releases resident ownership; bounded transient reader handles and result buffers are separate work.

Eleven new tests cover session lifecycle, all context fields, cold/warm equality, exact/narrower
limits, owner/reopen isolation, locked empty/exhausted cursors, late corruption after clear,
counter overflow, a 20,000-access independent LRU trace and scoped transaction wrappers. Storage
fault matrices cover 27 cold lookup/reopen and 54 directional cursor error/crash cases. Focused
storage session 87164 passed seven tests/1.15 s; session 92387 passed the two transaction cache
tests/0.02 s and workspace Clippy/8.34 s. The crypto session test passed. An initial nested module
path and Clippy `len_zero` finding were repaired; an initial transaction filter selected zero tests
and was corrected to `packed_read_only_cached` before counting verification.

Full gate session 20930 / `run-p720089-i21714531.scope` exited 0: 641 tests across 47 executables,
zero failed/ignored (graph disk 72/357.67 s, transaction integration 97/98.62 s), Clippy 0.07 s and
four-crate docs 5.96 s. Preflight 30 GiB available RAM/5.7 GiB free swap; sampled scope peak
2,649,554,944 bytes/zero swap. Native session 48956 / `run-p726348-i21636326.scope` exited 0:
71 active tests (58 unit/46.21 s, BM-01 process 3/12.03 s, BM-06 CLI 2/0.50 s, BM-06 process
8/74.55 s), two pre-existing exact-oracle ignores unchanged, release build 52.43 s and Clippy
3.42 s. Native preflight 29 GiB available/5.7 GiB swap/984 GiB disk; sampled peak 544,739,328
bytes/zero swap. These remain bounded regression runs, not qualifying BM-01/BM-06 campaigns.

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d162-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-crypto -p uste-storage -p uste-graph -p uste-txn --no-deps --locked --offline'
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d162-native-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Format, diff, docs and task graph checks pass; the lockfile digest remains
`7ed2b533b3c801250a89e008b4ca26c48f16400e38224d8a63447c0393aaa97b`. Next integrate trusted
graph cache configuration/maintenance diagnostics (Decision 0163), then packed native bootstrap,
origin rebuild and qualification readiness. The unregistered graph-cache tests/facade extension
and Decision 0163 draft are excluded from this commit. T-20/T-19 and full-roadmap/release gates
remain open; M1's pinned qualification/handoff is preserved. This section supersedes earlier next steps.

## Prior verified increment — authorized packed graph expansion (Decision 0161)

Implemented on pushed `05b797e` plus this increment: optional adjacency/evidence-support queries
share the existing v1 reader's pure validation, ordering, visibility and result-limit semantics.
Packed scans and record lookups share five aggregate budgets; callers cannot override them or
obtain cardinality telemetry. Self-loops are deduplicated, parallel edges remain distinct, and no
partial success escapes a limit/cancellation/corruption/I/O error. The v1 API and budgets are unchanged.

Five tests cover full-replay/cold reference equality, all directions, parallel edges/self-loops,
support and hidden references, disabled/unauthorized/cancelled reads, all five exact/narrower
budgets, late secondary/current ciphertext corruption and 1,224 observed read-error/crash cases
with cold recovery. The mixed-direction fixture's exact work is [89 pages, 1,828,505 encoded bytes,
10 candidates, 3,326 returned bytes, 10 record lookups]; these are fixture work bounds, not a
performance result. An initial temporary-borrow compilation error was fixed by retaining the
record ID before borrowing its bytes. Focused session 95385 / `run-p709543-i21679255.scope` passed
five tests (229.87 s) and Clippy (3.21 s).

Full gate session 14647 / `run-p711185-i21713869.scope` exited 0: 630 tests across 47 executables,
zero failed/ignored (graph disk 72/350.91 s; transaction integration 95/100.59 s), Clippy 0.07 s,
graph/transaction docs 2.44 s. Preflight 31 GiB available RAM / 5.7 GiB free swap; sampled scope
peak 793,317,376 bytes / zero swap. Format, diff, docs and task graph checks passed. Exact commands:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d161-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-graph -p uste-txn --no-deps --locked --offline'
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d161-native-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings'
```

Native session 49286 / `run-p714391-i21675993.scope` exited 0: 58 unit tests/46.17 s, BM-01 process
3/11.92 s, BM-06 CLI 2/0.51 s, BM-06 process 8/73.89 s; the two existing exact-profile oracle
ignores are unchanged. Build 50.79 s; Clippy 3.38 s. Preflight 30 GiB available / 5.7 GiB swap,
984 GiB disk available on Btrfs; sampled scope peak 546,172,928 bytes / zero swap. This updates
native regression beyond `42f9abb` but does not integrate packed native fixtures or qualify BM-01/06.
The native README's stale certificate-window/catalog descriptions are reconciled with implemented behavior.

Next implement owner/session-bound packed caching, then graph cache configuration and native
integration/qualification readiness. Decision 0162 and all unregistered vault-session, packed-cache,
cached storage/transaction access and test drafts are excluded from this commit. T-20/T-19 remain
open; M1 interfaces/evidence and lockfile are unchanged. This section supersedes earlier next steps.

## Prior verified increment — authorized packed point/history reads (Decision 0160)

Implemented on pushed `4d7c956` plus this increment: a restricted domain-read facade validates the
exact current durable policy and typed target permissions before I/O. Current packed lookup and
reverse historical seek return bounded records with existing all-or-nothing embedded-reference
visibility. History uses current authorization; future views, pending repair and uncertainty fail
closed. Observed cancellation is sticky. Expansion is explicitly unsupported in this increment.

Six tests cover full replay reference equality after cold reopen, temporal boundaries, hidden
references, foreign/absent identities, denied targets/history/scope, stale policy, pre-I/O refusal,
cancellation, exact/narrower bounds, late current/history ciphertext corruption, pending repair,
uncertain commit recovery and all 63 observed point/history read fault cases with cold restart.
Initial fixture compilation found redundant qualifications and a two-argument use of the
three-argument record-reference constructor; corrected without changing requirements. Focused
session 54757 / `run-p704435-i21706429.scope` passed six tests (2.17 s), Clippy 0.41 s.

Full gate session 29745 / `run-p704893-i21706474.scope` exited 0: 625 tests across 47 executables,
zero failed/ignored (graph disk 67/121.27 s; transaction integration 95/100.80 s), Clippy 0.07 s,
graph/transaction docs 2.67 s. Preflight 31 GiB available RAM / 5.7 GiB free swap; sampled scope
peak 1,513,119,744 bytes / zero swap. Format, diff, docs and task graph checks passed. Exact command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d160-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-graph -p uste-txn --no-deps --locked --offline'
```

Next integrate bounded packed adjacency/evidence expansion, then caching and native qualification
readiness. Decision 0161 and unregistered `authorized_read/expansion/semantics.rs`, packed
`live/authorized_read/expansion.rs` and `packed_authorized_expansion.rs` are excluded drafts.
T-20/T-19 remain open; no larger-than-memory/production qualification is claimed. Native standalone
remains last tested at `42f9abb`; pinned M1 interfaces and evidence are unchanged. This section
supersedes earlier next-step text below.

## Prior verified increment — read-only packed access (Decision 0159)

Implemented on pushed `664f6c4` plus this increment: immutable scoped packed index readers from
the current coordinator or authenticated maintenance target. Maintenance delegates exact lookup,
binding validation and forward/reverse traversal to the same implementation. No staging/publication
or caller authorization capability is exposed; uncertainty blocks construction. Existing checks
remain active even for exhausted cursors, and failures remain sticky.

Two new tests cover lookup/traversal equivalence, exact and three narrower lookup bounds, foreign
owners including exhausted cursors, no writes, and all 45 observed cursor I/O/error/crash cases.
Existing uncertain commit cases now also assert reader refusal; existing future/wrong-namespace
maintenance tests exercise the delegated path. An initial fixture used `FileLen` instead of the
fault adapter's `Metadata` operation; corrected before execution. Focused session 67113 /
`run-p698005-i21652082.scope` passed two tests (0.34 s) and Clippy (0.42 s).

Full gate session 18972 / `run-p698433-i21717236.scope` exited 0: 619 tests across 47 executables,
zero failed/ignored (graph disk 61/119.34 s; transaction integration 95/100.95 s), Clippy 0.06 s,
graph/transaction docs 2.64 s. Preflight 31 GiB available RAM / 5.7 GiB free swap; sampled scope
peak 1,653,665,792 bytes / zero swap. Format, diff, docs and task graph checks passed. Exact command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d159-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-graph -p uste-txn --no-deps --locked --offline'
```

Next implement authorized packed point/history reads, then bounded graph expansion, caching and
native qualification readiness. Decision 0160 and unregistered transaction `authorized_packed/read.rs`,
graph `live/authorized_read.rs` and `packed_authorized_reads.rs` test drafts are excluded here.
T-20/T-19 remain open. Native standalone remains last tested at `42f9abb`; M1's pinned interface
and qualification evidence are unchanged. This section supersedes earlier next-step text below.

## Prior verified increment — authorized packed writes (Decision 0158)

Implemented on pushed `497ce72` plus this increment: restricted inventory-free packed writes,
typed target authorization and quotas before clock/storage, shared exact retry/collision checks,
bounded graph preparation, content-free dependency errors and certified policy synchronization
before repair. Pending revocation survives publication failure; older retries cannot repair another
pending revision. Outcome uncertainty stays quarantined. No consumer maintenance capability escapes.

Five tests cover denied/foreign/revoked identities, scope/quota/inventory refusal before I/O,
reference outcomes, retry/expiry/cancellation/collision, hidden dependencies, policy replacement,
repair failure, read failure, uncertain commit and cold retry. An initial test used a nonexistent
error variant, then attempted to replace an unconsumed injected fault; both fixture defects were
fixed without weakening production behavior. Final focused session 59800 /
`run-p691702-i21550505.scope` passed all five tests (0.14 s), plus Clippy (6.57 s).

Full workspace log `/tmp/uste-d158-workspace-verification.log` records 617 passing tests across
47 executables, zero failures/ignored (graph disk 61/112.35 s; transaction integration 93/100.22 s).
Scope `run-p692433-i21651362.scope` finished; its terminal tool response was lost across context
compaction, so no unseen process exit status is claimed. Clippy and docs were explicitly rerun
after it finished in `run-p696385-i21674857.scope`, exit 0 (0.04 s each). Preflight: 31 GiB available
RAM / 5.7 GiB free swap; sampled test scope peak 1,508,417,536 bytes / zero swap. Commands:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d158-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-graph -p uste-txn --no-deps --locked --offline'
cargo fmt --all -- --check
git diff --check
python3 scripts/check_docs.py
python3 scripts/check_task_graph.py
```

Next implement read-only packed access and authorized graph queries, then native integration and
qualification readiness. Decision 0159 and unregistered `packed_maintenance/reader.rs` are excluded
drafts. T-20/T-19 remain open; no larger-than-memory campaign or production qualification is claimed.
Native standalone remains last tested at `42f9abb`; pinned M1 interfaces/evidence are unchanged.
This section supersedes earlier next-step text below.

## Prior verified increment — paired packed graph suffix recovery (Decision 0157)

Implemented on pushed `df8f54c` plus this increment: common owner/receipt admission validates a
historical graph/primary/quota triple, then an authenticated windowed cursor drives bounded graph
proofs and private graph/metadata staging. Original requests/results, retry/transaction collisions
and first-owner semantics remain enforced. Full exhaustion and exact terminal anchors precede
three terminal manifests and installation with empty overlays. The manifests are individually
durable, not an atomic three-file transaction; failed recovery returns no live coordinator.
Per-revision proof/delta/staging limits compose with admitted revision count; diagnostic counters
remain partial successful work, not complete authenticated/physical I/O accounting.

Five tests cover zero/one/two-revision tails, cold triple admission and frozen reference digests,
continued live writes/rebase, exact and seven narrower recovery limits, foreign ownership,
609 observed I/O/error/crash cases with restart, late certificate/graph/primary/quota ciphertext
corruption and authenticated retry/transaction collisions/false certified results. An initial test
reused deterministic entropy after reopening and correctly hit `AlreadyExists`; distinct test
sessions now use distinct entropy seeds, without changing production collision handling.
Focused session 90489 / `run-p684215-i21677514.scope` passed three tests in 42.13 s and Clippy
0.37 s; session 8454 / `run-p684901-i21674125.scope` passed the two added corruption/false-result
tests and Clippy. No threshold was weakened.

Full gate session 23834 / `run-p685385-i21705115.scope` exited 0: 612 tests across 47 executables
(graph disk 56/117.57 s; transaction integration 93/100.16 s), Clippy 0.06 s and graph/transaction
docs 2.58 s. Preflight: 31 GiB available RAM / 5.7 GiB free swap; sampled scope peak
1,361,862,656 bytes / zero swap. Format, diff, docs and task graph checks passed. Exact command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d157-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-graph -p uste-txn --no-deps --locked --offline'
```

After that gate, stronger assertions pin late private staging and exact false-result rejection.
Session 97177 / `run-p689895-i21712961.scope`, same resource/profile settings, passed
`cargo test -p uste-graph --test disk_index --locked --offline packed_graph_suffix_recovery_authenticated
-- --test-threads=1` (1/0.19 s) and workspace Clippy (0.36 s).

Next verify authorized packed writes, then query/native integration and qualification readiness.
Decision 0158 and unregistered transaction `authorized_packed/write.rs`, graph
`live/authorized_write.rs` and `packed_authorized_writes.rs` test drafts are excluded from this
increment. T-20/T-19 and qualifying campaigns remain open. Native standalone remains last tested
at `42f9abb`; M1's pinned implementation/handoff is unchanged. This section supersedes earlier
next-step text below.

## Prior verified increment — cold packed graph semantic admission (Decision 0156)

Implemented on pushed `f95fe7e` plus this increment: canonical admission of eight explicit packed
families followed by streamed history transitions, historical/current reference closure, terminal
history/current equality, exact derived membership/cardinality and terminal/current policy checks.
Existing pure graph semantic rules and frozen v1 digest framing are reused. Reverse packed proofs
select historical predecessors. Independent canonical, sequential and repeated-proof budgets avoid
interleaved reads overspending an old allowance. Only complete admission returns an opaque base;
its optional cached v1 digest is now also populated by cold semantic admission. No graph-wide maps
or cache writes are introduced. Reports count actual packed work, not complete physical I/O.

Six tests cover five populated reference states, policy-only and policy-absent states with explicit
empty families, exact and 24 narrower resource boundaries, eight authenticated/canonical but
semantically false graphs, false profile/digest claims, foreign owners, late policy ciphertext
corruption and 1,377 observed read/error/crash cases with restart. Initial Clippy feedback on a
collapsible conditional was corrected. Focused session 11362 / `run-p679343-i21622401.scope`
passed five tests in 29.82 s and Clippy in 0.38 s; session 84580 /
`run-p679894-i21677318.scope` passed the added empty-family test and Clippy.

Full gate session 60453 / `run-p680387-i21622442.scope` exited 0: 607 tests across 47 executables
(graph disk 51/74.75 s; transaction integration 93/100.37 s), Clippy 0.04 s and graph docs 1.13 s.
Preflight: 31 GiB available RAM / 5.7 GiB free swap; sampled scope peak 724,246,528 bytes / zero
swap. Format, diff, documentation and task graph checks passed. Exact command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d156-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-graph --no-deps --locked --offline'
```

Next integrate paired packed graph suffix recovery, followed by authorized consumer writes and
native qualification readiness. Decision 0157 and unregistered graph `live/recovery.rs`, transaction
`packed_coordinator/domain_recovery.rs` and `packed_suffix.rs` test drafts are excluded from this
increment. T-20/T-19, full I/O accounting and qualifying campaigns remain open. Native standalone
remains last tested at `42f9abb`; pinned M1 implementation and handoff are unchanged. This section
supersedes historical next-step text below.

## Prior verified increment — packed live graph state and repair (Decision 0155)

Implemented on pushed `b4b2c0e` plus this increment: exact published-root installation, explicit
proof-prepared commits, one certified pending graph plan, private staging/terminal publication and
opaque postcommit installation. Pending state exposes no current graph base or durable-policy
read, rejects fresh preparation and metadata rebase, and preserves exact coordinator retries.
Ready state supplies the distinct ordered commitment for paired primary/quota metadata rebase.
Snapshots are metadata-only; no v1 checkpoint trait or global in-memory graph is introduced.
Coordinator installation now independently rechecks retained reducer disk capabilities against
its journal owner; earlier caller-side validation alone cannot authorize foreign-owner handles.

Six tests cover six commit/repair/rebase cycles against independent full-reducer outcomes,
171 observed repair I/O/error/crash cases with cold replay, in-place I/O-error repair without
recertification, foreign owner installation, policy readiness, altered request and stale plans,
and commit-sync uncertainty quarantine. Cold replay here is explicitly the full test oracle,
not a claim of implemented packed cold semantic recovery. Two initial fixture compile errors
(fault-arm method and private limit fields) were fixed using supported APIs. Focused session
36733 / `run-p673581-i21650271.scope` passed six tests in 1.70 s and Clippy in 0.31 s.

Full gate session 45381 / `run-p674050-i21642769.scope` exited 0: 601 tests across 47 executables
(graph disk 45/43.86 s; transaction integration 93/100.37 s), Clippy 0.06 s and graph/transaction
docs 2.52 s. Preflight: 31 GiB available RAM / 5.7 GiB free swap. Sampled scope peak
1,524,744,192 bytes / zero swap. Format, diff, documentation and task graph checks passed.
Exact command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d155-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-graph -p uste-txn --no-deps --locked --offline'
```

Next verify cold packed graph semantic admission, then integrate authenticated packed suffix
recovery and authorized consumer writes. Decision 0156 and unregistered `packed/admission.rs` /
`packed_admission.rs` drafts are excluded from this increment. T-20/T-19, full I/O accounting and
qualifying campaigns remain open. Native standalone remains last tested at `42f9abb`; M1's pinned
implementation/handoff is unchanged. This section supersedes historical next-step text below.

## Prior verified increment — certified packed graph delta staging (Decision 0154)

Implemented on pushed `4d57b8d` plus this increment: the same bounded graph delta algorithm
now supplies receipt-bound private staging of all eight packed families. Exact canonical request,
result, predecessor certificate/commitment/counts/policy and live tree bindings are checked before
cache mutation. Bounded batches preserve old roots; only complete staging returns a typed base.
No graph root is published by staging. The opt-in `source_v1_digest()` accessor is now optional:
new staged states require the existing streaming compatibility export for their frozen v1 digest.
Unchanged trees and complete deletion legitimately require zero output pages, even at an exhausted
aggregate write budget; neither bypasses compare-and-swap or authenticated reads.

Four tests cover partition-independent exact old/new v1 exports, 333 observed staging I/O/error/
crash cases with restart, exact/minus-one ceilings, altered-request/same-result rejection,
corruption and foreign-owner refusal. Focused session 8382 / `run-p667592-i21621784.scope`
passed 15 packed graph tests in 16.53 s and Clippy in 1.93 s. Full gate session 21686 /
`run-p668282-i21676548.scope` exited 0: 595 tests across 47 executables, transaction integration
93/99.05 s, Clippy 0.04 s and graph docs 1.08 s. Preflight: 31 GiB available RAM and 5.7 GiB
free swap. One Cargo job/test thread; scope limits 3G high / 4G max / 512M swap. Exact command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d154-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-graph --no-deps --locked --offline'
```

Next implement packed live repair-only state and coordinator integration, followed by cold
semantic/recovery integration. Decision 0155 is an excluded next-increment contract draft.
T-20/T-19 and qualifying campaigns remain open. Native standalone remains last tested at
`42f9abb`; M1's pinned implementation and handoff are unchanged. Earlier next-step paragraphs
below are historical; this section is the authoritative continuation point.

## Prior verified increment — packed graph proof preparation (Decision 0153)

Implemented on pushed `30f9aa1` plus this increment: explicit packed current/history/reverse reads
construct only the bounded transaction proof closure and invoke the unchanged pure graph reducer.
The opaque prepared result binds scope, exact base certificate, ordered commitment, counts and
policy while preserving canonical request and journal-result digests. No v1 root anchor is forged.
Cached admitted metadata/policy is charged to logical proof retention. Every tree's live owner/key
binding is checked; the separate packed report, not zeroed legacy run/cache fields, records I/O.

Four tests compare 21 accepted/rejected requests with the full reducer (references, history
predicates, correction, cascade and policy replacement), cover exact and ten minus-one ceilings,
270 observed read/error/crash cases with cold restart, current/history/reverse ciphertext corruption
and foreign owner/scope refusal. Focused session 44918 / `run-p663038-i21642189.scope` passed four
tests in 3.33 s and Clippy in 0.30 s; the subsequently added exact reference-visit ceiling case
passed at the start of the full gate. No test or resource threshold was weakened.

Full gate session 60175 / scope `run-p663497-i21672746.scope` exited 0: 591 tests across 47
executables (graph disk 35/35.60 s; transaction integration 93/99.45 s), Clippy 1.89 s and docs
1.13 s. Preflight: 31 GiB available RAM / 5.6 GiB free swap; sampled scope peak 924,303,360 bytes /
zero swap. Format, diff, documentation and task graph checks passed. Exact command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
export CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true
cargo test -p uste-graph --test disk_index --locked --offline packed_graph_preparation_exact -- --test-threads=1 &&
cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d153-workspace-verification.log &&
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
RUSTDOCFLAGS="-D warnings" cargo doc -p uste-graph --no-deps --locked --offline'
```

Next implement receipt-bound graph delta staging, then live repair and cold semantic integration.
Decision 0154, unregistered `packed/preparation/staging.rs` and `packed_staging.rs` test drafts
are excluded from this increment. T-20/T-19, complete I/O accounting and qualifying campaigns remain
open. Native standalone remains last tested at `42f9abb`; M1 remains pinned and unchanged.

## Prior verified increment — streamed packed graph bridge (Decision 0152)

Implemented on pushed `da64673` plus this increment: an independently admitted v1 graph base
streams through bounded private batches into eight explicit canonical packed families. Every source
cursor must fully authenticate before the opaque graph base is returned. Counts, policy, records,
history and the original v1 digest remain intact; the ordered commitment has a distinct profile
and never substitutes for the frozen oracle digest. Bounded export recomputes the exact v1 hash.
Source and batch work ceilings are separate; reports exclude certificate/failed adapter work.

Seven integration tests plus an independent literal-vector unit test cover 1/2/3/512 partition
independence, all eight nonempty families, explicit empty families, exact/minus-one budgets,
405 observed bridge I/O/error/crash cases with cold restart, late source and packed ciphertext
corruption, foreign owner refusal, disk-backed certificate ownership and cold per-family admission
of the published result. Canonical family admission alone is not packed graph semantic admission.
`python3 scripts/check_packed_graph_vector.py` independently verified two profiles and one state
framing vector. Early fixture mistakes (non-cloneable model filesystem, private limit constructors,
certificate-byte geometry and validation-field naming) were fixed before the final focused pass.
Session 78211 / scope `run-p659200-i21672551.scope` passed 8 tests (integration 7/4.42 s) and
Clippy (0.27 s).

Full gate session 81051 / scope `run-p659696-i21408064.scope` exited 0: 587 tests across 47
executables (graph disk 31/31.63 s; transaction integration 93/98.90 s), Clippy 0.07 s and docs
1.66 s. Preflight: 31 GiB available RAM / 5.6 GiB free swap; sampled scope peak 398,118,912 bytes /
zero swap. Format, diff, docs and task checks passed. Exact full-gate command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d152-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-graph --no-deps --locked --offline'
```

Next implement packed graph proof preparation, typed delta staging and live/cold semantic
integration. Decision 0153 and unregistered `packed/preparation.rs` / `packed_preparation.rs` drafts
are excluded from this increment. T-20/T-19, complete I/O accounting and qualifying campaigns remain
open. No native command or M1 interface changed; their pinned baselines remain unchanged.

## Prior verified increment — bounded reverse packed traversal (Decision 0151)

Implemented on pushed `cb63dbd` plus this increment: descending `(lower, upper]` cursor and
owner/scope-bound wrappers reuse authenticated path/value verification and cumulative limits.
Compressed-prefix seek skips later subtrees rather than scanning them. Forward `[lower, upper)`
behavior is unchanged. Four new tests and expanded existing cases cover all byte/prefix bounds,
96 seeded sparse variable-length keys, empty/singleton and 16 MiB values, exact/minus-one budgets,
all observed read faults with restart, late content corruption and foreign/future/locked handles.
The 256-key shared-prefix test selects the first key in nine page reads and one candidate.

Focused session 11876 / scope `run-p648135-i21620886.scope` passed the initial three tests in
1.39 s and Clippy in 7.24 s. Session 65639 / `run-p649008-i21648652.scope` passed 71 storage and
56 transaction packed tests (8.49/98.95 s); subsequent Clippy caught unusual byte grouping in the
new sparse test's seed. Corrected without changing the seed or test requirements.

Final full gate session 80518 / scope `run-p650987-i21613870.scope` exited 0: 579 tests across
47 executables (transaction integration 93/99.26 s), Clippy 1.30 s and docs 2.63 s. Preflight:
31 GiB available RAM / 5.6 GiB free swap; sampled scope peak 2,194,870,272 bytes / zero swap.
Format, diff, documentation and task graph checks passed. Exact gate command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d151-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-storage -p uste-txn --no-deps --locked --offline'
```

Next implement the separately versioned packed graph bridge and v1 compatibility export, then
domain preparation/admission/live integration. Decision 0152 and unregistered graph `packed.rs`
and `packed_bridge.rs` drafts are excluded from this verified increment. T-20/T-19, complete I/O
accounting and qualifying campaigns remain open; native standalone and M1 baselines are unchanged.

## Prior verified increment — authorized packed reads (Decision 0150)

Implemented on pushed `ff3a6cd` plus this increment: exact packed retry/transaction reads and
paired committed-byte accounting now have a restricted, current-durable-policy-bound facade.
Authorization precedes clock, I/O and cardinality-sensitive admission. Foreign transaction
ownership is filtered before expiry; uncertainty, missing/pending policy and mismatched kernels
fail closed. Charges combine the admitted base with bounded first-owner overlays, surviving
failed rebase. Staged reservations, authorized writes and domain integration remain separate.

Nine new tests cover raw and authorized base/overlay/rebase/recovery reads, independent outcome
and quota permissions, revocation, corruption, inclusive expiry, zero-byte/first-owner accounting,
117 raw and 90 authorized observed read-error/crash cases. The earlier commit fault matrix was
also expanded from 60 to 66 cases after review found omitted SyncData boundaries. Initial test
compilation used a nonexistent error variant; corrected to the existing Unauthorized variant.

Full gate session 30721 / scope `run-p643633-i21640771.scope` exited 0: 575 tests across 47
executables (transaction integration 93/100.94 s), Clippy 0.42 s and docs 1.22 s. Preflight:
32 GiB available RAM and 5.6 GiB free swap; no scope peak is claimed. Exact command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d150-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-txn --no-deps --locked --offline'
```

Next implement bounded reverse packed traversal for historical disk-domain lookup, then packed
graph integration. T-20/T-19, complete I/O accounting and qualifying BM-01/BM-06 remain open.
Native standalone remains last tested at `42f9abb`; M1 stays pinned. Earlier next-step paragraphs
below are historical increment handoffs, superseded by this section.

## Prior verified increment — streamed packed recovery (Decision 0149)

Implemented on pushed `c2e8852` plus this increment: cold recovery consumes the exclusive owner
with an admitted published historical primary/quota pair and ready base reducer. One authenticated
transaction/preparation advances private metadata and receipt-bound state at a time, with no
suffix outcome/owner maps. Certified result digests, exact range exhaustion, unchanged commitment
profiles and terminal ready-state validation precede both terminal publications. Only the complete
pair installs; current bases return without I/O or slot rotation. Caller-supplied domain residency
is not qualified by this coordinator-only bound.

Focused session 94284 / scope `run-p635510-i21612791.scope` passed eight selected tests (six new
recovery cases plus two existing maintenance cases) in 25.25 s and Clippy in 0.39 s. Tests include
1/2/64-certificate windows, zero overlay capacity, exact retry and cold accounting, exact/minus-one
range budgets, 702 injected I/O/error/crash cases with restart, authenticated retry/transaction
collisions and false results, five late reducer/hook/anchor/profile failures, and suffix/derived
ciphertext corruption. Failed recovery publishes no intermediate root or partial live state.
An initial Clippy needless-borrow error after extracting shared admission was fixed before these
passes. The shared work aggregation and root validation were reviewed against the prior paths.

Full gate passed in session 27908 / scope `run-p636146-i21647906.scope`, exit 0: 566 tests across
47 executables (transaction integration 84/97.70 s), Clippy 0.07 s and docs 1.17 s. Format, diff
and documentation checks passed (224 documents, 152 definitions). Preflight: 32 GiB available RAM /
5.6 GiB free swap; sampled scope peak 1,886,171,136 bytes / zero swap. Exact full-gate command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d149-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-txn --no-deps --locked --offline'
```

Decision 0150's
restricted authorized packed-metadata reads are next; that contract draft and the unregistered
`authorized_packed.rs`, `packed_coordinator/reads.rs`, quota `live_reads.rs` and `packed_live_reads.rs` test drafts are excluded
from this increment. T-20/T-19, packed disk-domain integration, complete I/O accounting and qualifying
BM-01/BM-06 remain open. Native standalone remains last tested at `42f9abb`; M1 stays pinned.

## Latest verified increment — paired packed live rebase (Decision 0148)

Implemented on pushed `bdf0640` plus this increment: explicit live rebase reuses the authenticated
certificate-window cursor and packed staging through internal borrowed-journal helpers. The cursor
algorithm was mechanically compared with `bdf0640` and is unchanged apart from whitespace. No
exclusive journal ownership moves or reopens. Each suffix outcome/transaction is checked against
the bounded live overlays; exact first-owner correspondence and quota pairing precede terminal
publication. Only both durable roots install and clear overlays. Failed attempts retain all exact
retries and block fresh writes until successful rebase. Work counters are partial successful
primitive work, not complete adapter/physical I/O.

Five focused tests passed in session 71453 / scope `run-p629349-i21601091.scope` (18.69 s), then
workspace Clippy (0.39 s): repeated two-outcome/one-owner overlays, first-owner charges, cold
admission and retries, exact suffix byte/group limits, corruption in suffix/primary/quota and
four ready-domain/profile refusals. All 738 observed read/write/crash rebase cases retained the
old installed pair and recovered identical terminal commitments. An initial fault expectation
incorrectly required ResourceLimit after a simulated adapter crash: metadata reads occur before
the fresh-write ceiling. The corrected test requires the storage refusal for crashes, ResourceLimit
for noncrash faults, and no new writes in either case. A patch delimiter error was fixed before
the final focused pass; no engine requirement or fault boundary was removed.

Full gate passed in session 88673 / scope `run-p629898-i21619995.scope`, exit 0: 560 tests across
47 executables (replay 50/111.46 s; transaction integration 78/68.12 s), Clippy 0.08 s and docs
1.15 s. `cargo fmt --all -- --check`, diff checks and `python3 scripts/check_docs.py` passed
(223 documents, 152 definitions). Preflight showed 32 GiB available RAM / 5.6 GiB free swap;
sampled scope peak 1,631,547,392 bytes / zero swap. Exact full-gate command:

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d148-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-txn --no-deps --locked --offline'
```

Next implement Decision 0149 streamed packed recovery;
that draft, unregistered `packed_coordinator/recovery.rs` and `packed_recovery.rs` tests are
excluded next-package work.
T-20/T-19 remain open; no native/M1 interface switch or qualifying campaign ran.

## Latest verified increment — packed live coordinator (Decision 0147)

Implemented on pushed `95b6b03` plus this increment: a separate raw packed coordinator consumes
the recovery owner, matches published primary/quota receipts and independently validated ready
domain claims, and starts with empty bounded overlays. Shared authoritative admission now selects
run-backed or packed historical reads. Exact retries, expiry, collision, original-owner reference
binding, cancellation, external preparation and uncertainty keep the existing commit state machine.
Raw upload staging remains privileged and is not a consumer quota/authorization facade.

Six focused tests passed in session 30239 / scope `run-p622055-i21611950.scope` (1.19 s), followed
by warnings-denied workspace Clippy (0.34 s). Coverage includes zero/one-owner overlay limits,
seven rejected installation variants, corruption in all three read families, exhausted lookup
budgets, prepared binding/second cancellation and 60 injected read/write/crash cases. Each fault
restarts against an independent small-fixture full-replay oracle and cold-admits the unchanged
old packed pair. The full-map test oracle is not part of the live implementation. Initial compile
errors in imports, field names and test fixture API usage were corrected before passing tests.

Full workspace verification passed in session 55215, scope `run-p622532-i21639292.scope`,
exit 0: 555 tests across 47 executables, Clippy (0.07 s) and docs (1.14 s). Retained log:
`/tmp/uste-d147-workspace-verification.log`. Exact command (preceded by `cargo fmt --all -- --check`):

```sh
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d147-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-txn --no-deps --locked --offline'
```

Preflight showed 32 GiB available RAM / 5.6 GiB free swap; sampled scope peak 1,646,514,176 bytes
and zero swap. `python3 scripts/check_docs.py` passed (222 links/documents, 152 definitions), as did
format and diff checks. Next implement Decision 0148 packed live metadata rebase; its draft is
excluded from this increment. No native/M1 interface switch, benchmark qualification or task
completion is claimed. T-20/T-19 and the full roadmap remain open.

## Latest verified increment — bounded packed quota rebuild (Decision 0146)

Implemented on pushed `a3c0ffa` plus this increment: quota reconstruction streams an independently
admitted primary owner index, sorts/aggregates only 1–512 owners per private batch, and returns a
paired quota prefix only after exact cursor exhaustion and owner cardinality. Intermediate totals
are not publishable. Exact aggregate lookup and batch limits remain explicit; reports do not
claim complete physical I/O or resident-memory measurement. Five focused tests passed, including
empty/populated partition independence, the exact 512-owner batch, corruption/foreign-owner
rejection and 318 injected fault cases with cold restart. No v1/native/M1 behavior changed.

Final verification on this tree (excluding the unimplemented Decision 0147 draft):

```sh
cargo fmt --all -- --check
systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '
set -o pipefail
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 2>&1 | tee /tmp/uste-d146-workspace-verification.log &&
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings &&
CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-txn --no-deps --locked --offline'
```

Session 60206 / scope `run-p615689-i21619315.scope` exited 0: 549 tests across 47 executables,
Clippy and docs passed. Preflight: 32 GiB available RAM and 5.6 GiB free swap; no final peak claimed.
One Cargo job/test thread and one heavy workload were used. Native standalone last tested version
remains `42f9abb`; no qualifying campaign ran. T-20/T-19 remain open. Next implement Decision 0147
live packed coordinator admission and shared bounded commit reads, then packed rebase and domain /
authorized integration. Full graph-state-v1 scans and incomplete I/O accounting still preclude
larger-than-memory qualification. This current next action supersedes older chronological entries.

Latest completed task is T-68; M1 is complete at exact implementation `b9689f3`. Decision 0055 selects verified recovery commit `7393def` and freezes
the executable `memory-pilot-v1` limits in the new safe-Rust `uste-memory` crate. Implementation
commit `97537e5` adds durable source/fact admission, bounded authorized retrieval, exact citations,
corrections, revocation and fail-closed rebuild under Decision 0056. Commit `a07bdec` adds the
Decision 0057 restricted embedded Linux adapter, consumer-owned versioned source/outbox contract
and runnable offline demo. Decision 0058 records the passing M1-A–J matrix, frozen-limit release
measurements and exact consumer handoff. Work now resumes T-20 and then T-19; full-project tasks and
release gates remain open.
Branch history
through the current handoff adds T-20's encrypted disk-index, authorized-read, bounded checkpoint transport,
deterministic benchmark-fixture foundations, bounded graph deltas/reverse dependencies and the
complete certificate-anchored `graph-state-v1` root plus bounded semantic reconstruction. The
certificate-paired coordinator metadata increment adds an executable cold root-to-seeded-open path.
Decision 0032 adds authenticated bounded base/delta scratch merge, and Decision 0033 connects one
bounded graph transaction to an independently validated terminal `graph-state-v1` root. The
Decision 0034 makes ordinary graph/spatial/composite ingest preparation request-bounded.
Decision 0035 adds a bounded explicit-I/O current-record proof phase followed by storage-free graph
preparation. Decision 0036 adds complete bounded history/reverse proof buckets so every graph
operation and precondition variant can use that phase. Decision 0037 carries authenticated
metadata/counts into a proof-derived terminal-root plan. Decision 0038 binds
that proof result into the authoritative coordinator commit without repeating full-state
preparation. Decision 0039 adds explicit authorized cache/I/O measurement. Decision 0040 connects a
capped, nonqualifying production-engine equivalence driver to the pinned fixture and oracle.
Decision 0041 streams exact-profile construction through bounded transactions and pins its
212-revision plan. Decision 0042 adds resumable Linux/Btrfs materialization and authenticated
portable-recovery open phases. Decision 0043 adds a separate bounded oracle summary and Linux
correctness-query phase. Decision 0044 adds real durable-prefix SIGKILL/resume coverage. Decision
0045 adds the independent warm-up/measured oracle bundle. Decision 0046 adds repeated paired-cache
sampling without claiming a benchmark pass. Decision 0047 adds a preemptive persistent-worker query
deadline for the CLI. Decision 0048 removes the complete snapshot from proof-derived terminal-root
publication. Decision 0049 returns that admitted root directly for the next disk preparation.
Decision 0050 adds the resumable cursor and predecessor proof needed for cold admission. Decision
0051 uses them to semantically admit a cold `GraphDiskBase` without rebuilding complete graph maps.
Decision 0052 replaces the complete graph in the warm authoritative write loop with one admitted
base and one bounded pending terminal-root plan. Decision 0053 reopens that state at an authenticated
journal frontier as either the ready base or exactly one revalidated pending suffix, without
reconstructing `GraphState`. Decision 0059 at `8885da4` removes the hidden absolute-maximum root
pre-scrub from graph recovery: fixed manifest discovery now precedes one caller-bounded semantic
scan. Decisions 0060–0073 add the opt-in disk coordinator, bounded overlays and maintained
first-reference roots; Decisions 0074–0086 connect native development construction, recovery,
supervised queries, profile-derived work limits and partial adapter/index I/O observation.
Decisions 0087–0103 add the 10,000-entity native development profile, bounded cache refinements,
multi-revision private recovery stages, map-free certificate/blob catalog recovery, bounded
inventory publication and authorized upload charge transfer. These capabilities are locally
verified, not qualifying benchmark results. T-20 remains open
pending remaining scalability/accounting work, larger-than-memory qualification and qualifying
BM-01/BM-06 results. Review was
performed by Codex agents and does not represent independent external security certification.

T-49 is complete at its typed R1 transaction-contract scope. A T-19 audit found that its required
BM-01/BM-06 results depend on T-20, while T-20 incorrectly depended on T-19. Decision 0024 preserves
every budget and orders T-20 first; T-19 and R1 acceptance remain open. T-62 remains an independent,
unverified external distribution prerequisite.

## Completed this increment

- Implemented on pushed `779ad82` plus this increment: [Decision 0145](docs/decisions/0145-packed-quota-cold-admission.md)
  independently admits packed quota roots against the journal-validated primary prefix. Canonical
  family admission precedes a bounded principal/blob cursor, exact primary-owner comparisons,
  checked per-principal groups and namespace totals. Cardinality excludes extra aggregates/owners;
  missing or failed projections never become zero. Explicit no-I/O tree binding checks reject
  foreign/locked primary capabilities even for empty ledgers, including raw quota usage.
  Four tests cover empty/populated cold pairs, exact/minus-one aggregate lookup and cursor bounds,
  seven authenticated false projections, owner ciphertext corruption and 381 read error/crash
  cases (42 OpenExisting, 42 Metadata, 43 ReadAt boundaries, three actions). Restart admits the exact
  old pair. Existing storage tests additionally exercise explicit locked/unlocked/reopened binding
  validation without I/O. Focused scope `run-p608586-i21610995.scope`: four tests/13.90s and strict
  workspace Clippy/0.28s passed. Final scope `run-p609213-i21599893.scope` exited 0 under 3G/4G/512M,
  one job/test thread: `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1
  CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test
  --workspace --all-targets --all-features --locked --offline -- --test-threads=1` passed
  544 tests/47 executables (replay 50/115.72s). `CARGO_BUILD_JOBS=1 cargo clippy --workspace
  --all-targets --all-features --locked --offline -- -D warnings` passed/0.30s;
  `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-storage -p uste-txn --no-deps
  --locked --offline` passed/2.52s. Log `/tmp/uste-d145-workspace-verification.log`; preflight
  32 GiB RAM/5.6 GiB swap, sampled peak 2,964,242,432 bytes/zero swap (not final peak).
  Formatting, diff/docs/task checks passed. Native remains `42f9abb`; no qualification,
  M1/lockfile change or task completion. Unregistered quota rebuild implementation/test and
  Decision 0146 drafts are excluded next-package work. Next verify populated/owner-free bounded
  quota rebuilding, then live/domain integration, complete accounting and qualifying campaigns.

- Implemented on pushed `48f7531` plus this increment: [Decision 0144](docs/decisions/0144-packed-first-owner-quota-construction.md)
  adds a separately profiled three-family packed quota prefix. Exact target retry binding precedes
  even empty-inventory construction; first-owner witnesses select charges, checked arithmetic
  preserves zero-byte owners, and bounded copy-on-write updates maintain the head, principal totals
  and principal/blob ordering. Usage rechecks logical primary pairing and authenticates head and
  principal reads; missing accounting is not synthesized as zero. No intermediate root publishes.
  Four integration tests and one arithmetic/framing unit test cover empty and zero-byte ledgers,
  new/existing principals, repeated references, mismatched/missing/foreign bases, narrow reads,
  corrupt head/principal pages and 351 staging faults (34 each OpenExisting/Metadata/ReadAt,
  three each CreateNew/WriteAt/SetLen/SyncAll/SyncDirectory, three error/crash actions). Restart
  reconstruction matches all quota commitments and exact first-owner charges. Strict Clippy's
  test-module placement finding was fixed without suppressing the lint.
  Focused scope `run-p598851-i21599339.scope` passed 1 unit/4 integration tests (7.71s) and
  workspace Clippy/4.53s. Final scope `run-p599802-i21596367.scope` exited 0 under 3G/4G/512M,
  one job/test thread: `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1
  CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test
  --workspace --all-targets --all-features --locked --offline -- --test-threads=1` passed
  540 tests/47 executables (replay 50/117.46s). `CARGO_BUILD_JOBS=1 cargo clippy --workspace
  --all-targets --all-features --locked --offline -- -D warnings` passed/0.06s;
  `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-txn --no-deps --locked --offline`
  passed/1.18s. Log `/tmp/uste-d144-workspace-verification.log`; preflight 31 GiB RAM/4.0 GiB swap,
  sampled peak 1,691,549,696 bytes/zero swap (not final peak). Formatting, diff/docs/task checks pass.
  Native remains `42f9abb`; no qualification, M1/lockfile change or task completion. Unregistered
  quota admission implementation/test and Decision 0145 drafts are excluded next-package work.
  Next independently admit cold quota projections, then populated rebuild and live/domain
  integration, complete accounting and qualifying campaigns.

- Implemented on pushed `a37c40c` plus this increment: [Decision 0143](docs/decisions/0143-packed-coordinator-cold-admission.md)
  adds read-only canonical and journal-correspondence admission of the four packed coordinator
  families. Exact retry/transaction cardinalities and per-revision comparisons exclude collisions
  and extras. Owner/witness lookups plus the distinct-first-reference tally establish original
  ownership without complete maps or repeated per-owner journal passes. Separate certificate,
  family, journal and aggregate lookup limits fail closed; no partial prefix or index write escapes.
  Five tests cover cold reconstruction, exact/minus-one budgets, seven authenticated false metadata
  variants, malformed family shape, late certificate corruption, historical-base suffix advancement
  and 798 read error/crash cases (84 OpenExisting, 87 Metadata, 95 ReadAt boundaries, three actions).
  Every injected failure reopens and admits the same exact commitments. Focused scope
  `run-p580015-i21567166.scope` passed the first four tests/13.31s and strict Clippy/6.33s.
  Final scope `run-p580763-i21598407.scope` exited 0 under 3G/4G/512M, one job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 535 tests/47 executables.
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings` passed/0.29s; `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings"
  cargo doc -p uste-txn --no-deps --locked --offline` passed/1.15s. Log:
  `/tmp/uste-d143-workspace-verification.log`; preflight 33 GiB RAM/4.0 GiB swap; sampled peak
  1,655,091,200 bytes/zero swap (not final peak). Formatting, diff/docs/task checks passed.
  Native remains `42f9abb`; no qualification, M1/lockfile change or task completion. Unregistered
  `packed/quota.rs` and Decision 0144 are excluded next-package work. Next implement packed
  first-owner quota construction/admission, then live/domain pairing, accounting and qualification.

- Implemented on pushed `b670c9c` plus this increment: [Decision 0142](docs/decisions/0142-packed-coordinator-prefix-construction.md)
  adds the opt-in four-family packed coordinator prefix: exact retry outcomes, cross-principal
  transaction-ID collision lookup, first blob owners and first-reference witnesses. Construction
  advances one authenticated transaction from an opaque predecessor, starting only at revision one;
  it retains four handles and request-sized deltas, not complete metadata maps. No roots publish.
  Six tests cover 32 reference transactions, unchanged first owners, authenticated retry/transaction
  collisions, gaps/foreign-owner/limit rejection, late owner corruption, exact 512-reference
  admission and pre-I/O rejection of 513, and 123 observed error/crash cases. Every fault preserves
  prior metadata and restart reconstruction matches all four logical commitments.
  Initial compile fixed borrowed blob-ID bytes and the raw-journal fixture constructor. The
  reference fixture initially used unwindowed certificate recovery and exhausted its 2,000,000-byte
  allowance; selecting the supported 64-certificate window preserved that allowance. A write-count
  assertion was corrected from three nodes to two: copy-on-write retains the old leaf.
  Focused scope `run-p575488-i21594510.scope` passed six tests/2.88s and Clippy/0.24s.
  Final scope `run-p575968-i21555396.scope` exited 0 under 3G/4G/512M, one job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 530 tests/47 executables (replay 50/115.85s,
  storage 202/26.47s, coordinator 49/3.10s). `CARGO_BUILD_JOBS=1 cargo clippy --workspace
  --all-targets --all-features --locked --offline -- -D warnings` passed/0.28s;
  `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-txn --no-deps --locked --offline`
  passed/1.10s. Log `/tmp/uste-d142-workspace-verification.log`; preflight 33 GiB RAM/4.0 GiB swap,
  sampled peak 1,656,320,000 bytes/zero swap (not final peak). Formatting, diff/docs/task checks passed.
  Native baseline remains `42f9abb`; no benchmark/M1/lockfile change or task completion. Unregistered
  packed admission implementation/test and Decision 0143 drafts are excluded next-package work.
  Next complete bounded journal-correspondence cold admission, quota pairing and live/domain
  integration, then accounting and qualification. The private prefix is not an installed coordinator.

- Implemented on pushed `3197fe9` plus this increment: [Decision 0141](docs/decisions/0141-scoped-packed-root-publication.md)
  adds namespace-fixed packed-root publication/discovery to ordinary and recovery maintenance.
  Current discovery authenticates the frontier; historical recovery discovery takes an explicit
  revision and separate certificate/manifest limits. Storage rejects intermediate publication
  before creation. Three tests cover current-only discovery, wrong-certificate rejection, cold
  reopen/admission of unchanged historical packs, old-owner rejection, empty families and all
  scope-bound operations (including sticky cursor rejection), without claiming domain validation.
  Final scope `run-p571919-i21407265.scope` exited 0 under 3G/4G/512M, one job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-txn --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 65 tests/3 executables (43 coordinator/0.54s).
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings` passed/0.22s; `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings"
  cargo doc -p uste-txn --no-deps --locked --offline` passed/1.11s. Log:
  `/tmp/uste-d141-txn-verification.log`. Preflight 33 GiB RAM/4.0 GiB swap. Full workspace baseline
  remains D0140's 521/47; native remains `42f9abb`. No benchmark/M1/lockfile change or task completion.
  Next implement bounded packed coordinator metadata prefix construction and journal-correspondence
  admission, then live/domain integration, complete accounting and qualification.

- Implemented on pushed `dbdf4a1` plus this increment: [Decision 0140](docs/decisions/0140-scoped-packed-index-maintenance.md)
  adds exclusive namespace/certificate-target maintenance to ordinary coordinator and authenticated
  recovery paths. Retained proofs are reused without I/O, never silently replaced after rejection;
  unbound transactions require explicitly bounded certificate authentication. Admission, lookup,
  staging and sticky-error cursors reject future revisions or other namespaces before tree I/O.
  No root publication or consumer authorization is exposed by the private staging handle.
  Three tests cover chained historical stages, foreign-owner/future-tree refusals, cursor poisoning,
  exact retries/transaction collisions, certificate-budget refusal and all three applicable
  certificate ReadAt error/crash cases. Initial fixture compilation was corrected to use public
  value/delta APIs and the Metadata fault name. The fault test initially assumed certificate
  reopens/metadata calls; inspection confirmed its retained handle performs only ReadAt, and the
  final test injects every applicable operation rather than assuming nonexecuted faults occurred.
  Focused scope `run-p566893-i21566447.scope`: 3 tests/0.01s and strict workspace Clippy/5.13s passed.
  Full scope `run-p567468-i21566472.scope` exited 0 under 3G/4G/512M, one job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 521 tests/47 executables (replay 50/115.82s).
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings` passed/0.06s; `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings"
  cargo doc -p uste-txn --no-deps --locked --offline` passed/1.10s. Log:
  `/tmp/uste-d140-workspace-verification.log`. Preflight 33 GiB RAM/4.0 GiB swap; sampled peak
  1,556,697,088 bytes/zero swap, not final peak. Formatting, diff, documentation/task checks passed.
  No native matrix rerun (baseline `42f9abb`), benchmark/M1/lockfile change or task completion.
  Unregistered `packed_root_maintenance.rs`, `packed_roots.rs` test draft and Decision 0141 are
  excluded next-package work. Next connect scoped root discovery/terminal publication, then
  versioned domain integration, complete accounting and qualification.

- Implemented on pushed `687be92` plus this increment: [Decision 0139](docs/decisions/0139-certificate-bound-packed-tree-capabilities.md)
  adds opaque live-owner canonical packed-tree admission, lookup, sticky-error cursors and private
  certificate-targeted copy-on-write staging. Historical base capabilities survive same-owner
  appends, not reopen/foreign/poisoned owners; staging requires a current-frontier proof. Missing
  families, scope/profile/family/newer-base mismatches and locked keys fail closed, even for empty
  trees or completed cursors. This is not domain validation or consumer authorization.
  Six tests cover chained private stages, terminal publication/reopen, bounded admission, late
  corruption, owner and key rejection, 81 admission faults and 42 staging faults. An initial
  corruption test incorrectly expected a page not yet visited to fail the next cursor step;
  correction checks failure on the first actual damaged-page read and sticky failure thereafter.
  Final scope `run-p560432-i21515953.scope` exited 0 under 3G/4G/512M, one job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 518 tests/47 executables.
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings` passed/6.60s; `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings"
  cargo doc -p uste-storage --no-deps --locked --offline` passed/1.12s. Test log:
  `/tmp/uste-d139-workspace-verification.log`. Preflight 33 GiB RAM/4.0 GiB swap; no final peak
  measurement retained. No native matrix rerun (latest native baseline `42f9abb`), qualification,
  M1/lockfile change or task completion claimed. Next implement scoped coordinator/recovery
  maintenance, then versioned domain integration, complete accounting and qualifying campaigns.

- Implemented on pushed `e64d5b8` plus this increment: [Decision 0138](docs/decisions/0138-append-only-certified-packed-roots.md)
  adds opt-in immutable revision/attempt root slots, exact current-certificate publication and
  bounded proof-owned discovery without resident certificate history. Up to 64 fixed slots are
  admitted; publication never overwrites/deletes a fallback or scans a tree. Returned handles bind
  certificates only, leaving canonical/domain admission and authorization explicit.
  Seven tests cover nonempty packed content, same/next-revision fallback retention, cold disk-only
  discovery, pre-I/O binding/poison/key refusals, all 64 slots/267,328 bytes, exact/minus-one reads,
  seven malformed/context-swapped candidate classes, 18 injected read failures and 21 write boundary
  cases (20 actual injected failures plus one explicit harness no-op: CrashAfter an AlreadyExists
  result cannot inject a crash because the adapter operation failed). Every actual failure retains
  the old root and permits a bounded retry after restart. An initial test assumed that no-op was
  an injected failure; inspection of the harness corrected the expectation without weakening the
  real failure checks. Earlier compilation fixed fixture API/borrowing and adapter-error matching;
  strict Clippy fixes removed clone-on-Copy and a manual range pattern.
  Final scope `run-p554628-i21523733.scope` exited 0 under 3G/4G/512M, one job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 512 tests/47 executables, no failures/ignores
  (replay 50/115.59s). `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features
  --locked --offline -- -D warnings` passed/2.02s; `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings"
  cargo doc -p uste-storage --no-deps --locked --offline` passed/1.10s. Supplemental log:
  `/tmp/uste-d138-workspace-verification.log`. Preflight 33 GiB RAM/4.0 GiB swap; sampled scope peak
  1,714,601,984 bytes/zero swap (not final peak). Formatting, diff, documentation and task checks
  passed. No native matrix rerun (latest native baseline `42f9abb`), benchmark, M1/lockfile change
  or task completion claimed. Unregistered `journal_packed_trees.rs` and Decision 0139 are excluded
  next-package work. Next connect opaque canonical-tree admission/read/staging capabilities to
  the journal, then domain/facade integration, complete accounting and qualification.

- Implemented on pushed `d5f866b` plus this increment: [Decision 0137](docs/decisions/0137-packed-storage-process-loss-evidence.md)
  adds a real Linux/Btrfs process-loss test spanning four owned-child SIGKILL boundaries: first
  successful pack write before sync, complete pack sync without a manifest, partial synced test
  manifest, and complete manifest/file/directory sync. A new adapter/vault reopens the exact old
  tree at every boundary; only the complete new manifest recovers the exact changed tree with
  unchanged old-pack links. Synthetic claims/key wrapping/names do not grant production authority.
  The readiness pipe has a ten-second parent timeout; the child exits on parent-pipe EOF and an
  RAII guard kills/reaps only the owned child. Cleanup is restricted to known files in each uniquely
  created test directory. No engine runtime code changes in this increment.
  Scope `run-p550554-i21553750.scope` exited 0 under 3G/4G/512M, one job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-storage --test packed_process_restart
  --locked --offline -- --test-threads=1` passed one test/all four process boundaries in 0.13s;
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings` passed/0.10s. Formatting, diff, documentation and task checks passed. Preflight
  33 GiB RAM/4.0 GiB swap. `findmnt -T . -o FSTYPE,SOURCE,OPTIONS` confirms Btrfs on NVMe with
  `compress=zstd:1`; ext4, power-loss behavior and other platforms were not exercised. The full
  workspace baseline remains the 504-test/46-executable D0136 gate, not a newly claimed full run;
  native baseline remains `42f9abb`. No benchmark, M1/lockfile change, task completion or production
  root publication is claimed. Next implement certificate-bound packed-root publication/discovery
  without full-family rewriting, then domain integration, complete accounting and qualification.

- Implemented on pushed `a5e9a40` plus this increment: [Decision 0136](docs/decisions/0136-packed-root-manifest-framing.md)
  adds separately versioned encrypted packed-root manifests with fixed 2,048-byte plaintext,
  4,161-byte envelopes, up to sixteen sorted family commitments/locators and explicit state-commitment
  profile framing. Certificate/reducer/state claims remain raw data, not admission or commit
  authority. Existing locator encoding now shares the byte-identical fixed-array encoder.
  Four new tests pin literal fields, all slots, every plaintext/envelope truncation and ciphertext
  byte mutation, reserved/unused bytes, malformed claims/locators, context/key/format/sequence
  substitution and locked/pre-crypto refusal. The model transport test syncs two synthetic manifests,
  restarts, validates and queries both old and copy-on-write trees, including reused old-pack links.
  Synthetic names and state/certificate claims are explicitly not a production publication protocol.
  Focused scope `run-p545088-i21526644.scope` passed four tests/0.03s and strict Clippy/6.24s.
  Final scope `run-p546168-i21526686.scope` exited 0 under 3G/4G/512M, one job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 504 tests/46 executables, no failures/ignores
  (replay 50/115.76s). `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features
  --locked --offline -- -D warnings` passed/1.12s; `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings"
  cargo doc -p uste-storage --no-deps --locked --offline` passed/1.14s. Supplemental log:
  `/tmp/uste-d136-workspace-verification.log`. Preflight 33 GiB RAM/4.0 GiB swap; sampled scope peak
  1,707,024,384 bytes/zero swap (not final peak). Formatting, diff, documentation and task checks
  passed. No native matrix rerun (latest native baseline `42f9abb`), benchmark, M1/lockfile change,
  production publication or task completion claimed. Unregistered `tests/packed_process/support.rs`
  and Decision 0137 are excluded next-package work. Next verify real Linux process loss, then
  journal/domain admission, production publication/discovery/recovery, complete accounting and
  qualifying campaigns.

- Implemented on pushed `5c9d43a` plus this increment: [Decision 0135](docs/decisions/0135-bounded-packed-tree-cursor.md)
  adds a raw streaming packed-tree range cursor with compressed-prefix lower-bound seek, ordered
  successors, exact terminal root proofs and complete value verification. Cumulative candidate,
  returned-byte and page/encoded-byte budgets are enforced; errors permanently poison continuation.
  Conservative path/proof/key metadata reservation is capped at 32 MiB, separately from value and
  crypto/page buffers. The existing lookup reader is reused without algorithm changes.
  Seven tests cover every one-byte lower bound with multiple upper/prefix bounds, skipped-prefix
  seeks, empty/singleton/16 MiB values, hard/exact/minus-one limits, 117 observed read/error/crash
  cases, late content corruption after successful entries, wrong scope and sticky terminal states.
  A 256-key common-prefix tree's final-key seek uses ten reads and one candidate, not a preceding
  keyspace scan. The mixed range fault fixture pins 13 reads/117 cases; a full seven-key traversal
  uses 21 reads. Focused scope `run-p538822-i21522622.scope` passed six tests/1.69s and strict
  Clippy/1.86s before adding the metadata-geometry test. Earlier tests passed but strict Clippy
  rejected a complex test-helper return type; a named alias fixed it without suppressing the lint.
  Final scope `run-p539469-i21507284.scope` exited 0 under 3G/4G/512M, one job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 500 tests/46 executables, no failures/ignores
  (replay 50/115.63s). `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features
  --locked --offline -- -D warnings` passed/6.22s; `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings"
  cargo doc -p uste-storage --no-deps --locked --offline` passed/1.09s. Supplemental local log:
  `/tmp/uste-d135-workspace-verification.log`. Preflight 33 GiB RAM/4.0 GiB swap; sampled scope peak
  2,168,717,312 bytes/zero swap (not final peak). Formatting, diff, documentation and task checks
  passed. No native matrix rerun (latest native baseline `42f9abb`), benchmark, M1/lockfile change,
  root authority or task completion claimed. Unregistered `packed_root_manifest.rs` and Decision
  0136 are excluded next-package work. Next implement separately versioned packed-root manifests,
  then journal/domain admission, publication/recovery, Linux process checks, complete accounting
  and qualifying campaigns.

- Implemented on pushed `041cb89` plus this increment: [Decision 0134](docs/decisions/0134-streaming-packed-tree-validation.md)
  adds complete streaming packed-tree structural/content validation with one prior key, a bounded
  iterative path and incremental exact-length value hashing. Every reachable node and chunk is
  authenticated; subtree boundary bits, leaf routing, strict key order and aggregate counts/bytes
  are checked before a receipt can escape. This receipt is not domain or journal admission.
  Seven new tests cover generated reference trees, 16 MiB values, empty/singleton roots,
  exact/minus-one and hard admission, 189 observed read/error/crash cases, authenticated wrong
  partitions, late chunk/leaf corruption and cycles. The seven-key fixture pins 13 nodes,
  eight chunks and 21 reads/431,445 encoded bytes. Streaming hash partitions match whole-value
  commitments, with short/overlong input refusing completion. Initial test compilation required
  unwrapping the validated fault plan; the first run then caught a stale model directory handle
  reused after restart. Both test defects were fixed; production checks were not weakened.
  Focused scope `run-p532665-i21495233.scope` passed seven tests/0.70s and strict Clippy/9.28s.
  Final scope `run-p533323-i21495346.scope` exited 0 under 3G/4G/512M, one job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 493 tests/46 executables, no failures/ignores
  (replay 50/114.67s). `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features
  --locked --offline -- -D warnings` passed/0.05s; `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings"
  cargo doc -p uste-storage --no-deps --locked --offline` passed/1.08s;
  `python3 scripts/check_ordered_commitment_vectors.py` passed all four independent vectors.
  Supplemental log `/tmp/uste-d134-workspace-verification.log`; preflight 33 GiB RAM/4.0 GiB swap,
  sampled scope peak 1,412,562,944 bytes/zero swap (not final peak). Formatting, diff, documentation
  and task checks passed. No native matrix rerun (latest native baseline `42f9abb`), benchmark,
  M1/lockfile change or task completion claimed. Decision 0135 is an excluded next-package draft.
  Next implement bounded range traversal, then independent manifests, Linux process checks,
  recovery/domain integration, complete accounting and qualifying campaigns.

- Implemented on pushed `702ab33` plus this increment: [Decision 0133](docs/decisions/0133-bounded-packed-tree-batches.md)
  adds bounded private copy-on-write batches with exact sorted preconditions, unchanged-subtree
  reuse, conservative 64 MiB metadata reservation and iterative final-reachable-node serialization.
  Nine new tests cover 32 generated reference batches, 512 deltas, 16 MiB values, exact 64 MiB
  input admission, late conflicts/corruption, output exhaustion and 87 observed error/crash cases.
  Updating one leaf of a 128-key tree writes eight nodes; deleting the entire tree writes none.
  The mixed fault fixture pins seven reads and four output pages. Focused scope
  `run-p525118-i21525488.scope` passed nine tests/3.69s. An earlier expanded run caught an incorrect
  predicted fault-case count (84 versus observed 87); all injected checks passed, and the assertion
  now pins 87. Unnecessary test qualifications were corrected without weakening lint checks.
  The earlier full gate's terminal result was unavailable after context recovery, so it was not
  used as evidence. Retained-log gate `run-p529669-i21522072.scope` exited 0 under 3G/4G/512M,
  one job/test thread: `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1
  CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test
  --workspace --all-targets --all-features --locked --offline -- --test-threads=1`;
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings`; `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-storage
  --no-deps --locked --offline`. Tests passed 486/46 executables, no failures/ignores; replay
  50/115.75s; Clippy/0.04s and docs/0.03s. Log `/tmp/uste-d133-workspace-verification.log` is
  supplemental local evidence, not required for reproduction. Preflight 33 GiB RAM/4.0 GiB swap;
  sampled scope peak 291,110,912 bytes/zero swap is not the final peak. Formatting, diff,
  documentation and task checks passed. No native matrix rerun (latest native baseline `42f9abb`),
  benchmark qualification, root authority, M1/lockfile change or task completion is claimed.
  Unregistered `packed_tree_validation.rs` and Decision 0134 are next-package work, excluded from
  this tested commit. Next: complete bounded structural/content validation, then independent
  manifests, range queries, recovery/domain integration, complete accounting and qualification.

- Implemented on pushed `d12fe7f` plus this increment: [Decision 0132](docs/decisions/0132-bounded-packed-tree-lookup.md)
  adds bounded authenticated packed-tree lookup with exact parent summaries, terminal key-route
  proof and complete value-hash verification before returning a zeroizing result. The separate
  linked reader admits integral bounded pack geometry; existing descriptor reads retain exact
  total-length enforcement. Nine new tests cover reference membership/absence/empty values,
  16 MiB exact value recovery, exact/minus-one admission, 63 observed read/error/crash cases,
  authenticated false leaf/chunk/cycle/partition/repeated-bit cases, context/ciphertext substitution
  and strict-versus-linked geometry. The three-chunk fixture pins seven reads/143,815 bytes,
  three branches and three value chunks. Focused scope `run-p517302-i21065428.scope` passed
  all 28 packed-storage tests/0.64s and strict workspace Clippy/1.03s.
  Final scope `run-p518087-i21525127.scope` exited 0 under 3G/4G/512M, one job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 477 tests/46 executables, no failures/ignores
  (replay 50/116.78s, storage 162/19.46s, coordinator 37/0.56s).
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings` passed/1.02s; `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc
  -p uste-storage --no-deps --locked --offline` passed/1.11s. Preflight 34 GiB RAM/3.9 GiB swap;
  sampled scope peak 1,649,205,248 bytes, zero swap (not a final peak). Documentation/task and
  diff checks passed. No native matrix rerun (latest native baseline `42f9abb`), benchmark,
  authorization/root admission, task completion, packed-tree Linux process qualification or
  M1/lockfile change is claimed. Decision 0133 is an uncommitted next-package draft, excluded
  from this tested increment. Next implement bounded reachable-node copy-on-write batches,
  then admitted manifests, recovery/domain integration, complete accounting and qualification.

- Implemented on pushed `a64fc19` plus this increment: [Decision 0131](docs/decisions/0131-packed-tree-record-contract.md)
  adds strict borrowed tree-node/value-chunk decoding, zeroizing bounded encoding, fixed physical
  locators and structurally bounded imported logical-summary claims. Seven tests pin literal bytes,
  every truncation/trailing byte, tags/flags, locator revisions/bounds, key/value/count/byte limits,
  canonical chunk partitioning, physical-layout-independent logical hashes and an encrypted-pack
  model restart with exact child/value commitments. The pack writer now exposes its framing context
  for typed encoding; this does not grant authorization or root admission. Focused scope
  `run-p511215-i21494013.scope` passed seven tests/0.00s and strict workspace Clippy/6.25s.
  Final scope `run-p511887-i21524703.scope` exited 0 under 3G/4G/512M, one job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 468 tests/46 executables, no failures/ignores
  (graph disk 24/25.63s, replay 50/116.40s, storage 153/19.77s, coordinator 37/0.55s,
  M1 process 2/2.74s). `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features
  --locked --offline -- -D warnings` passed/0.06s; `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings"
  cargo doc -p uste-storage --no-deps --locked --offline` passed/1.06s. Preflight 34 GiB RAM/
  3.9 GiB swap; sampled scope peak 1,550,884,864 bytes, zero swap, not a final peak.
  Documentation/task checks passed 206 links (including the next decision draft)/68 tasks;
  diff check passed. No standalone native rerun (last baseline `42f9abb`), benchmark, Linux
  packed-tree process qualification, root/domain admission, task completion or M1/lockfile change.
  Unregistered `packed_tree_lookup.rs` and Decision 0132 remain next-package drafts explicitly
  excluded from this tested commit. Next connect bounded authenticated lookup and whole-value
  checks, then reachable-node copy-on-write batches and independently admitted root recovery.

- Implemented on pushed `7a98146` plus this increment: [Decision 0130](docs/decisions/0130-bounded-immutable-index-packs.md)
  streams encrypted records into create-new immutable packs with explicit page/record/payload
  ceilings, sticky append errors and file-then-directory durable finish. Exact-context reads
  admit one page's bytes before I/O, verify file length and return only authenticated framing.
  Seven tests cover multi-page/slot rollover and model restart, exact/minus-one limits, 21 observed
  write/error/crash cases, nine read/error/crash cases, sticky failed rollover, short/zero/overreported
  I/O, corruption, entropy failure and identity collision without overwriting prior durable packs.
  Initial focused tests passed but strict Clippy caught an unused test import; removed it.
  Focused scope `run-p505810-i21520725.scope` passed seven tests/0.02s and strict Clippy/1.62s.
  Final scope `run-p506373-i21493673.scope` exited 0 under 3G/4G/512M, one job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 461 tests/46 executables, no failures/ignores
  (graph disk 24/25.56s, replay 50/116.39s, storage 146/19.84s, coordinator 37/0.55s,
  M1 process 2/2.73s). `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features
  --locked --offline -- -D warnings` passed/0.04s; `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings"
  cargo doc -p uste-storage --no-deps --locked --offline` passed/1.02s. Preflight 34 GiB RAM/
  3.9 GiB swap; sampled scope peak 1,537,912,832 bytes, zero swap (not a final peak).
  Documentation/task checks passed 204 links/68 tasks before the next draft; diff check passed.
  Standalone native tests were not rerun for this unused module; their latest baseline is
  `42f9abb`. No pack-specific Linux process qualification, typed tree/root admission, benchmark,
  task completion or M1/lockfile change is claimed. Unregistered `packed_tree_record.rs` and
  Decision 0131 are next-package drafts excluded from this tested commit. Next wire/verify typed
  records, then bounded authenticated traversal, reachable-node updates and root recovery.

- Implemented on pushed `3a2a361` plus this increment: [Decision 0129](docs/decisions/0129-encrypted-packed-index-pages.md)
  adds the separate encrypted packed-page carrier: one 16 KiB zeroizing builder, at most 128
  borrowed records, exact slot/padding validation and an object-format-2 encryption context.
  Five tests cover literal framing, exact/minus-one byte and slot capacity, atomic append
  refusal, every ciphertext-byte mutation/truncation, authenticated malformed headers/directories,
  context and v1 substitution, wrong key, lock and entropy refusal. An initial compile caught a
  test-only overlapping vault borrow; corrected before testing. Focused scope
  `run-p500356-i21493241.scope` passed five tests/0.32s and strict workspace Clippy/8.85s.
  Final scope `run-p501050-i21504416.scope` exited 0 under 3G/4G/512M, one job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 454 tests/46 executables, no failures/ignores
  (graph disk 24/25.67s, replay 50/116.30s, storage 139/18.29s, coordinator 37/0.54s).
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings` passed/0.04s; `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc
  -p uste-storage --no-deps --locked --offline` passed/1.02s. Preflight 34 GiB RAM/3.9 GiB swap;
  sampled scope peak 1,537,359,872 bytes/zero swap, not a final peak. Documentation/task checks
  passed 203 links/68 tasks before the following unregistered draft was added; diff check passed.
  No native matrix rerun for this unused framing module; latest native verification remains
  `42f9abb`. No benchmark, root publication, typed node admission, task completion or changed
  M1/lockfile is claimed. Unregistered `packed_index_pack.rs` and Decision 0130 are next-package
  drafts, explicitly excluded from this tested commit. Next wire and verify bounded durable pack
  writes/reads, then typed copy-on-write traversal and publication/recovery integration.

- Implemented on pushed `42f9abb` plus this increment: [Decision 0128](docs/decisions/0128-canonical-ordered-content-commitments.md)
  adds canonical ordered content commitments, bounded borrowed membership/absence proofs and exact
  compare-and-swap insert/replace/delete root transitions. This is a storage-free, opt-in primitive,
  not a persisted index or a new source of authority. Existing v1 formats, M1 and task status are
  unchanged. Six tests cover independent reconstruction, four cross-language hash goldens, all
  576 small insertion/deletion order pairs, 1,024 generated updates, all 65,536 single-byte key
  pairs, context/proof mutations, exact/minus-one admission and key/value boundaries.
  Final scope `run-p495078-i21473707.scope` exited 0 with one job/test thread and
  `MemoryHigh=3G MemoryMax=4G MemorySwapMax=512M`. Commands:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` (449 tests/46 executables, no failures/ignores;
  graph disk 24/25.62s, replay 50/116.31s, coordinator 37/0.56s, M1 process 2/2.72s),
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings` (0.92s), and `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc
  -p uste-storage --no-deps --locked --offline` (1.01s). Preflight headroom was 35 GiB RAM/
  3.9 GiB swap; sampled scope peak was 2,107,744,256 bytes, zero swap (not a final peak).
  `python3 scripts/check_ordered_commitment_vectors.py` passed four vectors;
  `python3 scripts/check_docs.py` passed 202 links; `python3 scripts/check_task_graph.py`
  passed 68 tasks; `git diff --check` passed. The standalone native matrix was not rerun for
  this unused pure module; its latest verified baseline remains `42f9abb` below. No benchmark
  ran or scalability qualification is claimed. Next implement a separately versioned bounded
  copy-on-write carrier and its admission, publication/recovery and domain integration.

- Implemented on pushed `fa3897a` plus this increment: [Decision 0127](docs/decisions/0127-bounded-certificate-proof-windows.md)
  authenticates fixed windows of at most 64 certificate receipts, reuses them for forward group
  reads and selects the windowed cursor in private coordinator recovery. Every selected certificate
  is reread against its receipt; shared byte accounting includes all acquisition reads. Existing
  nonwindowed and resident-anchor paths remain. Seven new tests cover independent subrange proofs,
  64/excess capacity, exact/minus-one bytes, rollover, inventories, foreign/stale owners, authentic
  forks, after-lookahead corruption and all observed read error/crash boundaries with restart.
  Focused scope `run-p487393-i21450076.scope` passed four storage tests/0.02s, three cursor tests/
  0.20s and strict workspace lint/6.55s. Initial checks corrected an unnecessary qualification,
  a path-module declaration and test-adapter rearm-before-restart ordering. The first full gate
  `run-p488181-i21450160.scope` exposed the graph test's old exact-byte cost. Its boundary is now
  pinned to three selected certificate/group pairs plus four proof/window certificates, rather
  than six independent suffix-proof certificates; exact success, minus-one failure and no partial
  publication remain mandatory. No benchmark target or admitted profile allowance was lowered.
  Final scope `run-p490591-i21492582.scope` exited 0 under 3G/4G/512M, one job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 443 tests/46 executables, no failures/ignores
  (graph disk 24/26.75s, replay 50/112.09s, storage 128/18.63s, transaction coordinator 37/0.56s,
  M1 process 2/2.70s). `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path
  experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1` passed 58 active units/
  45.17s, BM-01 process 3/12.10s, BM-06 CLI 2/0.50s and BM-06 process 8/73.08s; two prior
  exact-oracle ignores unchanged. `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets
  --all-features --locked --offline -- -D warnings` (0.31s), `CARGO_BUILD_JOBS=1 cargo clippy
  --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings`
  (1.80s), and `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-txn
  -p uste-storage --no-deps --locked --offline` (1.97s) pass. Preflight 35 GiB available RAM/
  3.9 GiB free swap; sampled peak 447,025,152 bytes/zero swap, not final whole-run peak.
  Formatting/whitespace, docs (201 links), task graph (68 tasks) pass. No M1 source/lock changes
  or qualifying campaign. Next address whole-family rewrite/global digest scaling with an explicitly
  separate versioned index design, preserving all v1 semantics and independent reference checks;
  complete accounting and reserved-host qualification remain required. T-20/T-19 remain open.

- Implemented on pushed `de67ff3` plus this increment: [Decision 0126](docs/decisions/0126-recovery-transaction-proof-reuse.md)
  retains the forward cursor's already-accounted certificate proof with its bounded transaction.
  Private staging reuses it without additional certificate reads. Both staging APIs reject a
  bound transaction from another/reopened owner before I/O and never fall back on invalid proof.
  Unbound fallback, content equality, shared budgets, fresh corruption refusal and sticky cursor
  failure remain tested. New tests cover all storage subranges in both modes and every observed
  disk cursor read error; full graph/primary/first-reference/quota publication fault suites pass.
  Focused scope `run-p478775-i21472903.scope` passed three new tests and strict workspace lint.
  Final scope `run-p480221-i21429109.scope` exited 0 under 3G/4G/512M, one job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 436 tests/46 executables, no failures/ignores
  (graph disk 24/26.40s, replay 50/108.80s, transaction coordinator 34/0.33s, M1 process 2/2.69s).
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passed 58 active units/44.85s, BM-01 process 3/11.89s,
  BM-06 CLI 2/0.50s, BM-06 process 8/73.42s; two prior exact-oracle ignores unchanged.
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings` and `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path
  experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings` pass;
  `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-txn -p uste-storage --no-deps
  --locked --offline` passes (1.92s). Preflight 35 GiB available RAM/3.9 GiB free swap; sampled
  peak 1,649,557,504 bytes/zero swap, not final whole-run peak. Formatting/whitespace, docs
  (200 links), task graph (68 tasks) pass. No M1 source/lock or qualification target changes.
  Next: bounded certificate authentication windows to reduce repeated forward suffix scans,
  then immutable-family construction scaling and complete accounting. Two unregistered window
  implementation/test drafts are deliberately excluded from this tested commit for that next
  package; they are not compiled or claimed verified. T-20/T-19 remain open.

- Implemented on pushed `746ca3d` plus this increment: [Decision 0125](docs/decisions/0125-native-paired-metadata-streaming.md)
  connects ordinary paired-base native/model recovery to private per-revision metadata staging.
  Both ready-open suffix ceilings are zero. Mismatched graph/metadata bases retain the existing
  bounded overlay path; live capacity for subsequent writes is unchanged. BM-01/BM-06 diagnostics
  distinguish these paths and capacities. Added assertions cover paired zero/one/multi-step
  suffixes, mismatched bases, retained-root repair and owned-child SIGKILL/resume. No workspace
  library or M1 source/lock changed from the 433-test verified `746ca3d` baseline.
  Initial native gate `run-p474925-i21449285.scope` exited 0. Final gate after BM-01 diagnostics,
  `run-p476520-i21472800.scope`, exited 0 under MemoryHigh=3G/MemoryMax=4G/MemorySwapMax=512M:
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passed 58 active units/45.55s, BM-01 process 3/11.95s,
  BM-06 CLI 2/0.50s and BM-06 process 8/74.16s; two prior exact-oracle ignores unchanged.
  `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets
  --locked --offline -- -D warnings` passes. Preflight 35 GiB available RAM/3.9 GiB free swap;
  sampled peak 320,430,080 bytes/zero swap, not final whole-run peak. Formatting/whitespace,
  docs (199 links) and task graph (68 tasks) pass. No qualifying benchmark; T-20/T-19 remain open.
  Next reduce duplicate certificate proof work in recovery staging while preserving exact owner,
  frontier, corruption and budget semantics; immutable-family rewrite scaling/accounting remain.

- Implemented on pushed `b3650cd` plus this increment: [Decision 0124](docs/decisions/0124-streamed-quota-recovery.md)
  maintains admitted quota and first-reference projections through private per-revision metadata
  staging, and provides private genesis quota candidates including the zero-owner head. Only new
  first owners add charges; exact repeated references preserve their principal. Missing quota
  roots and omitted projection preservation refuse, never imply zero usage. Eight new tests
  cover new/existing principals, zero-byte blobs, populated/empty and origin/published bases,
  bounded lookup refusal, cold aggregate admission, late corruption and optional-root durability.
  The latter fixes private first-reference/quota roots attached to published primary roots being
  omitted from the rebase-required guard, including zero suffixes. Separate tests isolate both.
  Fault schedules: 57 genesis failures, 909 suffix cases (906 failures/three optional no-crash
  boundaries), 804 publication cases (788 failures/sixteen optional no-crash boundaries).
  Initial compilation corrected explicit storage-error conversions and merge-report field names.
  Focused scope `run-p468765-i21472403.scope` exited 0: seven quota tests/59.84s and strict
  workspace lint/6.13s. Final scope `run-p469496-i21425565.scope` exited 0 under 3G/4G/512M,
  one Cargo job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 433 tests/46 executables, no failures/ignores
  (graph disk 24/26.30s, replay 50/117.40s, storage 123/18.68s, M1 process 2/2.73s).
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passed 58 active units/45.06s, BM-01 process 3/11.90s,
  BM-06 CLI 2/0.50s and BM-06 process 8/74.09s; two prior exact-oracle ignores unchanged.
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings` (4.16s), `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path
  experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings` (2.25s)
  and `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-txn --no-deps --locked
  --offline` (1.06s) pass. Preflight 35 GiB available RAM/3.9 GiB free swap; sampled peak
  1,684,643,840 bytes/zero swap, not final whole-run peak. Formatting/whitespace, docs (198 links)
  and task graph (68 tasks) pass. M1 sources/locks unchanged; no qualifying campaign.
  Next: apply streamed metadata to native paired-base recovery, then construction/recovery
  scaling and complete accounting before qualifying campaigns. T-20/T-19 remain open.

- Implemented on pushed `c8dff73` plus this increment: [Decision 0123](docs/decisions/0123-streamed-first-reference-recovery.md)
  preserves admitted first-reference evidence during private per-revision metadata staging and
  supplies bounded private genesis witnesses. Empty-owner bases may bootstrap; populated bases
  without admitted witnesses refuse before I/O. Quota projections still explicitly refuse.
  Five tests cross ordinary/origin and empty/populated bases with zero/three suffix steps, pin
  exact earliest revisions and 15 runs/33 entries/3,657 logical bytes, rebase with zero suffix
  allowances and independently cold-admit with one correspondence pass. Fault schedules cover
  27 genesis failures, 678 suffix cases (675 failures/three optional no-crash boundaries) and
  552 terminal-publication cases (540 failures/twelve optional no-crash boundaries).
  Focused scopes `run-p461588-i21448389.scope` (4 tests/17.13s),
  `run-p462479-i21389843.scope` (publication matrix/15.55s) and strict workspace lint
  `run-p462072-i21464476.scope` (6.17s) exited 0. Final scope
  `run-p462925-i21428100.scope` exited 0 under MemoryHigh=3G, MemoryMax=4G,
  MemorySwapMax=512M, one Cargo job/test thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 425 tests/46 executables, no failures/ignores
  (graph disk 24/28.33s, replay 42/56.68s, storage 123/18.49s, M1 process 2/2.69s).
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passed 58 active units/45.21s, BM-01 process 3/11.88s,
  BM-06 CLI 2/0.50s and BM-06 process 8/73.85s; two prior exact-oracle ignores unchanged.
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings` (0.28s), `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path
  experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings` (2.29s)
  and `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-txn --no-deps --locked
  --offline` (0.98s) pass. Preflight 35 GiB available RAM/3.9 GiB free swap; sampled peak
  1,540,292,608 bytes/zero swap, not final whole-run peak. Formatting/whitespace, docs (197 links)
  and task graph (68 tasks) pass. M1 sources/locks unchanged; no qualification campaign.
  Next: quota-preserving streaming, then immutable-run scaling/accounting and qualification.

- Implemented on pushed `95fafcc` plus this increment: [Decision 0122](docs/decisions/0122-inventory-bearing-genesis-reconstruction.md)
  reconstructs one bounded inventory-bearing first transaction and streams unpublished primary
  retry/transaction/owner candidates. Independent journal admission remains mandatory; closed
  inventory-free APIs retain their refusal behavior. Four new reference/fault tests recover
  zero/three suffix revisions from no coordinator roots, preserve first owners, durably rebase
  and cold-admit; all 39 read and 84 staging fault attempts refuse and restart exactly. Existing
  bootstrap probes now verify reference admission before preparation and wrong-result refusal.
  A test-helper `let_and_return` lint was fixed before the final gate, without suppression.
  Focused scope `run-p456096-i21464153.scope` passed four tests/2.17s before that lint failure;
  the preceding `run-p455425-i21435796.scope` also passed the two bootstrap tests/0.01s.
  Final scope `run-p456705-i21464178.scope` exited 0 under 3G/4G/512M, one job/thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 420 tests/46 executables, no failures/ignores
  (graph disk 24/26.38s, replay 37/24.45s, storage 123/18.71s, M1 process 2/2.69s).
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passed 58 active units/44.98s, BM-01 process 3/11.80s,
  BM-06 CLI 2/0.50s and BM-06 process 8/73.49s; two old exact-oracle ignores unchanged.
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings` (1.32s), `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path
  experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings` (1.63s)
  and `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-txn --no-deps --locked
  --offline` (1.02s) pass. Preflight 35 GiB available RAM/3.9 GiB free swap; sampled peak
  1,754,238,976 bytes/zero swap, not final whole-run peak. Formatting/whitespace, docs (196 links)
  and task graph (68 tasks) pass. M1 sources/locks unchanged; no qualifying campaign.
  Next: maintain optional owner projections during bounded streaming recovery, then remaining
  immutable-run scaling and accounting before qualification. T-20/T-19 remain open.

- Implemented on pushed `3f793a6` plus this increment: [Decision 0121](docs/decisions/0121-streamed-primary-owner-recovery.md)
  extends paired-base private staging to primary blob owners. Only one inventory's new-owner
  deltas are retained; earlier references are checked against disk and preserve their first
  principals. Explicit revision/owner/reference/merge bounds and attached-projection refusal
  preserve the existing inventory-free API and general recovery behavior. Six focused tests
  cover populated/empty initial owner families, absent later inventories, exact retry/transaction
  outcomes, cold terminal admission, bounds, first-reference refusal and late corruption.
  The 552-case I/O matrix includes 548 failures and four optional no-crash boundaries.
  Initial compilation corrected imports/accessor use; fixture failures corrected an invalid
  `Some(empty_inventory)` to `None` and admitted the documented `(owners + 1) * revisions`
  legacy correspondence passes. No production limit or assertion was weakened.
  Focused scope `run-p449957-i21447666.scope` exited 0 (6 tests, 11.28s).
  Final scope `run-p450349-i21424420.scope` exited 0 under MemoryHigh=3G, MemoryMax=4G,
  MemorySwapMax=512M, one Cargo job/test thread; preflight 35 GiB RAM/3.9 GiB free swap.
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 416 tests/46 executables, no failures/ignores
  (graph disk 24/27.29s, replay 33/20.31s, storage 123/18.52s, M1 process 2/2.71s).
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passed 58 active units/45.18s, BM-01 process 3/11.82s,
  BM-06 CLI 2/0.50s and BM-06 process 8/73.74s; two previous exact-oracle ignores unchanged.
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings` (0.26s), `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path
  experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings` (2.29s)
  and `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-txn --no-deps --locked
  --offline` (0.99s) pass. Sampled peak 1,598,427,136 bytes, zero swap, not a final run peak.
  Formatting/whitespace, docs (195 links), task graph (68 tasks) pass. M1 sources and lockfiles
  unchanged; no qualification campaign. Next: inventory-bearing genesis bootstrap, optional
  owner projections and remaining rewrite/accounting scalability. T-20/T-19 remain open.

- Implemented on pushed `9378599` plus this increment: [Decision 0120](docs/decisions/0120-streamed-inventory-free-recovery-metadata.md)
  adds opt-in private per-revision retry/transaction-ID staging for paired, inventory-free
  domain/metadata bases. Native BM-06 origin rebuild now uses zero outcome/transaction/owner
  overlays: its 100 suffix revisions produce 300 merges, 10,400 output entries and 1,572,300
  logical output bytes. These are partial merge diagnostics, not complete authenticated I/O.
  Inventories, existing owners and optional owner projections explicitly refuse; the general
  recovery path is unchanged. Exact retry/expiry, authorization, authenticated ID collisions,
  preparation binding, byte/count limits and late corruption have focused coverage. The fault
  matrix schedules 936 cases: 930 failures plus six optional not-found operations without a
  successful crash-after boundary. Restart retains only old or terminal graph publication and
  unpublished metadata remains undiscoverable. A test-module placement lint failure was fixed
  before the final gate, without suppressing the lint.
  Final scope `run-p442048-i21360533.scope` exited 0 with one Cargo job/test thread and
  MemoryHigh=3G, MemoryMax=4G, MemorySwapMax=512M; preflight 35 GiB available RAM/3.9 GiB free swap.
  Exact commands: `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1
  CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test
  --workspace --all-targets --all-features --locked --offline -- --test-threads=1`;
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1`; `CARGO_BUILD_JOBS=1 cargo clippy --workspace
  --all-targets --all-features --locked --offline -- -D warnings`;
  `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets
  --locked --offline -- -D warnings`; `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc
  -p uste-txn -p uste-graph --no-deps --locked --offline`.
  Workspace regression passed, including 24 graph-disk tests (27.34s) and 27 replay tests
  (10.35s). Native BM-01 process 3/11.73s, BM-06 CLI 2/0.50s and BM-06 process 8/73.82s pass;
  the two prior exact-oracle ignores are unchanged. Strict lint (4.11s/1.34s) and API docs
  (2.07s) pass. M1 sources/lockfiles are unchanged. No qualifying campaign ran.
  Next: extend bounded disk metadata recovery beyond inventory-free origin staging and address
  immutable-family rewrite scaling/accounting before qualifying campaigns. T-20/T-19 remain open.

- Implemented on pushed `bdb4ac9` plus this increment: [Decision 0119](docs/decisions/0119-native-bm06-origin-rebuild.md)
  connects private genesis reconstruction to explicit native `bm06-linux-rebuild`, retaining the
  two-record cap and ordinary open/recover refusal on total graph-base loss. Native tests rebuild
  with intact caches and after corruption/removal-by-move of every optional root, verify all 200
  historical versions and preserve certificate/journal files byte-for-byte. Reports explicitly
  disclose 100 reconstructed suffix outcomes under the 101-outcome ceiling; this is not incremental
  large-history coordinator staging. Committed certificate corruption and premature rebuild refuse.
  The first intact-cache test failed `USTE_BM06_METADATA_REPAIR` because the ordinary stale-pair
  guard intentionally refused intermediate cached metadata roots. Its origin-only exception now
  requires a private, admitted revision-one base plus the fully validated suffix; the existing
  exact-content terminal reuse/publication rules remain. A core test proves ordinary published-base
  recovery still refuses that case while private origin reconstruction succeeds at unchanged authority.
  Native focused scope `run-p432026-i21434430.scope` exited 0: the new test passes both full-cache-loss
  variants (14.08s). `run-p432590-i21388766.scope` exited 0: the core stale-pair test passes (0.02s),
  workspace and experiment strict Clippy pass (4.09s/1.40s). All used one Cargo job/test thread and
  MemoryHigh=3G, MemoryMax=4G, MemorySwapMax=512M. Final scope `run-p433391-i21423399.scope`
  exited 0 with the same limits:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 404 tests across 46 executables, no failures or
  ignores (graph disk 19/13.88s, replay 27/10.35s, storage 123/18.29s, txn 32/0.35s, M1 process
  2/2.72s). `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path
  experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1` passed 58 active units
  (45.68s), three BM-01 process tests (11.89s), two BM-06 CLI tests (0.51s), eight BM-06 process
  tests (70.43s); two prior exact-oracle ignores remain unchanged.
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings` (0.04s) and `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path
  experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings` (1.07s) pass.
  Sampled scope peak 1,465,622,528 bytes, zero swap, not final whole-run peak. Preflight: 35 GiB
  available RAM, 3.9 GiB free swap. Formatting/whitespace, docs (193 links), task graph (68 tasks)
  pass. M1 sources and lockfiles are unchanged; no qualifying campaign ran.
  Next: incremental bounded origin coordinator staging and construction/recovery scaling, without
  claiming qualifying campaigns or T-20/T-19 completion.

- Implemented on pushed `368521c` plus this increment: [Decision 0118](docs/decisions/0118-private-genesis-index-reconstruction.md)
  supplies bounded first-transaction reconstruction and unpublished graph/retry/transaction-ID
  bootstrap candidates for origin recovery. Normal semantic/journal admission remains mandatory;
  exact-current-frontier publication and ordinary rejection of private metadata publication are
  preserved. Initial streaming-domain hooks admit private bases without weakening terminal checks.
  No native fallback is enabled yet and large origin metadata overlays remain explicitly bounded.
  Initial tests exposed ordinary metadata publication correctly refusing generation-zero input;
  the repair adds separate private initial hooks, not removal of that guard. A missing explicit
  error conversion and warnings-denied redundant qualifications were corrected during compilation.
  Focused scope `run-p420991-i21375957.scope` exited 0 under 3G/4G/512M, one job/thread:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-graph --locked --offline
  --test disk_index origin_graph -- --test-threads=1 --nocapture` passed both origin tests
  (1.12s), including 171 staging error/crash attempts across 57 observed I/O occurrences.
  `CARGO_BUILD_JOBS=1 cargo clippy -p uste-txn -p uste-graph --all-targets --locked --offline
  -- -D warnings` passed (3.66s). The preliminary full gate `run-p421565-i21359689.scope`
  exited 0 (403 workspace tests, 58 active native units, three BM-01 process, two BM-06 CLI and
  seven BM-06 process tests, both strict lints and crate docs). Review then found that zero-suffix
  metadata rebase could skip private roots. Recovery now marks private metadata as rebase-required;
  tests require discoverable terminal graph/metadata/transaction roots and normal cold reopen.
  Focused scope `run-p426209-i21376278.scope` passes both origin tests and 171 injections (1.17s).
  Final corrected scope `run-p426829-i21376317.scope` exited 0 using MemoryHigh=3G,
  MemoryMax=4G, MemorySwapMax=512M, one job/thread. Exact commands:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 403 tests across 46 executables, no failures or
  ignores (graph disk 18/13.72s, replay 27/10.00s, storage 123/18.54s, txn 32/0.33s, M1 process
  2/2.76s). `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path
  experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1` passed 58 active units
  (45.32s), three BM-01 process tests (11.81s), two BM-06 CLI tests (0.50s), seven BM-06 process
  tests (55.58s); two prior exact-oracle ignores remain unchanged. Both
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings` (4.28s) and `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path
  experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings` (1.39s) pass.
  `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-txn -p uste-graph --no-deps
  --locked --offline` passes (2.05s). Sampled scope peak 1,370,214,400 bytes, zero swap, not a final
  whole-run maximum. Preflight: 35 GiB available RAM, 3.9 GiB free swap. Formatting/whitespace,
  docs (192 links), task graph (68 tasks) pass; M1 sources and both lockfiles unchanged.
  Next: native bounded cache-loss integration and
  incremental origin metadata staging/scaling; T-20 and T-19 remain open.

- Implemented on pushed `19d2c36` plus this increment: [Decision 0117](docs/decisions/0117-native-bm06-prefix-resume.md)
  adds native bounded materialization resume and a supervised durable-prefix crash probe. New
  bootstrap retry/transaction identities bind the record count; legacy unbound or expired retry
  evidence refuses resume while existing open/recover compatibility remains. A policy-only prefix
  uses the explicit one-transaction/zero-owner/1 MiB bootstrap allowance; later prefixes stay on
  disk-backed state. Current maintenance authorization and durable policy checks precede the
  metadata binding lookup. Tests SIGKILL owned children at revisions 1, 2, 50, 99 and 100, refuse
  wrong-profile resume without changing certificates, verify all 198 checkpoint history versions,
  repeat resume without duplicate commits, then repair terminal 101 and verify all 200 versions.
  Revision-one loss precedes derived root publication. Invalid probe frontiers refuse before I/O.
  Focused scope `run-p414964-i21364736.scope` exited 0: seven native tests passed in 55.52s using
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline --test recovery_process -- --test-threads=1`, with MemoryHigh=3G,
  MemoryMax=4G, MemorySwapMax=512M. Full scope `run-p415572-i21422102.scope` exited 0:
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passed 58 active unit tests (45.05s), three BM-01
  process tests (11.90s), two BM-06 CLI tests (0.49s), seven BM-06 process tests (55.72s);
  the same two exact-oracle tests retain their documented separate-command ignores.
  `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml
  --all-targets --locked --offline -- -D warnings` passed (1.07s). Sampled scope peak
  376,000,512 bytes, zero swap, not a final whole-run peak. Formatting/whitespace, docs
  (191 links) and task graph (68 tasks) pass. Latest production workspace gate remains D0113.
  Preflight: 35 GiB available RAM, 3.9 GiB free swap. T-20/T-19 remain open; no benchmark cap,
  qualification threshold, M1 source or lockfile changed. Next: full graph-base-loss rebuild and
  scalable construction/recovery, including native multi-revision retained-base controls.

- Verified on pushed `134f116` plus this increment: [native BM-06 recovery controls](docs/evidence/native-bm06-recovery-controls.md)
  cover corrupted and missing terminal cache manifests, retained-root suffix rebuild, complete
  graph-base-loss refusal and exact incomplete certificate/journal tails. Reports now expose
  both storage repair-byte counters. Corrupt/missing terminal roots recover from graph/metadata
  revision 100 and preserve the entire certificate file while verifying all 200 versions at 101.
  Complete root loss fails closed; restoring only captured derived manifests restores verified
  access. Nine appended bytes in each authority file are reported/repaired exactly, preserving
  both committed files byte-for-byte; the next open reports zero repair. The six native tests
  retain SIGKILL, ownership, wrong-key/profile, committed-corruption, retry and phase guards.
  `run-p412685-i21380063.scope` exited 0 under 3G/4G/512M, one Cargo job/test thread:
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline --test recovery_process -- --test-threads=1 --nocapture` passes six tests
  (30.25s); `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml
  --all-targets --locked --offline -- -D warnings` passes (1.00s). Sampled scope peak 381,009,920
  bytes, zero swap, not final whole-run peak. The preceding full native gate remains recorded
  below; this focused increment changes only native reports and their recovery controls, not
  production crates. Formatting/whitespace, documentation and task-graph checks pass. Preflight:
  35 GiB available RAM, 3.9 GiB free swap. Next: native arbitrary-prefix materialization resume,
  then complete cache-loss rebuild and construction/recovery scaling. No BM-06 qualification,
  authoritative baseline promotion or T-20/T-19 completion is claimed.

- Verified on pushed `8749793` plus this increment: [Decision 0116](docs/decisions/0116-native-bm06-recovery-phases.md)
  adds native Btrfs create/tail/recover/open phases and an owned-child durable-tail SIGKILL probe.
  OS entropy, portable recovery, current authorization and durable flushes remain enabled; the
  native cap is two records, checked before root/credential access. Tests verify all historical
  payloads and record metadata, pre-tail wrong-profile refusal, live-owner exclusion, exact retry,
  fresh-process recovery/reopen, wrong credentials, phase-frontier guards and fail-closed committed
  certificate corruption followed by exact restored-byte recovery. Reports separate verified
  history/revision from the final frontier and distinguish partial adapter I/O from device traffic.
  Host caches are uncontrolled and no qualifying trial or benchmark pass is claimed.
  The first native focused run passed the SIGKILL case but failed one-record metadata repair:
  the 101-entry coordinator families exceeded the graph-only 100-entry merge bound. Fixed shared
  limits to include coordinator cardinality and extended the memory-model test to both record
  counts. BM-01 allowances and tests remain unchanged. Corrected focused gate passed seven unit
  tests (1.86s), two CLI tests (0.49s), three native tests (10.84s), strict lint (1.12s).
  Final `run-p411819-i21372279.scope` exited 0 under MemoryHigh=3G, MemoryMax=4G,
  MemorySwapMax=512M: `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path
  experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1` passes 58 active
  unit tests (45.06s), three BM-01 process tests (11.73s), two BM-06 CLI tests (0.49s) and
  three BM-06 native process tests including committed corruption (11.93s); two unchanged
  exact-oracle ignores remain. `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path
  experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings` passes
  (0.06s). Sampled scope peak 299,163,648 bytes, zero swap, not final whole-run peak.
  Native formatting/whitespace, docs (189 links) and 68-task graph checks pass; production
  workspace/M1 sources and lockfiles are unchanged. Preflight: 35 GiB available RAM, 3.9 GiB
  free swap, 985 GiB free Btrfs space. Reproduction commands and unsupported arbitrary-prefix
  create resume are documented in the experiment README. Next: native optional-cache and
  torn-tail controls, resumable materialization, then remaining construction/recovery scaling
  and reserved-host qualification. T-20/T-19 remain open; this is not a ten-million-event trial.

- Verified on pushed `6cc94e7` plus this increment: [Decision 0115](docs/decisions/0115-bm06-disk-state-equivalence.md)
  connects the BM-06 workload to authorized encrypted disk-state writes and recovery. The
  `bm06-disk-check --records 2` development command is capped before filesystem/key allocation
  and uses the durable memory model, not a native benchmark. It maintains roots through revision
  100, certifies 101 while deliberately refusing only derived publication, then cold-recovers the
  suffix, rebases metadata, checks exact retry, reopens and verifies all 200 historical versions
  and 819,200 payload bytes through authorized history reads. No full graph/coordinator or
  certificate/blob-history map is reconstructed after the policy-only bootstrap.
  BM-06-specific work limits admit 100 historical versions and at most 512 preparation records;
  exact-size limit construction is tested without running that database. The first focused run
  failed at historical lookup ordinal 117 under the inherited 64-page-visit BM-01 allowance;
  contextual diagnosis confirmed it, then a profile-derived 664-visit allowance passed. BM-01
  limits remain unchanged. The publication-refusal check accepts only the exact certified storage
  ResourceLimit, not arbitrary errors. Final focused tests passed two tests (0.50s), strict lint
  (1.10s), before adding exact retry and CLI coverage to the full final gate.
  `run-p408250-i21364298.scope` exited 0 under MemoryHigh=3G, MemoryMax=4G, MemorySwapMax=512M:
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passes 58 active unit tests (44.96s), three existing
  process tests (11.89s), two BM-06 CLI tests (0.51s), with two unchanged exact-oracle ignores.
  `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets
  --locked --offline -- -D warnings` passes (0.92s). Sampled scope peak 333,303,808 bytes,
  zero swap, not final whole-run peak. Native formatting/whitespace, docs (188 links) and task
  graph (68 tasks) pass. Production workspace/M1 sources and both lockfiles are unchanged from
  their preceding verified trees. Preflight: 36 GiB available RAM, 3.9 GiB free swap.
  Next: native BM-06 materialization/recovery and process-loss controls, then remaining scaling
  prerequisites. This is one development suffix revision, not the exact-size 196 or one of the
  required 30 reserved-host trials. T-20/T-19 and all benchmark thresholds remain open/unchanged.

- Verified on pushed `a7ee52b` plus this increment: [Decision 0114](docs/decisions/0114-bm06-versioned-event-materialization.md)
  adds an executable BM-06 materialization contract and bounded actual graph operation generator.
  Exact fixture: 100,000 records, 100 retained 4096-byte versions each, ten million events,
  19,601 revisions, checkpoint/root boundary 19,405 and 196 suffix revisions. History payload
  alone is 40,960,000,000 bytes, a logical fixture dimension, not measured recovery scalability.
  `bm06-manifest [--records N]` is explicitly fixture-only. Direct revision-to-batch construction
  retains at most 512 distinct-record operations and does not generate previous batches.
  The exact synthetic digest `c2d3b2fa0ce5d228213d03eb3fe4885fb11f41edcd6bc6935a8fb4b9702d5c3a`
  matches the independent existing fixture generator; small request/payload goldens are pinned
  in `acceptance/r1/bm06-materialization-v1.tsv` (SHA-256
  `4ab6a5979cbc86814d56baf907151be057a47b491c3c394efed31c2ecad854bb`).
  Under 3G/4G/512M, one Cargo job/test thread, scope `run-p404281-i21379536.scope` exited 0:
  `CARGO_BUILD_JOBS=1 cargo run --release --manifest-path experiments/fixture-generator/Cargo.toml
  --locked --offline -- digest events COUNT
  8f41d0a52b40f13f4a77bc3beae2026a8bc42ad48d12ce53d92e29f612111006`, for COUNT=200 and 10000000;
  `CARGO_BUILD_JOBS=1 cargo run --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- bm06-manifest`. These hash the 48-byte synthetic stream, not a materialized
  40.96 GB database. Final serial native gate in `run-p404877-i21358918.scope` exited 0:
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passes 56 active unit tests (44.67s), three existing
  process tests (11.83s), one new CLI test (0.01s), retaining two unchanged exact-oracle ignores;
  `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets
  --locked --offline -- -D warnings` passes (0.86s). Five new unit tests cover exact dimensions,
  batch boundaries, version-precondition mapping, every small-fixture historical version,
  checkpoint-plus-suffix equivalence, request/payload goldens and independent stream digest.
  CLI tests refuse invalid/mixed/unbounded arguments and assert nonqualification fields.
  Development corrected a wrong scoped-ID constructor and a rejected empty bootstrap; the test
  now installs a real policy-only transaction. No production test was weakened. Native/workspace
  locks and M1 sources are unchanged; production workspace remains at the preceding 400-test
  verified tree. Native formatting/whitespace, documentation (187 links) and 68-task checks pass.
  Preflight: 36 GiB available RAM, 3.9 GiB free swap. Next: BM-06 disk profile admission and
  authorized native development materialization/recovery; immutable rewrite scaling, all recovery
  controls, reserved-host trials and T-20/T-19 qualification remain open.

- Verified on pushed `136ddc5` plus this increment: [Decision 0113](docs/decisions/0113-populated-base-quota-rebuild.md)
  implements populated-base quota projection rebuild with 1–4096-owner batches, independent
  terminal validation and no intermediate root publication. Explicit owner/batch/source/merge/
  admission budgets and cumulative logical output include repeated immutable rewrites. Exact
  two-owner output is 602 bytes for two batches or 389 for one; one byte less refuses. Four
  integration tests cover cross-batch literal totals, cold admission/replacement, late certificate
  corruption, private terminal-admission failure and 210 I/O fault attempts with exact restart.
  Three successful optional NotFound paths consume planned CrashAfter without an actual crash.
  The batch lower-bound unit test and existing empty/zero-byte-owner test also pass. Final focused
  command: `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1
  CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test
  -p uste-txn -p uste-replay --locked --offline quota_rebuild -- --test-threads=1 --nocapture`
  (four integration tests 2.55s, one unit test); txn/replay all-target/all-feature strict Clippy
  passes (0.04s). A preceding focused run's final output was lost at context recovery and was
  rerun only after checking that no workload remained. The normal unoptimized empty-owner test
  also passed (0.35s) before the full gate.
  Full serial gate in `run-p398514-i21386929.scope` exited 0 under MemoryHigh=3G, MemoryMax=4G,
  MemorySwapMax=512M: assertion-enabled optimized workspace/all-target/all-feature tests passed
  400 tests across 46 executables, no failures or ignores (123 storage 18.38s; 27 replay/coordinator
  10.33s; 16 disk graph 13.19s; 31 transaction 0.32s; M1 process recovery 2.68s). Commands:
  the focused command above with `--workspace --all-targets --all-features` replacing packages
  and filter; `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path
  experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1` (51 active tests
  43.33s and three process tests 11.86s; two unchanged exact-oracle ignores);
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings` (3.93s); `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path
  experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings` (1.92s);
  `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-txn --no-deps --locked
  --offline` (0.96s). Sampled scope peak 1,347,969,024 bytes, zero swap, not final whole-run peak.
  `cargo fmt --all --check`, `git diff --check`, `python3 scripts/check_docs.py` (186 links)
  and `python3 scripts/check_task_graph.py` (68 tasks) pass. M1 crate sources and Cargo.lock
  still match `b9689f3`; no pilot restart or task completion is claimed. Preflight: 36 GiB
  available RAM, 3.9 GiB free swap. Next: T-20 recovery/construction scaling and executable
  BM-06 workload prerequisites; immutable-run rewrite cost and qualifying campaigns remain open.

- Verified on pushed `eda0c88` plus this increment: [Decision 0112](docs/decisions/0112-authorized-indexed-blob-accounting.md)
  connects explicit indexed accounting to authorized staging, inventory commits and quota
  inspection. Missing admission refuses configuration; inspection requires current authority
  before I/O. Legacy constructors retain streaming behavior. Staging-only mode cannot enable
  inventory commits; complete-outbox reconciliation is still required. Fixed lookup allowances
  and an additional local 64 KiB quota cache replace a complete ledger scan on the opt-in path.
  The raw coordinator's retry ordering is unchanged; authorized inventory retries retain their
  existing ownership/accounting checks. Exact charge release and uncertain-outcome handling
  remain unchanged. Four normal unoptimized focused tests pass (2.00s), including all six selected
  journal-sync failures in indexed mode, cold retry/reconciliation, current revocation, missing
  admission, denied/foreign identity, precommit accounting read failure with retained charges,
  populated principal aggregates and both actual inspection page-read failures. Commands:
  `CARGO_BUILD_JOBS=1 cargo test -p uste-txn --test transaction_coordinator --locked --offline
  authorized_indexed -- --test-threads=1 --nocapture` and `CARGO_BUILD_JOBS=1 cargo clippy
  -p uste-txn --all-targets --all-features --locked --offline -- -D warnings` (0.18s), serially
  under 3G/4G/512M. Full serial gate in `run-p389952-i21386402.scope` exited 0: assertion-enabled
  optimized workspace/all-target/all-feature tests passed 395 tests across 46 executables,
  with no failures or ignores (123 storage, 23 replay/coordinator, 31 transaction, 16 disk graph,
  and all remaining suites including M1 process recovery). Native release tests passed 51 active
  unit tests (43.27s) and three process tests (11.88s), retaining the two exact-profile oracle
  ignores. Strict workspace/native Clippy (4.48s/1.92s) and txn rustdoc (0.97s) pass. Commands and
  profile flags are the same full gate recorded for D0111/D0110 below. Sampled scope peak:
  1,511,714,816 bytes, zero swap, not final whole-run peak. Format/whitespace, docs (185 links),
  task graph (68 tasks) pass; M1 sources and lockfile still match the pinned implementation.
  No benchmark or task completion is newly claimed. Available RAM/swap:
  36 GiB / 3.9 GiB before builds. Next: bounded quota-cache bootstrap from a populated base,
  remaining immutable construction/rewrite costs and qualifying benchmark prerequisites.

- Verified on pushed `0b39864` plus this increment: [Decision 0111](docs/decisions/0111-disk-first-owner-quota-projection.md)
  adds optional disk principal totals and an independently validated principal/owner ordering.
  Empty-base bootstrap, bounded-overlay construction, quota-preserving rebase, cold admission
  and exact indexed reads are implemented. Existing authorized facades still use their streaming
  accounting path; explicit indexed adapter integration is next. Populated legacy-base bootstrap,
  whole-run rewrite amplification and qualification remain open. Focused assertion-enabled
  optimized checks pass six tests: four replay/integration (2.20s), one encoding/overflow unit,
  one empty bootstrap/zero-length-owner test. Coverage includes same/different principal updates,
  all 216 publication-fault attempts (214 errors, two successful optional cleanup paths), 12
  authenticated false-cache variants, five cold-admission read faults, exact/minus-one limits,
  retained prior admission and exact restart. Full serial gate in scope
  `run-p383697-i21370366.scope` exited 0: the same assertion-enabled optimized command with
  `--workspace --all-targets --all-features` instead of the focused packages/filter passes
  391 tests across 46 executables with no failures or ignores,
  including 123 storage tests (18.63s), 23 coordinator/replay tests (7.27s), 16 disk graph tests
  (12.73s), 27 transaction tests (0.28s) and unchanged M1 process tests (2.68s).
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passes 51 active unit tests (43.50s), three process
  tests (11.90s), with the same two exact-profile oracle ignores. Strict workspace all-target/
  all-feature Clippy (3.95s), native all-target Clippy (1.89s), and strict txn rustdoc (0.96s)
  pass using the same locked/offline commands recorded for D0110 below. Sampled scope peak:
  1,354,973,184 bytes, zero swap, not a final whole-run peak. Format/whitespace, docs (184 links)
  and task graph (68 tasks) pass. M1 crate trees and workspace lockfile still match the pilot pin.
  Test development corrected an invalid trailing struct-update comma, an incorrect one-page-visit
  lookup expectation (binary search and selected access need two even with cache hits), and an
  overstrict assertion that optional cleanup faults must fail publication. The initial normal
  unoptimized extended run failed that assertion after 88.70s; the corrected test requires exact
  successful state/restart and only permits cleanup success for RemoveFile. Successful paths
  consume planned CrashAfter at RemoveFile occurrences 1 and 4 on NotFound; no actual crash occurs.
  No production durability rule changed.
  Focused final command under 3G/4G/512M: `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1
  CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test
  -p uste-txn -p uste-replay --locked --offline blob_usage -- --test-threads=1 --nocapture`;
  strict all-target/all-feature Clippy for txn/replay also passes (1.41s). Preflight: 36 GiB
  available RAM, 3.9 GiB free swap. The previous `0b39864` increment is pushed to origin.

- Tested on pushed `d1da3c2` plus this increment: [Decision 0110](docs/decisions/0110-reverse-first-reference-rebase.md)
  constructs bounded post-base first-reference claims using authenticated reverse streaming.
  Every occurrence checks reference bytes; the completed scan must match the earliest principal.
  No root publication precedes complete validation. Three new tests cover all 256 principal
  histories, exact/one-short byte limits in both certificate modes, and 27 selected read-fault
  attempts (27 failures, all restart/retry checks pass). Normal unoptimized focused command
  `CARGO_BUILD_JOBS=1 cargo test -p uste-txn -p uste-replay --locked --offline
  reverse_first_reference -- --test-threads=1 --nocapture` passed (integration 18.37s).
  Corrected the new module's explicit relative path before compilation; strict Clippy then
  caught its unit-test placement before helper definitions, corrected by moving it to EOF.
  The interrupted gate had no recoverable result and was not counted. A fresh serial gate in
  scope `run-p376039-i21362324.scope` exited 0 under 3G/4G/512M, with 36 GiB available RAM and
  3.9 GiB free swap before launch:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed, including all 19 coordinator/replay integration
  tests (5.35s) and the unchanged M1 process cases. The native release test command below passed
  51 active unit tests (43.12s), three process tests (11.86s), and two unchanged oracle ignores.
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings`, the same native-manifest all-target Clippy, and `CARGO_BUILD_JOBS=1
  RUSTDOCFLAGS="-D warnings" cargo doc -p uste-txn --no-deps --locked --offline` all pass.
  Format/whitespace, docs (183 links), task graph (68 tasks) pass; the M1 crate trees and
  workspace lockfile still exactly match the pinned pilot implementation. No qualification
  or task completion is newly claimed. Next address remaining disk quota/accounting and
  construction costs before qualifying campaigns; T-19 remains behind T-20.

- The slot-cache comparison passed on pushed `d1da3c23ee20c6bee428f531054242f44d50cb6a`,
  binary SHA-256 `dae9f15ad93624941f935c5ae29cdf5a402152196c6e828ff4c644aff7efa84a`.
  Same 20,000-entity artifact, independent oracle and timed query command as the baseline below;
  release rebuilt locked/offline with one job before launch. Preflight: 36 GiB available RAM,
  3.9 GiB free swap; one heavy workload under 3G/4G/512M and 900s timeout. Exit 0:
  339.56s wall, 314,866ms query phase, 265,104 KiB peak RSS, zero swaps. All 384 expectations
  pass (313 results and 71 expected result-limit refusals). Programmatic comparison of 18
  result/work fields against the baseline found no differences, including both digests,
  cache hits/misses/evictions, adapter I/O and cached-index work. The evidence archive records
  the exact values. Sampled scope peak 274,907,136 bytes is not a final whole-run peak.
  The baseline rebuilt a stale catalog; this reopen reused it, so setup costs are not identical.
  One observation per version with uncontrolled host caches is not statistical speedup evidence,
  qualifying latency evidence or larger-than-memory proof.

- Tested on pushed `0d5eeeb` plus this increment: [Decision 0109](docs/decisions/0109-slot-addressed-cache-recency.md)
  replaces the age tree with safe bounded slot links and avoids the second key lookup on hits.
  Existing LRU/reference/parser/resource tests remain, with added slot reuse/capacity checks.
  Focused checks passed after the baseline exited: `CARGO_BUILD_JOBS=1 cargo test -p uste-storage
  --locked --offline index:: -- --test-threads=1` (15 tests, 5.70s), then the same command with
  filter `sparse_point_reads` (1 test, 27.86s); strict storage all-target/all-feature Clippy
  (2.03s). All ran in the unoptimized profile under 3G/4G/512M. No build overlapped the unchanged
  baseline query process; cache limits/accounting constants are unchanged. Full workspace gate:
  `CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test --workspace --all-targets --all-features
  --locked --offline -- --test-threads=1` passed 382 tests with no ignores: 123 storage unit
  tests (18.56s), 16 graph disk-index tests (12.65s), 17 coordinator/replay tests (4.95s),
  memory-adapter process tests (2.70s), 26 transaction tests (0.27s), and all other selected
  crypto/policy/ingest/memory/spatial/time/type/reference/adapter tests. Native release command
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passed 51 active unit tests (43.18s) and three process
  tests (11.75s), retaining two exact-profile oracle ignores. Strict workspace all-target/
  all-feature Clippy (4.66s), native all-target Clippy (1.78s), and strict storage rustdoc
  (0.94s) pass with one job, locked/offline dependencies, `-- -D warnings` for Clippy and
  `RUSTDOCFLAGS="-D warnings"` for `cargo doc -p uste-storage --no-deps`.
  All gates ran serially under 3G/4G/512M; sampled scope peak 2,812,538,880 bytes, zero swap
  (not final whole-run peak). Format/whitespace, JSON evidence, docs (182 links) and task graph
  (68 tasks) pass. M1 sources/lockfile remain unchanged; no task or qualification is newly complete.

- Native 20,000-entity construction on pushed `0d5eeebbd0d6f04d2975e171d3c4e27d99d043c0`
  passed at revision 44: 143.86s wall time, 266,292 KiB peak RSS, zero swaps, exit 0; actual final
  counts `[220001,420001,200000,200000,200000,600000,1,1]`, no complete graph/coordinator history
  maps. Binary SHA-256 `34f066f834f8d96227fe2cda0f92651e453b4e73647bff72e10ee9e66dbf6be0`;
  pinned lock SHA-256 unchanged. Command: `timeout --signal=TERM --kill-after=5s 900s
  /usr/bin/time -v experiments/t20-bench/target/release/uste-t20-bench linux-disk-create --root
  experiments/t20-bench/target/native-pressure20000.ya99oO --password-file
  experiments/t20-bench/target/native-pressure20000.ya99oO/password --entities 20000`, after
  `CARGO_BUILD_JOBS=1 cargo build --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline`, all under 3G/4G/512M. Synthetic password copied owner-only from the prior
  development fixture, not printed or committed. Adapter read/write bytes: 31,717,128,873 /
  11,167,788,572, not physical device traffic. The retained artifact is separate from the
  prior 10,000 fixture. `oracle-summary --entities 20000` now generates its separate summary;
  then the same timed command with `linux-disk-query` and `--oracle-file
  experiments/t20-bench/target/native-pressure20000.ya99oO/oracle-summary` runs serially in one
  new 3G/4G/512M scope. Query passed all 384 expectations (313 results, 71 expected result-limit
  refusals): 435.30s wall time, 401,232ms query phase, 265,384 KiB peak RSS, zero swaps, exit 0.
  The [archive](docs/evidence/native-disk-20000-development.json) records 7,577,807 evictions,
  8,531,739 page loads, 346,098,730 cache hits and 67,098,624 accounted bytes under the 64 MiB
  budget. Actual D0107 native rebuild discovers one terminal group and independently admits
  all 44 prefix groups; this first query's setup therefore differs from later catalog reuse.
  Sampled scope peak 273,215,488 bytes, not final whole-run peak. Preflight available RAM
  remained 36 GiB with 3.9 GiB free swap. This is not qualification or larger-than-memory proof.

- Tested on pushed `9fda196` plus this increment: [Decision 0108](docs/decisions/0108-native-cache-pressure-development-admission.md)
  raises only native development admission to 20,000 entities / 200,000 relationships. The
  1,000-entity memory-adapter cap, 64 MiB cache, fixed batch/request limits and refusal of
  qualifying 100,000-entity execution remain. Extended old/new-ceiling batch and pre-I/O refusal
  checks pass with the full native release regression: `CARGO_BUILD_JOBS=1 cargo test --release
  --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1`
  reports 51 active unit tests (43.65s), two unchanged exact-profile oracle ignores, and three
  CLI/process tests (12.15s). `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path
  experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings` passes (0.81s).
  Format/whitespace, docs (181 links) and task graph (68 tasks) pass. No new-scale execution or
  cache-pressure result is claimed yet. Host preflight: 36 GiB available RAM, 3.9 GiB free swap
  and 996 GiB free Btrfs space. One native test workload ran under 3G/4G/512M, one job/thread.

- Tested on pushed `ca0f5df` plus this increment: [Decision 0107](docs/decisions/0107-empty-history-catalog-construction.md)
  uses the maintained zero-binding count only to skip redundant empty-history construction;
  whole-prefix independent admission still precedes publication. Initial focused verification
  passed four tests including 45 selected I/O fault attempts; the group-corruption test exposed
  a wrong fixture assumption that reopen created a segment. Corrected to the retained segment
  layout and expanded corruption coverage to all five groups. All five focused tests now pass
  (2.47s) in the unoptimized profile: `CARGO_BUILD_JOBS=1 cargo test -p uste-storage --locked
  --offline empty_blob_rebuild -- --test-threads=1 --nocapture`; strict storage Clippy passes
  (1.58s). The full gate passed under 3G/4G/512M: `CARGO_BUILD_JOBS=1
  CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=true
  CARGO_PROFILE_TEST_OVERFLOW_CHECKS=true cargo test -p uste-storage -p uste-txn -p uste-replay
  -p uste-graph --all-targets --all-features --locked --offline -- --test-threads=1`.
  Optimization retains debug assertions/overflow checks; every fault case remains. Results:
  121 storage tests (18.56s), 16 graph disk-index tests (13.61s), 17 coordinator/replay tests
  (5.01s), 26 transaction tests (0.26s), 13 authorization tests (0.03s), all selected adapter/
  process/unit tests pass. Native gate `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path
  experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1` passed 51 active unit
  tests (42.58s) and three CLI/process tests (12.17s), with the two unchanged exact-profile
  oracle ignores. Strict workspace all-target/all-feature Clippy (3.91s), native all-target
  Clippy (1.26s), and `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-storage
  --no-deps --locked --offline` (0.93s) pass. All used locked/offline dependencies and one job;
  Clippy used `-- -D warnings`. Sampled scope peak 2,401,251,328 bytes / zero swap, not final
  whole-run peak. Format, whitespace, evidence JSON, docs (180 links) and task graph (68 tasks)
  pass. M1 sources/lockfile are unchanged; no task or qualifying gate is newly complete.
  The unchanged `ca0f5df260cba66ac4c286a8d7565419de2f5b57` native binary,
  SHA-256 `e7f896575bc1c4d8f7d22849829a083925456a77f67b4b9a95bb246ae7494eb1`, ran the same
  10,000-entity/384-query observation command recorded below. It was rebuilt before these edits;
  no second build/heavy workload ran concurrently. Preflight: 36 GiB available RAM, 3.9 GiB free swap;
  scope 3G/4G/512M. All 384 queries matched, exit 0, 246.20s wall time, 234,349ms query phase,
  265,100 KiB peak RSS, zero swaps. Fragment work: 789,750,846 versus 1,080,841,044 before;
  page loads, cache hits, adapter bytes and output digest are unchanged. The
  [archive](docs/evidence/native-disk-sparse-search-development.json) records both versions and
  measurement limitations; no qualifying speedup is claimed. Native report metadata now makes
  sparse-probe inclusion in the historical fragment field explicit, with a regression assertion.

- Tested on pushed `556fb7a` plus this increment: [Decision 0106](docs/decisions/0106-sparse-cached-page-search.md)
  wires the previously uncompiled sparse page directory and borrows cached layouts. New linear-
  reference and encrypted dense-page tests pass (3 tests, 27.80s), as do the complete `index::`
  parser/cache tests (13 tests, 4.77s) and strict storage all-target/all-feature Clippy (2.46s).
  Commands: `CARGO_BUILD_JOBS=1 cargo test -p uste-storage --locked --offline sparse --
  --test-threads=1 --nocapture`, then the same command with filter `index::`, then
  `CARGO_BUILD_JOBS=1 cargo clippy -p uste-storage --all-targets --all-features --locked
  --offline -- -D warnings`, under the resource scope below. A lost terminal result was not
  counted; rerunning exposed an unnecessary test type qualification, corrected before these
  passing results. Full core regression passed under the same scope:
  `CARGO_BUILD_JOBS=1 cargo test -p uste-storage -p uste-txn -p uste-replay -p uste-graph
  --all-targets --all-features --locked --offline -- --test-threads=1`:
  116 storage unit tests (704.87s, no fault-matrix skips), 16 graph disk-index tests (535.65s),
  17 coordinator/replay tests (196.89s), 26 transaction tests (8.04s), 13 authorization tests
  (0.90s), and all other selected adapter/process/unit tests passed. Native release regression
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passed 51 active unit tests (42.97s), with two unchanged
  exact-profile oracle ignores, and three CLI/process tests (12.30s). Strict lint commands
  `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
  -- -D warnings` (7.58s) and `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path
  experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings` (2.45s) pass.
  Sampled gate scope peak: 1,325,060,096 bytes, zero swap (not final whole-run peak).
  Format/whitespace, evidence JSON, documentation (179 links) and task graph (68 tasks) pass.
  M1 crate sources and the pinned lockfile remain unchanged. No task is newly complete.
  An unchanged exact-version native binary measured the pre-change query baseline:
  commit `556fb7ad11e687023b64c5ce02fde69587ccab0d`, binary SHA-256
  `4f794dad05551cbf755d04458aa43edbb7651c84711a3432fc78949d829f6658`.
  Command under 3G/4G/512M: `timeout --signal=TERM --kill-after=5s 900s /usr/bin/time -v
  experiments/t20-bench/target/release/uste-t20-bench linux-disk-query --root
  experiments/t20-bench/target/native-pressure.kedpTk --password-file
  experiments/t20-bench/target/native-pressure.kedpTk/password --oracle-file
  experiments/t20-bench/target/native-pressure.kedpTk/oracle-summary --entities 10000`.
  Release build completed before source edits; no second build/heavy workload runs concurrently.
  Preflight 36 GiB available RAM / 3.9 GiB free swap. [Baseline measurements](docs/evidence/native-disk-sparse-search-development.json)
  passed all 384 oracle comparisons: 252.50s wall time, 240,579ms query phase, 264,844 KiB peak RSS,
  zero swaps, exit 0. Query work: 1,080,841,044 fragments, 806,885 page loads, 420,631,561 cache
  hits, no evictions, unchanged output digest. Sampled scope peak 274,292,736 bytes (not final
  whole-run peak). Sparse source compilation/reference tests ran separately;
  no optimized performance result or qualification is claimed.
  `crates/uste-storage/src/journal/tests/blob_metadata_tests/empty_rebuild.rs` is unwired
  next-increment test preparation, excluded from this increment's verification and commit scope.

- Tested on pushed `16e9e9d` plus this increment: [Decision 0105](docs/decisions/0105-reverse-metadata-correspondence.md)
  applies authenticated reverse scanning to storage catalog and final coordinator retry/reference
  correspondence. Ordered construction/replay and compatibility first-owner discovery stay forward.
  The new exact/minus-one catalog budget test passes (0.42s), and the extended resident/disk
  first-reference claim/read-fault matrix passes (2.17s). Focused commands, under 3G/4G/512M with
  one Cargo job and one test thread: `CARGO_BUILD_JOBS=1 cargo test -p uste-storage --locked
  --offline blob_metadata_reverse_admission -- --test-threads=1` and `CARGO_BUILD_JOBS=1 cargo
  test -p uste-replay --locked --offline first_reference_claims -- --test-threads=1`. The full
  affected gate passed in the same resource scope: `CARGO_BUILD_JOBS=1 cargo test -p uste-storage
  --locked --offline blob_metadata -- --test-threads=1` (21 tests, 503.09s, including all affected
  catalog fault matrices), then `CARGO_BUILD_JOBS=1 cargo test -p uste-txn -p uste-replay
  -p uste-graph --all-targets --all-features --locked --offline -- --test-threads=1`
  (16 graph disk-index tests, 533.19s; 17 coordinator/replay tests, 193.21s; 26 transaction tests,
  8.09s; 13 authorization tests, 0.90s; all other selected tests pass). The unchanged shared
  range primitive already passed complete storage regression in `16e9e9d`. Native release
  gate `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passed 51 active unit tests (43.30s) and three CLI/process
  tests (12.17s); two unchanged exact-scale oracle ignores remain. Strict workspace all-target/
  all-feature Clippy (3.18s) and native all-target Clippy (1.26s) passed with one job, locked/offline
  and `-- -D warnings`. Format/whitespace, evidence JSON, docs (178 links) and task graph (68 tasks)
  pass. Host headroom remained 36 GiB RAM/3.9 GiB free swap; sampled scope peak 833,388,544 bytes,
  zero sampled swap (not the final whole-run peak). No task or qualification gate is newly complete.
  Unwired `crates/uste-storage/src/index_sparse.rs` is next-increment preparation, excluded from
  this increment's verification and commit scope.

- Pushed `16e9e9d` [native reopen comparison](docs/evidence/native-disk-reverse-validation-development.json)
  preserves revision 23, all state counts and zero resident history maps. Exactly 275 certificate
  reads / 1,144,275 adapter bytes were removed, matching `(23*24/2 - 1)*4161`. Time 11.99s,
  peak RSS 265,088 KiB, zero swaps, exit 0; versus prior reuse 11.88s, so no speedup is claimed.
  Release build and command match the preceding native observation under the same 3G/4G/512M
  scope, with 36 GiB available RAM and 3.9 GiB free swap. No qualifying campaign ran.

- Tested on pushed `ec1dbca` plus this increment: [Decision 0104](docs/decisions/0104-authenticated-reverse-journal-validation.md)
  adds constant-state reverse certificate-chain validation and uses it for order-independent
  transaction-ID admission. Forward replay/cursor semantics remain unchanged. Three focused
  storage tests pass (1.22s), including 57 injected read/open/metadata faults with restart, every
  subrange and exact/minus-one budgets, authentic forks and callback-time corruption. Command:
  `CARGO_BUILD_JOBS=1 cargo test -p uste-storage --locked --offline reverse_certificate_range
  -- --test-threads=1 --nocapture`. Initial strict lint found a test-only unnecessary `vec!`;
  corrected to an array. The full gate passed under the established 3G/4G/512M scope:
  `CARGO_BUILD_JOBS=1 cargo test -p uste-storage -p uste-txn -p uste-replay -p uste-graph
  --all-targets --all-features --locked --offline -- --test-threads=1` (112 storage unit tests,
  713.34s, with no fault-matrix skips; 16 graph disk-index tests, 526.76s; 17 coordinator/replay
  tests, 192.59s; 26 transaction integration tests, 8.05s; 13 authorization tests, 0.89s; all other
  selected unit, adapter and process tests pass). Then `CARGO_BUILD_JOBS=1 cargo test --release
  --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1` passed
  51 active native unit tests (42.76s) and three CLI/process-loss tests (12.34s), with two unchanged
  exact-scale oracle ignores. `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets
  --all-features --locked --offline -- -D warnings` passed (4.35s), as did
  `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets
  --locked --offline -- -D warnings` (2.18s) and `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings"
  cargo doc -p uste-storage -p uste-txn --no-deps --locked --offline` (2.10s). Format/whitespace,
  evidence JSON, docs (177 links) and task graph (68 tasks) pass. Preflight: 36 GiB available RAM,
  3.9 GiB free swap, one job/thread/heavy workload. Sampled scope peak 1,124,913,152 bytes, zero
  sampled swap (not the final whole-run peak). No task or qualification gate is newly complete.

- Before those code changes, pushed `ec1dbca` reopened the retained 10,000-entity/100,000-relationship
  native fixture twice at revision 23. [Selected exact-version measurements](docs/evidence/native-disk-storage-recovery-development.json)
  record optional catalog rebuild then reuse, 11.90s/11.88s wall time, 265,132/264,800 KiB peak RSS,
  and zero resident certificate/blob/inventory/namespace history entries. Command under the same
  memory scope: `/usr/bin/time -v experiments/t20-bench/target/release/uste-t20-bench linux-disk-open
  --root experiments/t20-bench/target/native-pressure.kedpTk --password-file
  experiments/t20-bench/target/native-pressure.kedpTk/password --entities 10000`; release rebuilt
  using `CARGO_BUILD_JOBS=1 cargo build --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline`. Both exit 0. These are uncontrolled development observations, not BM-06,
  larger-than-memory or qualifying resource reservations. Retained synthetic fixtures are intact.

- Tested on pushed `ee20df7` plus this increment: [Decision 0103](docs/decisions/0103-authorized-disk-inventory-commits.md)
  adds opt-in authorized ordinary inventory commits with exact first ownership, current target
  permissions, bounded accounting admission and staged-to-committed charge transfer. Eight focused
  tests pass (2.19s), including cold retry/reconciliation, six uncertain flush cases, zero-byte
  reservations, owner-bound refusal, foreign/changed references and known-outcome policy drift.
  Initial test module path resolution failed and was corrected with an explicit fixture path;
  a lost terminal result was not counted and the focused gate was rerun. All commands below used
  `systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M`, one Cargo
  job and one Rust test thread, with 36 GiB available RAM/3.9 GiB free swap at preflight:
  `CARGO_BUILD_JOBS=1 cargo test -p uste-txn --locked --offline authorized_disk_inventory
  -- --test-threads=1 --nocapture`; then `CARGO_BUILD_JOBS=1 cargo test -p uste-txn
  -p uste-memory -p uste-memory-adapter --all-targets --all-features --locked --offline
  -- --test-threads=1` passed (26 transaction integration tests, 8.06s; 13 authorization tests,
  0.88s; M1 process tests, 30.96s; all selected unit/ingest tests). The affected existing graph
  facade test passed (1.59s): `CARGO_BUILD_JOBS=1 cargo test -p uste-graph --test disk_index
  --locked --offline authorized_disk_expansion_matches_reference_and_shares_all_work_budgets
  -- --test-threads=1`. `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features
  --locked --offline -- -D warnings` passed (4.55s); `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings"
  cargo doc -p uste-txn --no-deps --locked --offline` passed (0.96s). No benchmark qualification
  or task completion is claimed. M1 crate sources remain identical to `b9689f3` and the lockfile
  SHA-256 remains `7ed2b533b3c801250a89e008b4ca26c48f16400e38224d8a63447c0393aaa97b`.

- Tested on pushed `fd44ca2` plus this increment: [Decision 0102](docs/decisions/0102-disk-coordinator-inventory-publication.md)
  connects ordinary/external prepared disk-coordinator commits to explicit bounded storage
  inventory publication. Shared retry/collision/first-owner admission is preserved; storage
  refresh releases no coordinator overlay. Four focused tests pass (72.81s), including 93 injected
  read/publication faults with exact suffix recovery/retry, wrong-mode/resource refusal, external
  preparation, expiry and first-owner/charge/rebase checks. Initial test compilation rejected two
  temporary byte-array borrows; owned fixture bindings corrected them. Command under the
  3G/4G/512M scope: `CARGO_BUILD_JOBS=1 cargo test -p uste-replay --locked --offline
  disk_inventory_write -- --test-threads=1 --nocapture`. Broader gate passed in the same scope:
  `CARGO_BUILD_JOBS=1 cargo test -p uste-txn -p uste-replay -p uste-graph -p uste-memory
  -p uste-memory-adapter -p uste-ingest -p uste-spatial --all-targets --all-features --locked
  --offline -- --test-threads=1`: 16 disk-index/graph fault tests (535.43s), 17 coordinator/replay
  tests (196.68s), 18 transaction tests (5.80s), 13 authorization tests (0.89s), M1 process tests
  (30.92s), all other selected graph/ingestion/spatial/memory cases passed. Unchanged storage
  code already passed its complete regression in `fd44ca2`. Then `CARGO_BUILD_JOBS=1 cargo test
  --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1`
  passed 51 active native unit tests (43.08s) and three process-loss/CLI tests (12.29s); two
  unchanged exact-scale oracle ignores remain. Strict workspace all-target/all-feature Clippy
  (5.72s), native all-target Clippy (2.35s) and strict storage/transaction Rustdoc (0.95s) pass,
  all one-job/locked/offline with warnings denied. Format/whitespace, docs (175 links) and task
  graph (68 tasks) pass. Host preflight 36 GiB available RAM/3.9 GiB free swap; sampled scope peak
  1,498,120,192 bytes, zero sampled swap (not the final whole-run peak). Unwired next-increment
  authorization files are excluded from this commit and its verification claims. No consumer
  inventory authorization or qualification is implied yet.

- Tested on pushed `a3d80f3` plus this increment: [Decision 0101](docs/decisions/0101-disk-blob-inventory-append.md)
  adds explicit bounded disk-backed inventory append and catalog refresh, without legacy history
  maps. Prepared overlays preserve first references, inventory protection, exact namespace bytes
  and binding counts; successful certification installs metadata without further fallible I/O.
  Six focused tests passed (49.82s), including 144 append fault attempts across empty/populated
  bases and a pending overlay, and 45 selected refresh mutation faults. Strict storage Clippy
  passed (2.30s). Added first-commit/empty-inventory coverage. Full regression passed under
  `systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M`:
  `CARGO_BUILD_JOBS=1 cargo test -p uste-storage -p uste-txn -p uste-replay --all-targets
  --all-features --locked --offline -- --test-threads=1` (109 storage unit tests, 721.93s,
  including the complete catalog/cold fault matrices with no skips; 13 coordinator/replay tests,
  121.82s; 18 transaction tests, 5.96s; 13 authorization tests, 0.90s; portable recovery, 15.67s;
  all adapter/process and other selected tests pass). Then `CARGO_BUILD_JOBS=1 cargo clippy
  --workspace --all-targets --all-features --locked --offline -- -D warnings` passed (7.02s),
  and `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-storage -p uste-txn
  --no-deps --locked --offline` passed (1.98s). Preflight 36 GiB available RAM/3.9 GiB free swap;
  sampled scope peak 829,804,544 bytes, zero sampled swap (not the final whole-run peak).
  Formatting, whitespace, docs (174 links), task graph (68 tasks) pass. M1 crate sources remain
  identical to pinned `b9689f3`; lockfile digest remains
  `7ed2b533b3c801250a89e008b4ca26c48f16400e38224d8a63447c0393aaa97b`.
  Unwired next-increment coordinator/test files are excluded from this storage commit and these
  verification claims. Coordinator write bridging, authorized charge transfer and all
  qualification gates remain open; no task is newly complete.

- Tested on pushed `eb7c229` plus this increment: [Decision 0100](docs/decisions/0100-native-disk-storage-recovery.md)
  connects map-free storage cold open to authenticated transaction recovery and both disk
  development drivers. Reports use actual residency modes and separate last-owner cold work;
  sampler supervision requires mode/count consistency. The nonempty transaction/inventory cursor
  test passes (0.13s). First native run had seven ResourceLimit/open failures because the new
  one-page catalog lookup allowed only one visit; the existing primitive needs binary search
  plus entry-read visits. Two visits fixed those paths; a new report assertion then distinguished
  one physical/authenticated read from one cache hit, now exposed explicitly. The next native run
  exposed the process-loss test's all-manifest assumption: optional storage catalog reconstruction
  can precede refused graph recovery. The test now requires all old graph/coordinator manifests
  byte-identical, exactly one added catalog, and no changes on another refused open.
  Under `systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M`,
  with 36 GiB available RAM/3.9 GiB free swap and one job/thread, verification passed:
  `CARGO_BUILD_JOBS=1 cargo test -p uste-storage --locked --offline blob_metadata --
  --test-threads=1 --skip blob_metadata_rebuild_faults_preserve_admissible_durable_fallback
  --skip disk_blob_cold_recovery_faults_keep_journal_authority_and_restart_exactly`
  (11 tests, 16.47s); those two fault sweeps passed in D99 and were not rerun for this counter-only
  storage change. `CARGO_BUILD_JOBS=1 cargo test -p uste-txn --all-targets --all-features
  --locked --offline -- --test-threads=1` passed. Final native gate:
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1`: 51 active unit tests (42.96s), three owned-process tests
  (12.42s), two unchanged exact-scale oracle ignores. Strict Clippy passed for workspace
  all-target/all-feature (6.02s) and experiment all-target (1.79s), both locked/offline with
  `-- -D warnings`; strict storage/transaction Rustdoc passed (1.98s). Both format checks,
  whitespace, documentation (173 links) and task graph (68 tasks) pass. No benchmark target or
  execution ceiling is lowered; T-20/T-19 and qualification remain open.

- Tested on pushed `6ae070a` plus this increment: [Decision 0099](docs/decisions/0099-disk-blob-cold-recovery.md)
  adds opt-in cold storage recovery without resident certificate/blob/inventory/namespace maps.
  Both physical scans verify payloads with explicit repeated-byte and tail-descriptor admission;
  catalog validation/staging precedes journal repair, resynchronization and replay. Existing or
  rebuilt catalogs preserve exact frontier/counts; legacy nonempty inventory writes refuse
  before I/O in this mode until the disk-aware append path exists. Five focused cold tests pass
  (2.03s) with strict storage Clippy. A stale-root test initially reused a scripted entropy range,
  correctly causing immutable-name AlreadyExists; fresh per-open counter ranges fix the fixture,
  not the storage collision behavior. Six focused cold tests then passed (309.21s), including
  191 exhaustive ready-open read/resync boundaries and 16 selected cold-rebuild write boundaries:
  621 attempts, 620 failures, one optional absent-root OpenExisting/CrashAfter non-crash, and exact
  restart after every attempt. Command under 3G/4G/512M scope: `CARGO_BUILD_JOBS=1 cargo test
  -p uste-storage --locked --offline disk_blob_cold -- --test-threads=1 --nocapture`, followed by
  strict storage all-target/all-feature Clippy (0.82s). Sampled scope peak 411,443,200 bytes,
  zero sampled swap. The broader storage/transaction/replay regression passed, partitioned
  with `--skip disk_blob_cold_recovery_faults_keep_journal_authority_and_restart_exactly` because
  that exact sweep already passed against the unchanged code. This is not a removed test or
  reduced fault matrix. Exact second gate under the same 3G/4G/512M scope, one job/thread:
  `CARGO_BUILD_JOBS=1 cargo test -p uste-storage -p uste-txn -p uste-replay --all-targets
  --all-features --locked --offline -- --test-threads=1
  --skip disk_blob_cold_recovery_faults_keep_journal_authority_and_restart_exactly` passed:
  101 storage unit tests (350.80s, including the complete 324-attempt catalog mutation sweep),
  13 coordinator/replay integration tests (120.95s), 17 transaction tests (5.68s), 13 authorization
  tests (0.88s), storage portable recovery (15.22s), real process/adapter and all other selected
  tests. Then `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked
  --offline -- -D warnings` passed (5.72s), followed by `CARGO_BUILD_JOBS=1
  RUSTDOCFLAGS="-D warnings" cargo doc -p uste-storage -p uste-txn --no-deps --locked --offline`
  (2.23s). Host preflight: 36 GiB available RAM/3.9 GiB free swap; sampled scope peak
  570,609,664 bytes, zero sampled swap (not the final whole-run peak). Formatting/whitespace,
  docs (172 links) and task graph (68 tasks) pass. Native adapters still use the previous mode;
  disk-aware nonempty inventory append and benchmark qualification remain open. No task is newly complete.

- Tested on `5b19bcb` plus this increment: [Decision 0098](docs/decisions/0098-storage-blob-disk-catalog.md)
  adds the journal-derived encrypted storage blob catalog, bounded one-inventory rebuild and
  independent disk/journal admission. The initial five focused tests pass (3.76s), including
  exact first references, repeated inventories, zero-byte namespace accounting, codec shapes,
  cold-owner rejection, exact aggregate rewrite budgets and five authenticated false catalogs.
  The first implementation incorrectly retained old-revision descriptors for unchanged families;
  index-v1 correctly rejected this. Rebuild now rewrites and budgets every nonempty family at
  each staged revision. Initial module-path/type/lint errors were repaired without suppression.
  Final mutation coverage passes all 108 observed boundaries/324 attempts, with 323 reported
  failures and one optional absent-file RemoveFile/1/CrashAfter non-crash. All restarted roots
  admit exactly. Selected early/middle/late admission reads add 27 fail-closed fault attempts;
  terminal certificate and later-inventory corruption also fail closed. This is not an exhaustive
  read-boundary sweep. Final gate exited 0 under `systemd-run --user --scope -p MemoryHigh=3G
  -p MemoryMax=4G -p MemorySwapMax=512M bash -lc '...'`, one job/thread:
  `CARGO_BUILD_JOBS=1 cargo test -p uste-storage --all-targets --all-features --locked --offline
  -- --test-threads=1 --nocapture` (96 unit tests, 358.40s; all integration/process tests pass,
  including portable recovery 14.68s); `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets
  --all-features --locked --offline -- -D warnings` (7.10s); `CARGO_BUILD_JOBS=1
  RUSTDOCFLAGS="-D warnings" cargo doc -p uste-storage --no-deps --locked --offline` (0.91s).
  Formatting, whitespace, docs (171 links) and task graph (68 tasks) checks pass. Host preflight
  showed 37 GiB available RAM/3.9 GiB free swap; sampled scope peak 529,092,608 bytes and zero
  sampled swap (not the final whole-run peak). No qualifying benchmark ran. M1 source crates
  remain unchanged from `b9689f3`; lock SHA-256 remains
  `7ed2b533b3c801250a89e008b4ca26c48f16400e38224d8a63447c0393aaa97b`.
  At that increment journal open/append still retained resident blob collections; its tests
  isolate only the catalog capability. Decision 0099 adds opt-in cold recovery, not disk-aware
  nonempty append. T-20, T-19 and benchmark/release gates remain unchecked.

- [Decision 0097](docs/decisions/0097-disk-coordinator-blob-reads.md) connects admitted disk
  owner/first-reference metadata to bounded certificate/inventory proofs and raw committed reads.
  It uses no resident storage blob-map lookup. Base first-reference roots avoid discovery; overlay
  owners require a bounded suffix, and legacy bases require a bounded prefix. Every path preserves
  uncertainty, scope, exact-reference and partial-output refusal. Consumer authorization remains a
  separate capability, not an implicit expansion of the metadata facade. Revision-only certificate
  proofs now derive the digest in the authenticated chain pass, with no uncharged preliminary read.
  Initial certificate tests passed 11 (0.79s), then 2 coordinator tests passed (16.38s, including
  51 resident-certificate overlay fault cases). A new test's unnecessary qualification compile
  error was fixed. Its legacy cold setup then correctly refused a one-pass allowance: a legacy
  owner needs a correspondence pass plus earliest-owner validation. The new fixture now explicitly
  admits those two passes; original fixture limits and independent read-refusal cases are unchanged.
  Expanded disk-certificate base/overlay coverage passed 3 tests (41.38s), including 31 boundaries/
  93 injected I/O failures. The new uncertain-commit case passed separately (0.43s).
  Final `aed1062` plus this increment passed `CARGO_BUILD_JOBS=1 cargo test -p uste-storage
  -p uste-txn -p uste-replay --all-targets --all-features --locked --offline -- --test-threads=1`:
  89 storage tests (173.76s), 13 coordinator recovery tests (119.32s), 17 transaction tests (5.74s),
  13 authorization tests (0.90s), and real adapter/process recovery. `CARGO_BUILD_JOBS=1 cargo test
  --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1`
  passed 51 unit tests (42.75s; 2 unchanged oracle ignores) and 3 CLI tests (11.85s). Workspace
  all-target/all-feature and native all-target strict clippy passed; `CARGO_BUILD_JOBS=1
  RUSTDOCFLAGS="-D warnings" cargo doc -p uste-storage -p uste-txn --no-deps --locked --offline`
  passed. Format/whitespace/docs (170 links)/task graph (68 tasks) passed. Heavy commands used one
  job/thread and 3G/4G/512M systemd limits; preflight 37 GiB available RAM/3.9 GiB free swap;
  sampled scope peak 341,057,536 bytes and zero sampled swap (not a final whole-run peak).
  T-20 remains open. Next replace remaining storage blob/inventory/namespace recovery and append
  accounting collections with authenticated disk metadata and bounded overlays; the separate
  authorized disk-blob consumer capability and all qualifying performance/resource campaigns remain.

- [Decision 0096](docs/decisions/0096-committed-blob-reference-proofs.md) adds a bounded
  committed-blob proof/read path that does not consult resident blob maps. It rechecks an exact
  certificate and inventory, admits encoded bytes before reads and authenticated reference count
  before reference-vector allocation, and retains only owner-bound evidence plus one exact
  reference. It neither grants principal access nor establishes first-owner/quota accounting.
  Five focused tests passed (1.47s initially; 1.49s after fixing the strict lint's unused legacy
  decoder finding). They cover four metadata-I/O boundaries/12 injected failures and restart,
  empty/exact payloads, absent/orphan/mismatched references, corruption, append continuity and
  owner invalidation. Review added exact refusal I/O counts and same-content foreign-owner tests.
  On `6bb95e8` plus this increment, `CARGO_BUILD_JOBS=1 cargo test -p uste-storage -p uste-txn
  -p uste-replay -p uste-memory -p uste-memory-adapter --all-targets --all-features --locked
  --offline -- --test-threads=1` passed: 88 storage tests (173.26s), 9 coordinator recovery tests
  (78.32s), 17 transaction tests (5.68s), 13 authorization tests (0.89s), real adapter/restart
  tests and unchanged M1 process/corruption tests (30.42s). `CARGO_BUILD_JOBS=1 cargo clippy
  --workspace --all-targets --all-features --locked --offline -- -D warnings` and
  `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc -p uste-storage --no-deps --locked
  --offline` passed. Format, whitespace, docs (169 links) and task graph (68 tasks) passed.
  One job/thread, MemoryHigh=3G/MemoryMax=4G/MemorySwapMax=512M; preflight 37 GiB available RAM,
  3.9 GiB free swap; sampled final-gate peak 746,741,760 bytes, zero group swap. Both journal
  scans and append admission still retain blob metadata. Next connect proven reads to admitted
  disk owner/first-reference metadata, then replace recovery/accounting collections without
  dropping uniqueness, exact first ownership, quota or durability checks. T-20 remains open.

- [Decision 0095](docs/decisions/0095-map-free-certificate-recovery.md) implements opt-in
  certificate-map-free recovery and proof-bound ordinary root APIs. Both scans and appends skip
  anchor insertion; range work charges certificate-proof re-reads. Nine focused storage tests
  passed (0.66s), including map-free open/append/staging/publication, exact byte accounting and
  no callbacks/repairs on admission or late corruption. Proven discovery covers 3 read boundaries/
  9 injected failures. Root initializer placement/missing-field compile errors and a test-only
  clone lint were corrected; review also preserved the existing resync poison guard. Native
  integration adds profile-derived proof allowances and reports actual zero certificate-anchor
  residency; a misplaced maintenance method caught by formatting was moved into its intended impl.
  The separate map-free graph fault sweep passed (275.07s): 231 I/O boundaries/693 attempts,
  of which six optional missing-file crash-after attempts did not crash (OpenExisting 34/37/38/39/45,
  RemoveFile 1); 687 actual injected failures refused provisional state and recovered only old or
  terminal roots. The original resident-map sweep remains intact. On `7040277` plus this increment,
  `CARGO_BUILD_JOBS=1 cargo test --workspace --all-targets --all-features --locked --offline
  -- --test-threads=1` passed, including 16 disk-graph tests/both sweeps (531.09s), 82 storage
  tests (173.61s), 9 coordinator recovery tests (78.95s), 17 transaction tests (5.78s), and M1
  process recovery/corruption (31.36s). The final test-only checkpoint regression and one-byte-short
  opening allowance were then verified with `CARGO_BUILD_JOBS=1 cargo test -p uste-storage
  certificate_proof_tests --locked --offline -- --test-threads=1`: 10 passed (0.78s).
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passed 51 unit tests (42.64s; 2 unchanged exact-profile
  oracle ignores) and 3 CLI/process tests (11.86s). Workspace all-target/all-feature strict clippy,
  native all-target strict clippy, and `RUSTDOCFLAGS="-D warnings" cargo doc -p uste-storage
  -p uste-txn -p uste-graph --no-deps --locked --offline` passed. Format, whitespace, documentation
  (168 links) and task graph (68 tasks) checks passed. All heavy commands used one job/thread and
  systemd process-group limits MemoryHigh=3G/MemoryMax=4G/MemorySwapMax=512M; preflight 37 GiB
  available RAM/3.9 GiB free swap; sampled peaks 1,490,661,376 bytes (workspace), 434,462,720 bytes
  (final native gate), zero group swap. M1 crate sources and lockfile remain unchanged at the pinned
  handoff. T-20 remains partial: next implement authenticated disk-backed blob lookup/accounting
  and remove the remaining resident storage metadata; preserve all qualifying benchmark prerequisites.

- [Decision 0094](docs/decisions/0094-disk-certificate-anchor-proofs.md) implements bounded
  certificate-chain proofs directly from disk without the historical anchor map, plus proven
  index lookup/cursors and scratch target admission. Proofs bind the exact live journal instance;
  historical reads permit its successful append while staging requires the exact frontier.
  A retained proof holds no key or filesystem lock. The first run passed 3 tests and rejected
  a test's empty staged root; corrected the fixture to construct a real run, preserving the
  nonempty-root contract. Four proof tests then passed (0.28s), existing four stage tests passed
  (0.83s before the final owner-identity addition), and workspace strict lint passed. Final
  append/reopen coverage passed all 4 proof tests (0.28s). Final source on `eaa4352` plus this
  increment passed `CARGO_BUILD_JOBS=1 cargo test -p uste-storage -p uste-graph --all-targets
  --all-features --locked --offline -- --test-threads=1`, including 77 storage tests (173.02s),
  15 disk-graph tests/full fault sweep (253.42s) and real adapter/process tests. Workspace
  all-target/all-feature strict clippy and `RUSTDOCFLAGS="-D warnings" cargo doc -p uste-storage
  --no-deps --locked --offline` passed. Release native regressions passed via `CARGO_BUILD_JOBS=1
  cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline
  -- --test-threads=1`: 51 unit tests (42.97s; 2 existing oracle ignores), 3 CLI tests (11.92s).
  Format, whitespace, docs (167 links) and task graph checks passed. Scope 3G/4G/512M, one
  job/thread; preflight 37 GiB available/3.9 GiB free swap; sampled peak 1,103,712,256 bytes,
  zero swap. This increment alone did not remove resident recovery maps or qualify T-20. Decision
  0095 above subsequently integrated proof-bound roots and map-free certificate recovery; remaining
  blob metadata and qualification work are the current next actions.

- Decision 0093's native process regression now also restores saved derived manifests from a
  killed revision-two child beneath an unchanged completed revision-four journal. Open refuses
  without root mutation; separate resume/open/query processes recover the two-revision suffix
  and reproduce all 384 oracle outputs and the pre-restore digest. Newer manifests are preserved
  inside the isolated synthetic test fixture until its normal cleanup; source/journal files are
  untouched. On `1147329` plus this test increment, `CARGO_BUILD_JOBS=1 cargo test --release
  --manifest-path experiments/t20-bench/Cargo.toml --test disk_process_loss --locked --offline
  -- --test-threads=1` passed all 3 tests (12.21s); native all-target strict clippy passed.
  Scope 3G/4G/512M, one job/thread; preflight 37 GiB available RAM/3.9 GiB free swap. No
  intermediate-stage SIGKILL or hardware power-loss qualification is inferred. Next address
  storage's resident certificate/blob metadata and its explicit authenticated disk lookup contract.

- [Decision 0093](docs/decisions/0093-native-streamed-suffix-resume.md) connects native resume
  to private multi-revision graph recovery with fixture-derived count/shared-byte bounds and
  separate suffix-merge diagnostics. Open/query require current graph/metadata bases before any
  repair, while ready-root resynchronization preserves slots. Native and memory development caps
  are unchanged. Two/three-revision native gaps now recover and match all 384 separate-oracle
  queries; the formerly unsupported valid two-revision fixture is now a positive regression,
  while missing-base refusal and core count/byte-overage tests remain intact. The first focused
  release run passed 5 native tests (37.37s) and strict native lint. An unused import exposed by
  removing the old pending-root repair block was removed. Final source on `d38e5ca` plus this
  increment passed `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path
  experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1`: 51 unit tests
  (44.42s; 2 unchanged exact-profile oracle ignores), 3 CLI SIGKILL/sampling tests (10.23s).
  Native all-target strict clippy, format, whitespace, docs (166 links) and task graph checks
  passed. Scope 3G/4G/512M, one job/thread, preflight 36 GiB available/3.9 GiB free swap;
  sampled peak 334,061,568 bytes, zero swap. Next extend native process coverage of stale-root
  gaps, then remove remaining resident storage metadata. No new intermediate-stage process-kill
  or qualifying benchmark result is claimed yet; T-20 remains open.

- [Decision 0092](docs/decisions/0092-private-streamed-graph-suffix-recovery.md) connects
  coordinator-owned authenticated suffix validation to private staged graph recovery and terminal
  publication. Exact request/result, retry/collision and first-owner checks remain mandatory;
  metadata publication rejects generation-zero intermediate graph roots. Shared range bytes and
  caller cache are retained; graph work has explicit total-count and per-revision limits.
  Initial graph integration compile exposed an attempted clone of the intentionally non-Clone
  admitted base and an unnecessary test qualification; corrected the API to consume ready
  `GraphDiskLiveState` directly. Workspace strict lint passed before the final test additions.
  Two initial end-to-end tests passed in 1.41s (three revisions, lagging metadata, ready reopen,
  retry/authorization checks, exact/minus-one count/byte bounds). The first fault harness assumed
  an external `MemoryFileSystem` clone (available only to its own crate tests); fixed by recreating
  deterministic fixtures. Its first sweep then exposed a harness assumption: `CrashAfter` does not
  crash after an optional `NotFound`. The corrected sweep passed in 229.73s: 211 operation
  boundaries / 633 attempts, with 6 explicitly logged no-crash optional-error cases and exact
  terminal-result checks; all actual injected failures refuse a coordinator and preserve old-or-final
  roots. No fault behavior or production check was relaxed. Subsequent additions boundedly resync
  an already-current root, test every resync flush failure, reject wrong preparation before domain
  advancement, and prove metadata rebase/exact retry/collision/new-write behavior. These 5 focused
  tests passed (6.04s), plus 3 transaction-cursor/scope tests (0.34s) and workspace strict lint.
  Final source tested on `b03d8e8` plus this increment: `CARGO_BUILD_JOBS=1 cargo test
  --workspace --all-targets --all-features --locked --offline -- --test-threads=1` passed,
  including all 15 disk-graph tests/full sweep (265.18s), 73 storage tests (180.63s), 9 checkpoint/
  first-owner tests (83.25s), 17 coordinator tests (5.97s) and M1 process tests (31.51s).
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passed 50 unit tests (40.39s; 2 existing exact-profile
  oracle ignores) and 3 CLI process/sampling tests (10.20s). Native all-target strict clippy and
  `RUSTDOCFLAGS="-D warnings" cargo doc -p uste-storage -p uste-txn -p uste-graph --no-deps
  --locked --offline` passed. Gates ran sequentially under 3G/4G/512M, one job/thread;
  preflight 30 GiB available RAM/3.9 GiB free swap; sampled peak 1,207,533,568 bytes, zero swap.
  Format, diff whitespace, docs (165 links) and task graph (68 tasks) checks passed. M1 crate
  sources and lock digest remain unchanged. Next connect the native recovery driver; resident
  storage metadata and qualifying BM-01/BM-06 remain open, so T-20 is not complete.

- [Decision 0091](docs/decisions/0091-unpublished-certified-recovery-roots.md) adds bounded
  certified-revision scratch merges and explicitly unpublished read roots for multi-step recovery.
  Durable root slots still require the exact current frontier; intermediate stages are never
  discoverable and failures leave only orphanable derived files. Three initial stage tests passed
  in 0.84s, including every observed merge I/O occurrence with error/crash-before/crash-after.
  All four focused stage tests passed (0.86s), including binding/corruption/poison/visitor cases.
  Tested on `77b3ba0` plus this increment: `CARGO_BUILD_JOBS=1 cargo test -p uste-storage
  -p uste-txn -p uste-replay -p uste-graph --all-targets --all-features --locked --offline
  -- --test-threads=1` passed, including 73 storage tests (178.23s), 9 checkpoint/first-owner
  fault tests (83.55s), 17 coordinator tests (6.03s), disk graph and real-process recovery.
  Workspace all-target/all-feature strict clippy and `RUSTDOCFLAGS="-D warnings" cargo doc
  -p uste-storage --no-deps --locked --offline` passed. Gates used the established 3G/4G/512M
  scope, one job/thread; sampled peak 982,634,496 bytes, zero swap; preflight 28 GiB available
  RAM and 3.9 GiB free swap. Docs/task checks passed (164 links, 68 tasks). Archived layout
  observation matches its retained raw output and all prior result/work counters exactly.
  Graph/coordinator integration and resident storage metadata remain open. Next connect private
  staged graph replay to terminal coordinator validation/publication.

- [Decision 0090](docs/decisions/0090-immutable-cached-page-layouts.md) retains fixed structural
  offsets for immutable authenticated cached pages. Every hit rechecks complete header context,
  especially family; failed parsing never installs a layout and eviction/clear discard it.
  No cache budget/capacity increase, format change or authorization relaxation. Initial compile
  caught two uncached-cursor field accesses after the layout refactor; corrected to use the
  layout fields, retaining full uncached parsing. The initial 9 index tests passed (2.76s).
  Tested on `fd9db90` plus this increment: all 11 index tests passed (4.90s), including new
  mutation/memoization/substitution/replacement cases. `CARGO_BUILD_JOBS=1 cargo test --workspace
  --all-targets --all-features --locked --offline -- --test-threads=1` passed, including 69
  storage tests (179.41s), 9 disk graph tests (31.72s), 9 checkpoint/first-owner fault tests
  (82.99s), 17 coordinator tests (6.02s) and M1 process recovery/corruption tests (31.11s).
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passed 50 unit tests (38.92s, 2 existing exact-profile
  ignores) and 3 CLI tests (9.95s). Workspace all-target/all-feature and experiment all-target
  strict clippy passed. Format/docs/task checks passed (163 links, 68 tasks). Gates ran
  sequentially under 3G/4G/512M, one job/thread; sampled peak 2,038,661,120 bytes, zero swap;
  preflight 30 GiB available RAM, 3.9 GiB free swap.
  The separate native query observation at pushed `77b3ba0` (binary SHA-256
  `d323e9fd06d0986a15db8186c8112f8df8bde9bb3f7de2a440fc9ab2851fca3c`) passed all 384
  queries on the retained 10,000-entity fixture: 266.46s elapsed, setup/query 11,925/254,454ms,
  peak RSS 265,084 KiB, zero swaps. Same scoped `linux-disk-query` command, oracle and 900s
  timeout as Decision 0087, with separate `query-layout-*` output files; sampled scope peak
  277,655,552 bytes. Query time was about 2.64x lower than the earlier 672,641ms observation.
  Output/oracle digests, cache hits/misses/residency, enumerated fragments, adapter read bytes
  and zero evictions are unchanged. [Pinned raw evidence](docs/evidence/native-disk-layout-development.json)
  records this single uncontrolled-cache development comparison, not a qualifying latency or
  larger-than-memory pass. Next implement unpublished certified-revision scratch roots for
  multi-step graph recovery; preserve the exact-current-frontier rule for durable root slots.

- [Decision 0089](docs/decisions/0089-allocation-free-page-validation.md) replaces temporary
  page-fragment vectors with complete allocation-free validation and a retained last-key slice.
  No cache-hit validation, format, cryptography, budgets or authorization checks are removed.
  Differential old-validator comparison passed all byte mutations and field boundaries in 0.46s;
  strict lint initially caught two test-only clones of a Copy descriptor, which were removed.
  A private native harness now compares all 384 queries under the unchanged 64 MiB public budget
  and a one-page regression budget, requiring actual evictions with exact oracle equivalence.
  Tested on `0bc8860` plus this increment: `CARGO_BUILD_JOBS=1 cargo test --release
  --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1`
  passed 50 unit tests (40.43s, 2 existing exact-profile ignores) and 3 CLI tests (11.93s),
  including actual native evictions with all 384 oracle results unchanged. Experiment all-target
  and workspace all-target/all-feature strict clippy passed. `CARGO_BUILD_JOBS=1 cargo test
  -p uste-storage -p uste-graph --all-targets --locked --offline -- --test-threads=1` passed,
  including 67 storage tests (175.49s), both parser matrices, 9 disk graph tests (31.31s),
  encrypted restart and real process-loss tests. Gates ran sequentially under 3G/4G/512M,
  one job/thread; sampled peak 1,037,750,272 bytes, zero swap; preflight 29 GiB available RAM
  and 3.9 GiB free swap. Format/docs/task checks passed (162 links, 68 tasks).
  Next safely retain validated cache metadata, then measure changed query code on the retained
  larger fixture; component pressure is not qualifying BM-01/BM-06 acceptance.

- [Decision 0088](docs/decisions/0088-resumable-authenticated-transaction-ranges.md) adds an
  opaque transaction-range cursor with one shared group/encoded-byte allowance, pinned scope and
  authenticated frontier, per-step reauthentication, sticky failure and terminal-only reporting.
  `visit_transactions` uses the cursor; canonical request/inventory ownership no longer retains
  encrypted/decrypted group buffers during the caller's index/reducer callback. This is not
  multi-revision graph-root publication or removal of storage metadata maps. Three new cursor
  tests passed in 0.34s, including every observed read fault, late corruption, exact/minus-one
  certificate-plus-group budget, owned inventory and scope/frontier substitution before I/O.
  Tested on `5a19c37` plus this increment: `CARGO_BUILD_JOBS=1 cargo test --workspace
  --all-targets --all-features --locked --offline -- --test-threads=1` passed, including 65
  storage tests (173.85s), 17 coordinator tests (5.86s), replay faults and M1 process recovery.
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passed 50 unit tests (28.20s, 2 existing exact-profile
  ignores) and 3 CLI tests (12.83s). Workspace all-target/all-feature and experiment all-target
  strict clippy passed; `RUSTDOCFLAGS="-D warnings" cargo doc -p uste-storage -p uste-txn
  --no-deps --locked --offline` passed. Commands ran sequentially with one job/thread under
  the 3G/4G/512M scope; sampled peak 1,538,392,064 bytes, zero swap. Docs/task checks pass
  (161 links, 68 tasks). Archived native raw reports match retained outputs exactly; M1 sources
  and lockfile are unchanged. Latest headroom: 29 GiB available RAM, 3.9 GiB free swap.
  Next remove repeated page-parser work and verify
  actual native cache evictions without changing qualifying cache/workload thresholds.

- [Decision 0087](docs/decisions/0087-native-development-cache-pressure-scale.md) gives native
  development commands a separate 10,000-entity ceiling; memory-backed checks stay at 1,000
  and qualifying native profiles are still refused. Admission precedes filesystem/process access.
  Shared batch streaming now follows adapter admission and tests pin the larger plan at 23
  revisions / 210,001 operations, with every batch respecting 10,000 operations / 16 MiB.
  On pushed `4503a25` plus this increment, `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path
  experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1` passed 50 unit tests
  (2 existing exact-profile ignores) in 28.53s and 3 CLI tests in 12.88s. Experiment strict
  all-target clippy passed. The 3G/4G/512M scope peaked at a sampled 333,647,872 bytes with zero
  swap; host preflight had 34 GiB available RAM and 3.9 GiB free swap.
  The completed native development observation ran at pushed `5a19c37`, binary SHA-256
  `20614f7cc71d5b13724ed3cd73679bb5f31fa4ad9e7021d17b48d65320da357a`, with artifacts retained in
  `experiments/t20-bench/target/native-pressure.kedpTk`. Separate `oracle-summary --entities 10000`
  completed in 3.13s / 10,836 KiB peak RSS (41,249-byte summary). Scoped `linux-disk-create
  --root <artifact-dir> --password-file <artifact-dir>/password --entities 10000` under
  `/usr/bin/time -v` and `timeout --signal=TERM --kill-after=10s 900s` exited 0 in 51.96s,
  peak RSS 266,520 KiB, no swaps; exact frontier 23 and final counts
  `[110001,210001,100000,100000,100000,300000,1,1]`. Adapter read/write returned bytes were
  7,698,911,038 / 2,881,473,171, including setup/rewrite work, not device traffic. The approximately
  2.7 GiB retained directory includes prior derived runs; T-35 reclamation is not implied.
  Separately scoped `linux-disk-query` with the same root/password/count and
  `--oracle-file <artifact-dir>/oracle-summary` exited 0 under the same 900s timeout and
  3G/4G/512M scope: all 384 queries matched, 701.28s elapsed, 264,896 KiB peak RSS, no swaps.
  Setup/query durations were 28,553/672,641ms. Diagnostic mixed-depth empty-cache p99 was
  8,308,109,947ns, not the qualifying warm per-depth metric. Output digest is
  `1330498a4b131c827113fd3a067e807eec804d83babb62d5d1fa64edebe83367`.
  The run recorded 420,631,561 cache hits, 806,885 misses, 1,080,841,044 enumerated fragments,
  16,577,452,325 query adapter read bytes, zero query writes and zero query cache evictions.
  Thus query equivalence passes at this development size, but query cache-pressure does not
  follow from the larger fixture. [The pinned raw reports](docs/evidence/native-disk-10000-development.json)
  retain all measurement boundaries. Neither larger-than-RAM nor qualifying BM-01/BM-06
  acceptance is claimed. Repeated page validation/allocation is a concrete next scalability target.

- [Decision 0086](docs/decisions/0086-ordered-cache-eviction.md) replaces linear oldest-page
  selection with a bounded ordered recency map. It preserves exact LRU/reference behavior,
  complete cache identity and zeroizing ownership; malformed/duplicate insertions precede
  mutation and clock overflow clears both maps. New logical metadata allowances charge 8 KiB
  fixed plus 1 KiB per 16 KiB page, fixing the old 96-byte inline-field undercount. Minimum
  budget is now 25,600 bytes; unchanged 64 MiB default admits 3,854 pages. These are not measured
  allocator/RSS bounds. Initial six index tests passed in 1.81s, including 20,000 independent
  vector-LRU trace operations; workspace strict clippy passed. A subsequent full-default-capacity
  test exposed redundant `size_of` qualifications at compilation; these were corrected without
  suppressions. On pushed `622a66c` plus this increment, `CARGO_BUILD_JOBS=1 cargo test --workspace
  --all-targets --all-features --locked --offline -- --test-threads=1` passed, including M1 process
  recovery, nine disk-graph tests (31.10s), nine coordinator-checkpoint tests (81.41s) and 65
  storage unit tests (173.83s). Release experiment tests passed 48 unit tests (2 existing ignores)
  in 27.98s and three real CLI tests in 12.77s using `cargo test --release --manifest-path
  experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1`. Both workspace and
  experiment strict all-target clippy passed. All ran sequentially under MemoryHigh=3G,
  MemoryMax=4G, MemorySwapMax=512M, one job/thread.
  Host preflight: 35 GiB available RAM, 3.9 GiB free swap and approximately 1,000 GiB free Btrfs
  space; sampled scope peak 1,556,017,152 bytes, zero swap. The final default-capacity test uses
  nonzero bytes verified across every page. After scoped `cargo test -p uste-storage --lib
  --locked --offline --no-run`, `/usr/bin/time -v target/debug/deps/uste_storage-3a9cf4c9b7716745
  --exact index::tests::cache_layout_allowances_and_default_capacity_are_explicit --test-threads=1`
  passed in 0.48s with 67,836 KiB whole-process maximum RSS, no swaps, 3,854 retained pages and
  67,098,624 accounted cache bytes before eviction. This is one debug-process observation, not
  allocator isolation, a universal bound or qualification. Strict storage docs and docs/task
  checks pass (159 links, 68 tasks). Next exercise native disk cache pressure with explicit
  development-scale resource admission before qualifying campaigns. T-20/T-19 remain open.

- [Decision 0085](docs/decisions/0085-cached-index-operation-telemetry.md) retains fixed-size
  cached exact/predecessor/prefix primitive work on successes and failures. Checked cumulative
  overflow invalidates only diagnostics, never changes a read result, and cache clearing preserves
  counters. The authorized reader requires current maintenance authority for reports. Native query,
  warm-up and paired sample counters explicitly exclude uncached/recovery/publication work and
  retain false completeness/device flags. Oracle and workload thresholds are unchanged.
  On pushed `8e13001` plus this increment, scoped one-job/thread `cargo test -p uste-storage
  --lib --locked --offline -- --test-threads=1` passed 61 tests in 171.61s;
  `cargo test -p uste-graph --test disk_index --locked --offline -- --test-threads=1` passed
  9 tests in 31.15s, including the adapter-fault, authorization and reference matrices.
  Workspace all-target/all-feature strict clippy passed. Initial clippy rejected redundant
  mutable borrows introduced by instrumentation; these were removed without lint suppression.
  Scope limits were MemoryHigh=3G, MemoryMax=4G, MemorySwapMax=512M; sampled peak 355,876,864
  bytes, zero swap. Scoped `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path
  experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1` passed 48 unit tests
  (2 existing exact-profile ignores) in 28.01s and 3 CLI tests in 12.73s. The first native run
  caught the supervisor's obsolete `not-measured` label; worker/validator now agree on partial
  cached primitives and the validator rejects obsolete/complete/device labels. Experiment strict
  all-target clippy and `RUSTDOCFLAGS='-D warnings' cargo doc -p uste-storage -p uste-txn
  --no-deps --locked --offline` passed. Sampled final native scope peak 310,415,360 bytes,
  zero swap. Docs/task checks pass (158 links, 68 tasks).
  Host preflight: 35 GiB available RAM, 3.9 GiB free swap. T-20/T-19 remain open; no qualifying
  campaign ran. Next address cache eviction complexity/accounting and remaining resident storage
  metadata while preserving authenticated recovery and campaign prerequisites.

- [Decision 0084](docs/decisions/0084-native-adapter-io-observation.md) adds fixed-size native
  adapter-call/failure and requested/returned-byte observation. Forwarded results and durability
  ordering are unchanged, including short I/O/errors; overflow invalidates reports rather than
  storage operations. Setup, query, warm-up and paired sample counters are separate and explicitly
  are not physical-device or complete authenticated-index measurements. No path/payload is retained.
  On pushed `5982002` plus this increment, the scoped release experiment suite passed 46 unit
  tests (2 existing exact-profile oracle tests ignored) in 27.95s and all 3 real CLI tests in
  12.81s; experiment all-target strict clippy passed. Command: `CARGO_BUILD_JOBS=1 cargo test
  --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline --
  --test-threads=1`, followed by the same manifest's all-target `cargo clippy --locked --offline
  -- -D warnings`. Native sampling verified nonzero setup/empty-cache reads, zero query writes,
  zero retained-cache returned bytes and the independent output digest.
  Final verification includes the exact-count assertion and the certificate-inclusive maximum
  journal-group boundary regression: `systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G
  -p MemorySwapMax=512M bash -lc 'CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 CARGO_NET_OFFLINE=true
  bash scripts/check.sh'` exited 0. Workspace tests, strict lint, documentation and prerequisite
  checks passed; isolated experiment tests passed 47 unit tests (2 existing ignores) in 466.01s
  and all 3 CLI tests in 254.79s. Sampled scope peak 2,586,546,176 bytes, zero swap.
  Host preflight: 35 GiB available RAM, 3.9 GiB free swap. Docs/task checks pass (157 links,
  68 tasks). Next retain authenticated index-operation
  counters across failure paths under maintenance authorization. No qualifying campaign ran.

- [Decision 0083](docs/decisions/0083-profile-derived-disk-driver-limits.md) routes both disk
  adapters through validated fixture/batch-derived admission, preparation and merge limits.
  It pins 212 journal groups, 30,001 preparation proofs and a three-million-entry largest
  family at exact size; every accepted profile's constructor validates without database work.
  Encoded fixture-kind/status/revision tests verify the per-proof byte premise. Native size
  guards remain intact. Admission now also uses the accepted 64 MiB cache, dropped between phases.
  Verified on pushed `4efc450` plus this increment under the established 3G/4G/512M scope,
  one job/thread: `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path
  experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1` passed 43 unit tests
  (2 existing exact-profile oracle tests ignored) in 27.84s and 3 real CLI tests in 12.79s;
  experiment all-target strict clippy passed. The first clippy run rejected an eight-argument
  helper; grouping batch identity and operations corrected it without suppressing the lint.
  `systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M
  /usr/bin/time -v timeout --signal=TERM --kill-after=10s 180s
  experiments/t20-bench/target/release/uste-t20-bench disk-engine-check --entities 1000`
  exited 0: all 384 comparisons passed at 1,000 entities/10,000 relationships, cold frontier 4,
  digest `698901177087f1d26e98857d5cee1e575b90dcacc77908c3a70eb6a4771b43e4`, 61.64s elapsed,
  maximum RSS 152,312 KiB, 64 MiB cache, 46,287,696 hits/370 misses. Sampled scope peak was
  153,366,528 bytes with zero swap use; this is not a 24 GiB reservation or qualifying campaign.
  Host preflight: 36 GiB available RAM, 3.9 GiB free swap. Docs/task checks pass (156 links,
  68 tasks). Next implement native query I/O accounting and retain its measurement boundaries,
  then validate campaign readiness and BM-06's independent protocol. T-20/T-19 remain open.

- [Decision 0082](docs/decisions/0082-aggregate-graph-admission-work-limits.md) corrects cold
  admission's aggregate lookup configuration: repeated proof visits/returned bytes use operation
  count and per-operation ceilings, not a single scan's physical capacity. Explicit budgets,
  checked runtime counters, cache-hit charging and remaining-budget clamps are unchanged.
  Physical run/scan limits remain fixed. New tests cover both per-operation maxima, zero and
  impossible budgets, configuration-product saturation and runtime overflow/exhaustion.
  Verified on pushed `b74f320` plus this increment under the 3G/4G/512M scope, one job/thread:
  `cargo test -p uste-graph --all-targets --locked --offline -- --test-threads=1` passed all
  48 graph tests, including unchanged exact-minus, corruption and fault recovery tests (nine
  disk integration tests, 31.38s). After extending the boundary test, `cargo test -p uste-graph
  --lib --locked --offline admission_lookup_work -- --test-threads=1` passed. The complete release
  experiment command passed 41 unit tests (2 existing exact-profile oracle tests ignored) in
  29.36s and 3 real CLI tests in 13.54s. Workspace and experiment all-target strict clippy and
  `RUSTDOCFLAGS="-D warnings" cargo doc -p uste-graph --no-deps --locked --offline` passed.
  Docs/task checks pass (155 links, 68 tasks). Host preflight: 36 GiB available RAM, 3.9 GiB free
  swap. No qualifying campaign ran. Next derive driver limits from the pinned fixture/batch plan,
  validate them without lifting the development ceiling prematurely, and complete I/O accounting.
  T-20/T-19 remain open; M1 and release gates are unchanged.

- [Decision 0081](docs/decisions/0081-native-cold-admission-measurements.md) retains authenticated
  cold graph scan/lookup measurements and initial graph/metadata revisions in native setup
  reports, separately from final repaired/materialized cardinalities. No extra reads or complete
  maps are used. Final fixture counts are checked before returning a native query session;
  the correct Evidence binding and revision no longer suffice for a truncated fixture.
  The prefix matrix verifies eight runs/1,845 entries on completed 20/200 cold admission and
  separates initial revision 1/2 bases from final revision 4. These are graph-admission-only
  counters, not complete authenticated I/O or qualifying measurements.
  Verified on pushed `b42ee30` plus this increment under MemoryHigh=3G/MemoryMax=4G/
  MemorySwapMax=512M, one job/thread: `cargo test --release --manifest-path
  experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1` passed 40 unit tests
  (2 existing exact-profile oracle tests ignored) in 28.48s and 3 real CLI tests in 13.58s.
  After adding the final negative fixture, the same release command filtered by
  `native_disk_binding_and_frontier` passed (1 test, 1.09s); experiment all-target strict clippy
  passed on the final tree. Docs/task checks pass (154 links, 68 tasks). Host preflight remains
  29 GiB available RAM, 3.9 GiB free swap. No qualifying campaign ran. Next validate scalable
  aggregate admission work bounds against repeated lookups, then derive exact-profile driver
  limits and complete I/O accounting. T-20/T-19 and release qualification remain open.

- [Decision 0080](docs/decisions/0080-trusted-disk-writer-cache-budget.md) adds trusted writer
  cache configuration without changing the default, request limits or durable formats. Benchmark
  writer batches now explicitly select 64 MiB, independently of the later query cache. Oversize
  refusal, custom-budget authorization/fault/reference-write checks and default-budget exact retry
  pass. Verified on pushed `a32bb56` plus this increment under the 3G/4G/512M process scope,
  one Cargo job/test thread: `cargo test -p uste-graph --test disk_index --locked --offline
  disk_queries -- --test-threads=1` (1 passed); `cargo test -p uste-txn --all-targets --locked
  --offline -- --test-threads=1` (5 unit, 13 authorization, 14 coordinator passed);
  `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` passed.
  `cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline
  -- --test-threads=1` passed 39 unit tests (2 existing exact-profile oracle tests ignored)
  in 28.28s and 3 real CLI tests in 13.62s. Experiment all-target strict clippy and
  `RUSTDOCFLAGS="-D warnings" cargo doc -p uste-txn --no-deps --locked --offline` passed.
  Host preflight: 29 GiB available RAM, 3.9 GiB free swap. No qualifying campaign ran.
  Next retain measured cold-admission work/counts in native reports so profile limits can be
  checked against actual authenticated work; then complete qualifying admission and I/O accounting.
  T-20/T-19 remain open and the pinned M1 result is unchanged.

- Decision 0079 adds `linux-disk-sample` and its persistent worker under the existing 30-second
  preemptive supervisor/watchdog. Shared plans/validators preserve 96 warm-ups, 384 paired queries,
  separate outcome/depth/topology populations, exact digest and the fixed five-by-60-second
  qualifying plan. Native commands remain development-capped. Reports omit unavailable complete
  authenticated-I/O counters, disclose resident storage metadata and withhold budget evaluation.
  Parent validation now also checks schema, deadline seconds, sample windows and rounds/counts.
  The real CLI sample passes with 768 executions, 32 groups, the independent oracle digest,
  64 MiB cache and zero retained-cache misses on 20/200. Counter rollback/overflow and report
  substitution tests pass. Review found that missing bootstrap identities could make policy
  installation a fresh append; the new pre-commit tuple/cardinality guard plus canonical retry
  validation refuses five unrelated prefixes without advancing their durable revision.
  Verified on pushed `0cf71bf` plus this increment under MemoryHigh=3G/MemoryMax=4G/
  MemorySwapMax=512M, one job/thread: `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path
  experiments/t20-bench/Cargo.toml --locked --offline -- --test-threads=1` passed 39 unit tests
  (two pre-existing exact-profile oracle tests ignored) in 28.15s and all three real CLI tests
  in 13.43s. `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml
  --all-targets --locked --offline -- -D warnings` passed. A missing mode argument in the old
  deadline test was corrected during compilation; that test now checks both engine modes.
  Docs/task checks pass (152 links, 68 tasks). Latest host headroom: 34 GiB RAM, 3.9 GiB free swap.
  No qualifying campaign ran. Next derive/verify qualifying-profile admission and complete I/O
  accounting, address remaining storage metadata residency, and execute BM-01/BM-06 only when
  their prerequisites are met. T-20, T-19 and the full roadmap remain open; M1 is unchanged.

- Decision 0078 adds the native disk crash-probe CLI and real process-loss integration matrix.
  Prefix 1 pauses after policy certification before roots; prefixes 2/3 pause after each complete
  graph/metadata publication. The test verifies the marker, SIGKILLs/reaps only its owned child,
  and uses fresh CLI processes to resume the exact prefix, open revision 4, verify all 384 queries
  against a separately generated oracle summary, and retry resume without advancing revision.
  Invalid prefix arguments fail before filesystem access. Harness output has concurrent bounded
  drains and deadlines; no unrelated process or application is modified.
  Verified on pushed `d991263` plus this increment under the established 3G/4G/512M scope,
  one job/thread: `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path
  experiments/t20-bench/Cargo.toml --locked --offline --test disk_process_loss --
  --test-threads=1` passed both tests in 10.42s. The complete release experiment suite then passed
  35 unit tests (two pre-existing exact-profile oracle tests ignored) in 23.04s and both CLI
  integration tests in 10.38s. Experiment all-target strict clippy and docs/task checks (151 links,
  68 tasks) passed. Host preflight: 35 GiB available RAM, 3.9 GiB free swap. This verifies selected
  process-loss cases only, not hardware power loss, larger-than-memory performance or production.
  Next connect native disk supervised sampling without changing the accepted windows/deadline,
  then qualifying-profile admission and remaining storage-metadata scalability. T-20 remains open.

- Decision 0077 adds `linux-disk-query`: a bounded, separately generated oracle summary drives
  native authorized disk queries without full graph/coordinator recovery or oracle adjacency
  construction in the query process. All 384 native queries, aggregate digest and total logical
  bytes/visits match the independent expectations. Warm-up/wrong-profile/truncated summaries
  fail before database I/O. Cache/report checks preserve the accepted 64 MiB USTE cache and
  disclose storage residency, uncontrolled host caches and the absence of a preemptive deadline
  in this single-pass correctness command. This is not the supervised sampler or qualification.
  Verified on top of pushed `080024c` under the 3G/4G/512M scope, one Cargo job/test thread:
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passed 35 tests, with the two existing exact-profile
  oracle tests ignored, in 23.09s. Experiment all-target strict clippy and docs/task checks
  (150 links, 68 tasks) passed. Host preflight
  remained 35 GiB available RAM and 3.9 GiB free swap. Next connect native disk sampling to
  preemptive worker supervision and add process-loss/resume coverage, then qualify the larger
  profile and remove remaining storage metadata residency. T-20 and full roadmap remain open.

- Decision 0076 adds native `linux-disk-create/resume/open` development commands, distinct from
  the legacy database/commands. Shared fixture generation and authorized disk writes preserve
  the oracle and deterministic batch plan. Recovery selects paired metadata roots, admits a
  ready graph base or one pending suffix, repairs derived roots and rebases before fresh writes.
  Missing roots above bootstrap and suffixes longer than one revision fail closed. Bootstrap
  alone has the one-outcome/zero-owner/1-MiB full-replay allowance; open publishes no roots.
  Native tests cover empty/policy bootstrap, pending graph roots, lagging/partially published
  metadata, exact resumed retries, wrong profile binding, duplicate create and missing roots.
  Initial debug matrix exposed a real shared-helper identity mismatch (`BM01DEV` versus native
  `BM01LIN`), failing prefix-2 resume after 230.59s. Fixed the helper to accept typed explicit
  identities; retained the native fixture and retry assertions. No acceptance target was changed.
  Final scoped command (MemoryHigh=3G/MemoryMax=4G/MemorySwapMax=512M, one job/thread):
  `CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- --test-threads=1` passed 34 tests, with two pre-existing exact-profile
  oracle tests ignored, in 20.93s; this includes all native cases and the frozen disk oracle.
  `CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets
  --locked --offline -- -D warnings` passed. After ensuring fixture parents exist with an external
  Cargo target directory, the release `native_disk` filter passed both tests again in 18.34s,
  followed by strict clippy. Docs/task checks pass (149 links, 68 tasks).
  Host preflight: 35 GiB available RAM, 3.9 GiB free swap, Btrfs repository; the initial debug
  scope's sampled peak was 488,796,160 bytes with no swap. This is not a final RSS measurement or
  benchmark reservation. Commands remain development-capped; storage metadata remains resident.
  Next add native disk query/oracle verification and preemptive sampling/process-loss coverage,
  then qualifying-profile admission and removal of the remaining storage memory boundary.
  T-20 and BM-01/BM-06 remain open; M1's pinned implementation and handoff are unchanged.

- Decision 0075 adds an ownership-preserving, explicitly count/owner/byte-bounded bootstrap
  replay handoff. The disk development check now restarts before bootstrap root publication and
  admits only one policy outcome, zero blob owners and 1 MiB encoded bytes before building roots.
  Tests cover empty genesis, short budgets before preparation, wrong reducer result, exact retry,
  first ownership, exclusive-lock continuity, every read fault and post-open certificate corruption.
  Initial corruption fixture targeted the log header; corrected it to revision one's certificate
  after confirming the format offset. No runtime authentication requirement was weakened.
  Under the existing 3G/4G/512M scope with one job/thread: txn all-target tests passed (5 unit,
  13 authorization, 14 coordinator); the disk oracle passed in 30.86s; workspace all-feature and
  experiment all-target strict clippy passed. Commands: `cargo test -p uste-txn --all-targets
  --locked --offline -- --test-threads=1`; `cargo test --manifest-path
  experiments/t20-bench/Cargo.toml --locked --offline disk_engine_matches -- --test-threads=1`;
  `cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings`;
  corresponding experiment clippy. After extending the fault fixture to two revisions,
  `cargo test -p uste-txn --test transaction_coordinator --locked --offline bounded_bootstrap
  -- --test-threads=1` passed both tests (0.25s), followed by txn strict clippy and warnings-denied
  rustdoc. Docs/task checks passed (148 links, 68 tasks). This is not larger-than-memory or Linux
  benchmark qualification.
  Next integrate bounded bootstrap and admitted-base/pending-suffix recovery into the Linux disk
  runner. T-20 remains open; storage metadata remains resident; M1 remains pinned unchanged.

- Decision 0074 adds runnable `disk-engine-check` using disk graph/coordinator state after a
  policy-only bootstrap, bounded authorized writes/rebase and independent cold root admission.
  All 384 existing 20/200 oracle queries match the frozen digest; no fixture/target was changed.
  Trusted reader construction can select the accepted 64 MiB cache; report/clear require current
  `ManageSchema`, denial leaves cache unchanged, and clearing retains cumulative counters.
  The initial exploratory 64 KiB-cache debug run was intentionally SIGINTed in its own scope
  (exit 130, no test result); the same corpus passed with the accepted 64 MiB setting in 30.44s.
  Capped commands (one job/thread, MemoryHigh=3G/MemoryMax=4G/MemorySwapMax=512M):
  `cargo test -p uste-graph -p uste-txn --all-targets --locked --offline -- --test-threads=1`,
  `cargo test --manifest-path experiments/t20-bench/Cargo.toml --locked --offline --
  --test-threads=1` (32 passed, two pre-existing exact-profile tests ignored), workspace all-feature
  and experiment all-target strict clippy, and workspace warnings-denied rustdoc all passed.
  `CARGO_BUILD_JOBS=1 cargo run --release --manifest-path experiments/t20-bench/Cargo.toml
  --locked --offline -- disk-engine-check --entities 20` exited 0: revision 4, 384 queries,
  digest `46f1bdb3138d6325e4c0f56b5fd3bbf5ff092d816e8a0f6c23acd15687b910b5`, 67,108,864-byte
  cache, 580,665 hits and 8 misses. This is memory-adapter semantic evidence, not BM qualification;
  reports disclose the in-process oracle and resident storage metadata. Docs/task checks pass
  (147 links, 68 tasks); host preflight 36 GiB available RAM, 3.9 GiB free swap.
  Next port this path to the Linux runner with bounded bootstrap and pending-root resume, preserving
  ownership and all existing oracle/sample/deadline contracts. M1 handoff remains pinned unchanged.

- Decision 0073 retains admitted first-reference roots and advances them with one authenticated,
  owner/group/byte-bounded suffix pass plus a native insertion-only merge. Three-root publication
  retains the old base/overlays until terminal success; exact retry and cold repair survive every
  observed create/write/length/file-sync/directory-sync/removal fault (error, crash-before/after).
  Tests also cover owner-free bootstrap, an owner-free suffix, unchanged first ownership and
  short suffix budgets. Initial removal testing used an absent fallback slot, where the fault
  adapter correctly does not crash after `NotFound`; strengthened the fixture to populate both
  slots rather than weakening the assertion. Targeted two-test matrix passed in 67.24 seconds,
  followed by strict txn/replay all-target clippy. Full repository gate passed:
  `systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M bash -lc
  'CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 CARGO_NET_OFFLINE=true bash scripts/check.sh'` exited 0.
  Workspace all-feature tests/lint/docs, M1 process tests, docs/task/dependency/vector checks and
  the experiment's 31 tests passed (two pre-existing exact-profile oracle tests remain ignored).
  Replay checkpoint suite: 9 passed. Last sampled cgroup peak was 1,500,987,392 bytes and zero
  swap, not a final peak measurement. Docs/task checks: 146 links, 68 tasks. The unreferenced
  next-increment benchmark adapter draft is excluded from this tested/committed increment.
  Next: connect the disk coordinator/live graph/authorized read-write path to the benchmark
  driver, which still uses full `GraphState`; preserve fixture/oracle/budget profiles and keep
  development evidence nonqualifying. Storage-resident metadata and BM-06 remain open.

- Decision 0072 adds optional authenticated first-reference evidence and single-journal-pass
  first-owner admission without reconstructing an owner comparator map. Existing compatibility
  admission remains unchanged. The legacy bridge publisher explicitly bounds its temporary map
  and replay; it is not a disk-backed incremental builder. Tests reject false earlier/later
  revisions, absent IDs, malformed values, wrong owners and short budgets, and fail closed at
  every observed cold-admission read fault. Warm maintenance followed in Decision 0073 above.
  Capped validation (one build job/thread; MemoryHigh=3G, MemoryMax=4G, MemorySwapMax=512M):
  `CARGO_BUILD_JOBS=1 cargo test -p uste-replay -p uste-txn -p uste-graph --all-targets --locked
  --offline -- --test-threads=1`, `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets
  --all-features --locked --offline -- -D warnings`, and `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D
  warnings" cargo doc --workspace --all-features --no-deps --locked --offline` all exited 0.
  Replay checkpoint suite now has 7 tests; txn unit suite has 5. Docs/task checks pass (145 links,
  68 tasks). Preflight: 36 GiB available RAM, 3.9 GiB free swap. No qualifying benchmark run.
  Next: maintain first-reference evidence across bounded disk overlays/rebase, then address
  resident storage certificate/blob metadata. T-20 and the remaining roadmap remain open.

- Decision 0071 adds `AuthorizedDiskUploads`: bounded staging-only reservations, fail-closed
  complete-outbox reconciliation, private disk owner lookup and exact streaming committed charges.
  Unknown recovered staging is explicitly marked incomplete. Graph inventory rejection remains;
  finalized uncommitted blobs cannot be aborted or silently freed. No task checkbox changed.
  Owner identity lookup is tested against the original first owner and after metadata rebase.
  Upload tests cover exact 1 MiB quota/one-byte refusal, eight live handles, 32 unresolved
  reservations, durable resume/abort, finalization retention, duplicate outbox rejection, and an
  injected reconciliation read failure with authorization before I/O. The initial fixture wrongly
  expected abort after finalization; corrected the fixture, not immutable-blob semantics.
  Validation: under `systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G
  -p MemorySwapMax=512M`, `CARGO_BUILD_JOBS=1 cargo test -p uste-graph -p uste-txn -p uste-replay
  --all-targets --locked --offline -- --test-threads=1` and `CARGO_BUILD_JOBS=1 cargo clippy
  --workspace --all-targets --all-features --locked --offline -- -D warnings` passed. After adding
  the reconciliation fault assertion, reran the graph disk expansion fixture and graph all-target
  clippy under the same cap: passed. `cargo fmt --all`, docs and task checks passed (144 links,
  68 tasks). Preflight: 37 GiB available RAM, 3.9 GiB free swap; no qualifying benchmark run.
  Next: scalable authenticated first-owner admission and remaining storage metadata bottlenecks;
  inventory commit must retain domain contracts (graph explicitly prohibits inventories), and
  needs a domain-compatible authorized staging-to-certified-charge transfer capability.

- Added 81 graph terminal-publication model faults: every create (5), write (6), length change
  (5), file sync (5) and directory sync (6), each with error/crash-before/crash-after. Every fault
  fires. Failed publication preserves pending state, overlays and exact journal anchor;
  in-process retry succeeds, and restart admits either the older graph base plus pending suffix
  or the completed root. Repair reproduces the reference digest and rebase empties overlays.
- Verification on `a3b2d7c` plus this increment: `cargo test -p uste-graph --test disk_index
  --locked --offline -- --test-threads=1 --nocapture` passed 8, including both 30-case read and
  81-case publication matrices; strict all-target graph clippy passed. One job/thread and the
  established 4 GiB scope; preflight 17 GiB available RAM, 1.5 GiB free swap. This does not
  replace real process/filesystem/power-loss or larger-than-memory qualification. Continue
  authorized graph read/write and upload reconciliation integration.

- Added a dynamically enumerated disk-graph suffix read-fault matrix: 10 read boundaries ×
  error/crash-before/crash-after = 30 injected cases. Every fault fires; no provisional coordinator
  escapes, and restart repairs the same certified transaction to the reference digest, then
  rebases metadata to empty overlays. Committed-certificate mutation after admission fails both
  suffix validation and fresh open. An armed fault proves denied metadata reads perform no disk
  or clock work and cannot consume the fault. These are model faults, not power-loss qualification.
- Verification on `3f24834` plus this increment: `cargo test -p uste-graph --test disk_index
  --locked --offline -- --test-threads=1 --nocapture` passed 7; `cargo test -p uste-storage
  --test fault_harness --locked --offline -- --test-threads=1` passed 11. Strict all-target
  storage/graph clippy passed. One job/thread in the established 4 GiB scope; preflight 17 GiB
  available RAM, 1.5 GiB free swap. Next extend graph terminal-publication fault coverage and
  authorized write/upload/query integration; no task checkbox or qualification changes.

- Connected warm graph proof preparation to the disk metadata coordinator, retaining narrow
  bounded read methods rather than exposing its overlay-only legacy coordinator. Ready roots
  must match the journal anchor and profiles; pending state refuses new preparation. The
  regression continues through two additional prepare/commit/root/rebase cycles, exact retries
  while pending, proof-byte refusal and final full-reference equality from both journal and root.
  No consumer write authorization or large-scale qualification is implied.
- Warm continuation verification on `bb75477` plus this increment: `cargo test -p uste-txn
  -p uste-graph --all-targets --locked --offline -- --test-threads=1` passed; the final enhanced
  `cargo test -p uste-graph --test disk_index --locked --offline -- --test-threads=1` passed 5.
  Strict all-target txn/graph clippy passed. One job/thread under the existing 4 GiB scope;
  preflight 18 GiB available RAM and 1.5 GiB free swap. Documentation/task graph checked; T-20
  stays open for the outstanding authorization, recovery coverage and qualification work.

- [Decision 0065](docs/decisions/0065-authorized-disk-metadata-reads.md) adds a restricted borrowed
  consumer facade for own retry/transaction outcomes and committed-byte usage. Authentication
  and independent action permissions precede clock/disk work; current durable policy must match
  exactly without constructing a snapshot. Graph pending state refuses the facade until repair.
  Base/overlay isolation, foreign-kernel denial, expiry, quota permissions, missing/mismatched
  policy and pending-state rejection pass in `cargo test -p uste-graph --test disk_index
  --locked --offline -- --test-threads=1` (5 tests). Strict all-target txn/graph clippy passed
  on `512e4d6` plus this increment. This is not authorized writes/uploads or graph reads.
  `CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 CARGO_NET_OFFLINE=true bash scripts/check.sh` passed
  under `MemoryHigh=3G MemoryMax=4G MemorySwapMax=512M`: workspace all-feature tests/clippy,
  rustdoc, vectors, documentation/task graph, dependency/fixture builds and capped T-20 driver
  (31 passed, 2 pre-existing exact-profile acceptance cases ignored). This is not a qualifying
  benchmark. Preflight 17 GiB available RAM, 1.5 GiB free swap; last sampled cgroup peak was
  2,847,420,416 bytes with zero cgroup swap, not an end-of-run peak measurement.

- Added privileged bounded streaming committed-byte accounting as a disk-aware authorization
  prerequisite. It preflights base-plus-overlay owners, authenticates the complete owner run and
  returns exact namespace/principal first-owner charges only on terminal success. No complete
  quota owner map is built. This is O(owners) reference accounting, not a scalable aggregate index,
  staged-upload accounting or a consumer authorization facade.
- Accounting verification on `da7b40c` plus this increment: `cargo test -p uste-replay --test
  coordinator_checkpoint --locked --offline -- --test-threads=1` passed all 5 tests, including
  empty/poisoned state, owner/byte refusal, base plus overlay totals, principal isolation,
  repeated references preserving first charges, and identical totals after rebase. Strict
  all-target txn/replay clippy passed. An initial test compile exposed a missing type qualification;
  repaired without production changes. One job/thread under the established 4 GiB process scope;
  preflight 18 GiB available RAM and 1.5 GiB free swap. No benchmark or task completion claimed.

- [Decision 0064](docs/decisions/0064-disk-coordinator-graph-suffix.md) connects the disk metadata
  coordinator to independently admitted graph state. Metadata may lag the ready graph root;
  authenticated streaming rebuilds only bounded post-metadata-base overlays. Recovery accepts a
  ready graph frontier or exactly one revalidated externally prepared pending change, never a
  complete graph-map fallback. A shared terminal publication helper repairs the disk coordinator's
  pending graph root before metadata rebase releases overlays.
- Verified on `5be9afc` plus this increment: `cargo test -p uste-txn -p uste-graph --all-targets
  --locked --offline -- --test-threads=1` passed all targets; `cargo test -p uste-replay --test
  coordinator_checkpoint --locked --offline -- --test-threads=1` passed 5 including existing
  commit/rebase fault matrices. Graph recovery adds seven cases for ready/pending recovery,
  outcome/byte refusal, absent/misplaced suffixes and failed terminal publication followed by
  repair/rebase. Strict all-target txn/replay/graph clippy passed. The initial byte-refusal test
  expected a coordinator-level error; corrected it to require the existing wrapped storage
  `ResourceLimit`, without changing production behavior. One job/thread and the existing 4 GiB
  cgroup; preflight 19 GiB available RAM, 1.5 GiB free swap. Dedicated disk-graph faults/corruption
  and disk-aware authorization remain next work; no benchmark qualification or task closure.

- Caller-bounded fallback validation now precedes metadata rebase and graph terminal-root
  overwrite selection. Per-run page/entry/byte refusal leaves root slots unchanged, and a corrupt
  newest run cannot displace the sole good older fallback. The compatibility API remains available.
  Verified on `eb02a33` plus this increment: `cargo test -p uste-storage
  bounded_root_publication_preserves --locked --offline -- --test-threads=1` passed 1;
  the existing `encrypted_index_runs_round_trip_large_values_with_bounded_cache_and_root_fallback`
  filter passed 1; `cargo test -p uste-graph --test disk_index --locked --offline --
  --test-threads=1` passed 5; the same flags for `-p uste-replay --test coordinator_checkpoint`
  passed 5 including the 48-case rebase matrix. Strict all-target storage/txn/replay/graph clippy
  passed. One job/thread, `MemoryHigh=3G MemoryMax=4G MemorySwapMax=512M`; preflight 17 GiB
  available RAM, 888 MiB free swap. No qualifying benchmark or task completion is claimed.

- [Decision 0063](docs/decisions/0063-coordinator-metadata-rebase.md) adds streaming insertion-only
  metadata rebase, exact output checks and overlay release only after both current roots succeed.
  A matching partial root is reauthenticated and resynchronized without slot rotation. Pending
  rebase blocks new writes (including after restart) while permitting exact retries; advancing a
  legacy writer beyond an intermediate partial root requires explicit cache rebuild, not deletion
  of the pinned pair. The subsequent bounded-publication extension above closes that path's
  caller-admission gap. First-owner read amplification and storage metadata maps remain open.
- Focused rebase verification passed 48 error/crash-before/crash-after cases covering run/root
  syncs, directory syncs and root writes, plus one repeated partial-pair failure. Same-process
  retries resynchronize previously visible unsynced roots; cold reopen installs the new pair
  with empty overlays and permits another commit. Existing/new owners and merge-limit refusal
  are covered. An initial test failure exposed reset synthetic entropy colliding with surviving
  scratch object IDs; the fixture now supplies fresh deterministic entropy per simulated process.
  No production entropy or acceptance requirement was weakened.
- Rebase verification on parent `267dce2` plus this increment, one Cargo job/test thread under
  `MemoryHigh=3G MemoryMax=4G MemorySwapMax=512M`: `cargo test -p uste-replay --test
  coordinator_checkpoint --locked --offline -- --test-threads=1` passed 5; the same test flags
  with `-p uste-graph --test disk_index` passed 5, `-p uste-txn` passed 29 and `-p uste-storage`
  passed 59 unit plus 18 integration tests. Strict all-target clippy passed for
  storage/txn/replay/graph. Documentation/task-graph checks passed. Preflight: 11 GiB available
  RAM and 207 MiB free swap. No qualifying benchmark was attempted.

- Added the disk coordinator's 13-case commit fault matrix: cold metadata-read failure and
  error/crash-before/crash-after at group/certificate writes and data-sync boundaries. Each fault
  is required to fire. Tests verify prepublication read errors do not poison the old state,
  publication errors deny reads with `OutcomeUnknown`, no provisional overlay escapes, restart
  restores the exact old/new certified frontier, and retry applies the transaction exactly once.
  These are deterministic model faults, not physical power-loss qualification.
- Fault verification on parent `169a718`: `cargo test -p uste-replay --test
  coordinator_checkpoint --locked --offline -- --test-threads=1` passed all 4 tests (the new test
  exercises 13 injected cases); strict all-target replay clippy passed. One Cargo job/test thread
  and the existing 4 GiB cgroup remained in effect. Preflight: about 10 GiB available RAM but only
  556 KiB swap free. Documentation/task-graph checks passed; no benchmark was attempted.

- Decision 0062 now includes authenticated multi-revision suffix replay for ordinary reducers.
  It revalidates the historical base against the current journal owner, admits suffix cardinality,
  streams canonical requests with cumulative byte limits, checks base/overlay collisions and
  first owners, and reproduces every reducer result digest. Only terminal success exposes the
  coordinator. Two-revision restart tests retain only two outcomes/one new owner, preserve old
  ownership and exact retries, and refuse insufficient outcome, owner and early/late byte budgets.
  Disk-graph external preparation is not implemented by this ordinary-reducer path; no full graph
  map fallback or general graph suffix claim is made.
- Suffix verification on parent `43caccb` plus this increment: one job/test thread under
  `MemoryHigh=3G MemoryMax=4G MemorySwapMax=512M`; `cargo test -p uste-replay --test
  coordinator_checkpoint --locked --offline -- --test-threads=1` passed 3, and
  `cargo test -p uste-graph --test disk_index --locked --offline -- --test-threads=1` passed 5
  (including zero-suffix disk-graph recovery with zero overlay/byte allowance). Strict all-target
  clippy passed for txn/replay and graph. Documentation/task-graph checks passed. Latest host
  preflight showed 9.9 GiB RAM available but only 54 MiB swap free; no larger workload was started.

- [Decision 0062](docs/decisions/0062-disk-coordinator-overlays.md) installs the admitted disk
  metadata base without coordinator-prefix maps and adds bounded live overlays. The separate
  privileged `DiskCommitCoordinator` consumes recovery ownership at the exact frontier, requires
  a domain-state anchor proof, and shares the existing commit ordering/durability implementation.
  Disk retry/transaction/owner lookups have explicit I/O bounds; new owner slots are admitted
  before temporary allocation or journal publication. Legacy authorization adapters cannot access
  its incomplete internal overlay-only coordinator. Disk-aware authorization, metadata rebase,
  suffix restart and scalable owner proof remain open, as do storage's own memory-resident maps.
- Verification on parent `459d123` plus this increment, one job/thread and the established
  3/4 GiB memory-high/max and 512 MiB swap cgroup: `cargo test -p uste-replay --test
  coordinator_checkpoint --locked --offline -- --test-threads=1` passed 3;
  `cargo test -p uste-txn --locked --offline -- --test-threads=1` passed 29;
  `cargo test -p uste-graph --test disk_index --locked --offline -- --test-threads=1` passed 5.
  `cargo clippy -p uste-txn -p uste-replay -p uste-graph --all-targets --locked --offline --
  -D warnings` passed. Tests cover empty-overlay graph installation, disk and overlay retries,
  transaction collision, principal isolation, expiry, cancellation, both overlay limits,
  repeated first ownership and legacy restart of the new durable commit. Latest preflight:
  9.5 GiB available RAM, 769 MiB free swap. No benchmark was attempted.

- [Decision 0061](docs/decisions/0061-coordinator-disk-base-admission.md) adds a paired,
  journal-validated `CoordinatorDiskBase` retaining roots rather than coordinator maps. It proves
  exact retry outcomes, transaction ordering and first-owner/reference correspondence. Raw
  explicit-I/O retry and owner reads are recovery-only, not authorization capabilities. Owner
  proof uses one entry and one complete prefix pass per owner, with checked aggregate group
  admission and per-pass byte bounds. This O(owners * revisions) compatibility path is explicitly
  not the large-scale recovery algorithm; live mutation maps and storage metadata remain open.
- Verification on parent `5a923ec` plus this increment used one job/test thread under the same
  `MemoryHigh=3G MemoryMax=4G MemorySwapMax=512M` scope: `cargo test -p uste-replay
  --test coordinator_checkpoint --locked --offline -- --test-threads=1` passed 3;
  `cargo test -p uste-graph --test disk_index --locked --offline -- --test-threads=1` passed 5;
  `cargo test -p uste-txn --locked --offline -- --test-threads=1` passed 29. Strict all-target
  clippy passed for txn/replay/graph. Tests cover restart, exact and absent retries, first-owner
  preservation after another principal reuses a blob, authenticated last-owner substitution,
  mismatched root anchors, aggregate work refusal and no-owner admission. Initial compilation
  caught two missing slice borrows, a misplaced fixture edit and an unnecessary qualification;
  all were repaired before the final passing runs. Documentation/task graph checks passed.
  No task or benchmark acceptance was advanced.

- Added authenticated inclusive journal-range visitation with preflight group admission,
  cumulative certificate/group-envelope byte admission and one-group/inventory retention.
  Every reread certificate must match the exclusively owned journal's authenticated anchor;
  group bytes, segment header and inventory are authenticated again. Callbacks have explicit
  filesystem access for disk-index correspondence checks and must discard provisional work on
  any later error. Inventory format caps remain separate from the envelope-byte budget; blob
  payloads are not reread. The transaction wrapper validates canonical groups and scope without
  constructing coordinator maps. Storage's anchor/blob maps remain memory-resident, so this is
  not larger-than-memory recovery or completed multi-revision graph suffix recovery.
- Extended Decision 0060 with direct cold transaction-index admission: authenticate the complete
  run, require exact revision cardinality, then compare every entry to streamed journal outcomes
  with bounded lookups. No coordinator comparator maps are built. Restart tests admit the exact
  root, reject an authenticated wrong principal and enforce group/byte/result budgets. Existing
  metadata, consumer interfaces and pinned M1 implementation are unchanged.
- Verification on parent `8f47f1b` plus this increment, all under
  `systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M`,
  `CARGO_BUILD_JOBS=1`, `--locked --offline` and `-- --test-threads=1`:
  `cargo test -p uste-storage` passed 59 unit and 18 integration tests (including real process
  loss); `cargo test -p uste-graph --test disk_index` passed 5; `cargo test -p uste-replay
  --test coordinator_checkpoint` passed 3. After adding the range-specific group-corruption
  regression, `cargo test -p uste-storage exact_groups_replay_in_order_and_ownership_is_exclusive`
  passed again. Strict all-target clippy passed for storage/txn/graph and separately replay.
  Documentation/task-graph checks passed; headroom at the second run was 9.4 GiB RAM and 1.1 GiB
  swap. These capped tests are not BM qualification.

- Added [Decision 0060](docs/decisions/0060-coordinator-transaction-index.md): a separate encrypted
  transaction-ID ordering with certificate binding, exact bounded raw lookup, stale-frontier
  refusal and complete bounded re-admission against recovered coordinator metadata. The current
  comparator and coordinator remain memory-resident; T-20 remains open. Focused checkpoint tests
  cover missing IDs, byte/entry budgets, exact outcomes and an authenticated wrong principal.
  Verification uses one job/thread and the established 4 GiB cgroup. The host preflight showed
  about 10 GiB available RAM and 1.1 GiB free swap; no qualifying benchmark was launched.
- Added `CoordinatorRecoveryLimits` and `open_journal_anchored_prepared_bounded`: callers can
  cap retained outcomes and first blob owners before coordinator map insertion during recovery.
  Duplicate blob inventory use consumes no additional owner slot. Refusal returns no coordinator
  and releases ownership; a later adequately admitted open recovers the exact durable suffix.
  The first regression exposed nested storage error mapping; recovery budget refusal now returns
  typed `TransactionError::ResourceLimit`. Final sequential capped verification passed 29
  transaction tests, 5 graph disk-index tests and warnings-denied transaction clippy. These are
  count limits for coordinator maps, not journal/RSS bounds or disk-backed metadata completion.
- Resumed after the capacity interruption and pushed `8885da4`/`808f2fd` without rewriting history.
  Confirmed the M1 pin's lock digest and unchanged pilot sources against Decision 0058, and
  reconciled obsolete T-64 next-step instructions with completed T-63–T-68 status.
- Extended bounded manifest discovery to both coordinator metadata owners. The existing seed
  reconstruction continues to authenticate all families and enforce caller budgets before return.
  `CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo test -p uste-replay --test coordinator_checkpoint
  --locked --offline` passed all 3 cases; strict `uste-txn --all-targets` clippy passed. Both ran
  sequentially under `MemoryHigh=3G`, `MemoryMax=4G`, `MemorySwapMax=512M`.
- Advanced T-20 at `8885da4` with Decision 0059. Graph root discovery now authenticates only two
  fixed manifests and journal anchors before caller-selected cursor budgets apply; it no longer
  scans every run twice or allocates the default scrub cache before recovery limits. A same-length
  corrupt run remains provisional and fails cursor/admission validation. The publication overwrite
  path deliberately retains its complete scrub to protect the sole good fallback.
- Sequential 4 GiB-cgroup verification passed 43 graph, 77 storage and 28 transaction tests,
  including complete storage crash matrices, plus warnings-denied clippy. The complete repository
  gate also passed with 132 documentation links, 129 active IDs, 152 definitions, the 68-task graph
  and 31 passed/2 exact-profile-ignored scaled T-20 tests. No BM-01/BM-06 campaign ran because host
  swap remains saturated and the coordinator-metadata boundary is not yet honest.
- Completed T-68/M1 at `b9689f3` plus Decision 0058 evidence. A real child acknowledged memory
  revision 4, was SIGKILLed and recovered the exact citation in a fresh process. Wrong password and
  committed-certificate mutation fail closed. Existing no-space publication matrices, pilot query
  cancellation/budgets, authorization/revocation and independent-oracle recovery complete M1-F.
- The exact `b9689f3` release path measured 55,512,414 B/s for a durable encrypted 1 MiB ingest, 355 ms cold
  recovery, warm authorized-query p99 below 1 microsecond reporting resolution across 1,000 samples,
  and 265,180 KiB peak RSS. All pass the thresholds frozen in T-63; no threshold or full-product
  benchmark was changed.
- Added the exact-version consumer handoff pinned to full commit, Cargo.lock SHA-256, Rust 1.95.0,
  default features and x86_64 Linux/Btrfs. It requires separate consumer-side source/outbox/policy
  mapping, shadow testing and rollback; it authorizes no AgentMage change or authoritative migration.
- Completed T-67 at `a07bdec` with `uste-memory-adapter`. One mutable embedded adapter owns the
  Linux/Btrfs writer, key, scope, policy principal and authority generation; it returns only bounded
  owned results. The consumer remains authoritative for immutable source bytes and a strict
  version-1 upload outbox that is durable before staging and cleared after an exact idempotent commit.
- Added `scripts/run_memory_pilot_demo.sh` and a separate-root synthetic consumer. The real encrypted
  Btrfs run reached generation 2/revision 11 after exact ingest/citation, competing-owner refusal,
  reopen, correction/history, contradiction, revocation and rebuild. Adapter tests reject unknown
  checkpoint versions/scope/duplicates and warnings-denied clippy passes. The complete capped
  repository check passes with 127 documentation links/124 active IDs/152 definitions, the 68-task
  graph, and 31 passed/2 qualifying-only ignored scaled T-20 cases. No IPC, cloud, model, provider
  credential, AgentMage change or authoritative migration is included.
- Completed T-64 at `97537e5`: canonical `UMEM` 1.0 transactions now admit immutable exact-text or
  opaque source versions, evidence-bound facts, corrections, contradictions, retractions and source
  revocation through the existing encrypted blob/journal/policy path. Retry returns the same durable
  outcome after reopen. A trusted complete upload outbox can reconcile committed/aborted/empty
  staging and reopen ingestion without disabling quotas; unresolved durable staging remains closed.
- Completed T-65 with identity, bounded lexical and one-hop reads plus exact source citation
  resolution. The pilot explicitly supports recorded current/as-of knowledge and exact/missing
  source-event filters only. An independent fixture oracle agrees; cross-scope and unsupported
  queries, candidate/output budgets and cooperative cancellation fail explicitly.
- Completed T-66 with current source-policy filtering, durable revocation, process-local stale-view
  invalidation and consumer authority generations. Begin-rebuild durably clears/blocks the derived
  projection, remains blocked across restart and serves only after exact reimport plus completion.
  Decision 0056 and the core evidence state that this is read exclusion, not physical erasure.
- Focused sequential verification under the 4 GiB/512 MiB cgroup passed 6 `uste-memory` and 28
  `uste-txn` tests plus warnings-denied clippy. The complete `scripts/check.sh` also passed in that
  capped scope: documentation reported 124 links/121 active IDs/152 definitions, the 68-task graph
  passed, and the scaled T-20 experiment reported 31 passed with its two qualifying cases ignored.
  The exact end-to-end M1 fault/measurement matrix has not run and is reserved for T-68 after the
  T-67 executable adapter exists.
- Recovered the unstaged Decision 0053 implementation without discarding or presuming it valid.
  Focused transaction/graph suites, warnings-denied clippy and the complete repository check passed
  before it was committed and pushed as `7393def`.
- Completed T-63 with Decision 0055 and `uste_memory::PILOT_PROFILE`: 4,096 commits, 256 source
  versions, 2,048 retained facts, 32 MiB total/1 MiB per source, 16 MiB logical state, bounded
  staging/query/concurrency, 512 MiB RSS and predeclared recovery/latency/throughput thresholds.
  These are pilot limits, not replacements for BM-01/BM-04/BM-06.
- Added T-63 evidence recording the exact baseline, dirty-work disposition, host RAM/swap/Btrfs
  preflight and sequential 4 GiB-cgroup verification. The large T-20 campaign was not run.

- Recorded owner-authorized Decision 0011, separating local development governance (T-06)
  from operational private-reporting verification (new T-62) without claiming verification.
- Corrected reporting participation to include the reporter and authorized repository security/
  triage participants, rather than the inaccurate maintainer-only description.
- Closed T-06/T-07 and R0 at the development decision/evidence scope after aligning stale
  pre-decision specification text and rerunning its actual prerequisites.
- Added a task-graph check proving the 62-task graph is acyclic, T-08 does not inherit T-62,
  and R4 decision T-44 does inherit T-62. R2/R3 are explicitly local readiness gates and no
  executable distribution is allowed while T-62 is open.
- Completed T-08 with a Rust 1.95.0/resolver-3 workspace, initial safe-Rust `uste-types` crate,
  root lockfile, read-only CI, active-document/reference validation and reproducible quality/
  supply-chain scripts. Excluded experiment workspaces retain their independent pinned locks.
- Completed T-09 with Decision 0012, bounded generic values, typed/scoped identities, UTC
  instants, nonzero monotonic commit revisions and a strict canonical format-1.0 codec.
- Completed T-10 with an independent `uste-testkit` oracle using ordered maps, clone-and-publish
  transactions, retained revision snapshots and scans rather than future production reducers.
- Completed T-11 with Decision 0013 and a safe first-party `uste-crypto` boundary around the pinned
  RustCrypto profile: exact object/recovery envelopes, fixed authenticated context, padded in-place
  AEAD, bounded nonce sessions, redacted lockable key ownership and a portable fixed-cost Argon2id
  recovery adapter.
- Added literal `crypto-v1` profile vectors and fail-closed coverage for every context field and
  ciphertext mutation, truncation, wrong key/password/database, entropy/nonce failures, locked use,
  maximum payload, authenticated malformed recovery padding and writer-incarnation separation.
- Corrected review findings before binding evidence: caller-sized crypto buffers now reserve
  fallibly, rejected credentials zeroize, resource failures remain retryable, guarded-memory is not
  claimed and future clone/restore/rotation/hardening responsibilities stay explicit.
- Completed T-12 with Decision 0014 and safe handle-relative filesystem, clock and randomness
  capabilities plus a deterministic volatile/durable memory adapter and per-operation fault plans.
- Covered interrupted and short I/O, zero/over-reported progress, disk full, failed file/directory
  sync, crash-before/after rename and data flush, sticky crash/restart, stale handles, wall rollback,
  random failure and exact repeatability. Crash state cannot be cleared without adapter restart.
- Added a guarded real-process scenario under Btrfs-backed Cargo target scratch: the child flushes
  file bytes and the directory, signals over a pipe, is killed with SIGKILL, and exact bytes reopen.
  This is process-loss harness evidence, not a power-loss or production-filesystem claim.
- Began T-13 with Decision 0015 and the pinned `rustix 1.1.5` x86_64 Linux adapter: descriptor-rooted
  no-symlink opens, checked object types, positional I/O, 0600/0700 creation, no-replace rename,
  explicit data/full/directory syncs, recovery truncation and unique-FD nonblocking ownership.
- Implemented encrypted creation manifest, authenticated log/segment headers, exact opaque group
  envelopes, fixed hash-chained commit certificates, poisoned uncertain writers and bounded
  streaming recovery. T-13's empty-inventory profile is extended by T-15's verified nonempty blob
  inventories.
- Verified exact byte replay, competing-owner rejection, durable lost-response recovery, previous-
  frontier recovery after certificate-sync failure, incomplete-tail repair and hard failure for
  complete certificate/group corruption. A Btrfs child was SIGKILLed after certificate sync and
  the production adapter reopened the exact committed group.
- Completed T-14 with Decision 0016 and `uste-txn`: owned prepare/publish changes, encrypted canonical
  transaction groups, coherent pinned readers, namespace/principal retry scope, durable outcome
  reconstruction, expiry and uncertain-handle quarantine. Literal group/malformed recovery,
  complete initial publication-fault, short-I/O, both cancellation-boundary, 32-caller conflict,
  restart and lost-response retry tests pass.
- Completed T-15's local acceptance with Decision 0017: bounded encrypted blob chunks, temporary-
  to-canonical staging, per-chunk authenticated progress witnesses, paired terminal witnesses,
  key-derived opaque inventory names, canonical inventory publication, commit-gated range reads and
  bounded recovery-time verification. Missing first/middle/last acknowledged chunks, single
  terminal-copy loss, malformed canonical objects, cross-context replay and foreign handles fail
  closed without token resurrection or silent shortening.
- Exercised error, no-space, short/zero progress and crash-before/after behavior across chunk,
  progress, multi-chunk finish, both terminal copies, abort cleanup, inventory, group and certificate
  publication. Blob-bearing lost-response retry, exact/+1 storage-profile limits and same-byte
  upload/namespace isolation pass.
- Measured the release-built production Linux-adapter path with normal Argon2id recovery wrapping,
  encryption and durability: a 12 GiB commit/reopen/full-hash round trip used 267,636 KiB peak RSS,
  1.06653x disk bytes and reproduced SHA-256
  `7eb969346fd20004fd1bb01f0ba1a8b356aea4c684ba22b1f8fde7c0014589c4`. Ingest was 95.923 MiB/s,
  below BM-04's 250 MiB/s target, so BM-04 remains unpassed despite T-15 correctness/RSS closure.
- Completed T-16 with Decision 0018 and a storage-independent, safe-Rust `uste-policy` kernel:
  trusted-adapter authentication mints opaque issuing-kernel-bound principals; absent policy denies;
  action permissions are independent; record rules narrow namespace grants; and policy revisions
  invalidate leases. The consumer transaction request has no caller-controlled principal field.
- Added the mandatory authorized transaction facade. It checks namespace and reducer-declared
  record requirements before state/index/storage access, rejects cross-namespace reducer targets,
  binds views/uploads to their issuing coordinator and exposes no generic reducer snapshot.
  Transaction-ID outcomes are owner-filtered, and denied unknown/existing paths use content-free
  failures. A review-found same-scope fabricated resume-token capability was closed by requiring
  authenticated durable journal evidence for every token absent from the in-memory ledger.
- Enforced checked exact-plaintext quotas for request bytes, staging, committed unique blobs, live
  handles and range reads. Failed writes reconcile accepted bytes; finalize retains the reservation;
  abort releases it; commit moves it once. Recovery reconstructs first-publication ownership and
  committed charges. Zero-byte reservations are capped at 32 and inventory lookup has a blob index.
- Format 1.0 cannot enumerate abandoned uncommitted reservations, so a recovered authorized
  coordinator conservatively rejects new upload starts. Evidenced known uploads, including a
  zero-byte final marker, remain resumable for reconciliation/abort; T-35 owns complete enumeration.
- Completed T-17 with Decision 0019 and the safe-Rust `uste-graph` production reducer: typed scoped
  entities/evidence/assertions/relationships, strict canonical requests, revision history, explicit
  valid time, lifecycle/corrections, reference closure and exact bounded cascade/retract deletion.
- Added atomic ordered outgoing/incoming adjacency and evidence-provenance indexes with independent
  rebuild checks. Cycles, self-loops, parallel edges and terminal transitions retain deterministic
  behavior. A separate 1,000,000-candidate visit ceiling stops before over-limit allocation.
- Added durable native graph policy bootstrap/replacement/history and reducer-owned authorized
  projections. Missing/mismatched durable policy fails closed, consumer install is rejected, stale
  idempotent replacement retries preserve the current version, revocation stales views and an
  uncertain commit quarantines existing views.
- Closed review-found graph disclosure paths: corrections require read authority on the new ID;
  candidate caps apply after filtering; direct/traversal/provenance results authorize every embedded
  endpoint, evidence, correction and nested property reference without returning hidden counts.
- Bound recovery receipts to complete canonical affected-record and policy bytes. Future policy and
  record history revisions fail explicitly. The production reducer agrees on shared modeled record
  state with the independent ordered-map oracle after each of 160 generated transactions, validates
  its derived-index rebuild separately, and restores graph/policy state after encrypted restart.
- Completed T-18 with Decision 0020 and `uste-replay`: contiguous cold replay verifies result
  digests before publish, graph checkpoints preserve all record/policy history, and canonical
  coordinator checkpoints bind retained outcomes and first-commit blob ownership.
- Added opaque storage-authenticated candidates, exact historical certificate anchors and seeded
  coordinator open. Recovery reauthenticates the full journal, exactly compares prefix coordinator
  metadata at the anchor and applies the ordinary reducer only to the suffix.
- Added alternating encrypted 1 MiB checkpoint chunks with a 256 MiB cap and terminal manifests.
  The 28-case crash matrix, 21-case authenticated malformed-carrier matrix, missing/corrupt/swapped
  chunk fallback and encrypted graph checkpoint/suffix restart equivalence pass.
- Closed review findings for split-brain generations, frontier-only anchor matching, forgeable
  seed provenance, infallible large-frame allocation and encoder/decoder invariant drift. Final
  code/evidence review reported no remaining high- or medium-severity T-18 finding.
- Completed T-45 with Decision 0021 and the safe-Rust `uste-time` crate while keeping `uste-types`
  std-only: strict RFC 3339 and explicit-unit numeric normalization, checked full-range Gregorian
  arithmetic, hash-verified embedded TZDB 2026c local resolution and explicit local presentation.
- Added bounded canonical timestamp envelopes carrying exact source token, artifact/version/locator,
  interpretation, precision, uncertainty, assumptions and accepted result. Closed invariants reject
  unknown profiles, contradictory states and over-limit inputs before copying or scanning them.
- Proved graph cold replay restores the authenticated accepted instant without reparsing the source
  token or resolving a zone. Equal and rolling-back wall samples still publish and recover distinct
  consecutive revisions. All 12 R0 time vectors execute across time/graph tests, and all 3,652,059
  supported Gregorian days round-trip through the independent checked calendar arithmetic.
- Completed T-48 with Decision 0022, exact fixed-point local/geographic primitives in std-only
  `uste-types`, and safe-Rust `uste-spatial` schemas for worlds, immutable frames/geometries and
  source-backed position observations. Missing height/position stays unknown and stored spatial
  truth has no floating/nonfinite representation.
- Added a bounded atomic reference-history catalog with exact recorded-revision visibility,
  gap-free versions, category/scope/dimension checks, root/parent closure, a 32-edge frame-depth
  ceiling, cycle rejection, immutable observation IDs, idempotent source events and prior-commit
  same-entity/world corrections. An independent scan oracle agrees on every tested frame depth.
- Added canonical transactions, effect-bound insert/retry results, cold replay and canonical
  checkpoint rebuilding. A 64 MiB canonical logical-byte bound makes the R1 catalog's hostile-input
  limit explicit; Decision 0034 later removes its prepare clone, but it remains an in-memory
  correctness catalog rather than a scalable index or BM-10 result.
- Bound 11 spatial record variants plus a transaction and checkpoint to exact length/SHA-256
  fixtures. Every record/transaction/checkpoint truncation and trailing input fails; checkpoint
  count, scope, ordering, duplicate and future-revision mutations fail closed. Final agent review
  found no remaining high-severity T-48 core issue.
- Completed T-49 with Decision 0023 and safe-Rust `uste-ingest`: one capability-free reducer owns a
  cloned graph/spatial/private-ledger candidate and publishes it only after exact source/mapping,
  authorization, checkpoint and current/historical external-reference closure all pass.
- Added bounded canonical start/batch contracts, trusted no-write preview, namespace-global source-
  event identity, durable job/batch receipts, logical hashing and 256 MiB composite checkpoints.
  Exact coordinator retry survives encrypted memory-adapter restart; alternate retry identity,
  changed bindings, stale batches and duplicate events fail closed.
- Closed review findings for same-ID/different-metadata blob binding, job deletion, embedded job-
  binding authorization leakage, cross-job event reuse, infallible frame copies and restore-time
  temporal closure. Final agent review found no remaining high-severity T-49 issue.
- Kept the T-49 boundary explicit: payload digests are caller-declared commitments, preview is a
  trusted raw helper, batches are fully accepted and nonempty, and T-54 still owns canonical per-row
  effect verification, CSV/JSON mapping, rejected-row reports, CLI and exact item/price fixtures.
- Advanced T-20 with Decision 0025 and native Rust encrypted immutable sorted runs: exact 16 KiB
  logical pages, 2 KiB roots, opaque two-slot root names, exact certificate/profile bindings,
  bounded prefix reads and a 64 MiB default/256 MiB maximum decrypted-page cache.
- Added current-record, outgoing/incoming adjacency and provenance disk families. Admission now
  compares the supplied snapshot with the coordinator's exact live reducer state, independently
  recomputes mandatory family/count/digest expectations, fully scrubs durable pages and rejects a
  stale handle after any later commit.
- Closed review findings for cross-database cache confusion, run/root binding, plaintext-bearing
  cache diagnostics, cached scrub bypass, same-length/trailing run corruption, transient I/O
  misclassification, corrupt-newest overwrite fallback and same-revision foreign snapshots.
- Pinned the page/root format and opaque `IndexName` role in literal acceptance vectors. Root and
  run corruption, operational read faults, all root-publication crash boundaries, encrypted restart,
  self-consistent wrong projections and raw graph reference equivalence pass. This is explicitly
  not T-20 closure or BM evidence; see `docs/evidence/disk-index-foundation.md`.
- Added reducer-owned indexed reads to the mandatory authorization facade. Top-level targets are
  authorized before index I/O; adjacency/provenance candidates and every embedded reference are
  filtered without exposing hidden counts; stale policies/views fail before disk access; current
  roots reject historical requests. Facade maintenance requires `ManageSchema`, handles keep cache
  counters opaque, mixed-direction scans share global candidate/byte limits, and admitted view
  binding is constant-time. Authorized in-memory/disk results agree before and after an encrypted
  restart.
- Added borrow-aware reducer checkpoint access so cold replay, checkpoint metadata and graph/spatial
  state encoding do not clone retained snapshots solely for inspection. Graph canonical checkpoint
  bytes now stream to a fallible sink and remain byte-identical to the format-1.0 collector.
- Added declared-length checkpoint publication with a one-new-payload-chunk plaintext buffer and
  terminal-manifest visibility only after exact production. Explicit producer failure, short and long
  streams leave the earlier candidate usable. Index prefix scans now also expose a bounded visitor
  path instead of requiring result collection.
- Added bounded checkpoint recovery transport: discovery authenticates/rehashes candidates without
  retaining complete plaintext, filters opaque metadata against the verified certificate chain and
  revalidates a selected chunk stream under the live owner/key context. Partial sink output is never
  publishable unless the terminal digest succeeds; current reducer decoders remain full-state.
- Added Decision 0027 and bounded graph transaction overlays. Successful preparation clones and
  validates only changed records; publication applies exact history, adjacency and provenance
  contributions instead of replacing/rebuilding the complete snapshot. Prepared deltas are
  non-cloneable and bound to their exact scope/base revision before any live mutation.
- Proved a one-record update in a 1,024-record graph retains one ordered before/after change, matches
  the prior canonical result digest and a full-rebuild reference snapshot, and rejects a stale
  same-base prepared delta without mutation. Cascade retraction removes adjacency but retains
  provenance. Final agent review found no remaining high- or medium-severity finding.
- Added Decision 0028 and an incrementally maintained reverse-dependency map with explicit owner
  kind/state/version/revision and ORed roles for entity, assertion and relationship references.
  Checkpoint decode rebuilds it and independent derived-index validation compares it.
- Delete now reconciles the target's base bucket with earlier changes in the same transaction,
  rather than scanning unrelated records. Regressions cover added/removed property references,
  accepted/proposed/retracted status changes, exact cascade and duplicate-mutation ordering, nested
  duplicate role aggregation and rejected-prepare atomicity.
- Added Decision 0029 and the pinned `graph-state-v1` profile: metadata, current records, complete
  record history, outgoing/incoming adjacency, provenance, reverse references and current/policy
  history stream into at most eight encrypted immutable runs under an exact certificate/root bind.
  Metadata offsets and all reverse kind/state/role codes are literal acceptance data.
- State-root publication independently verifies produced family counts/digests; admission compares
  the exact live scope/revision/logical digest, rejects self-consistent wrong metadata, fully scrubs
  pages, survives encrypted restart and becomes stale after the next commit. It remains optional and
  cannot construct reducer state or change journal authority. A canonical fixture pins every
  nonempty family key/value/count and seven run digests; focused agent re-review found no remaining
  high- or medium-severity issue.
- Added Decision 0030 and a privileged, certificate-rechecked full-run visitor outside consumer
  APIs. It uses one stable handle, bypasses shared cache, enforces caller page/entry/logical-byte bounds,
  authenticates every page, retains at most one assembled entry and verifies exact length,
  ordering, entry count and terminal logical digest. Partial visitor output is explicitly provisional.
- Added a distinct anchored graph-state candidate and bounded reconstruction path. It validates
  metadata/family shape before allocation, decodes current/history/policy into private maps through
  the checkpoint semantic constructor, rebuilds and stream-compares derived families, and checks
  all descriptors plus the state digest before returning. The all-eight-family fixture proves exact
  equivalence, restart, stale historical reconstruction, wrong-root and caller-budget rejection;
  authenticated descriptor-consistent current/history and adjacency mismatches also fail closed.
- Reconstruction removes a monolithic checkpoint byte buffer but still reads already-scrubbed runs
  again and materializes full ordinary `GraphState`. Caller load limits govern only this second
  pass; discovery's first scrub is bounded by absolute carrier maxima. The root omits retry/
  transaction/blob-owner metadata, so no coordinator seed, larger-than-memory recovery or
  benchmark pass is claimed. Focused final agent review found no remaining high- or medium-severity
  issue; this is not independent external security certification.
- Added Decision 0031 and the frozen `coordinator-meta-v1` profile with mandatory metadata plus
  optional ordered outcome and first-blob-owner families. Counts, fixed logical bytes, reserved
  fields, transaction uniqueness, blob shape and descriptor families are checked before seed use.
- Added a temporary authenticated recovery owner and complete cross-profile anchor identity. Cold
  recovery now rejects an unpaired newer metadata root, reconstructs a historical graph/metadata
  pair, drops the reader, and lets seeded open reauthenticate the prefix and replay the suffix.
- Seeded prefix verification now moves entries from expected maps to verified maps while decoding,
  avoiding an additional complete prefix-map copy. Graph and coordinator maps remain full-memory;
  caller reconstruction limits do not bound discovery or allocator RSS.
- Added Decision 0032 and a trusted authenticated merge from one optional `index-v1` base plus
  exact sorted before/after deltas into one unpublished encrypted current-revision run. Present
  before-values compare byte-for-byte, absent before-values require absence and absent after-values
  are tombstones; empty output creates no run.
- The merge holds one stable source handle through terminal length/count/digest verification and
  separately caps source pages/entries/logical bytes, delta count/bytes and output entries/bytes.
  Multi-page insert/replace/delete, all-tombstone output, no-base construction, wrong-before/order
  rejection, corruption discovered after provisional output and ten target create/write/size/sync
  crash boundaries pass. Frozen `index-v1` still has one terminal run per family, and the primitive
  itself neither validates graph semantics nor publishes a root.
- Added Decision 0033 and an opaque no-I/O graph delta plan bound to the exact admitted base anchor,
  scope, base/target revision and prepared result digest. It converts current/history, accepted
  adjacency, provenance, canonical reverse-role and current/history policy changes into sorted,
  coalesced exact deltas under aggregate count/byte limits.
- Postcommit publication requires the matching durable outcome, merge-rewrites all eight families,
  independently recomputes the expected terminal descriptors and full logical digest from the
  actual current reducer, and publishes only the complete matching family set. Cache failure cannot
  undo or weaken the journal transaction; partial scratch runs remain invisible.
- The revision-three-to-four disk fixture combines entity reference replacement, accepted-edge
  retraction, source-backed assertion creation and policy replacement. It rejects an undersized
  preflight, incremental count and logical-byte plan budgets plus a wrong outcome, then proves a
  postcommit merge-budget failure leaves the committed state intact and no target root visible
  across storage restart. The test harness retains the non-durable plan for an in-process retry;
  actual process loss requires a full-root rebuild. The retry exercises insert/replace/delete plus
  empty adjacency output, proves delta and full roots both reconstruct to the exact live state, and
  reopens both after restart. The
  independent validator remains a full in-memory scan, so no larger-than-memory or benchmark claim
  is made.
- Added Decision 0034 and removed complete retained-state candidate copies from ordinary spatial
  and composite ingest preparation. A borrowed graph view overlays exact changes for closure
  validation; the spatial overlay retains request indexes, per-request effect revisions/outcomes
  and inserted records only; the ingest ledger delta retains one job/receipt plus admitted rows.
- Graph scope/revision/policy-version/touched before-values, the optional spatial scope/revision/catalog
  fingerprint and job/sequence/source-event bases are all preflighted before component mutation.
  Canonical request/result/checkpoint/reducer profiles and journal authority are unchanged.
- Added pinned spatial and ingest result/chain digests plus same-batch version/forward-reference,
  retry/correction, retry-only checkpoint, one-record-over-populated-base and stale/foreign-plan
  atomicity coverage. Current maps, closure scans, snapshots and checkpoint decode remain
  full-memory; this is not T-20 closure or BM evidence.
- Added Decision 0035 and a privileged `graph-state-v1` preparation loader that retains only the
  exact positive/negative current-record closure plus current policy. Caller limits bound unique
  proofs, reference occurrences and logical proof bytes; the caller-owned page cache remains
  explicit and authenticated index work is reported.
- The resulting view owns no storage/coordinator/key capability and reuses the existing reducer in
  a pure consuming phase. Durable commit result digests match for a referenced assertion create
  and an existing assertion transition whose unchanged references must be proven.
- Exact proof budgets pass; one-less proof/reference/logical-byte budgets fail; an occupied
  correction ID is proven and rejected; stale roots fail; deletion and historical predicates fail
  as unsupported before root/page access. Review-found correction-ID and preallocation/work-limit
  holes were corrected before acceptance. Complete
  reverse/history proofs, root-delta generation from the partial view, live disk overlays and
  BM-01/BM-06 remain open.
- Added Decision 0036 and complete bounded family-3 history-prefix and family-7 reverse-owner
  proofs to disk preparation. Aggregate history/reverse entry and logical-byte limits combine with
  the frozen per-prefix result cap; decoded IDs, revisions, versions and reserved bytes fail closed.
- A two-operation retract-then-delete suppresses the authenticated base reverse dependency through
  the transaction overlay, and a historical `ReadView` replacement uses its complete prefix. Both
  match their actual durable commit digests. Zero entry budgets reject each one-entry proof, so no
  partial bucket is admitted as complete.
- All graph operation and precondition variants are now representable by the preparation proof.
  The live/recovered reducers remain full-memory; T-20 stays open.
- Added Decision 0037 and an exact authenticated family-1 metadata proof. The proof budget/report
  now covers its key/value and index lookup, and decoded revision, policy invariants and all family
  counts are checked against the admitted root descriptors.
- The pure proof result retains the exact root anchor, base metadata counters and current policy,
  then derives all eight bounded terminal-family delta sets without filesystem, coordinator, cache
  or key-vault access. Its record-plus-policy fixture matches the full-snapshot plan's delta
  count/bytes, publishes the proof-derived root through bounded merge, and admits it against the
  live reducer.
- Added Decision 0038 and the opt-in `ExternallyPreparedTransactionState` coordinator contract.
  Graph privately binds every prepared delta to its exact canonical request digest, then validates
  request bytes, absent blob inventory, next revision, scope, policy version and touched
  before-values before the ordinary durable journal append and live publication sequence.
- The end-to-end fixture rejects request/prepared substitution without revision advancement,
  durably commits the exact proof-prepared record-plus-policy transaction, returns its exact prior
  outcome for an idempotent retry, rejects a new-identity stale token, publishes the derived root,
  and reconstructs the same graph through ordinary journal replay after storage restart.
  Live publication, independent postcommit validation and recovery are still full-memory; no T-20
  or benchmark completion is claimed.
- Added Decision 0048 and a provisional exact-output visitor to the authenticated run merge. The
  graph publisher now validates proof-derived target counts and reproduces the existing canonical
  logical-state digest from the actual merged current/history/policy entries without acquiring the
  complete live snapshot. Only one record's history frames are retained, under a required caller
  budget.
- Storage proves the provisional output equals a later authenticated read. Graph unit coverage
  pins the streamed digest to the canonical reducer digest, rejects malformed secondary-family
  entries/counts and rejects a one-byte history budget.
  The end-to-end terminal-root fixture rejects wrong outcomes, undersized merge/history budgets and
  partial visibility across restart, then proves proof-only and full-state roots reconstruct to the
  same graph. This removes postcommit full-state validation, not the live reducer or recovery
  boundaries; T-20 and BM-01/BM-06 remain open.
- Added Decision 0049. Successful proof-only publication now returns the exact manifest-backed
  `DerivedGraphStateRoot` after terminal validation, rather than discarding it to a numeric receipt.
  The disk-preparation fixture carries that handle across filesystem/coordinator reopen and uses it
  for the next bounded proof without a complete-snapshot root rediscovery. Cold candidate admission
  still requires a resumable authenticated cursor and bounded predecessor lookup.
- Added Decision 0050. Storage, live coordinators and authenticated recovery now expose an opaque
  resumable run cursor whose terminal report requires exhaustion and full count/order/digest/length
  verification. A bounded two-pass predecessor proof resolves the greatest prefix key at or before
  an upper bound without consumer scan caps and assembles only the final value. Coverage interleaves
  cursor and exact reads, exercises fragmentation/corruption/early finish and proves a large prior
  value cannot exhaust a later small predecessor's result cap.
- Added Decision 0051. Live and recovery owners now stream-admit a cold `graph-state-v1` candidate
  into a capability-free `GraphDiskBase`. Admission checks history transitions, historical/current
  reference closure, current/history equality, every derived family, policy history and the
  canonical digest before returning the exact root/counts/policy handoff. The base drives existing
  bounded disk preparation without reconstructing complete graph maps.
- Added bounded exact-key reads and aggregate admission limits for history-group versions/bytes,
  exact/predecessor operations, proof page visits/result bytes and semantic reference comparisons.
  Iterator-oriented shared reference validation charges and resolves one requirement at a time,
  with no intermediate requirement vector. Exact-minus semantic/page/byte fixtures fail closed;
  targeted agent re-review found no remaining high- or medium-severity issue.
- Added Decision 0052 and `GraphDiskLiveState`. A consuming, opt-in coordinator transition proves
  the admitted base equals the complete reducer at the exact scope/revision/policy/logical-digest/
  certificate anchor while preserving journal, retry, transaction and blob-owner state.
- The warm reducer accepts only a disk-proof/terminal-plan bundle. After journal certification it
  hides the stale base and blocks distinct progress while exact idempotent retry remains available.
  A disjoint derived-index capability streams terminal publication without journal append access;
  a failed one-byte history bound leaves the plan pending, and adequate retry installs the exact
  next base and permits the next bounded proof.
- Added Decision 0053. Authenticated recovery retains one opaque final-journal transaction while
  independently validating the complete journal; the final coordinator open accepts only the
  exact admitted base or its one exact successor, revalidates the external graph preparation and
  durable outcome, and rejects mismatches, multi-revision gaps and concurrent journal advancement.
- Graph recovery decodes the captured canonical request internally and reconstructs only its
  caller-bounded current/history/reverse proof closure. Restart coverage reopens a pending suffix,
  preserves exact retry while blocking distinct progress, survives a bounded publication failure,
  installs the recovered root and then proves a ready-root restart. Focused agent review found no
  high- or medium-severity issue. Coordinator maps still replay from journal origin and bounded
  discovery/scrub remain T-20 work; no BM-01/BM-06 result is claimed.
- Added Decision 0039 and a synchronized runtime for every admitted authorized graph root. Its
  privileged report now includes cumulative decrypted-cache occupancy/events, completed authorized
  reads/raw operations, authenticated pages, fragments and logical result bytes. The counters are
  explicitly cardinality-sensitive, not consumer-visible telemetry.
- The authorized path accounts the storage statistics it previously discarded. Tests prove an
  empty handle, page reads on the first query, cache reuse without another page read, zeroizing
  clear with cumulative counters retained and a later miss/read. Current `ManageSchema` checks and
  issuer-instance-bound opaque roots reject unauthorized, revoked and foreign-coordinator access;
  outcome-uncertain coordinators reject diagnostics before possibly stale policy is consulted. This
  controls only USTE's cache; kernel/device caches and qualifying BM-01 remain unclaimed.
- The exact BM-01 engine workload was not launched while the reference host had only 5,835,172 KiB
  available and essentially all 8 GiB swap occupied, so it could not provide the accepted 24 GiB
  reservation. The Btrfs/NVMe volume still had 999 GiB free. This is a transient measurement-only
  blocker; no smaller workload is presented as qualifying evidence.
- Added Decision 0040 and the `engine-check` development verifier. It maps fixture IDs to exact
  scoped graph IDs, adds shared source Evidence, commits proposed then accepted relationships,
  publishes the encrypted index, restarts/replays and loads the persisted authorized root.
- At 20 entities/200 relationships, all 384 measured depth/class/direction queries composed from
  production one-hop reads exactly match the independent oracle after revision-4 recovery. The
  aggregate digest is `46f1bdb3138d6325e4c0f56b5fd3bbf5ff092d816e8a0f6c23acd15687b910b5`.
  The command is capped at 1,000 entities, emits `engine_benchmark:false`, and is not timing/RSS,
  Linux-filesystem, portable-recovery or BM-01 qualification evidence.
- Added Decision 0041 and bounded fixture construction to maximum-10,000-operation transactions.
  The exact qualifying profile has 212 durable revisions: one policy, 11 shared-Evidence/entity,
  100 relationship-create and 100 relationship-accept revisions. The accepted TSV and generated
  manifest pin this plan while continuing to emit `engine_benchmark:false`; it is not a completed
  qualifying run.
- Added Decision 0042 and Linux-only `linux-create`, `linux-resume` and `linux-open` phases using
  the production Btrfs adapter, OS entropy and portable Argon2id recovery. Password files use a
  no-follow descriptor and must be current-user-owned, singly linked, owner-only regular files with
  1–1024 exact bytes. Errors and JSON reports are content-free. Authenticated profile-digest,
  recovered/returned/final revision checks prevent same-frontier profile mislabeling and synthetic
  frontier reporting.
- A release-built 20/200 Btrfs smoke created revision 4 and one current encrypted root, reopened it
  in a new process, idempotently resumed every deterministic transaction without frontier advance,
  then reopened revision 4/root count 1 again. Aggregate phase times were 399/396/357/344 ms. A
  same-frontier 30/300 open was rejected by its authenticated profile binding. This is
  nonqualifying platform/recovery evidence, not BM-01 query timing or a host-cold result.
  This initial smoke covered only completed-frontier retry; Decision 0044 below adds small-profile
  interrupted-prefix process loss. Retries remain limited by the fixed 30-day outcome retention.
- Added Decision 0043, `bm01-oracle-summary-v1` and `bm01-result-v1`. A separate process now emits
  at most 256 KiB of canonical, content-free expectations bound to the engine mapping and measured
  corpus. The exact accepted profile pins 299 successful outputs, zero visit-limit outcomes, 85
  expected result-limit outcomes and summary digest
  `5e9cb81200b2016ab470419021561e0a304e1eb1d6610e0633b35925b27df402`.
- Added Linux `linux-query`: it independently reopens revision/profile/root, clears the USTE page
  cache before each authorized disk traversal, and requires exact visits/counts/logical bytes/
  digest or the exact typed limit refusal. Its content-free report includes outcome counts,
  diagnostic percentiles, RSS and authenticated cache/index counter deltas.
- A release-built 20/200 Btrfs correctness smoke matched all 384 outputs after revision-4 recovery.
  It reported an 891 ms query phase, 2.430/4.892/4.955 ms p50/p95/p99, 5,660 KiB current RSS and
  265,104 KiB peak RSS. The host caches were uncontrolled and graph state remained full-memory;
  `engine_benchmark:false` is preserved and no BM-01 pass is claimed.
- Added Decision 0044 and `linux-create-crash-probe`. It admits only incomplete frontiers, flushes a
  content-free readiness marker after the selected commit returns durable, and parks so an external
  harness can SIGKILL the exact process before ordinary portable recovery/resume.
- Release-built Btrfs children were SIGKILLed after revisions 1, 2 and 3, covering every incomplete
  phase of the 20/200 plan. Each fresh resume completed revision 4/root count 1 with zero repaired
  certificate-tail or ignored journal bytes, and each fresh open admitted that result. This is
  actual process-loss prefix evidence, not exact-scale duration, intra-transaction fault coverage
  or BM-06 qualification.
- Added Decision 0045 and `bm01-oracle-bundle-v1`, constructed outside the future sampler from one
  independent oracle. Bounded canonical sections contain 96 disjoint warm-up expectations and the
  unchanged 384 measured expectations. Exact scale pins warm-up outcomes at 74 successes, zero
  visit limits and 22 expected result limits, with bundle digest
  `d52869f24d635476f86374813e754be364d0d2df470544b71221c24b96145fae`.
- Added Decision 0046 and `linux-sample`. Exact scale has no duration/sample lowering controls: it
  validates one 96-query warm-up, then runs five complete measured windows of at least 60 seconds.
  Every query is correctness-checked as an empty-USTE-cache/retained-cache pair; only the engine
  call is timed, and success/refusal percentiles plus successful-work/index counters stay separate
  by cache state. Depth/topology and all-topology depth aggregates are both retained.
- A release-built 20/200 Btrfs sampler smoke validated the warm-up and one complete 768-execution
  paired round in 1,725 ms, with 5,788 KiB current and 264,916 KiB process-lifetime peak RSS. Empty
  versus retained USTE cache attribution reported 2,815 versus zero page reads. Host caches remained
  uncontrolled and graph state full-memory. The 30-second deadline is post-checked but cannot yet
  preempt a hung synchronous read, so no exact BM-01 budget evaluation or pass is claimed. Exact-run
  preflight found 6.2 GiB available RAM and 212 KiB free swap rather than the accepted 24-GiB
  reservation (with 998 GiB Btrfs free), so the qualifying runner was not launched or downscaled.
- Added Decision 0047's parent/worker deadline protocol. Flushed content-free markers bracket each
  engine call in one persistent worker; absence of finish after 30 seconds kills and reaps the exact
  child without sacrificing retained-cache pairing. A lifetime pipe terminates the child on parent
  death, and the parent validates typed report counts before it marks enforcement true. A real child
  timeout test exercises kill/reap.
- A release-built 20/200 Btrfs CLI smoke supervised all 864 warm-up/measured engine calls, completed
  the measured round in 1,758 ms, set deadline enforcement and post-checking true, and preserved
  2,815 empty-cache versus zero retained-cache page reads. This is still nonqualifying development
  evidence because host reservation and the full-memory graph boundary remain unresolved.
- Pinned `bm01-materialization-v1` with the exact accepted 100k-entity/1m-relationship uniform,
  distributed-hub and ring fixture, typed IDs, disjoint measured/warm-up query corpora and an
  independent adjacency-array BFS oracle. Golden digests are checked, but the manifest says
  `engine_benchmark: false`; no BM-01 timing or T-20 closure is claimed.
- Corrected review findings for normalizer-construction bypass, cap-before-copy behavior, malformed
  date-only inputs and open semantic reason combinations. Final review found no remaining high- or
  medium-severity T-45 finding; dependency unsafe remains explicitly inventoried.
- Modeled scoped entity/evidence/assertion/relationship records, evidence-backed relationship
  lifecycle, explicit correction preconditions, final-state reference closure, bitemporal reads,
  bounded reject/cascade/retract deletion and typed atomic failures.
- Executed every literal assertion transition plus the complete valid/invalid action matrix; added
  deterministic bounded entity and evidence-backed graph histories with an independently asserted
  claim/correction projection.
- Enforced the `limits-v1` 10,000-operation and conservative 100,000-reference request caps before
  state cloning. The operation wire-size cap remains with the later admission/codec boundary.
- Added independent literal R1 goldens for all assigned value tags and numeric/time/reference
  endpoints; malformed length/integer/version/tag/order/UTF-8/depth inputs fail closed.
- Added a fixed 262,144-node aggregate budget and fallible decode allocation after review found
  that per-container/wire limits alone permitted heap amplification. Adversarial nested input now
  rejects before the violating container reserves its declared children.
- Added isolated pinned `cargo-fuzz` targets for arbitrary malformed frames and generated valid
  trees, a reproducible runner and bounded CI smoke campaigns. The native fuzz runtime remains a
  test-only graph outside the safe-Rust product workspace.
- Read the complete current requirement set, both accepted product decisions and every
  current domain specification. No applicable USTE `AGENTS.md` exists.
- Preserved the clean starting worktree and created the requested branch from `be5f7a4`.
- Added Decisions 0003–0010 covering the v1 data/time, storage, cryptography/retention,
  content-worker, benchmark/limit, governance, spatial and physics profiles.
- Added literal R0 acceptance vectors and a safe-Rust standalone vector test.
- Added a storage-publication model that exhaustively tests every record cut and byte
  corruption for the proposed commit frontier.
- Resolved and compiled the pinned strict-profile dependency candidates; recorded a lockfile,
  native-link/license feasibility and the remaining unsafe/advisory review scope.
- Added the `synthetic-v1` Rust fixture-generator kernel with strict seed parsing, domain
  separation, bounded-memory emission and a pinned BLAKE3 golden digest.
- Added reproducible Fedora Kinoite/Toolbox setup and runnable synthetic commands.
- Added a committed dependency policy and ran cargo-deny 0.20.2: zero advisory, license or
  source errors; one documented duplicate-version warning in the combined parser graph.
- Materialized 19 deterministic JSON/PDF/OOXML/archive/image/audio/video fixtures with pinned
  byte counts and SHA-256 values; positive formats and hostile archive metadata are smoke-tested.
- Classified direct cryptographic/parser unsafe boundaries and selected-feature reachability;
  kept JPEG SIMD disabled and parser unsafe outside the privileged engine boundary.
- Verified unprivileged bubblewrap mount/network/user/PID isolation and in-worker address-space,
  CPU-time and descriptor limits on the reference runner; recorded cgroup/seccomp gaps.
- Completed the R0 cross-decision audit, corrected BM-04 from an inadmissible 20 GiB blob to
  a 12 GiB within-cap stream, and checked T-01–T-05/T-46–T-47 with linked evidence.
- Recorded the exact external distribution blocker without claiming disclosure verification or
  any implementation/release gate.

## Verification run

~~~text
rustc --edition=2024 --test tests/r0_vectors.rs -o /tmp/uste-r0-vectors
/tmp/uste-r0-vectors --nocapture
# 12 passed; 0 failed
rustc --edition=2024 --test experiments/storage-publication.rs -o /tmp/uste-storage-publication
/tmp/uste-storage-publication
# 4 passed; 0 failed
cargo build --manifest-path experiments/dependency-audit/Cargo.toml --locked --offline
# success
cargo test --manifest-path experiments/fixture-generator/Cargo.toml --locked --offline
# 4 passed; 0 failed
cargo test --manifest-path experiments/content-fixtures/Cargo.toml --locked --offline
# 4 passed; 0 failed
cargo-deny ... --frozen check all --show-stats
# both lockfiles: 0 errors; parser graph: one documented duplicate warning
cargo fmt ... -- --check; rustfmt --check ...
# success
python3 scripts/check_task_graph.py
# task_graph=ok tasks=62 local_implementation_gate=T-07 distribution_gate=T-62 release_gate=T-44
bash scripts/check.sh
# workspace format/clippy/test/doc pass; 265 workspace tests including 77 uste-storage, 13
# uste-crypto, 43 uste-graph, 4 uste-ingest, 34 uste-spatial, 23 uste-types, 15 uste-time,
# 11 uste-replay, 14 uste-testkit, 4 uste-policy and 27 uste-txn tests;
# docs=ok (116 links, 113 active IDs, 146 definitions); task graph=ok; R0/content/fixture tests
# and 31 isolated T-20 fixture/engine/Linux-runner tests pass; two exact-profile oracle tests are
# intentionally ignored in debug and executed under release-profile acceptance commands
cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline \
  oracle_summary::tests::qualifying_summary_outcomes_and_digest_are_golden -- --ignored --exact
# 1 passed in 5.40 s; exact 299/0/85 outcome split, accepted digest and round-trip
cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline \
  oracle_bundle::tests::qualifying_bundle_outcomes_and_digests_are_golden -- --ignored --exact
# 1 passed in 7.84 s; exact warm-up/measured splits, all accepted digests and round-trip
cargo test -p uste-graph --test disk_index bounded_disk_preparation_supports_current_history_reverse_and_stale_roots -- --exact
# 1 passed; exact proof-prepared commit, retry/stale/mismatch checks, root publication and restart
cargo test -p uste-graph --test disk_index
# 5 passed; 0 failed
cargo test -p uste-txn --all-targets
# 27 passed; 0 failed
cargo test -p uste-graph --all-targets --locked --offline
# 43 passed; 0 failed
cargo clippy -p uste-graph --all-targets --locked --offline -- -D warnings
# passed
cargo test -p uste-memory --all-targets --locked --offline
# 2 passed; 0 failed
cargo test -p uste-spatial --all-targets --locked
# 34 passed; 0 failed
cargo test -p uste-types --test spatial_primitives --locked
# 4 passed; 0 failed
cargo test -p uste-graph --test spatial_replay --locked
# 1 passed; 0 failed
cargo clippy -p uste-spatial --all-targets --locked -- -D warnings
# passed
/tmp/uste-t09-tools/bin/cargo-deny --locked check advisories licenses sources bans
# advisories ok, bans ok, licenses ok, sources ok
CARGO_DENY_BIN=/tmp/uste-t09-tools/bin/cargo-deny bash scripts/check_supply_chain.sh
# all five lockfiles including rustix 1.1.5: zero advisory/license/source errors;
# documented miniz_oxide warnings only
cargo +nightly-2026-08-01 fuzz run decode_v1 -- \
  -max_total_time=60 -seed=1592639215 -max_len=4096 -rss_limit_mb=1024 -print_final_stats=1
# 14,518,800 executions; 61 seconds; 513 MiB peak RSS; no crash artifact
cargo +nightly-2026-08-01 fuzz run structured_v1 -- \
  -max_total_time=60 -seed=1592639215 -max_len=4096 -rss_limit_mb=1024 -print_final_stats=1
# 1,610,094 executions; 61 seconds; 554 MiB peak RSS; no crash artifact
~~~

Reference runner observed: Fedora 44, kernel 7.1.10, Btrfs 7.1/local NVMe, Intel i9-13900KF,
64 GiB RAM, Rust/Cargo 1.95.0. Pinned lockfiles now pass cargo-deny advisory/license/source
policy checks. The T-15 12 GiB component measurement is recorded, but BM-04's throughput target and
remaining mixed workload have not passed.

## Limitations and external prerequisites

- T-62 requires the repository owner/administrator to enable and harmlessly test GitHub private
  vulnerability reporting. The previously observed GitHub CLI token is invalid and was not
  retried; SSH Git access is not administrative access. This blocks executable distribution and
  T-44, but it does not block local implementation, integration or artifact preparation.
- R0 decisions do not provide implementation, achieved benchmark performance or production
  security evidence. Transitive unsafe validation, the T-23 supervisor and actual BM results
  remain later-gate work.
- The canonical kernel through T-49 plus T-20's encrypted current-graph disk projection is
  implemented, but there is no database executable or production qualification. Privileged raw
  disk APIs remain separate from the new authorization-preserving current-graph consumer path.
  Checkpoint discovery and seeded open each authenticate the journal, and publication can now stream
  with a one-new-payload-chunk buffer, and checkpoint transport can recover through a bounded chunk
  stream. Ordinary graph prepare/publish is now change-bounded, but checkpoint decoding, explicit
  snapshots remain full-state boundaries. Ordinary composite writes now retain request-sized
  deltas, but graph-only closure still scans the complete borrowed catalog/job map. Reverse dependencies add an
  in-memory structure with no accepted aggregate/per-target fanout cap.
  The bounded scratch merge and graph terminal planner can rewrite and cross-check all eight
  families for one revision without collecting base runs. Proof-derived publication now validates
  actual merged outputs and the canonical digest with one caller-bounded history bucket rather than
  scanning the complete reducer. Cold semantic admission now returns a bounded `GraphDiskBase`
  without complete graph-map reconstruction. The warm live reducer now advances that base with
  one bounded pending plan and no complete graph map. Decision 0092 extends journal-anchored
  restart to an explicitly bounded multi-revision suffix with private intermediate stages and
  terminal-only publication, without `GraphState`. Decision 0059 bounds graph manifest
  discovery before run scanning. The explicit-I/O preparation proof now supports
  all graph operation/precondition variants with bounded current/history/reverse proofs and now
  feeds both the authoritative coordinator commit and a separately published terminal-root plan.
  The legacy coordinator still replays complete metadata maps; Decisions 0060–0063 add a separate
  disk-base/overlay coordinator with bounded ordinary-reducer suffix recovery and streaming
  metadata rebase. Decision 0064 connects the original ready/one-pending graph suffix path.
  Dedicated suffix read-fault/certificate-corruption
  and terminal-publication model cases now pass. Decision 0065 adds
  restricted authorized own-outcome and committed-usage reads against a ready durable policy;
  Decisions 0067–0070 add bounded graph reads and inventory-free authorized graph writes with
  explicit certified-versus-repair outcomes. Blob inventories and staging remain unsupported by
  that graph writer; Decisions 0102–0103 separately support ordinary disk-coordinator inventory
  commits and authorized staged-charge transfer. This is not a complete graph consumer interface.
  Decisions 0095–0101 add opt-in map-free certificate/storage-blob cold recovery and bounded
  inventory append; legacy APIs retain their resident collections. Decisions 0111–0112 add
  opt-in admitted disk principal totals and authorized indexed accounting; legacy accounting
  still streams all owners. Populated-base quota bootstrap remains open, and catalog construction
  still rewrites immutable families,
  and compatibility first-owner discovery remains read-amplified. Decisions 0104–0105 make
  order-independent metadata correspondence use linear authenticated reverse certificate scans.
  Opt-in metadata/graph publication now bounds fallback scrubbing explicitly;
  the legacy publication API retains its compatibility scrub.
  Qualifying BM-01/BM-06 campaigns have not run; capped native development observations do not
  substitute for them. Graph policy is
  durable; the trusted adapter must supply its exact
  current copy at authorized open. The oracle
  intentionally scans records and is not scalable. The fuzz runner requires nightly Rust plus a C++
  compiler, both confined to development tooling.
- T-45 normalizes timestamps and preserves their provenance but does not add temporal indexes,
  content-adapter extraction, clock-drift estimation or leap/TAI/GPS conversion tables. T-21 and
  T-24 own those layers. The admitted named-zone behavior is pinned to embedded TZDB 2026c.
- T-48's transform reference remains an opaque same-scope version binding. T-49 now proves its
  target exists as an active graph Entity, but T-50 still owns transform schema/version semantics
  and evaluation. The 64 MiB spatial catalog and T-49 in-memory job ledger are correctness
  baselines; Decision 0034 bounds their ordinary preparation deltas but does not make them
  disk-backed. T-59 owns native disk indexing and the one-million-item BM-10 workload.
- T-49 structural maxima are not simultaneous capacity claims. Its accepted-row cursor has only
  `Open`/`Completed` states; no rejected-row advancement/report, mapping execution, CSV/JSON parser,
  public authorized preview, item/price fixture, throughput/RSS/concurrency or platform-crash result
  is claimed. T-54 owns those tooling and provenance extensions.
- T-13 local acceptance is complete on the reference Btrfs runner and the independently identified
  ext4 mount `/var/mnt/archive_vault` (`/dev/sda1`). These SIGKILL tests do not simulate controller
  cache loss or actual power loss. The certificate log fails closed at 1 GiB pending later
  T-35 maintenance/rollover design.

## Next dependency-permitted work

Current action: T-20 remains the priority. Decisions 0133–0205 have since implemented the
packed copy-on-write, admission/domain integration, native construction/recovery and bounded
cache work that the older handoff below proposed. Do not restart those capabilities. The
current verified baseline is `184b54b`; D0206's larger correctness pass and sampling timeout
are archived at its earlier `18a45e4` binary. D0207's redundant commitment-computation removal passed
726 workspace and 122 native tests; D0208 measured its separately pinned read-only behavior.
D0209 passed 730 workspace and 122 native tests. Continue bounded cache lookup work and separately
pinned measurement under the one-workload safeguards, then native scaling and remaining accounting/lifecycle
prerequisites for qualifying BM-01/BM-06. A 20,000-entity oracle match and an 8,192-entity BM-06
development run do not satisfy the exact qualifying profiles or reserved-runner requirements.
T-19 follows T-20; M1 remains pinned separately. The topmost sections carry current commands,
results and next actions. The following chronological evidence is historical, not an active plan.

Historical handoff through Decision 0132: Decisions 0108–0120, including the retained native
20,000-entity comparison and explicit BM-06 origin rebuild, are committed and pushed through
`3f793a6`; their evidence is recorded above, not in flight. Decisions 0121–0122 extend primary-owner
suffix staging and bounded inventory-bearing genesis bootstrap. Decision 0123 maintains first
references on that path; Decision 0124 maintains quota projections (pushed `746ca3d`). Decision
0125 applies streamed metadata to native paired-base recovery (pushed `de67ff3`). Decision 0126
removes duplicate staging proofs (pushed `fa3897a`). Decision 0127 adds bounded certificate
windows to private forward recovery (pushed `42f9abb`). Decision 0128 adds locally verified
canonical ordered commitments (pushed `3a2a361`) without changing any persisted v1 profile.
Decision 0129 adds the encrypted packed-page framing carrier (pushed `7a98146`); Decision 0130
adds bounded durable pack I/O (pushed `a64fc19`). Decision 0131 adds the typed record codec
(pushed `d12fe7f`); Decision 0132 adds bounded authenticated lookup. At that point the next work was
reachable-node copy-on-write batches, root admission and domain integration, and
complete accounting before qualifying BM-01/BM-06 campaigns. Preserve retained fixtures, caps,
M1's exact-version handoff and all qualification targets. T-19 follows T-20. The entries below
preserve chronological implementation evidence, not requests to repeat completed work.

T-20 bounded prefix-scan increment (Decision 0066), tested on `d8fd8fa` plus this increment:
storage and both coordinators now admit prefix-scan page visits (including cache hits), entry
count and key-plus-value bytes. Fragmented entries are admitted before entry allocation.
Cold/warm budget refusals, exact-byte success, absence and provisional-visitor refusal pass.
Verification: `systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G
-p MemorySwapMax=512M bash -lc 'CARGO_BUILD_JOBS=1 cargo test -p uste-storage -p uste-txn
-p uste-graph --all-targets --locked --offline -- --test-threads=1 && CARGO_BUILD_JOBS=1
cargo clippy -p uste-storage -p uste-txn -p uste-graph --all-targets --locked --offline
-- -D warnings'` exited 0. Host check after completion: 17 GiB available RAM, 1.5 GiB free
swap. No qualifying benchmark ran.

T-20 metadata authorization hardening, tested on `8e85535` plus this increment: the restricted
facade now owns its private 64 KiB cache and fixes outcome admission at 64 visits/136 bytes.
Consumers cannot inspect cache telemetry or choose undersized budgets that reveal another
principal's transaction before ownership filtering. Decision 0065 records the interface change;
the pinned M1 interface is unchanged. Existing base/overlay isolation and denial-before-I/O tests
pass, as do new missing-versus-foreign transaction checks before and after expiry.
Commands under the same 3G/4G/512M scope: `CARGO_BUILD_JOBS=1 cargo test -p uste-graph
--test disk_index --locked --offline -- --test-threads=1` (8 passed), then the same command
with filter `cold_root_pair_reconstructs_seed_and_replays_graph_suffix` after the final assertions
(1 passed); `CARGO_BUILD_JOBS=1 cargo clippy -p uste-txn -p uste-graph --all-targets
--locked --offline -- -D warnings` passed. `python3 scripts/check_docs.py` and
`python3 scripts/check_task_graph.py` passed (139 links, 68 tasks).
The next point-read increment (Decision 0067), tested on `c754264` plus its changes, adds a
restricted disk reader with trusted-adapter fixed limits/private cache, exact durable policy,
target authorization before I/O, and cancellation before/candidate/terminal checks. Graph
current/history reads use admitted ready roots, exact journal anchors and embedded-reference
filtering. Tests cover current/historical values, absence, future revision, foreign namespace,
foreign kernel, record denial, cancellation, byte refusal and embedded-reference suppression;
the fault fixture proves denied point reads do not consume an armed disk-read fault.
Initial compile checks found and corrected anchor-result/root-type mismatches, a test delimiter
and unnecessary qualifications. Final verification under the same memory scope:
`CARGO_BUILD_JOBS=1 cargo test -p uste-txn -p uste-graph --all-targets --locked --offline
-- --test-threads=1` and `CARGO_BUILD_JOBS=1 cargo clippy -p uste-txn -p uste-graph
--all-targets --locked --offline -- -D warnings` exited 0. Docs/task checks passed (140 links,
68 tasks). Last sampled cgroup peak was 638,480,384 bytes with zero swap (not a final peak).
Decision 0068 extends that reader to adjacency/provenance with one shared page/entry/byte/lookup
budget, fixed by the trusted adapter. The renamed `GraphDiskReadLimits` can still disable expansion.
The synthetic reference fixture covers self-loops, parallel edges, both directions, provenance,
hidden references, historical permissions and cancellation. It succeeds at exactly 24 visits,
six scanned entries, ten lookups and calculated encoded bytes; each one-less budget fails,
including repeated populated-cache calls. Every observed cold adjacency read error refuses
partial output and a subsequent retry matches the reference; denied expansion consumes no I/O.
Tested baseline: `03cd200` plus the graph expansion files and Decision 0068. The next preflight
increment was drafted in separate, unreferenced files and is explicitly excluded from this gate.
`systemd-run --user --scope -p MemoryHigh=3G -p MemoryMax=4G -p MemorySwapMax=512M
bash -lc 'CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 CARGO_NET_OFFLINE=true bash scripts/check.sh'`
exited 0: workspace/all-feature formatting, lint, tests and rustdoc; docs/task graph; R0 and
publication vectors; dependency/fixture checks; isolated T-20 driver (31 passed, two pre-existing
exact-profile oracle tests ignored) and its strict lint. M1 SIGKILL/corruption tests remain green.
Last sampled scope peak: 2,516,131,840 bytes, zero swap; this is not a final peak or benchmark
qualification. Host still had 17 GiB available RAM and 1.5 GiB free swap. T-20 remains open.
Decision 0069 now connects shared read-only commit admission before external disk preparation.
Tested on `678d64f` plus this increment: the extracted admission prefix compares unchanged after
whitespace/result-wrapper normalization; normal commit and new `check_commit` use the same rules.
The new fault-backed test proves no preparation/publication/reservation, exact retry/collision/
expiry/cancellation/owner limits, uncertain-state rejection and certified restart. Its initial
harness error attempted to rearm an unconsumed fault; corrected the test to require that fault
to fire on the subsequent actual commit, then recover before continuing.
Commands under the established 3G/4G/512M scope, one Cargo job/test thread:
`cargo test -p uste-replay --test coordinator_checkpoint --locked --offline -- --test-threads=1`
(6 passed); `cargo test -p uste-txn -p uste-graph -p uste-memory --all-targets --locked --offline
-- --test-threads=1` passed; `cargo clippy -p uste-txn -p uste-replay -p uste-graph --all-targets
--locked --offline -- -D warnings` passed. Preflight host headroom improved to 29 GiB available RAM
and 2.7 GiB free swap, but the implementation boundary still does not justify qualifying campaigns.
Decision 0070 implements authorized inventory-free disk graph commits, tested on `a507b36` plus
the writer increment. The facade authorizes namespace/targets, quota and scope before clock/I/O;
shared preflight handles retry/collision before bounded proof preparation. One clock observation
is reused. Certified policy changes synchronize before root repair, and errors after certification
carry the exact durable outcome. A revoked principal cannot continue while root repair is pending.
Review found hidden dependency counts in raw graph preparation errors; the writer now uses the
existing content-free reducer error classification, covered by a hidden-dependency delete test.
Tests also cover foreign authentication, byte refusal, precommit I/O failure, exact retry with
cancellation/tiny proof budget, expiry/collision, root budget/I/O repair failure, journal-sync
uncertainty, independent reference digests and cold replay with authenticated principal identity.
Initial fixture compile mistakes (merge argument count and private-anchor access) were corrected
using the supported manifest read API. Final scoped commands (3G/4G/512M, one job/thread):
`CARGO_BUILD_JOBS=1 cargo test -p uste-txn -p uste-graph -p uste-replay -p uste-memory
-p uste-memory-adapter --all-targets --locked --offline -- --test-threads=1`;
`CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked --offline
-- -D warnings`; `CARGO_BUILD_JOBS=1 RUSTDOCFLAGS="-D warnings" cargo doc --workspace
--all-features --no-deps --locked --offline` all exited 0, including M1 real-process tests.
Docs/task checks pass (143 links, 68 tasks). Host preflight: 38 GiB available RAM, 3.9 GiB free
swap. No qualifying benchmark ran. Next add disk-aware staged-upload accounting/reconciliation
and inventory admission, then remove scalable first-owner/storage recovery metadata bottlenecks.

T-63–T-68 and M1 are complete at implementation `b9689f3`, qualified by Decision 0058 and the
exact-version consumer handoff. The resumed audit confirmed that commit's lockfile digest and
unchanged pilot sources, and the complete gate at `8885da4` included the pilot recovery tests.
The two interruption-pending commits `8885da4` and `808f2fd` are now pushed to origin.
Continue the preserved full-product work below. Large benchmarks still require adequate host
headroom; the resumed preflight showed 4.1 GiB available RAM and 112 KiB free swap.

Continue T-20 by moving coordinator retry, transaction and blob-owner metadata off the full
in-memory journal replay path. Decision 0060 supplies the missing transaction-ID disk ordering;
its cold admission now uses authenticated journal/disk correspondence without a coordinator
comparator map. Decision 0061 pairs retry, transaction and owner indexes into an admitted disk
metadata base with exact first-owner proofs and explicit read amplification. That base is now
installed by the opt-in disk coordinator with bounded mutation overlays (Decision
0062). Ordinary-reducer suffix recovery and disk-graph ready/one-pending recovery are implemented;
Decision 0073 now maintains single-pass first-reference evidence across bounded disk metadata rebase.
Decisions 0074–0085 connect independent development oracles, native construction/recovery/query,
real process-loss tests and supervised sampling to the disk path. Profile-derived limits now
validate across the accepted size range, and the 1,000/10,000 development oracle check passes.
Native commands remain capped pending qualification readiness. Cached primitive work now complements
adapter counters, but uncached/recovery/publication accounting is incomplete. Decision 0086
addresses ordered cache eviction and explicit logical accounting; Decision 0087's larger native
development run matches every oracle query but records zero query evictions. Decision 0088 adds
resumable authenticated transaction ranges. Decisions 0089–0090 remove repeated page-parser
allocation/validation and add actual native one-page eviction regression; the pinned larger
development comparison preserves all results and work counts with reduced CPU time. Decision
0091 supplies unpublished certified-revision storage roots. Decision 0092 connects them to
verified private multi-revision graph/coordinator recovery. Decision 0093 integrates native
resume with explicit fixture-derived limits and separate suffix diagnostics. Native
process coverage of stale-root gaps is now passing. Decision 0094 supplies bounded authenticated
disk certificate proofs and exact live-owner
binding for proven reads/cursors/staging. Decision 0095 integrated proof-bound handles and map-free
certificate recovery; Decisions 0096–0097 added committed blob proofs and disk-coordinator reads.
Decision 0098 adds the verified bounded catalog rebuild/admission; Decision 0099 supplies opt-in
map-free blob cold recovery. Decisions 0100–0103 integrate native recovery, bounded inventory
append, disk coordinator inventory commits and authorized certified upload charge transfer.
The existing graph domain intentionally prohibits inventories. The first-reference
publisher remains a bounded legacy bridge, not larger-than-memory construction.
Explicit-I/O outcome APIs and journal-prefix validation must preserve exact retry,
transaction collision and first-owner semantics. The authenticated streaming suffix is now
implemented; native failure qualification, full immutable-run rewrite amplification and
accounting qualification remain open. Decision 0113 implements bounded populated-base quota
construction. Decisions 0114–0117 pin BM-06 materialization and add bounded model/native recovery,
cache-loss refusal/retained-root controls and authenticated prefix resume. Decisions 0118–0119 add
private origin candidates and explicit bounded native graph/coordinator cache-loss rebuild.
Exact-scale construction, incremental large-history origin metadata staging and the 30-trial
campaign remain open. Decisions 0111–0112
add admitted disk principal totals and authorized opt-in accounting without whole-ledger scans.
Decisions 0104–0110 reduce authenticated
reverse-scan, empty-catalog, point-lookup and cache-recency work without qualifying T-20.
Run the exact five-sample BM-01 campaign under the accepted host
24 GiB reservation once the implementation boundary is honest, and implement/run BM-06's
pinned 10-million-event protocol. Then return to T-19's
remaining VT gaps and BM-02/BM-04 work; no failed or absent benchmark is accepted as passing.
T-62 remains independent and must not be represented as complete without owner-administered
evidence. BM-04 performance optimization remains later acceptance work and is not silently treated
as passed.

Persistent-goal continuity: the existing goal remains recorded as blocked by the service following
the capacity interruption. Its API exposes completion/blocking but no resume operation; creating a
replacement is rejected because the existing goal is unfinished. The user's resumed implementation
authorization remains in force. This status is a control-plane limitation, not project completion
or a blocker to local work; preserve the original goal and use this handoff for continuity.
