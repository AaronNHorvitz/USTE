# Decision 0139 — Certificate-bound canonical packed-tree capabilities

Date: 2026-09-19

Status: accepted and locally verified T-20 maintenance contract; domain integration remains open.

Decision 0138 binds a manifest to a journal certificate but deliberately does not admit its tree
shape. Add an opaque per-family canonical-tree capability owned by that live journal instance.
Cold admission validates the root's certificate receipt and exact family, then completes Decision
0134's bounded structural/content scan before issuing the capability. A missing family is not
silently treated as empty; manifests already represent empty families explicitly.

Route exact lookup and in-process range cursors through the journal's vault/directory without
exposing either capability. Recheck the live historical certificate owner before every operation
and cursor step, and refuse a locked vault even for empty or completed cursors. A
reopened/different/poisoned journal cannot reuse prior receipts, including
otherwise identical content. A cursor error, including owner rejection, permanently poisons that
cursor. Later successful appends by the same exclusive owner retain historical content binding;
consumer facades must still apply their own current/historical policy and stale-view rules.

Stage exact copy-on-write deltas from an admitted canonical capability or an explicitly empty
private construction base. The target requires a bounded certificate proof at the current live
frontier; its anchor may be a retained historical revision for private recovery construction.
Reject mismatched database/namespace/profile/family or a newer base before index I/O. Use the
current journal epoch/writer, with the proven target revision as the immutable tree view. A
successful staged result carries a new canonical capability and the exact target certificate
receipt, allowing the next bounded private batch without rescanning the complete tree. No
intermediate root publication or journal mutation is performed.

Canonical/certificate capability does not mean reducer correctness, state-digest validity or
consumer authorization. Those remain separate domain/facade admission steps. The inductive shape
argument is: a fully validated canonical base (or exact empty tree), checked exact deltas under
Decision 0133, and a durable successful staged pack. Existing raw codec/maintenance primitives are
unchanged; the safe journal bridge does not accept an arbitrary caller-constructed physical root
as an admitted tree.

Required tests exercise empty/cold/admitted/private-staged paths, exact lookups and range reads,
multiple staged batches and later publication/reopen, pre-I/O scope/profile/family/revision/owner
refusals, poisoned owners/cursors, corruption after admission and recovery fault bounds. No v1/M1
change, authoritative migration or benchmark qualification is implied. Coordinator/domain
integration, complete measurement and the accepted release gates remain necessary.
