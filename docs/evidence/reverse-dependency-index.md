# Incremental reverse-dependency evidence

Decision 0028 adds one derived target/owner entry for every current graph reference owner. A unit
fixture repeats a target in nested relationship properties while also using it as an endpoint and
proves the pair is deduplicated, its role bits are ORed, and an accept transition updates status,
version and modified revision. Full derived-index rebuild checks now include the reverse map.

Transaction-overlay regressions prove:

- removing a base entity-property reference before delete allows the delete;
- adding a reference before delete produces the exact blocker without publishing either change;
- a proposed-to-accepted relationship transition is seen by the later cascade check;
- a missing declaration fails before duplicate-mutation detection, while an exact declaration
  reaches the existing duplicate-mutation rule; and
- retracting an accepted relationship earlier in the transaction removes the delete dependency,
  while adjacency disappears and provenance remains.

Existing checkpoint/replay tests reconstruct `GraphSnapshot` from canonical bytes and compare the
new derived map through snapshot equality and explicit rebuild validation. The map is not serialized
or included in the logical digest.

Reproducible focused commands:

```console
cargo test -p uste-graph --all-targets --locked --offline
cargo clippy -p uste-graph --all-targets --locked --offline -- -D warnings
```

This is an in-memory correctness/indexing increment. It does not establish durable
`graph-state-v1`, bounded checkpoint decode, an aggregate fanout cap or benchmark performance.
