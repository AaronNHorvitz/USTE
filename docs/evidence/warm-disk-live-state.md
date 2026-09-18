# Warm disk-backed live graph state evidence

Decision 0052 replaces the complete live graph publication target for a warm coordinator with one
admitted disk base and at most one bounded pending terminal-root plan.

## Verified behavior

- A consuming, source-owned representation transition requires exact scope, revision, policy,
  logical digest and journal certificate equivalence while preserving coordinator-owned retry and
  transaction metadata. Its associated destination prevents downstream self-attestation, and the
  pending disk state exposes no onward transition.
- Only request-bound `GraphDiskCommit` values produced by disk proofs are accepted; ordinary
  reducer preparation remains closed.
- The journal remains authoritative. After certification the reducer is pending and hides its
  stale base, rejects distinct progress and permits only exact coordinator retry or root repair.
- Root maintenance has a disjoint read-only reducer borrow and a merge/root-publication capability;
  it cannot append transactions or mutate retry metadata.
- An undersized history-validation budget fails after commit without clearing pending state.
  Retrying with adequate limits publishes and installs the exact root, counts and policy.
- The installed revision-two base immediately proof-prepares revision three without complete graph
  map reconstruction.
- Graph and transaction package suites plus strict clippy pass without weakening existing stale-
  token, crash/recovery, malformed-input or authorization coverage.

~~~text
cargo test -p uste-graph --all-targets --locked
# 43 passed; 0 failed
cargo test -p uste-txn --all-targets --locked
# 27 passed; 0 failed
cargo clippy -p uste-graph -p uste-txn --all-targets --locked -- -D warnings
# passed
bash scripts/check.sh
# 265 workspace tests passed; documentation=ok with 116 links, 113 active IDs and 146
# definitions; task_graph=ok; 31 isolated T-20 tests passed; 2 exact-profile debug tests remained
# intentionally ignored
~~~

## Deliberate boundary

This is warm in-process state. `GraphDiskLiveState` has no checkpoint codec and a crash with a
pending plan recovers through the existing complete `GraphState` journal replay. Coordinator
metadata, suffix replay, candidate discovery/initial scrub and qualifying BM-01/BM-06 runs remain
T-20 work. A root publication error can leave only encrypted unreferenced runs; T-35 still owns
their bounded reclamation.
