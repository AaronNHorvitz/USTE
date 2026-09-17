# Decision 0009 — Spatial, movement and navigation profile

Date: 2026-09-16

Status: accepted for profile `space-v1`.

Closes D-08 and is the decision artifact for T-46.

## Geometry and units

Authoritative scalar storage uses checked signed fixed-point SI values where a schema can do
so: local distance is integer nanometres and duration integer nanoseconds. Geographic input
stores signed longitude/latitude nanodegrees. Longitude canonicalizes to `[-180°, 180°)`;
latitude is `[-90°, 90°]`. `+180°` becomes `-180°`. Geographic height, when present, is
ellipsoidal WGS 84 height in nanometres; absent height remains unknown. Calculations may use
binary64 only inside the named algorithm/profile, reject nonfinite/intermediate overflow, and
quantize results back to the declared fixed-point unit using ties-to-even.

The geographic CRS is WGS 84 longitude/latitude (`EPSG:4326` axis meaning is made explicit as
lon,lat in this API). Great-circle point distance/radius v1 uses the WGS 84 authalic sphere
radius 6,371,007.1809 m and the stable haversine/atan2 formulation. It is not an ellipsoidal
survey distance: declared model error versus WGS 84 geodesic is up to 0.56%, plus 2 mm numeric
tolerance on the admitted range. Near-antipodal nearest ordering within that uncertainty is
`AmbiguousState` unless a caller accepts the profile. Poles have canonical longitude 0 for
point identity comparisons; boxes touching a pole cover all longitudes at that latitude.

Local frames are right-handed Cartesian 2D/3D metres with named axes. A static transform is a
versioned rigid rotation plus translation; scale/shear is unsupported. Chains are acyclic and
at most 32 deep. Time-dependent transforms are piecewise observations queried under the same
knowledge revision; no extrapolation. WGS84-to-local v1 is an explicitly anchored East-North-Up
frame using the WGS 84 ellipsoid (`a=6378137 m`, `f=1/298.257223563`) through geodetic→ECEF→ENU.
The inverse uses a bounded 10-iteration latitude solution and must converge within 1 nm local
or fail `TransformUnavailable`. Transform provenance lists every version in order.

Local boxes and geographic latitude bounds are closed. A wrapped geographic box has west >
east and represents `[west,180) union [-180,east]`; equal west/east means a zero-width meridian,
not the whole world. Radius is inclusive (`distance <= radius + 2 mm`). Nearest ties use exact
quantized distance then entity ID. Degenerate points/zero-area boxes are supported; polygons
at R3 must be simple, closed, non-self-intersecting, at most 100,000 vertices, with boundary
included and even-odd interior on an unwrapped local tangent plane. A polygon spanning more
than 180° longitude or containing a pole is unsupported in v1.

## Movement state

Observations are immutable and keyed idempotently by `(source, session, event-id)`. Same key
and canonical payload is a duplicate; different payload conflicts. Equal-time observations
from different evidence remain `conflicting` unless policy explicitly selects one. Linear
interpolation is the sole v1 estimate, requires bracketing observations in one transformable
frame, gap <= 24 hours, no conflict and no endpoint recorded after the selected knowledge
revision. Extrapolation is never implicit. Uncertainty linearly envelopes endpoint radial
uncertainties and reports the method; it is not silently averaged into a fact.

## Index and navigation

The reference implementation is an authorized bounded scan. R3 disk indexes are immutable
16 KiB-page packed R-trees built with deterministic sort-tile-recursive packing over quantized
bounding boxes and `(namespace, frame, time partition, entity ID)` tie order. Geographic boxes
crossing the antimeridian split into two entries. Candidate search may over-select; the exact
profile predicate decides results. Runs are encrypted and revision-covered, rebuildable from
observations, and never an authorization authority. Time partitions are UTC calendar months
for resolved observations plus an unindexed unresolved bucket.

Navigation v1 uses directed edges and nonnegative `u64` nanounit costs under a named metric.
The supplied graph must use one coherent valid-time/knowledge slice. Dijkstra's algorithm is
the reference; the priority order is `(total cost, hop count, lexicographic full entity/edge
ID path)` and checked addition rejects overflow. Bounds are Decision 0007's visit/depth/result/
scratch caps. `Unreachable` is a complete result; exceeding visits/memory/time is
`ResourceLimit`. Authorization happens before adjacency expansion, so hidden vertices/edges
cannot contribute to cost, ranking or counts.

## Feasibility, alternatives and acceptance

All algorithms are first-party safe Rust with `std`; no GIS/spatial engine dependency is
required. A general projection catalog and S2/GEOS/PROJ bindings were rejected for the strict
baseline. Floating coordinates as stored truth were rejected because NaN/canonical equality
would infect indexes. The authalic-sphere distance is intentionally simpler than survey GIS
and exposes its error envelope.

`acceptance/r0/spatial.tsv` contains antimeridian, pole, closed-boundary, transform, distance,
tie and route vectors. `tests/r0_vectors.rs` executes a dependency-free subset. VT-18/19/22
must compare optimized results to the bounded authorized scan.
