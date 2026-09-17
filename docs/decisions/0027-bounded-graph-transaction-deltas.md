# Decision 0027 — Bounded graph transaction deltas

Date: 2026-09-17

Status: accepted as T-20 groundwork. T-20 remains open; no disk-backed state or benchmark result is
claimed.

## Context

The graph reducer previously prepared every transaction by cloning the complete `GraphSnapshot`,
validating every record and rebuilding every derived index. This preserved atomicity but made the
live write path proportional to total retained graph state even for a one-record transaction. The
frozen `graph-current-v1` projection cannot safely be reinterpreted as mutable state, and adding a
second full-state projection would preserve the same scaling defect.

## Decision

Graph preparation now uses a transaction-local overlay over the last journal-certified record map.
Only records actually changed by the request are cloned. The prepared value contains strictly
ordered, unique before/after record changes, the optional policy change, revision and the existing
canonical result digest. Preconditions still read the pre-transaction state; operations apply
sequentially through the overlay; final validation covers each changed record against the complete
merged view.

Prepared deltas are non-cloneable and bind their scope plus exact base revision. Publication checks
that binding before its first mutation, so a stale or duplicate same-base delta fails closed rather
than merging two results under one revision. The transaction coordinator is the supported publisher.

Publication removes each changed record's old derived-index contributions, adds its new
contributions, appends its history version, applies policy history and advances the revision. It is
infallible and does not rebuild the full indexes. Accepted relationships contribute adjacency;
assertion and relationship evidence contributes provenance in every lifecycle state. Canonical
request, result, checkpoint and `graph-current-v1` bytes and the reducer profile do not change.

At this decision boundary, entity deletion still scanned the merged graph because
outgoing/incoming/provenance did not cover all
dependencies. The scan counts accepted and proposed claims plus active-entity property references
without collecting a graph-sized dependent list. A cascade is accepted only when its bounded,
strictly sorted declared set exactly equals the accepted dependencies, after which only those
records enter the overlay.

## Consequences and next work

Successful non-delete preparation and publication are proportional to transaction changes rather
than retained record/index count. Explicit snapshot/checkpoint capture still clones or materializes
the full state, checkpoint decoding remains full-memory, and ingest retains its clone-based
candidate. Decision 0028 subsequently replaces deletion's total-record scan with an incrementally
maintained reverse-dependency target bucket plus transaction-overlay reconciliation.

Publication performs ordinary Rust collection insertions after journal durability. Their logical
inputs are already prepared and cannot return a domain error; memory exhaustion follows the
process-failure/reopen path and must not be caught as a recoverable reducer error. A future immutable
disk-root install will narrow that post-certificate allocation boundary further.

The later `graph-state-v1` profile must include a general reverse-reference family covering entity
properties, assertion subjects/objects and relationship endpoints/properties, including lifecycle
status. That family is required for bounded disk-backed delete validation; adjacency and provenance
alone are insufficient. Encrypted scratch runs, tombstones, bounded merge overlays, root-based
recovery and BM-01/BM-06 evidence remain required before T-20 can close.
