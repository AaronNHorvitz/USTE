# Decision 0064 — Disk coordinator recovery with independent graph roots

Date: 2026-09-18

Status: T-20 partial implementation; no larger-than-memory or production qualification.

The immutable coordinator metadata pair and ready graph root need not have identical revisions.
Metadata rebase may lag graph terminal-root publication. Reconstructing complete coordinator maps
merely to bridge that gap would undo Decisions 0060–0063.

`DiskCommitCoordinator::recover_with_prepared_suffix` consumes the existing exclusive authenticated
journal owner, an admitted metadata pair at M, and independently admitted ready domain state at D.
Require M <= D and identical scope/reducer profile. The trusted domain supplies its full ready-state
identity; when M = D its complete anchor must match metadata. When M < D, streaming replay must
encounter D with the exact admitted domain certificate. No graph prefix is replayed in memory.

The journal frontier must be D or exactly D+1. In the latter case an opaque recovered transaction
and externally prepared change are required. Re-read and compare every transaction binding,
including certificate, event/inventory digests, principal, retry key, outcome, canonical request
and inventory. The domain validates the prepared change against its ready base and the stored
result digest must match before private publication. Missing, extra or unrelated suffixes fail
closed. More than one unpublished graph change remains forbidden.

The shared metadata replay engine streams M+1 through the frontier under explicit outcome, owner,
lookup and encoded-byte limits. It preserves base/overlay collision and first-owner checks.
Only terminal success exposes the coordinator. A ready newer graph root can therefore recover
multiple metadata revisions without graph reconstruction; a pending graph change still requires
terminal-root repair before normal domain progress. Storage's own maps remain memory-resident.

`publish_graph_disk_coordinator_base` shares the existing terminal-root merge and installation
implementation with the legacy coordinator. A failed merge leaves the state pending. Successful
repair allows metadata rebase and overlay release; it does not constitute consumer authorization.

Synthetic regression cases cover pending and ready recovery from an older metadata pair,
outcome/byte admission refusal, absent and misplaced suffixes, failed terminal publication followed
by repair, and metadata rebase to empty overlays. Ordinary-reducer recovery and commit/rebase fault
matrices remain regression tests. Dedicated disk-graph fault/corruption campaigns, disk-aware
authorization, scalable first-owner admission and qualifying BM-01/BM-06 remain required.

## Warm continuation extension

`load_graph_disk_coordinator_preparation_view` now loads the existing bounded graph proof directly
from `DiskCommitCoordinator<GraphDiskLiveState>`. The ready root must match the exact journal
frontier/certificate and graph profiles. Pending state refuses preparation. Narrow privileged
read methods delegate to the established scope/uncertainty/certificate checks; they do not expose
the internal overlay-only legacy coordinator or confer consumer authorization.

Tests continue from repaired/rebased recovery through two further proof/commit/root-publication/
metadata-rebase cycles. Each result matches the reference reducer; exact retries work while
pending, proof-byte refusal leaves overlays empty, and further preparation waits for root repair.
The final derived root and independent journal replay reconstruct the same complete reference
history. Complete maps exist only in the test oracle/reconstruction, not the warm production path.
