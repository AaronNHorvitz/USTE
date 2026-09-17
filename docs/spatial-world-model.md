# Spatial world model, movement and navigation

Accepted contract · updated 2026-09-17 · T-48 schema/history subset implemented

Owns FR-27 through FR-30. Decision 0009 freezes v1 numeric representations, reference frames,
exact geometry predicates, error tolerances, index layout and limits. All operations inherit
namespace authorization, current permissions, retention and bounded-query rules.

Decision 0022 implements the R1 exact-unit schemas, canonical records, immutable reference
histories, observation correction rules and qualification reducer. Transform bindings are opaque
until T-50; Decision 0023/T-49 now composes authorized external graph/source closure in the
capability-free import reducer. Movement queries,
predicates, navigation and scalable indexes remain at their assigned later tasks.

## Records and identity

| Record | Required meaning |
|---|---|
| World | Scoped identity, root frame, unit/geometry profile, references to contained entities; not an authorization bypass |
| SpatialFrame | Stable identity, parent if any, coordinate reference definition, dimensions, units, axis order, handedness, transform version and validity |
| GeometryVersion | Immutable point/region/shape data, frame, bounds, precision and supported interpretation |
| PositionObservation | Entity, source/evidence, observed UTC instant or unresolved source time, recorded revision, frame, position, uncertainty and optional measured orientation/velocity |
| TrajectorySegment | Entity, source observation references, interval, interpolation/model profile, gap limits, status and derivation availability |
| NavigationEdge | Directed endpoints, explicit traversability, nonnegative cost and units, constraints, valid interval and provenance |
| SpatialResult | Entity/version, query revision/time, frame, observed/estimated/simulated/unknown status, uncertainty, evidence and accessible content references |

World membership and physical containment are relationships, not primary keys or proof of
ownership. A document, preference or abstract entity need not have coordinates. Multiple
sources can disagree about an object's location; preserve conflict rather than averaging
silently. A known observation time is required to enter the timed trajectory index; unresolved
observations may be retained and found by identity/status pending an explicit correction.

## Coordinates and frame transformations

R2 baseline: named local Cartesian 2D/3D frames in meters; WGS84 geographic points and
latitude/longitude boxes; bounded-radius and nearest-neighbor queries over supported points.
Geographic axes are explicitly named longitude/latitude in degrees; height, when present,
declares its vertical reference and units. Degrees are not Euclidean meters.
Decision 0009 freezes the actual reference definition, distance algorithm and accuracy envelope.
Missing height is unknown, not zero. Local and geographic frames are never implicitly mixed.

Frame graphs are acyclic. Transforms have versions and time applicability; moving parent
frames require transforms evaluated at the requested time and knowledge revision. Reject
unknown, cyclic, expired or unsupported transformations. No network geocoding, coordinate
grid download or timezone lookup is implicit. Named transforms requiring absent local data
fail explicitly. Transform provenance travels with the result.

R3 adds supported 2D polygons/regions, trajectory-region intersection and native disk spatial
indexes. Decision 0009 defines boundary inclusion, longitude wrap/antimeridian handling, poles,
degenerate geometry and numeric tolerance. Never claim arbitrary GIS projection or mesh support.
Index candidate filtering may over-select; exact predicates under the declared tolerance
determine final matches. Completeness and error bounds must be tested, not assumed.

## State at a time

A lookup binds entity, knowledge revision, world time, requested frame and branch if any.
Return one of observed, estimated, simulated, conflicting or unknown with supporting evidence.
The same time with a later knowledge revision may legitimately yield a corrected answer.
Entity properties, relationships, geometry and content references share the same read view.

The default is no silent extrapolation. An explicitly requested last-known observation names
its age; it is not a current position. Interpolation requires an admitted versioned method,
bounded gap and available endpoint observations. Endpoints learned after the selected
knowledge revision cannot enter the estimate. Predictions name model, branch and horizon.
No zero coordinates, generated motion or current clock substitute for missing state.

Late/out-of-order observations produce corrections and index invalidation without changing
recorded order. Duplicate source-event IDs are idempotent within their source scope; differing
payloads under the same key conflict. Inferred velocity is a derivation, not a measured fact.

## Query and navigation baseline

- R2: entity state, bounded time-range observations, local box/geographic box, point radius,
  bounded nearest neighbors, explicit relationship traversal and supplied navigation graphs.
- R3: region membership across an interval, trajectory crossing under an explicit segment
  model, correction-aware spatial history and stable paginated larger-than-RAM retrieval.
- Every response reports truncation, unsupported geometry and uncertainty. Crossing between
  sampled endpoints is an estimate unless directly observed; distinguish possible from certain
  membership where uncertainty prevents a definite conclusion.

Graph navigation computes a bounded path over explicitly traversable edges under a named
cost metric. Decision 0009 selects a reference shortest-path method with nonnegative costs, stable
tie-breaking, time semantics, visit/memory limits and unreachable/budget-exhausted outcomes.
Baseline paths use one coherent time slice, not a promise of optimal time-dependent routing.
Graph connectivity alone does not imply walkability or a safe route. Free-space pathfinding,
navmesh generation and robotics actuation are later extensions; no live map service is required.

## Security and lifecycle

Location histories may reveal homes, workplaces and routines. Their coordinates, bounds,
search counts, nearest-neighbor ranks, route alternatives and caches inherit sensitivity.
Apply authorization before expansion/ranking; hidden objects must not influence visible
nearest/path results through an unqualified global index. Resource limits remain enforceable
without returning sensitive internal visit counts. Do not claim timing-side-channel immunity.
Revocation and purge invalidate geometry indexes, trajectories, route caches, transform-derived
state, simulated outputs and downstream content references where dependent on the source.

VT-18/19 cover geometry, frames, ordering and reference-query equivalence; VT-22 covers
mixed retrieval; VT-23 covers spatial privacy and deletion. BM-10/11/13 measure declared
spatial/history/mixed workloads with encryption and durability enabled.
