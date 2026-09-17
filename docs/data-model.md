# Data model

Living contract · 2026-09-17 · T-17 entity/evidence/assertion/relationship subset implemented;
artifact, event, branch, purge and schema extensions remain later tasks

Owns FR-01, FR-04, FR-05, FR-06 and the shared record vocabulary; FR-26 is detailed in
[time normalization and ordering](time-and-ordering.md).

## Identity, values, and scope

All durable identities include database/namespace scope and an opaque stable identifier.
Paths, filenames, extracted labels, and content similarity do not define identity.
Identifiers are not capabilities. Physical layout never determines user-visible identity.
Branches add an explicit branch identity; references never cross namespaces implicitly.

Values use a closed, versioned type system: booleans, bounded strings/bytes, checked integers,
specified numeric values, timestamps, references, bounded lists/maps, and explicit nulls.
NaN/infinity and ambiguous numeric conversions are rejected unless a future schema defines
their exact behavior. A parser may retain the original token as text instead.
Canonical field ordering, Unicode handling, maximum nesting, and encoding are part of D-06.

## Records

| Record | Required content |
|---|---|
| Namespace | Identity, owner/policy reference, sensitivity defaults, quotas, retention policy |
| Entity | Identity, type/schema version, validated properties, lifecycle, creation revision |
| Assertion | Identity, subject, predicate, object/value, evidence IDs, status, valid interval, recorded revision, sensitivity, optional confidence and its meaning |
| Relationship | An assertion with entity endpoints, direction/type, validated properties; both endpoints must exist in the permitted scope |
| Evidence | Immutable source artifact/version or captured observation, digest, exact available locator, capture provenance, sensitivity and retention |
| Artifact | Logical file/object identity, immutable versions, names and supplied/detected media types, policy, ownership |
| ArtifactVersion | Byte length, integrity identity, blob reference, predecessor, source/capture metadata, processing states |
| Derivation | Source versions, adapter/model/config versions, output artifact IDs, coverage, limitations, locators, status and resource receipt |
| Decision | Authenticated actor/delegation identity, target revision/digest, disposition and bounded rationale |
| Event | Transaction ID, ordered revision, operation version, actor/policy receipt, affected identities, normalized accepted inputs |
| Branch | Base revision/epoch, assumptions, model profile, retention pins, status, owner and limits |
| Snapshot | Database identity, history epoch, endpoint, format/profile versions, canonical state digest, verified object inventory |
| Procedure | A versioned artifact with evidence and approval metadata; never automatic permission to execute |

World, frame, geometry, observation, trajectory and navigation records extend this vocabulary
under the same transaction/policy rules; see [spatial records](spatial-world-model.md).
Location is optional. An object is not its coordinates, and a file is not its parsed text.

These are logical fields, not a frozen disk encoding. Security-relevant fields cannot be
overridden through an arbitrary user-properties map. Required fields are validated at the
API and again during import/recovery.

Decision 0019 implements scoped entity/evidence/assertion/relationship records in `uste-graph`.
The current evidence record contains an immutable digest and exact supplied locator; richer
artifact/version/capture provenance remains T-22. The reducer retains current records and revision
histories. Rebuildable outgoing/incoming indexes expose accepted current relationships; provenance
retains every current claim that names an evidence record, including terminal claims needed for
traceability. These are not yet bounded-cache disk indexes.

## Assertions and decisions

Lifecycle: proposed → accepted or rejected; accepted → disputed, superseded, retracted,
or expired under authorized policy. Purge removes retained payloads rather than merely
changing status. Rejection must not retain forbidden content through a tombstone.

Acceptance records a decision, not proof of truth. Contradictory evidence can coexist.
Correction is a new assertion linked to the previous identity; it does not silently overwrite
history. Proposal promotion and procedure execution are distinct authorities.
Confidence values must name their basis; uncalibrated model scores are not probabilities.

A consuming application supplies approval policy through the adapter boundary. The engine
validates authorization and records the decision; it neither assumes all changes need human
approval nor allows a model to self-approve merely by declaring an actor field.

## Time

- Commit revision is a monotonically ordered logical sequence per database.
- Recorded order is the committed revision; a captured wall timestamp is informational only.
- Resolvable instants normalize to UTC with original timestamp/zone/precision provenance.
- Source event/document times, receipt, commit observation and derivation availability are distinct.
- Valid time is an explicitly supplied interval when a claim applies in the modeled world.
- Intervals are half-open; unbounded endpoints require explicit tags. Unknown is not unbounded.
- Wall-clock rollback does not reorder commits.
- Simulation time is a separate integer clock, never a substitute for commit revision.

An as-of query specifies both knowledge revision and valid time when required.
Late-arriving evidence creates new recorded history even when its valid interval is earlier.
Queries outside retained history return HistoryUnavailable, not a fabricated empty result.
Current authorization applies to historical and branch views.
Timestamp envelopes, ambiguous dates, leap-second limits, clock domains and point-in-time
visibility follow the shared time contract. File metadata is a source claim, not commit authority.

## Constraints and atomic changes

Transactions validate endpoints, allowed entity/relationship types, uniqueness, reference
closure, evidence existence, namespace policy, and revision preconditions. Node deletion
must follow an explicit reject/cascade/retract policy; dangling current relationships are
not allowed. All affected adjacency and provenance indexes publish atomically.

Identity merges require an explicit, reversible logical decision with evidence. Matching
names or equal file hashes do not justify merging people, claims, or artifact identities.
Deduplication may share physical bytes within an approved boundary while retaining separate
logical permissions and retention references.

## Provenance and derived state

Every extracted chunk and accepted extracted assertion can reach its exact source version
through a derivation/evidence chain. Source deletion or revocation updates eligibility
through that chain; an embedding or summary must not preserve access by losing provenance.
Entity existence does not imply access to every linked record.

Source text is not executable authority. An extracted statement such as “ignore previous
instructions” is a quoted source claim, not a command to the database or consumer.

See [content](content-ingestion-and-parsing.md) for file-specific records and
[security](security-and-privacy.md) for deletion and evidence-minimization rules.
