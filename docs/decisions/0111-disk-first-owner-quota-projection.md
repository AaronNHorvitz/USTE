# Decision 0111 — Disk first-owner quota projection

Date: 2026-09-19

Status: accepted and locally verified. T-20 remains open.

Add optional `USTE coordinator-blob-usage-v1` roots beside the unchanged metadata,
transaction and first-reference profiles. The journal and independently admitted primary
first-owner ledger remain authoritative. The new root shares their complete cross-profile
anchor: namespace, revision, certificate, reducer and logical state digest.

All integers are big-endian unsigned 64-bit values. Three sorted index-v1 families are:

| Family | Key | Value |
|---|---|---|
| 1 | literal `blob-usage-v1` | owner count, namespace bytes, principal count (24 bytes) |
| 2 | principal digest (32 bytes) | owner count, principal bytes (16 bytes) |
| 3 | principal digest then blob ID (48 bytes) | unchanged canonical coordinator owner value (80 bytes) |

Family 1 always has exactly one entry. Families 2 and 3 exist exactly when owners are
nonzero. Zero-length blobs still contribute one owner and may create a zero-byte principal
aggregate. Counts and additions are checked; namespace charges count each blob once under
its original first owner, never under a later referencing principal.

Cold admission authenticates every family and streams family 3 with one cursor entry and
one current principal aggregate. Each secondary owner must exactly match the admitted primary
owner lookup, including reference bytes and principal. Ordered unique composite keys plus
primary correspondence and equal cardinality prove a bijection, without a whole-owner map.
Every principal group must equal its family-2 count/bytes; exact principal cardinality excludes
extra aggregate entries. The namespace sum must match family 1. No partial admission escapes.
A failed replacement retains the previously admitted projection. Per-run and per-lookup budgets
are explicit; total lookup count is bounded by admitted owners plus principals. The complete
base owner count is refused before I/O if over the requested ceiling or the existing hard cap.

Explicit `bootstrap_blob_usage_index` enables the empty projection at an owner-free fully
rebased frontier, without a synthetic transaction; repeated calls resynchronize an exact root.
Quota-aware rebase bootstraps from an owner-free base with a pending revision,
or continues an already admitted projection. It sorts only new-owner overlays, aggregates their
charges by principal and merges both orderings. Temporary maps/vectors are bounded by the existing
overlay owner cap, not the full base cardinality; the builder additionally uses a fixed 64 KiB
page cache. Bootstrap from a populated legacy base remains explicitly unsupported rather than
quietly materializing the complete ledger. Whole immutable-family rewrite amplification remains.
Caller merge/reuse bounds apply to each family; the separate quota lookup bound applies to
previous principal totals. `CoordinatorBlobUsageLimits.run` is the cold-admission run bound.

Quota publication joins the retry-safe root set. Installed bases and overlays change only after
all selected roots succeed; a partial publication remains retryable at the same frontier.
Omitting quota preservation on an already quota-indexed live base fails before entering rebase.
Cold adapters may ignore this optional cache and use the original complete streaming accounting;
they cannot treat absence or corrupt totals as zero. Existing formats and legacy APIs remain.

The new privileged `committed_blob_usage_indexed` performs two bounded base lookups (one for an
empty index), then adds only disjoint bounded overlays. It returns no partial totals after any
error and does not grant consumer authorization. Authorized upload/quota facades still use their
existing streaming path in this increment; integrating explicit indexed accounting is the next
step. No M1 interface, benchmark target, durability or authorization semantics changes. Neither
these bounds nor the fixture tests establish larger-than-memory or production qualification.
