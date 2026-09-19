# Decision 0081 — Native cold-admission measurements and fixture cardinality

Date: 2026-09-18

Status: T-20 partial implementation; qualifying admission, complete I/O and campaigns remain open.

Retain the graph semantic-admission report returned by the engine, instead of discarding it.
Return measurements only with successful terminal coordinator recovery. Native setup reports
include the admitted graph and paired coordinator-metadata revisions, admitted state counts,
scan runs/entries/logical bytes/pages, exact/predecessor lookups, semantic references, lookup
page visits including cache hits, lookup result bytes and peak history bucket bytes.
No new storage reads or complete graph maps are introduced to collect these existing counters.

Scope these privileged synthetic-fixture measurements to cold graph semantic admission only.
They exclude coordinator correspondence passes, pending suffix preparation, root repair,
materialization and queries, and explicitly disclaim complete authenticated-I/O accounting.
Report the initial base counts separately from final counts so bootstrap or pending-root
measurements cannot be mislabeled as full-fixture cold admission. Native commands remain capped.

Validate final fixture cardinalities before returning a native session or running disk-oracle
queries. For E entities and R relationships, state counts are
`[E+R+1, E+2R+1, R, R, R, 3R, 1, 1]` in current/history/outgoing/incoming/provenance/reverse/
policy-history/current-policy order. The extra record is the Evidence binding; relationships have
creation and acceptance versions and three distinct dependencies. The fixed generator excludes
self-loops. This is a fixture invariant, not a general graph limit or replacement for exact oracles.

The native prefix matrix checks initial versus final revisions and all measured field types;
completed 20/200 cold admission has eight runs and 1,845 entries. The exact-size cardinality
formula is pinned independently without running a campaign. A valid authenticated database with
the expected profile binding and final revision but only one relationship must fail final
cardinality validation. M1, persistent formats, benchmark thresholds and release gates are unchanged.
