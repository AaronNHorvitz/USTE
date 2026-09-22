# Decision 0248: Borrowed authorization evaluator

Date: 2026-09-22

Status: Implemented and locally verified; development observation pending.

Decision 0247 left the retained four-hop path at 489.870038 ms with zero adapter reads. Inspection
found that every relationship, neighbor and embedded-reference check repeated the namespace-policy
and principal-grant B-tree lookups and constructed then immediately dropped an
`AuthorizationLease`, including an `Arc` clone. A single adjacency request can perform many such
checks against one immutable policy, principal and namespace.

Add a borrowed `AuthorizationEvaluator` resolved from the issuing kernel, authenticated principal
and exact namespace. It holds only the immutable grant, scope and policy version and cannot outlive
the kernel borrow; replacing policy requires the evaluator to be dropped. Its immediate check
requires exact target scope, action permission and per-record denial just as before. The existing
lease-producing `PolicyKernel::authorize` now delegates to this evaluator, so the two paths cannot
drift semantically.

The packed authorized reader still performs its top-level authorization and durable-policy
validation before request work. It resolves one evaluator for the request, uses it for every
declared requirement and candidate/reference check, and still polls cancellation before every
candidate authorization. Foreign scopes, foreign-kernel principals, missing grants, denied actions
and record-specific denials remain unauthorized. No lease or policy version is retained beyond the
read call, and no persisted format or public authorization result changes.

Policy tests compare evaluator and lease-producing authorization across allowed, denied-action,
record-denied and foreign-scope targets, and separately reject a principal issued by another
kernel. Focused packed graph tests preserve permission-disabled and sticky-cancellation boundaries
and prove revocation denies before vault work. Strict policy/transaction/graph Clippy passed.

The complete optimized workspace gate passed **764 tests** with strict all-target/all-feature
Clippy. The complete optimized standalone T-20 gate passed **142 active tests with five unchanged
opt-in ignores** and strict Clippy. Logs: `/tmp/uste-d248-workspace-verification.log` and
`/tmp/uste-d248-native-verification.log`. Both gates used one Cargo job, one test thread, locked
offline dependencies and the 4 GiB process address-space limit under the verified enclosing 5/6
GiB high/max and 512 MiB swap caps. The enclosing scope recorded no maximum-limit, OOM, OOM-kill or
CPU-throttle event.

No benchmark ran, so no performance, T-20, M1 or qualification claim follows. After commit and
push, rebuild and pin the release benchmark, then run one unchanged medium-range-pressure
observation to measure the zero-I/O retained path. Preserve every authorization, cancellation,
cache-default and qualification prerequisite.
