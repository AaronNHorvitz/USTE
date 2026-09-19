# Decision 0083 — Profile-derived disk driver limits

Date: 2026-09-18

Status: T-20 partial implementation; native commands remain development-capped, not qualified.

Replace scattered development admission/write/merge constants with one validated
`DiskProfileLimits` derived from the accepted fixture and maximum-10,000-operation batch plan.
The native adapter constructs it before filesystem access. Both disk adapters use the same
limits for cold transaction/metadata correspondence, graph admission, pending preparation,
ordinary writes and metadata/graph merges. The batch's identities and operations travel together;
retry identities and canonical fixture bytes are unchanged.

For G planned revisions, admit G transaction outcomes, no blob owners, and G+1 metadata entries.
Bound each journal-prefix pass by G times the format's maximum encoded group plus certificate
(16,781,377 + 4,161 = 16,785,538 bytes; corrected during Decision 0084 review);
the two-outcome metadata overlay retains an independent two-group encoded suffix allowance.
The one-pending-graph-revision contract is unchanged. Fixed-width coordinator runs use explicit
count-derived page/byte limits. No full-memory fallback is introduced.

Use Decision 0081's exact final graph counts to bound scan entries and the largest merge family.
Allow two pages per fixture entry with fixed run headroom and 16 KiB logical bytes per entry.
These are cumulative work ceilings, not allocations, predicted database size or reservations.
Graph admission retains two versions per record, 64 KiB history buckets, explicit semantic/
lookup headroom and 64-page/16-KiB predecessor limits. Decision 0082 permits repeated lookup
work independently of physical scan size; all actual limits still fail closed.

For a relationship batch of B records, preparation allows B absent/current relationship proofs,
up to min(2B,E) distinct endpoints and one Evidence record, or the larger entity-creation batch.
This gives 30,001 proofs at exact size. Fixed fixture records plus identity accounting fit within
1 KiB per proof; add 1 MiB policy headroom. The fixture uses no ReadView-history or deletion-reverse
buckets, so those preparation allowances are zero. This does not restrict the generic graph
engine's accepted operations. Delta limits remain one million entries/64 MiB. Admission and each
writer use the fixed 64 MiB cache, dropped between phases; query cache semantics are unchanged.

Tests construct limits for every accepted size from 2 through 100,000 entities without database
work, pin the exact-size 212-group/30,001-proof/three-million-largest-family values, and encode
all fixture record kinds, relationship classes and both statuses with maximal revision fields to
check the per-proof byte premise. Development cold/fault/process-loss/oracle tests exercise the
actual routed limits. Constructor validity and development results do not establish sufficient
qualification-time resources, complete I/O accounting or qualifying performance. Larger campaigns,
BM-06's ten-million-event protocol and T-20/T-19 completion remain separate work.
