# Bounded graph-state reconstruction evidence

Decision 0030 adds a privileged complete-run stream and uses it to reconstruct `graph-state-v1`
without collecting any run into a result vector. The storage regression covers a value spanning
multiple encrypted pages, empty values, exact ordered output and accounting, descriptor page/entry
preflight, aggregate-byte failure after provisional output, visitor failure, warm-cache independence,
one-open stable-handle accounting, late-page corruption after provisional output, first-page
corruption and appended-file rejection.

The graph disk integration fixture now exercises all eight state families, including current and
historical policy. It publishes and discovers a distinct root candidate, reconstructs state equal to
the journal reducer, reports exact run/entry counts, rejects caller record/page budgets before record
allocation, rejects a separately authenticated wrong-metadata candidate, repeats reconstruction
and a cross-scope coordinator, rejects authenticated descriptor-consistent current/history and
derived-adjacency mismatches, repeats reconstruction after process-style filesystem restart, and
reconstructs the historical candidate after a later commit without treating it as current.

Reproducible focused commands:

```console
cargo test -p uste-storage journal::tests::encrypted_index_runs_round_trip_large_values_with_bounded_cache_and_root_fallback --locked --offline
cargo test -p uste-graph --test disk_index --locked --offline
cargo clippy -p uste-graph -p uste-storage -p uste-txn --all-targets --locked --offline -- -D warnings
```

This evidence does not claim a disk-backed reducer, one-pass candidate discovery, coordinator-seed
recovery, BM-01 or BM-06. `GraphStateLoadLimits` cover reconstruction's second pass only; prior
candidate discovery scrubs under absolute `index-v1` format maxima rather than those caller limits.
Complete records, histories and rebuilt indexes remain resident in the returned `GraphState`.
