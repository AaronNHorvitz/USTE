# Decision 0099 — Disk blob metadata during cold recovery

Date: 2026-09-19

Status: locally verified T-20 increment. Disk-aware inventory append and adapter
integration remain open.

## Cold-open contract

Add opt-in `JournalStore::open_with_disk_blob_metadata`, preserving both legacy open modes.
Authenticate the entire certificate/group chain and every named committed blob before exposing
any replay callback. Neither validation nor replay retains certificate anchors, committed blob
references, inventory IDs/counts or namespace byte maps. Both passes check that their history
collections remain empty. Only fixed frontier state, counters, one format-bounded decoded
inventory, bounded crypto/group buffers and explicitly admitted segment-tail descriptors survive
a step. This changes storage metadata residency, not journal authority or consumer authorization.

After the first physical validation pass, retain the exclusive owner while independently
admitting Decision 0098's current disk catalog. With `AdmitOrRebuild`, a discovered current
candidate must pass admission; absence or only older candidates triggers bounded reconstruction
to the exact journal frontier. A current candidate failing semantic admission fails the open;
it is not silently treated as valid or rolled back. Explicit `Rebuild` ignores candidate selection
and reconstructs derived state from the journal. Corrupt committed source data still fails either
mode. Optional malformed/unreadable-format root slots follow existing index discovery rules.

Split catalog construction into private staging/admission and terminal publication, preserving
the public rebuild operation. During cold open, all catalog semantics are checked before journal
tail repair or resynchronization. Scratch runs may be written but no catalog root is published
yet. Then repair the certificate tail and synchronize the exact certificate frontier, repair and
synchronize admitted segment tails, and publish a new catalog if needed. Only afterward perform
the authenticated replay pass. As in legacy open,
replay visitor effects are provisional until successful return: a replay-time I/O failure can
occur after callbacks. No store/base is returned on failure. A crash may leave optional orphan
runs or lose a derived cache, never make an uncommitted blob authoritative.

An empty journal needs no catalog. Empty-inventory appends remain available and leave the
historical blob catalog content valid; its revision is not relabeled. A later cold open advances
the derived catalog to the new exact frontier. Nonempty calls to legacy inventory append reject
before I/O in this mode. Empty resident maps must never be interpreted as absent collisions,
uncommitted inventory identity or spare namespace quota. A later disk-aware append path must
establish all those checks explicitly before publication. Ordinary legacy behavior is unchanged.

The admitted base supports existing exact disk lookups and committed-reference proofs/range
reads. The old resident-map range read is not silently rerouted. Principal authorization remains
the coordinator/consumer capability's responsibility.

## Work and resource bounds

Callers supply catalog build/admission limits, a bounded page cache, maximum logical blob bytes
verified per scan, and maximum pending uncommitted segment tails. Initial certificate/group
counts are admitted before scanning. Each scan charges certificate plus encoded-group bytes
before reading the group. Reference-binding counts are checked before payload verification.
The entire current inventory's logical payload verification is admitted before reading its
payload. Tail count is admitted before retaining a descriptor or performing any repair.

Unlike legacy deduplication, this first map-free implementation verifies payloads for repeated
inventories again in both passes; it keeps no hidden digest-to-count memoization. Both pass
allowances are independent and reports expose the repeated cost. Limits of zero for payload
bytes or pending tails permit genuinely zero-work cases, not bypasses. Existing absolute blob,
binding and namespace limits are still proved by independent catalog admission before callbacks.

Reports split validation, replay, catalog discovery proofs, catalog admission and optional
rebuild. Scan reports count groups, certificate/group encoded bytes, reference bindings, verified
logical payload bytes, largest live inventory cardinality and peak pending-tail count. They are
not filesystem/device I/O totals or measured maximum RSS. Inventory/header reads and crypto
buffers retain their existing separate format bounds; the catalog's rewrite/proof amplification
and transient bounded inventory clone remain explicit. The unchanged 24 GiB benchmark reservation
and qualification protocols are not satisfied by running tests under a 4 GiB process cap.

## Remaining work

Wire this opt-in mode through authenticated transaction recovery and native adapter diagnostics;
implement bounded disk-aware nonempty inventory append and domain-authorized quota transfer.
Qualify native process recovery and larger-than-memory behavior before claiming T-20. BM-01,
BM-06, T-19 and the full retained roadmap remain open. M1 and AgentMage are unchanged.

## Verification boundary

Focused tests cover actual map-free cold open with missing/current/stale catalogs, exact first
references and raw proven bytes, empty-journal startup, repeated-inventory verification charges,
pre-callback/pre-repair resource and payload-corruption refusals, explicit false-catalog rebuild,
tail-count admission, uncertain certificate resynchronization and legacy inventory-write refusal.
The stale-catalog fixture initially reused a deterministic entropy range across rebuilds, correctly
eliciting immutable object-name AlreadyExists; each reopen now supplies a fresh scripted range.

Ready-open fault coverage is exhaustive for OpenDirectory (1), TryLockExclusive (1), OpenExisting
(45), Metadata (55), ReadAt (88) and SyncData (1): 191 boundaries/573 attempts. Missing-catalog
reconstruction additionally tests first/middle/last CreateNew, WriteAt, SetLen, SyncAll and
SyncDirectory plus the sole SyncData boundary: 16 selected boundaries/48 attempts. The separate
Decision 0098 sweep covers every catalog mutation boundary. All 621 attempts restart at the exact
journal frontier; 620 report failure, while one optional absent-root OpenExisting/CrashAfter is
not an actual crash. This does not claim exhaustive cold-rebuild read/write fault coverage or
native process/hardware qualification. The six focused tests passed in 309.21s with strict storage
Clippy; the broader regression gate is recorded separately in PROGRESS.
