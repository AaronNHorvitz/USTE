# Certificate-paired cold recovery evidence

Decision 0031 adds `coordinator-meta-v1`, a temporary authenticated index-recovery owner and an
exact-pair graph recovery API. The focused graph test publishes a graph root and metadata root at
revision one, commits a revision-two suffix, publishes an intentionally unpaired newer metadata
root, restarts, rejects the wrong-rank pair, reconstructs the older complete pair and then drops the
reader before seeded open. Seeded open independently verifies the revision-one prefix and returns
the exact revision-two graph and both durable retry outcomes.

The coordinator fixture includes both a retry outcome and a committed blob owner. It verifies exact
run/entry accounting, outcome/owner caller-limit rejection before map construction, wrong reducer
state rejection, an authenticated descriptor-consistent outcome with nonzero reserved bytes,
restart, exact prefix recovery and suffix replay. Existing checkpoint tests continue to cover
malformed payload fallback; transaction tests cover conflicts, concurrent publication, lost
responses, committed-blob recovery and process restart.

Reproducible focused commands:

```console
cargo test -p uste-graph --test disk_index cold_root_pair_reconstructs_seed_and_replays_graph_suffix --locked --offline
cargo test -p uste-replay --test coordinator_checkpoint --locked --offline
cargo test -p uste-txn --test transaction_coordinator --locked --offline
cargo clippy -p uste-storage -p uste-txn -p uste-replay -p uste-graph --all-targets --locked --offline -- -D warnings
```

This evidence does not claim bounded RSS, a disk-backed reducer, one-pass discovery, compaction,
BM-01, BM-06 or T-20 completion. The graph and coordinator maps remain fully memory-resident.
