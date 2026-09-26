# Memory consumer contract `uste-memory-consumer` 1.0

Candidate producer-owned contract · 2026-09-26 · Package DB-R04 (CAP-08, CAP-17, CAP-21, CAP-25,
CAP-28, CAP-32, CAP-40) · [Decision 0281](decisions/0281-producer-owned-memory-consumer-contract-delegation.md)
admits its design and [Decision 0282](decisions/0282-memory-consumer-contract-1-0.md) pins it.

This is USTE's candidate contract for a local runtime or coordinator that keeps source-backed
research, documentation and repository memory in USTE. It is written by the producer from the
existing implementation. No consumer has reviewed or adopted it, and no combined integration has
run. Consumer review, consumer implementations and an authorized integration run remain separate
acceptance work.

## Shape

The contract is the Rust trait `uste_memory::contract::MemoryConsumerContract`, a synchronous,
in-process API. There is no socket, service or wire format in 1.0. One implementation instance
owns exactly one namespace, one authenticated principal and one owning process. The producer
implementation is `uste_memory_adapter::research::ResearchMemoryProducer` over the authorized
durable coordinator. The record vocabulary is [research memory records](research-memory-records.md).

| Operation | Contract call | Effect |
|---|---|---|
| Negotiate | `negotiate_contract(name, version)` | Same name and major version, requested minor ≤ 0; otherwise `UnsupportedVersion` |
| Start generation | `BeginGeneration` | Clears derived records; the generation is not queryable until completed |
| Admit | `PutSource`, `PutArtifact`, `PutClaim`, `PutEdge` | One record per write; source bytes are stored by the producer |
| Correct | `PutClaim` with `corrects` | Supersedes the named active claim |
| End a claim | `RetractClaim`, `ExpireClaim` | Terminal status transition, not deletion |
| Revoke | `RevokeSource` | Withholds that version's excerpts from every later read |
| Finish generation | `CompleteGeneration` | Makes the generation queryable |
| Change authority | `ReplacePolicy` | Replaces the namespace policy; returns no journal revision |
| Read | `view()` then `read(view, request)` | `GetClaim`, `SourceVersion`, `SourceVersions`, `EdgesFrom`, `Search` |

`PutArtifact` in 1.0 admits artifacts without retained content. Artifact content awaits a later
minor version with its own retry verification.

## Authority and scope

The consumer's trusted adapter creates the policy kernel and authenticated principal and passes
them to the producer; the producer never mints authority. Every write and read is authorized
against current policy with the action set recorded in Decisions 0277 and 0278. Every candidate
record in a read passes a per-candidate check first, so hidden records contribute neither content
nor counts. References never cross namespaces. A policy replacement makes earlier views fail as
`StalePolicy`.

## Revisions, views and generations

Each successful write returns a `WriteReceipt` with its commit revision. A view is coherent at one
revision under one policy; a read through a view taken before a newer commit fails as
`StaleView`. Reads name an authority generation; a non-current generation fails as
`StaleGeneration`, and an incomplete one as `Rebuilding`. The `SourceVersion` confirmation read is
served while rebuilding. Knowledge revisions before the generation's retained start or after the
current revision fail as `HistoryUnavailable`.

## Citations, support and freshness

A claim read returns its visible citations with exact locators, excerpt digests and, unless the
source version is revoked, the excerpt text. Effective support is the recorded support kind,
`RecordedUnsupported`, `Revoked` with the latest revocation revision, or `NoVisibleCitation`.
Freshness is evaluated at a caller-supplied instant: `Fresh`, `Stale` with the age, or `Unknown`
when that instant precedes retrieval. USTE never refetches, resolves or executes anything a
record describes; stale or inaccessible sources are stored states.

## Budgets

All limits are the frozen `research-memory-v1` profile in `RESEARCH_PROFILE`, including 16 MiB per
source version and 1 MiB per request. Read candidate, result and output limits must be non-zero
and within the profile; results report visited candidates and truncation. A request over any
limit fails before commit with `ResourceLimit`; there is no partial acceptance.

## Idempotency, cancellation and restart

Each write carries a consumer-chosen `OperationId`. Replaying the same request returns the
original receipt. Reusing the identity for a different request is a `Conflict`. Retries are
honoured for the producer's 30-day outcome retention, after which they fail with
`IdempotencyExpired`. A retried `PutSource` is answered from the recorded outcome after the
stored version is compared field by field, with content compared by length and SHA-256. The
bytes are never stored twice.

Cancellation is cooperative. It is observed before any upload and before publication, and
returns `Cancelled` with nothing committed. `OutcomeUnknown` means publication may have happened
and the consumer must reopen before retrying. Reopening replays the authenticated journal. Contract
1.0 keeps no upload outbox, so an upload interrupted by process loss was never committed. It
leaves unreferenced staged bytes for later reclamation (T-35) and has no logical effect.

## Errors

`ContractError` has exactly these variants: `UnsupportedVersion`, `Unauthorized`, `StalePolicy`,
`StaleView`, `StaleGeneration`, `Rebuilding`, `HistoryUnavailable`, `NotFound`, `Conflict`,
`InvalidRequest`, `SourceChanged`, `ResourceLimit`, `UnsupportedQuery`, `Cancelled`,
`OutcomeUnknown`, `IdempotencyExpired`, `Unavailable`, `IntegrityFailure` and `Storage`. Only
`Unavailable` and `Storage` are retryable unchanged. Errors carry no content.

## Conformance

`uste_memory::contract::conformance::run_conformance` drives any implementation through 34
ordered synthetic steps covering every error class above that a correct implementation can
reach deterministically. The step names, expected outcomes and canonical fixture records are
pinned by `fixture_digest()`, recorded in Decision 0282. A consumer-side or alternative
implementation passes when every step passes and the digest matches the pinned value.

## Not in 1.0

Out of scope for 1.0:

- a wire protocol;
- streaming uploads above one in-memory write;
- artifact content;
- deletion or purge (T-34);
- migration of pilot data (Decision 0279 plans it read-only);
- any claim of consumer agreement or cross-stack integration.
