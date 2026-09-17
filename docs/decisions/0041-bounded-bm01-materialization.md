# Decision 0041 — Bounded exact-profile BM-01 materialization

Date: 2026-09-17

Status: accepted as T-20 benchmark-runner groundwork. T-20 and BM-01 remain open.

## Context

Decision 0040 connected the frozen BM-01 fixture and independent oracle to the production graph
path, but its implementation collected each complete entity, relationship and acceptance phase in
memory before committing it. The accepted 100,000-entity and 1,000,000-relationship profile cannot
use that development-only shape, and the production graph contract admits at most 10,000 operations
per transaction.

## Decision

The production-backed materializer streams each phase into fixed maximum-10,000-operation batches.
It commits a batch before constructing the next one and advances transaction sequence numbers with
checked arithmetic. Mapping remains unchanged: one shared Evidence record precedes all fixture
entities; every relationship is then created as `Proposed` and accepted in a later operation.

For the exact qualifying profile this protocol has 212 durable revisions:

- one namespace-policy revision;
- 11 entity/evidence revisions for 100,001 records;
- 100 relationship-creation revisions; and
- 100 relationship-acceptance revisions.

The calculation is executable and the qualifying value is pinned in
`acceptance/r1/bm01-materialization-v1.tsv`. Generated manifests identify
`bm01-uste-graph-v1`, report their exact durable revision count and continue to declare
`engine_benchmark:false`.

## Consequences and limits

Materialization's operation-vector memory is now transaction-bounded rather than profile-sized,
and the exact accepted profile has a deterministic transaction plan. This does not bound the
current in-memory graph reducer, replace the development memory filesystem/test key wrapper, or
provide Linux durability, portable recovery, latency, RSS or cache-state evidence. The existing
1,000-entity command cap remains. A qualifying runner must still supply those facilities and run
the exact workload under the accepted 24 GiB reservation.

## Verification

~~~text
cargo test --manifest-path experiments/t20-bench/Cargo.toml --locked --offline
# 12 passed; includes the 212-revision qualifying plan and 20/200 production/oracle equivalence
cargo clippy --manifest-path experiments/t20-bench/Cargo.toml \
  --all-targets --locked --offline -- -D warnings
# passed
~~~
