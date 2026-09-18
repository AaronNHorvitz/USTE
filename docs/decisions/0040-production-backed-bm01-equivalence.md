# Decision 0040 — Production-backed BM-01 development equivalence

Date: 2026-09-17

Status: accepted as T-20 benchmark-driver groundwork. T-20 and BM-01 remain open.

## Context

The pinned `bm01-materialization-v1` fixture and independent adjacency-array oracle previously had
no executable connection to the production graph, transaction, policy, crypto or disk-index code.
Decision 0039 supplied privileged measurement telemetry, but a qualifying driver still needs a
validated record mapping and proof that client-composed depth-one-through-four traversal has the
same semantics as the independent oracle.

## Decision

The standalone `experiments/t20-bench` crate now depends on the production Rust crates and provides
an `engine-check` command for profiles of at most 1,000 entities. The cap is deliberately below the
accepted 100,000-entity profile and the output always declares `engine_benchmark:false` and
`nonqualifying-development-equivalence`.

The mapping profile is `bm01-uste-graph-v1`:

- typed fixture entity and relationship IDs become the exact bytes of scoped graph `RecordId`s;
- entities use type `bm01-entity-v1`, schema 1 and null properties;
- one shared Evidence record binds relationships to the versioned engine mapping and its profile-
  specific materialization digest;
- relationship types distinguish uniform, distributed-hub and ring topology, with null properties,
  unknown valid time and the shared Evidence reference; and
- every relationship is created `Proposed` and transitioned to `Accepted` in a later transaction
  before index publication.

The verifier installs a durable namespace policy, commits entities/evidence, relationships and
acceptance through `AuthorizedCoordinator`, publishes the encrypted current index, simulates a
durable adapter restart, replays the journal, discovers the persisted root and then queries only
through `read_indexed`. Its breadth-first adapter uses stable ordered frontiers, validates every
returned relationship/endpoint/status/neighbor against the materializer, and applies the frozen
global one-million-visit and 100,000-unique-result caps. Every measured query shape must have the
same outcome as the independent oracle: successful outputs exactly match visits, relationship
ordinals, reachable entity ordinals and digest, while bounded refusals must match the same typed
limit. The evidenced 20/200 profile has 384 successful outputs.

## Consequences and limits

This closes the fixture-to-engine semantic gap at bounded development scale and pins the 20/200
aggregate output digest. It also exercises production object encryption, authorization, journal
replay and encrypted index reads, but uses the durable memory fault-model filesystem, deterministic
development entropy and a test key wrapper. It records no latency, RSS, real filesystem behavior or
portable recovery cost.

The current production API exposes one-hop adjacency only, so the driver composes multi-hop BFS in
the caller. This is honest equivalence for the documented supplied-graph navigation semantics, not
latency for a native multi-hop request. A qualifying runner still needs Linux/Btrfs storage,
portable recovery wrapping, exact 100k/1m batched materialization, separate oracle generation,
cold/warm repeated samples, RSS/environment evidence and the accepted 24 GiB reservation.

## Verification

~~~text
cargo test --manifest-path experiments/t20-bench/Cargo.toml --locked --offline \
  engine::tests::production_engine_matches_oracle_for_every_scaled_query -- --exact
# 1 passed; 20 entities, 200 relationships, revision-4 recovery, all 384 measured queries
cargo run --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- \
  engine-check --entities 20
# engine_benchmark=false; recovered_revision=4; queries=384;
# output_digest=46f1bdb3138d6325e4c0f56b5fd3bbf5ff092d816e8a0f6c23acd15687b910b5
cargo clippy --manifest-path experiments/t20-bench/Cargo.toml \
  --all-targets --locked --offline -- -D warnings
# passed
~~~
