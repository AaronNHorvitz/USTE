# Decision 0118 — Private genesis index reconstruction

Date: 2026-09-19

Status: accepted implementation contract; full T-20 qualification remains open.

Complete graph-base loss needs an origin path without loading the entire graph or publishing
historical roots. Add `recover_inventory_free_genesis` to the exclusive authenticated recovery
owner. It reauthenticates exactly the first certified transaction under a caller-supplied encoded
range-byte ceiling, verifies its reducer result digest, and returns an opaque `RecoveredGenesis`.
The caller supplies trusted genesis state; its reducer's memory behavior remains explicit. Only
one request/state is reconstructed. Empty journals, inventories, changed bytes and failed reducer
checks refuse; inventories are not silently stripped and no first-owner evidence is invented.
The terminal journal frontier and exclusive owner are retained unchanged. This handle is trusted
recovery input, not current consumer state, consumer authorization or a durable receipt.

`stage_graph_genesis_root` streams that first graph snapshot into bounded per-family scratch runs.
`stage_inventory_free_genesis_metadata` stages the one exact retry outcome, transaction-ID entry
and zero-owner metadata. Both return generation-zero candidates, not admitted live state. Existing
independent semantic graph admission and journal/metadata correspondence admission must validate
them before streaming recovery. The unchanged on-disk profiles and exact digest algorithms apply.
No intermediate root slot or source transaction is published; failed stages leave only derived
scratch files subject to the existing T-35 reclamation work.

The streaming domain gains initial-anchor hooks with defaults requiring ordinary ready state.
The graph implementation explicitly accepts independently admitted private initial roots. It
preserves exact scope/revision/certificate/reducer/logical-state pairing. This does not relax
ordinary `DiskCoordinatorState` metadata publication: generation-zero graph roots remain refused
there, and terminal validation still uses that ordinary contract. At a first-revision terminal
frontier, the private graph root is published through the normal current-frontier path; a discovered
ready root instead retains the existing bounded resynchronization behavior. Later revisions use
the existing authenticated streaming reducer, collision/retry/first-owner validation and private
root advancement, with publication only at the exact terminal frontier.
Private initial coordinator roots mark rebase required even when the suffix is empty. This
prevents the empty-overlay optimization from skipping their durable terminal publication and
keeps new writes blocked until that publication succeeds. Normal cold reopen then admits the
published pair rather than repeating origin reconstruction.

This is a bounded recovery building block, not an automatic native fallback or larger-than-memory
qualification. Metadata suffix overlays still obey their explicit count admission; an origin
rebuild spanning a large journal needs bounded incremental metadata staging rather than a hidden
full-size overlay. Native integration, complete corrupt-cache controls and scalable construction
remain subsequent work. No M1 source, authoritative migration, erasure or release claim changes.

Tests reconstruct revision one behind a later authenticated frontier without appending, refuse
inventories before reducer preparation, check wrong/empty genesis and exact result digests, inject
every observed genesis read error, and reject post-open certificate corruption. Graph tests start
with no graph/coordinator root manifests, re-admit staged candidates, recover zero/three suffix
revisions, compare the independent full-reducer digest, publish/rebase only terminal roots, verify
every retry lookup, enforce current authorization concealment and recover again after restart.
All 57 observed staging I/O occurrences receive error, crash-before and crash-after injection
(171 attempts); no failed attempt leaves discoverable roots, and each restarts and recovers.
