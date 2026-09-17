# Decision 0022 — Spatial schemas and reference history

Date: 2026-09-17

Status: accepted and locally qualified for T-48. This implements the R1 schema/history subset of
FR-27/28 and Decision 0009. Transform evaluation remains T-50; observation history queries and
estimation remain T-51; scalable native indexes remain T-59.

## Component boundary

`uste-types::spatial` owns capability-free exact primitives: nonzero spatial versions, versioned
record references, signed local nanometres, nonnegative radii, longitude/latitude nanodegrees,
canonical poles, local/geographic points and closed boxes, and the closed coordinate profiles.
It remains std-only. The safe-Rust `uste-spatial` crate owns world, frame, geometry and position-
observation schemas, their strict canonical codec, an independent-reference-tested admission
catalog, and a replay/checkpoint reducer. Neither crate can read files, clocks, environment,
credentials or networks, and neither introduces a database, GIS or physics dependency.

The v1 wire profile is `uste-spatial-record-v1` inside the existing canonical generic `Value`
format; it adds no generic record tag. World, frame and geometry identities have immutable nonzero
versions. Geometry and observations name one exact frame version. Geographic longitude is
canonical `[-180°, 180°)` and pole longitude is zero. Missing height and missing position remain
unknown; no constructor or decoder invents an origin. Stored truth contains no floating-point
value, so NaN and infinity are unrepresentable.

## Reference and observation semantics

A world names an exact root-frame version. Root frames have no parent. Non-root frame versions
name an exact parent-frame version; the retained frame graph is acyclic and at most 32 edges deep.
Later frame versions cannot rewrite earlier geometry ancestry. Geometry versions form an exact,
gap-free predecessor chain and must match their frame dimension. All record references in this
slice must share the namespace scope.

`FrameParent.transform` is an opaque, same-scope, version-pinned binding in T-48. It is preserved
canonically but its target existence, validity interval and numeric transform meaning are not
claimed here. T-50 owns the transform record/schema, closure validation and evaluation. Calling
the field a binding does not authorize treating arbitrary referenced content as executable.

Observations are immutable and retain entity, world, exact frame, source/session/event identity,
evidence, complete source timestamp envelope, exact position, uncertainty and an optional prior
observation correction. Same canonical source key and payload is idempotent; differing payload is
a conflict. An observation ID cannot overwrite another observation. Corrections must reference an
earlier committed observation for the same entity and world, preventing forward or cyclic
correction chains. Observed data remains distinct from the `unknown`, `estimated`, `simulated` and
`conflicting` result categories reserved for later query/motion layers.

## Durable reducer and limits

Spatial transaction bytes are closed, canonical and scoped, with 10,000 records and 16 MiB as
hard admission limits. The original implementation prepared one unpublished R1 catalog clone.
Decision 0034 replaces that path with a request-sized borrowed overlay and preflighted publication
delta, then hashes the same request-ordered canonical stored effects. Observation effects
distinguish insert from idempotent retry and bind the original recorded revision. Publication
remains atomic.

Canonical checkpoints retain every immutable record and its recorded revision, reject reordered,
duplicate, future-revision, cross-scope, truncated and trailing input, and rebuild reference
invariants rather than trusting derived maps. Logical hashing streams the complete canonical state
and works at genesis without pretending genesis is checkpointable. Checkpoint bytes are capped at
256 MiB. The R1 in-memory catalog additionally caps canonical record data plus framing at 64 MiB
and entries at 1,000,000; the byte cap normally binds first. These are hostile-input correctness
bounds, not BM-10 capacity claims. The in-memory map catalog is intentionally not the T-59
larger-than-RAM index.

`SpatialState` is an internal qualification reducer implementing the generic transaction and
checkpoint traits; it intentionally does not implement `AuthorizedTransactionState`. T-48 can
validate spatial-local scope and history, but cannot prove that external graph entities, evidence,
sources or transform records exist. T-49 must compose graph/spatial/import mutations, declare
Commit and ReadRecord requirements, and enforce atomic external reference closure before exposing
this path through an authorized coordinator. The graph integration test proves only opaque
canonical-value preservation through graph replay; it is not spatial validation.

## Qualification

The production catalog is compared with an independent scan-based frame-history oracle. Tests
cover forward world/root admission, version visibility, category collisions, dimensional and scope
mismatch, cycles/depth, malformed roots, observation retry/conflict/overwrite/correction behavior,
atomic failure, replay, post-state effects, and checkpoint rebuild. Literal R0 longitude, pole,
cycle and depth vectors execute. R1 goldens bind all record kinds and six geometry variants, two
observation states, a transaction and a checkpoint. Every record and transaction truncation is
rejected; checkpoint truncation, count, duplicate, ordering and future-revision mutations fail.

This local review and test evidence is not independent security certification. T-62 remains an
unverified external executable-distribution prerequisite and is unrelated to T-48 implementation.
