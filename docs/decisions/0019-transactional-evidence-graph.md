# Decision 0019 — Transactional evidence graph and durable policy

Date: 2026-09-17

Status: accepted and locally qualified for T-17. This implements the R1 graph-record and
authorization slice of Decisions 0003, 0016 and 0018. Disk-backed indexes, replay checkpoints,
artifact/version records, schema registries, retention purge and query composition remain owned by
later tasks.

## Record and transaction profile

`uste-graph` is a safe-Rust reducer over the encrypted `uste-txn` journal. Format
`uste-graph-request-v1` is a strict canonical `uste-types::Value` frame with an explicit namespace,
bounded operations and an optional policy mutation. Unknown, missing, duplicate, noncanonical and
trailing fields fail closed. A transaction admits at most 10,000 operations and 100,000 counted
record-reference occurrences before reducer state is cloned.

The implemented records are typed entities, immutable evidence descriptors, assertions and
directed relationships. Every record has a scoped stable identity and nonzero version. Assertions
and relationships retain evidence identities, explicit valid-time state, recorded/modified
revisions and the closed proposed/accepted/rejected/disputed/superseded/retracted/expired
lifecycle. Correction creates a new proposed record linked to the accepted predecessor and requires
an explicit absence/read-view precondition; it never overwrites history. Final transaction state
must have same-scope reference closure and existing evidence/endpoints.

Entity deletion is explicit. Reject mode refuses live dependents. Cascade-and-retract requires an
exact sorted declaration of every affected assertion/relationship and a caller limit; changed
topology cannot expand the mutation after authorization. Records remain in revision history rather
than being physical-purge receipts. T-35 owns retention deletion and purge.

## Derived indexes and views

Current records and per-record histories are authoritative reducer state. Outgoing adjacency,
incoming adjacency and evidence provenance are derived ordered indexes rebuilt atomically with a
published snapshot and independently checkable from records. Only accepted relationships
participate in current adjacency; provenance retains every current assertion/relationship that
names the evidence, including terminal claims needed for traceability. Self-loops, cycles and
parallel edges are retained with stable record-ID ordering.

Raw snapshot helpers are trusted reducer surfaces. Consumer reads go through
`AuthorizedCoordinator` and a reducer-owned projection: direct record, record-at-revision,
one-hop adjacency and evidence-supported-record queries. The caller result cap is applied only
after authorization so hidden candidates do not disclose cardinality. Candidate scanning has a
separate content-free 1,000,000-visit ceiling and stops before allocating beyond it. This is an
in-memory correctness index, not the T-20 disk-index or cache-pressure result.

## Durable policy and concealment

Graph state requires an engine-native namespace policy before an authorized coordinator can open.
Initial installation is a privileged raw-coordinator bootstrap operation. The consumer facade
rejects install, missing durable policy and any adapter policy that is not byte-equivalent to the
recovered durable policy. Later replacements are ordinary journaled graph transactions requiring
`ManagePolicy`, exact current version and a strictly increasing next version. After commit or an
idempotent retry, the live kernel synchronizes from current reducer state rather than replaying a
stale request mutation.

Static authorization covers every target, precondition, correction identity, reference and exact
cascade mutation before reducer state access. Projections reauthorize candidates and every embedded
record identity in endpoints, evidence, correction links and nested property values before
returning a containing record. Hidden candidates are omitted without exposing hidden totals.
Policy replacement stales existing leases; an uncertain commit invalidates existing views before
snapshot access.

## Replay integrity

Prepared result receipts hash the revision, sorted affected records using a complete canonical
encoding of every stored field, and the complete optional namespace policy including grants,
quotas and record rules. Recovery therefore rejects same-target semantic drift such as an
accept/reject change or a policy-permission change. Policy history and record history reject future
read revisions instead of fabricating a current answer.

The production reducer is checked after every generated revision against the independent
`uste-testkit` ordered-map reference model. That oracle remains intentionally scan-based and does
not share the production codec, reducer or derived-index implementation.

## Bounded limitations

The snapshot currently retains records, histories and derived indexes in memory. T-18 owns
deterministic replay/checkpoint qualification, and T-20 owns immutable disk runs and bounded cache
pressure. Evidence records in this increment identify a supplied digest/locator; T-22 adds full
artifact/version/derivation lineage. Physical purge, restore epochs and stale-backup denial remain
lifecycle tasks. No path, arbitrary-depth traversal, spatial query, parser, network service or
database executable is claimed by this decision.
