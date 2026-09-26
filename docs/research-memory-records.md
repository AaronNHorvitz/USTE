# Research, documentation and repository memory records (`research-memory-v1`)

Accepted specification · 2026-09-25 · Package DB-R02 (CAP-20, CAP-22, CAP-27, CAP-29) ·
[Decision 0275](decisions/0275-research-memory-record-specification.md); canonical codec only
([Decision 0276](decisions/0276-research-memory-record-codec.md)) and reducer
([Decision 0277](decisions/0277-research-memory-reducer.md)); queries not implemented

This document specifies versioned, scoped records that a consumer uses to retain what it learned
from web research, documentation packs and repositories, with exact provenance. It extends the
[data model](data-model.md) vocabulary and the bounded [memory pilot](memory-first-milestone.md);
it does not replace either and claims no capability. USTE stores and retrieves these records.
It never fetches the web, resolves a URL, runs a search, executes tools, calls a model or grants
permission; consumers own acquisition, execution and authority.

## Ownership boundary

- A consumer (runtime or coordinator) acquires bytes, decides what they claim and submits records
  through an authorized transaction. USTE validates structure, scope, references, budgets and
  revisions, then stores, indexes and returns them with citations.
- Retrieved content is data, never instructions. A stored claim such as "run this command" is a
  quoted source statement with no execution authority inside USTE or its adapters.
- A derived index is rebuildable and is not a source of truth or a permission. Authoritative
  consumer stores remain authoritative until a migration is separately qualified (CAP-29).
- Storage performs no network or DNS activity; a missing, stale or inaccessible source is a
  stored state reported to the caller, never a trigger for refetching.

## Scope and identity

Every record belongs to exactly one namespace (`NamespaceRef`); references never cross
namespaces. Identities are opaque 16-byte `RecordId` values chosen by the consumer and stable
across versions. URLs, paths, titles and content hashes are attributes, never identities.
Cross-repository relations (CAP-27) are represented only when both repositories are modelled as
records inside the same namespace; a relation never confers read or write access to either.

## Record kinds

All kinds are immutable once committed. A change is a new version with an explicit predecessor.

| Kind | Required content |
|---|---|
| `Source` | identity; `source_kind` (`web_page`, `doc_pack_page`, `repository_file`, `repository_manifest`, `supplied_document`); canonical locator text (URL, pack-relative path, or repository-relative path); `version_label` (documentation version, repository commit, or `unversioned`); retrieval provenance (`retrieved_at` UTC instant, consumer run identity as opaque bytes, retrieval route label); fetch outcome (`complete`, `partial{reason}`, `truncated{limit}`, `inaccessible{reason}`); content digest (SHA-256), exact byte length, media type and blob reference when bytes were retained; license/terms label and redistribution flag; freshness policy |
| `SourceVersion` | the pilot's `SourceVersionId` shape: source identity plus a monotonically increasing version, bound to one immutable byte stream or to an `inaccessible` outcome with no bytes |
| `Artifact` | a derived object (extracted text, section map, report) with its derivation: input source versions, producer identity and version (parser or model name, pinned revision, configuration digest), coverage and limitations, created revision |
| `Claim` | subject, predicate and value text within field budgets; `support` (`direct_quote`, `paraphrase`, `inference`, `unsupported`); one or more citations; optional valid interval; status lifecycle from the data model (`proposed`, `accepted`, `disputed`, `superseded`, `retracted`, `expired`); optional confidence only with a named basis |
| `Citation` | a source version, an exact locator (`ByteRange` or `Utf8Lines`, as in the pilot), the SHA-256 of the cited excerpt bytes, and an optional bounded exact excerpt |
| `Edge` | typed relation between two records in the scope: `supports`, `contradicts`, `corrects`, `derived_from`, `depends_on`, `documents`, `related`; provenance (the claim or artifact that asserts it); optional valid interval |

