# T-20 benchmark fixture foundation

This standalone experiment pins `bm01-materialization-v1`. It prepares deterministic synthetic
fixture semantics and an independent adjacency-array BFS oracle. Its bounded `engine-check`
command also validates the mapping against production encrypted, authorized, durable graph/index
code after a simulated restart. It does **not** measure latency or provide BM-01 acceptance evidence.

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
cargo run --release --locked --offline -- oracle-summary --entities 20 > ORACLE
cargo run --release --locked --offline -- engine-check --entities 20
cargo run --release --locked --offline -- linux-query --root ROOT \
  --password-file PASSWORD --oracle-file ORACLE --entities 20
cargo test --locked --offline
cargo clippy --all-targets --locked --offline -- -D warnings
```

The default is the exact qualifying fixture size. `--entities N` derives exactly `10*N`
relationships while preserving the 80/10/10 split and a full `N`-entity ring. Any value below
100,000 is labeled `nonqualifying-small-scale`; it is only for development and tests.

`engine-check` is capped at 1,000 entities and always emits `engine_benchmark: false`. It maps typed
fixture IDs to scoped graph IDs, adds one shared source Evidence record, creates relationships and
then accepts them in a separate durable revision. After encrypted index publication it restarts the
durable memory adapter, replays the journal, loads the persisted authorized root and compares all
384 measured query shapes with the independent oracle. The 20-entity/200-relationship golden output
digest is `46f1bdb3138d6325e4c0f56b5fd3bbf5ff092d816e8a0f6c23acd15687b910b5`.

`oracle-summary` is intended to run separately from `linux-query`, so the independent oracle's
adjacency arrays do not enter the query process. The bounded 256 KiB summary pins profile/query
digests and exact output or limit outcomes without record identifiers. At exact scale it contains
299 successful outputs and 85 expected result-limit refusals. `linux-query` reopens the production
Btrfs database, clears USTE's page cache before each authorized traversal and checks exact outcome
equivalence. Its single-pass timings/RSS/counters are correctness diagnostics and the JSON still
sets `engine_benchmark:false`.

## Oracle semantics

The oracle constructs independent outgoing and incoming adjacency arrays. For each depth level it
expands a stable entity-ordinal frontier and scans stable relationship-ordinal candidates.
`visits` counts every adjacency candidate examined, including a relationship revisited from its
other endpoint. Results are unique relationship ordinals; reachable entities are reported
separately and exclude the root. The default limits are global across the entire query: 1,000,000
visits and 100,000 unique relationship results. Exceeding either fails the query rather than
returning a truncated answer.

## Deliberate limitations

- The development verifier uses the durable memory fault model, deterministic development entropy
  and a test key wrapper, not the Linux adapter or portable recovery profile.
- It does not collect latency, RSS, real-filesystem I/O, result-byte or performance evidence.
- Multi-hop traversal is stable client-side composition of production one-hop authorized reads;
  there is no native multi-hop engine request yet.
- `qualification: qualifying-fixture-size` describes only exact fixture dimensions. It is not a
  performance or release claim.
- BM-01 still needs cold/warm authorized encrypted disk-query runs on the reference machine. BM-06
  and streaming larger-than-memory recovery are outside this fixture increment.
