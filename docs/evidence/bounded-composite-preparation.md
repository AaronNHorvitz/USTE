# Bounded composite preparation evidence

Decision 0034 replaces retained-state candidate clones in ordinary spatial and composite ingest
preparation with opaque request-sized deltas. It preserves canonical request, result, checkpoint,
reducer-profile and journal bytes.

## Implemented behavior

- Graph closure checks use a borrowed current-record overlay and retain only changed before/after
  records plus constant-size base metadata.
- Spatial preparation retains staged request indexes, one effect revision per requested record,
  observation outcomes and only records that insert. Catalog entry/logical-byte counters and a
  reconstructible private content fingerprint make base preflight constant-time.
- Composite ingest retains a graph delta, optional spatial delta and one job mutation. A batch
  mutation holds one receipt plus at most 10,000 source-event/digest pairs; unrelated retained jobs
  and source events are not copied.
- Graph, spatial, job, sequence and source-event bases are all checked before the first live
  component changes.

## Focused verification

~~~text
cargo test -p uste-graph -p uste-spatial -p uste-ingest --all-targets
# graph, spatial and ingest suites passed
cargo clippy -p uste-graph -p uste-spatial -p uste-ingest --all-targets -- -D warnings
# passed
bash scripts/check.sh
# workspace format/clippy/test/rustdoc/docs pass; 261 workspace tests and all isolated suites pass
~~~

The spatial fixtures prove forward world/root and frame references, request-ordered same-record
versions, exact retry byte accounting, old observation retry plus correction, rejection of a
correction to a new same-batch observation, and a retry-only outer revision across checkpoint
decode. A 102-entry base prepares one new geometry as exactly one retained publication record.
Pinned spatial and composite ingest result/chain digests remain unchanged.

Stale same-base plans and a same-revision plan from a different spatial catalog panic during
preflight while the complete logical state remains unchanged. The ingest fixture prepares the same
spatial-bearing batch twice, publishes one, rejects the stale plan before component mutation and
then proves checkpoint/logical equality. Its trusted preview matches the direct prepared outcome
and digest.

## Deliberate boundary

This is bounded preparation memory, not a larger-than-memory reducer or benchmark result. Current
graph/spatial/job maps, graph-only full-catalog/job closure scans, snapshots, checkpoint decoding
and recovered state remain materialized; graph result hashing can still materialize canonical
current-policy bytes. The private spatial fingerprint is process-local defensive
binding, not storage authentication or commit authority. T-20 still requires disk-backed
base/overlay preparation, streaming semantic validation, persistent overlay lifecycle and
qualifying BM-01/BM-06 results; T-59 owns native scalable spatial query indexes and BM-10.
