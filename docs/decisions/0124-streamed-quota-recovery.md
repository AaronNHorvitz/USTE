# Decision 0124 — Stream first-owner quota projections during recovery

Date: 2026-09-19

Status: accepted implementation contract; T-20 and qualification remain open.

Add `recover_with_indexed_usage_streaming_domain` with explicit principal lookup limits. It
requires independently admitted primary, first-reference (when owners exist) and quota roots
at the paired domain revision. Even an empty owner base requires an admitted quota head; absence
is not zero. Existing recovery entry points continue refusing attached projections they do not
maintain. Per-revision primary/first-reference staging now optionally advances the quota families.

Only newly encountered first owners contribute charges. Their principal is the current certified
transaction's principal; each exact repeated reference preserves the original owner and charge.
One principal delta updates an existing aggregate or inserts a new aggregate, and sorted new-owner
entries extend its principal/owner ordering. Count/byte arithmetic is checked. Zero-byte blobs
increase owner counts and may create zero-byte principal totals. No cumulative suffix map is
retained. Per-family merge and total owner limits remain explicit, and the principal lookup uses
its own caller limit and fixed 64 KiB temporary cache. The normal independent cold bijection/
aggregate admission is unchanged.

`stage_genesis_blob_usage` builds a private first-transaction quota candidate, including the
zero-owner head. Its bounded current-inventory owner map is not a full-history map. Owner admission
precedes allocation/staging; the returned root still needs independent quota-to-primary admission.
Only terminal quota-preserving rebase publishes discoverable roots. No intermediate quota values
become consumer authority, and normal principal authorization/expiry remain outside raw recovery.

Fix a mixed-root durability edge: `CoordinatorDiskBase::has_unpublished_roots` now includes
attached first-reference and quota roots, not only primary retry/transaction roots. A private
optional root attached to published primary roots therefore still requires terminal rebase even
with an empty suffix. Separate tests isolate first-reference-only and quota cases, preventing one
optional root from masking the other in the publication guard.

Reference tests cross initially empty/populated bases, private origin/published primary roots,
new/existing principals and zero/nonzero blob sizes. Exact indexed charges match independent
primary-owner scans and cold quota admission after publication. Three suffix steps produce 24 runs;
the two-principal fixture has 48 entries/4,824 logical key/value bytes, and the one-principal
fixture has 45 entries/4,680 bytes. These are partial merge diagnostics, not complete I/O.
Missing projections, omitted preservation, narrow principal lookup budgets and late certificate
corruption refuse without exposing partial results. Fault matrices include 57 genesis staging
failures, 909 suffix schedules (906 failures/three optional no-crash boundaries), and 804 terminal
publication schedules (788 failures/sixteen optional no-crash boundaries). Restart reconstructs
the same certified first-owner charges and publishes only terminal roots.

This completes these explicit paired-base metadata projection paths, not T-20. General lagging
domain/metadata base selection, immutable-family rewrite amplification, certificate-proof work,
complete accounting and qualifying campaigns remain distinct scalability concerns. Existing
bounded-overlay recovery stays available; native BM-06 keeps its inventory-free development
profile. M1, frozen formats, benchmark targets, physical-erasure and release claims are unchanged.
