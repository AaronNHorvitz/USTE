# T-48 spatial schema and reference-history evidence

Date: 2026-09-17 · implementation commit: `5f24e76` · local correctness evidence,
not independent security certification

## Implemented result

- Added exact fixed-point spatial primitives and closed coordinate profiles to std-only
  `uste-types`, with canonical longitude/pole behavior, checked box bounds and unknown height.
- Added safe-Rust `uste-spatial` world, immutable frame/geometry-version and source-backed position-
  observation schemas. Strict canonical decoding rejects aliases, unknown fields/profiles,
  inconsistent timestamps, invalid scope, versions and bounds.
- Added an atomic bounded catalog retaining exact recorded-revision histories. It rejects category
  collisions, version gaps, invalid roots, missing parents, frame cycles/depth overflow,
  dimensional mismatch, source-event conflicts, observation-ID overwrites, and invalid forward or
  cross-entity corrections.
- Added canonical spatial transaction and checkpoint codecs plus `TransactionState` and
  `CheckpointState` replay. Result digests bind request-ordered stored effects and insert/retry
  outcomes; checkpoint restoration rebuilds every invariant and full-state logical digest.
- Added an independent scan-based frame-history oracle in `uste-testkit` and an opaque graph-value
  replay fixture. The latter proves byte preservation only; it does not substitute for the spatial
  reducer.

## Pinned profiles and limits

| Item | Value |
|---|---|
| Primitive/numeric profile | `space-v1` |
| Record profile | `uste-spatial-record-v1` over canonical generic `Value` |
| Local units | signed integer nanometres; right-handed XY/XYZ |
| Geographic units | WGS 84 longitude/latitude nanodegrees; optional ellipsoid height |
| Frame ancestry | exact version references; maximum 32 edges |
| Transaction | 10,000 records; 16 MiB canonical bytes |
| R1 catalog | 1,000,000 entries and 64 MiB canonical logical bytes |
| Checkpoint | 256 MiB canonical bytes |
| Golden matrix | 11 record variants plus transaction and checkpoint |

The 64 MiB logical-byte cap bounds the clone-based R1 correctness catalog; it is not a measured
RSS bound or the one-million-item BM-10 claim. T-59 owns native disk indexes and larger-than-RAM
behavior.

## Focused verification

~~~text
cargo test -p uste-types --test spatial_primitives --locked
# 4 passed; 0 failed
cargo test -p uste-spatial --all-targets --locked
# 26 passed; 0 failed
cargo test -p uste-graph --test spatial_replay --locked
# 1 passed; 0 failed
cargo clippy -p uste-spatial --all-targets --locked -- -D warnings
# passed
~~~

The golden matrix records exact byte lengths and SHA-256 digests for world, root/child frame, all
six point/box geometry variants, resolved and unresolved/corrected observations, a transaction and
a checkpoint. All record and transaction cuts plus trailing data fail. Checkpoint cuts, trailing
data, zero/overstated counts, duplicate/noncanonical records and future recorded revisions fail.
The catalog agrees with the independent parent-scan oracle at every admitted depth and rejects the
first over-depth edge atomically.

## Deliberate boundaries

T-48 preserves the transform record/version field as an opaque same-scope binding. T-50 owns its
schema, reference closure, validity and evaluation. `SpatialState` is not an authorized public
coordinator: T-49 must compose graph/spatial/import transactions, authorize every touched and
referenced graph record, and prove entity/evidence/source closure atomically. T-51 owns temporal
observation queries, correction-aware motion history and estimation. T-52/T-59 own query operators
and scalable indexes. No live map/feed account or provider credential is used.
