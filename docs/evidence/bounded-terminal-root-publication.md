# Bounded terminal-root publication evidence

Decision 0048 removes the complete live graph snapshot from proof-derived postcommit root
publication while retaining the existing journal, reducer, result-digest and `graph-state-v1`
formats.

## Verified behavior

- The authenticated storage merge provisionally visits exactly the ordered output later recovered
  from its encrypted run. The run remains invisible until a separate root publication succeeds.
- The proof-derived plan carries exact target counts for metadata, current records, history,
  outgoing/incoming adjacency, provenance, reverse references and policy current/history entries.
- Publication streams the actual merged entries, validates primary key/content bindings plus
  secondary framing/count constraints, and reproduces the existing canonical graph logical-state
  digest without calling
  `GraphState::current_snapshot` or regenerating full-state family iterators.
- One record's history framing is the only retained content. Its caller-selected logical-byte cap
  is explicit and a one-byte cap fails before any target root becomes visible.
- Wrong outcomes and undersized family budgets fail after no root publication. Restart retains the
  authoritative journal commit and no partial root; a retry with adequate limits succeeds.
- The successfully published proof-only root and a separately built full-state root both admit and
  reconstruct to the exact same graph after insertions, replacements, deletions, policy change and
  empty-family output.

~~~text
cargo test -p uste-storage --lib --locked \
  journal::tests::authenticated_index_merge_streams_exact_deltas_and_publishes_only_terminal_output -- --exact
# 1 passed
cargo test -p uste-graph --lib --locked \
  merged_output_stream
# 2 passed
cargo test -p uste-graph --test disk_index --locked
# 4 passed
cargo clippy -p uste-storage -p uste-txn -p uste-graph --all-targets --locked -- -D warnings
# passed
bash scripts/check.sh
# 263 workspace tests and 31 isolated t20-bench tests passed; 2 exact-profile release tests ignored
~~~

## Deliberate boundary

This establishes a bounded base-plus-delta publication proof but does not install a disk-backed
live reducer. `GraphState`, coordinator metadata and recovery remain complete in-memory maps;
`graph-state-v1` rewrites complete terminal family runs; root admission still compares or
reconstructs full state; BM-01 and BM-06 remain unqualified. A later streaming `GraphDiskBase`
admission and a new bounded live base/overlay lifecycle must address those separate boundaries.
