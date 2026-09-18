# Decision 0056 — Bounded memory write, query and lifecycle semantics

Date: 2026-09-18

Status: accepted implementation profile for T-64 through T-66 at implementation commit
`97537e5`. This is a disposable derived-index pilot, not an authoritative source migration,
physical-erasure guarantee, production qualification or full FR-05/FR-11/FR-20 implementation.

## Durable write contract

`uste-memory` reuses the encrypted blob store, journal coordinator and mandatory policy facade. Its
canonical `UMEM` 1.0 transaction format admits only the frozen `memory-pilot-v1` limits. A source
version is immutable, sequential per source identity and bound to exactly one newly committed blob
inventory entry. Trusted exact text must use `text/plain; charset=utf-8`; its length and SHA-256 must
match the blob exactly. Opaque bytes use `application/octet-stream` and are never interpreted.
Unknown formats and versions fail closed.

Facts contain bounded subject/predicate/value fields, an exact source version, an honest byte range
or UTF-8 line locator, an optional source-event instant and at most eight already-admitted scoped
links. Corrections add a new fact and terminally supersede the predecessor at the same commit;
independent conflicting facts remain separately visible with their own evidence. Retraction and
source revocation are durable terminal records. State preparation clones and validates before the
journal publishes, so rejected source bytes, locators, references or limits do not partially mutate
the projection.

The generic authorized coordinator still refuses new uploads after recovery because format 1.0
cannot enumerate abandoned staging. A trusted adapter may reopen that gate only by supplying its
complete durable upload-token outbox. Every token must identify a committed blob, a durable abort,
or no durable staging evidence; an uncommitted resumable upload keeps ingestion closed. The outbox
is capped by both the policy quota and coordinator ceiling. This protocol does not enumerate the
filesystem or reclaim arbitrary orphans; T-35 retains that broader work.

## Query and time contract

The supported pilot reads are fact identity lookup, case-insensitive all-term lexical search,
explicit one-hop fact links and citation resolution. Every top-level request and returned candidate,
source and embedded link passes the policy facade. Denied candidates do not contribute returned
records, snippets, graph paths or reported visit counts. Citation resolution returns the exact blob
reference, content digest, immutable source version, locator and exact UTF-8 slice when one exists.
The consumer-facing adapter must not expose the privileged namespace-scoped raw blob API; it may use
that API only after the memory projection has authorized the owning source.

`KnowledgeAt::Revision` means recorded journal knowledge as of that commit. `EventTimeFilter` is
limited to any, missing or exact equality against the fact's declared source-event instant. A later
correction cannot enter an earlier knowledge view. A source replacement excludes the older version
at and after its superseding revision while retaining earlier history. Current source-policy or
durable source revocation excludes the source even from an otherwise retained historical query.
Ranges, intervals, local-time interpretation, derivation availability, spatial predicates, ranking
and vector similarity are unsupported and must be rejected rather than approximated; T-21 and the
preserved query/content roadmap own those broader semantics.

Search terms, candidate visits, returned results and output bytes are request-bounded within the
frozen profile. Result-cap/output-cap exhaustion returns an explicit truncated result; candidate
budget exhaustion fails. Authorized reads can use cooperative cancellation checked before dispatch,
during policy requirements, at every candidate authorization and after reducer return. Cancellation
returns `TransactionError::Cancelled`, never a partial result presented as complete.

## Revocation and rebuild contract

Every read carries the consumer's authority generation. A different generation returns
`StaleGeneration`; any commit invalidates process-local older views as `StaleView`. Policy version
replacement invalidates issued read leases before reducer access. Record/source denial is reapplied
to each fresh read.

`BeginRebuild` durably increments the generation, marks the projection unready and clears its
logical derived records. Reads then return `Rebuilding`, including after interruption and recovery.
Only `CompleteRebuild` after the consumer has reimported its current approved sources makes that
generation servable. A copied older projection receives the new consumer generation and refuses it.
The consumer source store remains untouched and authoritative throughout.

Clearing the logical projection does not prove that encrypted journal history, superseded blobs,
backups or storage-device remnants are physically erased. M1 offers immediate read exclusion and a
fail-closed disposable-index rebuild only. Full reclamation, purge certificates, backups and
authoritative retention remain T-35/T-36 and later release work.

## Consequences

The bounded in-memory projection is replayable and useful for the local pilot, but it makes no
larger-than-memory claim. The T-67 adapter must own the durable source/outbox checkpoint, expose only
the restricted operations above, serialize the single writer, cap readers and map its approval
generation exactly. T-68 must still run real-process fault cases, resource measurements and the
offline consumer handoff before M1 is complete.
