# Decision 0094 — Bounded on-disk certificate anchor proofs

Date: 2026-09-19

Status: verified partial T-20 implementation; resident-map replacement remains open.

Add an explicit disk proof of an exact historical certificate against the authenticated journal
frontier. Read the existing fixed-offset certificate records from the selected revision through
the frontier, authenticate each existing format/context, verify contiguous revisions and hash
links, and compare the terminal digest to the pinned frontier. Both the caller's certificate-count
and encoded-byte ceilings are admitted before I/O. Retain one encoded/decoded certificate at a
time, not a history map. No historical resident anchor is consulted or populated. Successful
reports count the complete certificate-only work; group/header/inventory/blob reads are excluded.
This is O(frontier minus target) work, not a constant-time lookup or benchmark performance claim.

The opaque proof binds database, epoch, writer, certificate log, target and frontier. A private
process-local shared identity binds it to the exact open JournalStore instance without new
cryptography, persistent bytes or entropy consumption. Keeping the identity alive keeps neither
the key nor the filesystem lock alive. Proofs cannot authorize another instance, including a
reopen of identical bytes. Content authentication is not consumer authorization or validation of
transaction uniqueness, ownership or domain semantics.

Bounded exact index lookup and resumable index cursors can use this proof instead of the resident
certificate map. Their normal page/key/run authentication, budgets and terminal cursor checks are
unchanged. Historical reads may continue after this same exclusive owner's successful append:
its committed chain only extends. Poison/uncertainty always refuses. A future destructive history
maintenance path must invalidate this owner identity or separately prove retained ancestry; the
current implementation has no such path. Certificates changed on disk after proof acquisition
are not continuously re-read by these handles, just as existing admitted in-memory anchors are
not; fresh recovery/proof acquisition remains fail-closed.

Scratch recovery stages instead require the exact unchanged proof frontier. Both legacy and
disk-proven stages now retain exact owner/context/frontier binding, so later stage operations need
not repeat a resident-map lookup for their already-admitted target. Existing base-root validation
and ordinary root APIs still use the resident map. Intermediate roots stay unpublished.

Synthetic tests clear the entire resident anchor map and prove historical/terminal anchors,
scratch construction, bounded lookup and complete cursor reads. Tests cover exact/minus-one
budgets, future targets, poison/context/reopen/foreign-owner rejection, append continuation,
authentic fork substitution, corruption in each selected certificate, truncation and all nine read
error/crash boundaries with restart. Existing staging/publication fault matrices remain required.

This is an executable lookup/read capability, not removal of the journal's recovery maps. The
full scan still builds certificate and blob collections; graph/coordinator callers still use
their existing APIs. Next integrate explicit proofs through admission/read paths and replace
resident history without hiding extra disk work. Blob metadata, BM-01/BM-06, T-20 and the wider
roadmap remain open. The existing certificate-log/segment limits and all M1 claims are unchanged.
