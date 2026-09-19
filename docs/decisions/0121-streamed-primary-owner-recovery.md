# Decision 0121 — Stream primary blob-owner recovery metadata

Date: 2026-09-19

Status: accepted implementation contract; T-20 and qualification remain open.

Extend Decision 0120's paired-base private staging with an explicit inventory-bearing API:
`DiskCommitCoordinator::recover_with_primary_metadata_streaming_domain`. The existing
inventory-free API retains its closed refusal behavior and native BM-06 selection. Neither
API accumulates retry, transaction-ID or owner maps across suffix revisions.

`PrimaryMetadataRecoveryLimits` bounds total suffix revisions, total committed owners, each
inventory's reference count and each per-family merge. Storage still authenticates/decodes an
inventory under its independent hard limits before the coordinator's narrower reference admission.
The current inventory alone supplies bounded new-owner deltas. Each reference is compared with
the current private primary owner index: an exact repeat preserves the earliest principal, a
different binding fails closed, and an absent reference adds the current principal. Metadata
counts and exact merge insertions/output counts include the new owner family. A later transaction
without inventory retains the complete prior owner family. Private handles advance only after
the whole step succeeds; no intermediate root is published to discoverable slots.

The new API requires an independently admitted domain/primary metadata pair at the same revision.
It refuses attached optional first-reference and quota projections, rather than dropping or
mislabeling them. General recovery still supports those projections through its existing bounded
overlay path. Total owner admission precedes filesystem I/O when the initial base already exceeds
the limit; reference/new-owner admission precedes domain advancement and scratch writes. Ordinary
authorization, expiry, retry/collision, canonical preparation and terminal publication checks are
unchanged. Private terminal metadata requires durable rebase before fresh writes.

The report aliases Decision 0120's partial merge diagnostics. The four-revision reference fixture
starts with one owner, adds a second at revision two, repeats both under a third principal, then
commits without inventory. Three suffix steps produce 12 runs, 27 output entries and 3,513 logical
key/value bytes. Both original principals survive, all retry/transaction outcomes remain exact,
and cold independently admitted terminal roots match with zero recovery overlays. A second case
starts with no owners and creates the owner family during recovery. Storage recovery uses disk
certificate/blob metadata, not resident historical maps.

Six focused tests cover these reference/restart cases, exact owner/reference bounds, attached
first-reference refusal before I/O, late certificate corruption and exhaustive observed I/O
fault boundaries. The matrix schedules 552 cases: 548 injected failures and four optional absent
operations without a successful crash-after boundary. Failed recovery exposes no coordinator,
retains only the old discoverable metadata pair and restarts successfully.

This is primary metadata suffix recovery, not complete inventory-bearing origin reconstruction.
Generic genesis bootstrap, maintenance of optional owner projections on this path, legacy
per-owner initial admission costs, immutable-family rewrite amplification and complete I/O
accounting remain distinct work. No larger-than-memory or BM-01/BM-06 qualification is claimed.
