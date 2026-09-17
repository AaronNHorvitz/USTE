# Decision 0028 — Incremental graph reverse dependencies

Date: 2026-09-17

Status: accepted as T-20 groundwork. T-20 remains open; no durable state profile or benchmark
result is claimed.

## Context

Decision 0027 made ordinary graph writes proportional to their changed records, but entity deletion
still scanned every current record to discover property, assertion and relationship dependencies.
That scan also prevented a future disk state root from validating deletion with a bounded target
prefix read. Existing adjacency and provenance indexes do not cover nested values, proposed claims,
entity properties, evidence links or correction links.

## Decision

`GraphSnapshot` now maintains a derived target-to-owner reverse map. Each target/owner pair has one
descriptor containing an explicit owner kind, lifecycle or assertion status, owner version,
modified revision and ORed reference-role bits. Roles distinguish entity properties; assertion
subject, object, evidence and correction; and relationship endpoints, properties, evidence and
correction. Nested lists/maps are traversed and repeated occurrences do not create duplicate pairs.

The reverse map is not canonical authority and is omitted from checkpoint and logical-state bytes,
like adjacency and provenance. Checkpoint decode rebuilds it from validated records; independent
derived-index validation recomputes and compares it. Delta publication removes every contribution
from the prior record and adds every contribution from the resulting record, including lifecycle,
status, version and revision changes.

Deletion reads only the target bucket plus the bounded transaction overlay. The overlay reconciles
base owners with changed records and adds changed-only owners, so operations earlier in the same
transaction can add, remove or change a dependency without consulting stale base metadata. Delete
keeps the existing error priority, exact cascade declaration, relationship-before-assertion mutation
order and self-referencing target-entity exception.

## Consequences and next work

Delete discovery is O(target fanout + transaction changes), not O(total graph records), and the
target/owner ordering can feed a future durable reverse-reference family without external sorting.
The accepted graph-state profile will pin durable role/status codes separately; these internal
constants do not freeze storage bytes.

This index temporarily increases in-memory snapshot size and is cloned by explicit snapshot and
composite ingest paths. There is no accepted aggregate or per-target reverse-reference cardinality
cap. A valid target can therefore exceed current consumer scan limits. Those constraints, durable
family encodings, scratch construction and root-based recovery remain required for T-20 and BM-06.
