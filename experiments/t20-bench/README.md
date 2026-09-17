# T-20 benchmark fixture foundation

This standalone experiment pins `bm01-materialization-v1`. It prepares deterministic synthetic
fixture semantics and an independent adjacency-array BFS oracle; it does **not** run USTE engine
queries, measure latency, or provide BM-01 acceptance evidence.

The qualifying-size profile always uses the accepted BM-01 seed and exactly:

- 100,000 typed entity IDs and 1,000,000 typed relationship IDs;
- 800,000 uniformly shaped directed relationships with self-loops excluded;
- 100,000 relationships distributed across 100 hubs, alternating outward/inward direction and
  excluding self-loops;
- 100,000 clockwise relationships forming one complete entity ring;
- 32 measured and 8 disjoint warm-up roots for each topology class and each depth 1 through 4.

Typed IDs combine a seed-derived, type-separated prefix with the big-endian ordinal. They are
deterministic and collision-free within the admitted ordinal range. Endpoint shaping consumes the
byte-compatible `synthetic-v1` graph identity stream. The manifest records separate synthetic
entity/relationship stream, materialization, topology, measured-query, and warm-up-query digests.
It emits no individual record, endpoint, or query-root values.

## Commands

From this directory:

```text
cargo run --release --locked --offline -- manifest
cargo run --release --locked --offline -- manifest --entities 1000
cargo test --locked --offline
cargo clippy --all-targets --locked --offline -- -D warnings
```

The default is the exact qualifying fixture size. `--entities N` derives exactly `10*N`
relationships while preserving the 80/10/10 split and a full `N`-entity ring. Any value below
100,000 is labeled `nonqualifying-small-scale`; it is only for development and tests.

## Oracle semantics

The oracle constructs independent outgoing and incoming adjacency arrays. For each depth level it
expands a stable entity-ordinal frontier and scans stable relationship-ordinal candidates.
`visits` counts every adjacency candidate examined, including a relationship revisited from its
other endpoint. Results are unique relationship ordinals; reachable entities are reported
separately and exclude the root. The default limits are global across the entire query: 1,000,000
visits and 100,000 unique relationship results. Exceeding either fails the query rather than
returning a truncated answer.

## Deliberate limitations

- The crate does not open a durable database, publish an encrypted index, authorize a principal,
  control caches, or execute an engine query.
- It does not collect latency, RSS, I/O, result-byte, recovery, or correctness-equivalence evidence
  against the engine.
- `qualification: qualifying-fixture-size` describes only exact fixture dimensions. It is not a
  performance or release claim.
- BM-01 still needs cold/warm authorized encrypted disk-query runs on the reference machine. BM-06
  and streaming larger-than-memory recovery are outside this fixture increment.
