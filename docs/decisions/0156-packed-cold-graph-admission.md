# Decision 0156 — Cold packed graph semantic admission

Date: 2026-09-19

Status: implemented and locally verified; T-20 qualification remains open.

A published packed root and canonical trees do not establish graph semantics. Before returning
an opaque cold `PackedGraphBase`, require the exact graph index/reducer/ordered-state profiles,
scope and authenticated certificate, eight explicit families and their recomputed ordered-state
commitment. Fully admit each canonical family under separate aggregate validation bounds.

Then stream every family and reuse the existing graph history-transition, reference-requirement,
secondary-membership and frozen v1 digest rules. Check metadata/counts, every first/successor
history state, historical reference predecessors, terminal/current equality, current closure,
derived cardinalities and owner-local membership, and terminal/current policy equality. A reverse
packed cursor proves the newest reference version at or before its required revision without
scanning the complete history. Explicit empty families remain part of the commitment.

Keep canonical validation, sequential semantic scans and repeated point/predecessor proof budgets
separate, so interleaved reads cannot spend a stale shared allowance. Retain only bounded traversal
paths, current entries and one bounded history group; no full graph or reference maps. Reuse
existing logical admission ceilings; report actual packed work distinctly from legacy run/cache
work. The frozen v1 digest may be recomputed for compatibility, never replaced by the ordered hash.

Test valid cold reopen against the independent reducer and compatibility export, authenticated
but semantically inconsistent trees, wrong claims, exact/minus-one resource limits, late corruption,
all observed read failures and foreign owner/key refusal. Failure returns no admitted graph state
and performs no authoritative mutation. Cold suffix integration, authorized packed consumer writes
and qualifying campaigns remain separate requirements; this does not close T-20.