Inference is never recorded as `direct_quote`. A claim without a resolvable citation is stored
only as `unsupported` and is returned with that label. `contradicts` edges are symmetric in
meaning but stored directionally with the asserting provenance; both claims remain visible.

## Revisions, corrections and deletion

- Every commit has a recorded revision. Source retrieval time, document time and commit revision
  are distinct (see [time and ordering](time-and-ordering.md)).
- `Correct` creates a new claim version that names its predecessor; the predecessor becomes
  `superseded` at that revision and remains readable in history.
- `Retract` and `Expire` are terminal status transitions, not deletion.
- `RevokeSource` makes every citation of that source version ineligible from that revision on.
  A claim whose only citations are revoked is returned as `unsupported` with the revocation
  revision, never silently as supported.
- Source deletion and purge follow the retention/purge contract (T-34). Until that task exists,
  this profile supports revocation and generation rebuild only, and says so in every report.

## Freshness

A `Source` carries a freshness policy: `pinned` (versioned content that does not expire), or
`max_age{seconds}` relative to `retrieved_at`. Queries take an explicit evaluation instant and
return `fresh`, `stale{age}` or `unknown` per cited source. USTE never refreshes; a consumer that
refetches submits a new `SourceVersion`. Documentation packs (CAP-22) use `pinned` with a
`version_label` and support explicit refresh, retention and deletion through the same path.

## Budgets (`research-memory-v1`, per namespace unless stated)

Numbers are frozen before implementation and may only change by a versioned decision.

| Limit | Value |
|---|---|
| sources / source versions | 65,536 / 262,144 |
| retained source and artifact bytes / per version | 4 GiB / 16 MiB |
| artifacts / claims / edges | 262,144 / 1,048,576 / 4,194,304 (artifact limit added by Decision 0277) |
| citations per claim / edges per record (fan-out) | 16 / 256 |
| claim field bytes (subject, predicate, value each) | 4 KiB |
| stored excerpt bytes per citation | 4 KiB |
| locator text bytes | 2 KiB |
| request bytes | 1 MiB |
| query candidates / results / output bytes | 65,536 / 256 / 1 MiB |

A request that would exceed any limit fails before commit with a typed resource error; there is
no partial acceptance. Reads never return more than their requested maxima and report truncation.

## Encoding and versioning

Records use the canonical `uste-types` value codec inside a new record kind with its own format
major version. Unknown record kinds, versions, enum values or fields fail closed. The pilot's
`memory-pilot-v1` records remain readable unchanged; a pilot source maps to `Source` +
`SourceVersion`, a pilot fact to `Claim` + one `Citation`, and pilot links to `related` edges.
Migration is a separate, qualified step; no automatic conversion is implied.

## Queries (bounded, authorized)

Get record at a knowledge revision; list versions of a source; resolve a claim's citations with
exact excerpt bytes and freshness; claims supported or contradicted by a source version; bounded
one-hop edge expansion by kind; lexical search over claim fields and cited excerpts. Every query
applies authorization before expansion, exposes no hidden counts, and reports stale generations,
unavailable history and truncation explicitly.

## Acceptance for DB-R02 (specification package)

This package is complete when this specification, its limits and its mapping from the pilot are
accepted in a decision and split into implementation subtasks with fixtures. Implementation and
its tests belong to the DB-R02 subtasks below; DB-R03 then tests corrections, contradictions,
revocation, source deletion, stale generations, cross-scope denial, restart and rebuild.

- DB-R02.1: specification and frozen `research-memory-v1` limits (this document).
- DB-R02.2: canonical codec and golden vectors for every record kind, including unknown-version,
  unknown-field, oversize and non-canonical rejection.
- DB-R02.3: reducer with transaction validation, budgets, revision/correction/revocation rules and
  an independent reference model; reopen and replay equivalence.
- DB-R02.4: bounded authorized queries with citation resolution, freshness evaluation and
  truncation reporting; cross-scope and hidden-record denial.
- DB-R02.5: read-only mapping from `memory-pilot-v1` records with equivalence fixtures.
