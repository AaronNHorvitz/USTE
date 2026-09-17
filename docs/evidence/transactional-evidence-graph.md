# T-17 transactional evidence graph evidence

Date: 2026-09-17 · implementation commit: `8f035d7` · local correctness evidence, not an
independent security certification

## Implemented result

- Added the safe-Rust `uste-graph` production reducer with typed entity, evidence, assertion and
  relationship records; version/valid-time/recorded-revision history; corrections; lifecycle
  transitions; exact declared deletion cascades; and same-scope final-state reference closure.
- Added strict canonical graph transaction and durable-policy encoding with exhaustive truncation,
  unknown/extra/missing-field and policy-chunk canonicality checks.
- Added symmetric ordered outgoing/incoming adjacency and evidence-provenance indexes, atomic
  rebuild validation, stable handling of cycles/self-loops/parallel edges and bounded raw helpers.
- Added reducer-declared write requirements and reducer-owned authorized projections. Direct,
  historical, adjacency and provenance reads check current policy before state access and check
  every embedded record reference before returning a containing record.
- Added durable graph policy bootstrap/replacement/history. Authorized open requires an exact
  recovered policy, consumer installation is rejected, replacement is journaled, old policy
  retries are idempotent, revocation stales existing views, and uncertain commits quarantine them.
- Bound recovery receipts to canonical complete changed-record and policy contents rather than
  affected identities alone.

## Focused verification

~~~text
cargo test -p uste-graph --all-targets --locked
# 13 passed; 0 failed
cargo test -p uste-txn --all-targets --locked
# 25 passed; 0 failed
cargo clippy -p uste-policy -p uste-txn -p uste-graph --all-targets -- -D warnings
# passed
~~~

The graph tests cover exact and first-over-limit requests, every encoded truncation, malformed
root/action/policy forms, forward reference closure, correction history, acceptance and terminal
index changes, conflict atomicity, exact cascade declarations, cycles, self-loops, parallel edges,
pre-install/current/future policy history and semantic result-digest separation.

The differential run applies 160 deterministic generated transactions across entity and
evidence-backed graph histories. It compares every shared, reference-modeled record field with the
independent reference model after every revision; a separate production check reconstructs and
compares all derived indexes from production records. Focused authorization tests use the actual
encrypted journal to prove privileged policy bootstrap, denied consumer
install, hidden candidate/cardinality/reference concealment, denied correction before state
mutation, revocation, historical idempotent policy retry, restart recovery and exact-policy reopen.
A crash-after-sync test proves `OutcomeUnknown` invalidates already issued authorized views.

Review identified and the implementation closed: pre-filter result-limit leakage, policy-install
divergence, stale idempotent policy replay, uncertain-view reuse, missing correction-ID read checks,
embedded-reference disclosure, missing durable-policy admission, post-limit candidate allocation,
noncanonical policy chunk partitions, coarse reducer result receipts and future policy-history
answers. The final review reported no remaining high- or medium-severity T-17 finding.

## Limits and next owners

This is a correctness-first in-memory graph projection over the durable encrypted journal. It does
not claim bounded-cache/disk-index scalability or BM-01/BM-06; T-20 owns those results. T-18 owns
checkpoint/replay equivalence beyond journal reconstruction. T-22 owns complete artifact and
derivation lineage. T-35 owns physical purge/compaction/restore-epoch behavior. Arbitrary-depth
navigation, combined query planning and public API/CLI surfaces remain T-55, T-53 and T-27.

T-62 remains an external distribution prerequisite: GitHub private vulnerability reporting has
not been owner-verified. It does not block this local implementation evidence and is not claimed
complete here.
