# Decision 0278: Bounded authorized research memory queries

Date: 2026-09-25

Status: Accepted for the development implementation of DB-R02.4; DB-R02 remains open.

Decision 0277 admits research records, but consumers could not yet read them through the
mandatory authorization facade, see effective support after revocation, or evaluate freshness.

Implement `AuthorizedReadState` for `ResearchState` with four requests. `GetClaim` returns one
claim with its status at the requested knowledge revision, its visible citations, each cited
source's freshness at a caller-supplied evaluation instant, and an effective support label.
`SourceVersions` lists one source's versions recorded by the knowledge revision, oldest first.
`EdgesFrom` lists edges leaving one record, optionally of one kind, in identity order.
`Search` returns active claims whose subject, predicate, value or visible excerpts contain every
ASCII-case-folded term (at most eight terms of at most 256 bytes).

Request-level authorization: `ReadRecord` on a requested claim; `ReadRecord` and `ReadHistory` on
a listed source; `ReadRecord` and `ExpandGraph` on an edge origin; namespace `Search` for search.
Every other candidate (a cited source, an edge's identity, target and asserting record, a searched
claim) passes through the per-candidate callback first; a hidden candidate contributes neither
content nor any count, matching the memory pilot. A claim whose citations are all hidden reports
`NoVisibleCitation`, not a fabricated support level.

Revocation applies to every read after it commits, including historical knowledge views, as in
the memory pilot and the data model's rule that current authority applies to history: a revoked
source version never returns excerpt text, excerpt-only search terms stop matching, and a claim
whose visible citations are all revoked reports `Revoked` with the latest revocation revision.
Freshness is `Fresh`, `Stale` with the age, or `Unknown` when the evaluation instant precedes
retrieval; pinned sources never go stale. Nothing is refetched.

Generations, readiness and history bounds follow the pilot: a mismatched authority generation is
`StaleGeneration`, an incomplete rebuild is `Rebuilding`, a revision after the current one or
before the retained start is `HistoryUnavailable`. Result, candidate and output-byte requests must
be non-zero and within the frozen profile; list results report visited candidates and truncation.

Verification (focused): query tests cover freshness in all three states, partial and complete
revocation, withheld excerpts, historical revocation, hidden sources, hidden claims, history
bounds, generations, readiness, output limits, version visibility by revision, edge kind filters,
hidden edge targets with uncounted candidates, truncation, multi-term search, revoked-excerpt
non-matching and read requirement vectors; all `uste-memory` tests, strict workspace Clippy,
warnings-denied rustdoc and the memory-adapter tests passed. The full gate was not rerun.
DB-R02.5 (pilot mapping) remains open.
