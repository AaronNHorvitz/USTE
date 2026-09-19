# Decision 0122 — Inventory-bearing genesis reconstruction

Date: 2026-09-19

Status: accepted implementation contract; T-20 and qualification remain open.

Add `AuthenticatedIndexRecovery::recover_primary_genesis` beside the unchanged closed
inventory-free bootstrap. It reconstructs exactly the first certified request and inventory,
under the shared encrypted-range byte allowance and an explicit inventory-reference ceiling.
The narrower reference check occurs after storage's independently bounded decoding and before
reducer preparation. Exact result-digest validation and cursor completion precede the opaque
`RecoveredGenesis` handle. The trusted caller still supplies the genesis reducer; its memory
behavior remains explicit. No full history, owner map, journal append or root publication occurs.

`stage_primary_genesis_metadata` streams primary owner entries directly from that one canonical
inventory, assigning every reference to the first transaction's principal. The total-owner bound
is checked before scratch I/O. Metadata, retry, owner and transaction-ID families use the existing
frozen profiles and exact merged insertion/output counts. Returned candidates have generation zero
and still require independent journal correspondence and domain admission. Optional first-reference
and quota projections are not implicitly attached. The inventory-free staging wrapper explicitly
refuses an inventory-bearing handle before any filesystem work.

Together with Decision 0121, this permits primary metadata reconstruction from the journal origin
without accumulating suffix retry/transaction/owner maps. Only the current bounded transaction and
new-owner deltas are retained. Private terminal metadata requires durable rebase even with no
suffix. Only then may fresh writes proceed. The authoritative journal/certificate frontier is not
changed and no receipt or consumer authorization is created by a private recovery handle.

Reference tests start with no coordinator root manifests, reconstruct an inventory-bearing first
transaction, independently admit its staged candidates and recover either zero or three suffix
revisions. Exact first principals, retries and transaction lookups survive terminal rebase and cold
re-admission. Initial limits and the closed wrapper refuse before staging. A preparation-count probe
checks reference refusal before preparation, wrong reducer digest and exact inventory preservation.
All 39 observed genesis read fault cases and 84 staging error/crash cases refuse; no failed stage
leaves a discoverable root, and restart recovers the exact inventory and terminal owner ledger.

Native BM-06 remains on its inventory-free path and two-record development ceiling. This does not
finish optional owner projection maintenance, eliminate immutable-family rewrite amplification,
qualify larger-than-memory recovery or alter M1's pinned consumer handoff. Existing storage and
independent admission limits still apply; complete I/O accounting and qualifying campaigns remain.
