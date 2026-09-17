# API and integration

Draft contract · 2026-09-17 · T-16 transaction/blob, T-17 graph and T-45 time-library subsets
callable; public database/API/CLI operations remain proposed

Owns FR-13, FR-14, FR-15, FR-25 and the consumer boundary.

## API principles

Expose typed bounded operations, not arbitrary SQL, shell commands, or model-generated code.
Every operation carries a trusted principal/capability, namespace and resource budget.
Identifiers and caller-declared roles are not authority. Structured values avoid assembling
query strings, but do not replace schema or permission validation.

The implemented T-16 subset authenticates through a trusted adapter during protected initialization
and returns an issuing-kernel-bound opaque principal; the consumer facade accepts no authenticator.
Its external commit request has no identity field: the authorized
coordinator derives the persisted principal. Pinned read views and upload handles are opaque and
must be presented back for current-policy and coordinator-instance revalidation. Generic views
expose only the revision; T-17 supplies reducer-owned direct-record, historical, one-hop adjacency
and evidence-provenance projections without exposing the raw snapshot. Direct raw transaction and
storage handles are trusted internals, not an API for untrusted consumers.

| Surface | Required operations |
|---|---|
| Database | Create, unlock, inspect health/version, open bounded read view, close |
| Transactions | Begin/stage constrained mutations, commit with preconditions/idempotency key, inspect outcome |
| Knowledge | Propose/accept/retract/correct assertion, add entities/relationships/evidence |
| Query | Scoped entity/property lookup, bounded traversal, temporal lookup, explain |
| Content | Begin/resume/abort upload, finalize version, inspect metadata, authorized original/range read |
| Parsing | Request/cancel extraction, inspect job/coverage, read derivation, resolve citation |
| Retrieval | Lexical/hybrid search with byte/result budget, source version, score explanation and freshness |
| Simulation | Create/list/expire branch, run pinned model, compare branch, propose promotion |
| Lifecycle | Expire/purge with receipt, inspect retention dependencies, subscribe from revision |
| Operations | Verify/scrub, checkpoint, backup, restore preview/apply, migrate preview/apply |
| World | Create scoped world/frame, attach geometry/content, record observation, retrieve state at time |
| Spatial/navigation | Scoped box/radius/nearest/region queries, movement history, supplied-graph path |
| Unified query | Compose permitted graph/content/space/time predicates and retrieve actual records/source handles |
| Batch import | Validate mapping, preview, stream atomic batches, inspect/resume/cancel job |
| Physics | Validate admitted run profile, start/cancel/resume branch, inspect last durable tick/results |

Streaming APIs do not buffer entire files or result sets. Local IPC authenticates the actual
peer/session and constrains socket permissions; it is not an unauthenticated localhost port.
No remote listener or automatic cloud fallback in the initial profile.
These are typed operations, not an invented textual query language. See
[unified retrieval](ingestion-and-unified-retrieval.md), [space](spatial-world-model.md)
and [physics](physics-and-motion.md) for scope, response fields and release allocation.

## Read and write semantics

Reads name a coherent committed revision and explicit temporal parameters, bounded by
retention. Responses report revision, source versions, applied policy, limits and truncation.
Search results include coverage/partial status and evidence locators, not just generated prose.
Pagination cursors bind query, revision, scope and permissions and expire predictably.

Timestamp input/output obeys [time and ordering](time-and-ordering.md): resolved instants
use UTC, interpretation provenance remains available, and local display is a presentation
choice. Unknown dates are not replaced with now. Historical queries constrain both source
and derivation availability; wall-clock cutoffs cannot silently substitute for revisions.
The implemented `uste-time` subset accepts strict RFC 3339, declared numeric units or explicit
local interpretation, returns a bounded `TimestampEnvelope`, and can embed that envelope as a
canonical graph `Value`. Local presentation requires an explicit IANA zone. Content adapters do
not yet populate these envelopes; that remains T-24.

Writes return a transaction identity, durable revision, result digest and operation outcome.
On lost connection, clients query/retry using the same idempotency key. Reusing a key with
different operation content fails. Cancellation after a commit may stop response delivery
but cannot pretend the transaction was rolled back.

## Errors and subscriptions

Closed, versioned error categories include Unauthorized, NotFound, Conflict, InvalidSchema,
UnsupportedFormat, PasswordRequired, PartialExtraction, ResourceLimit, Cancelled,
OutcomeUnknown, HistoryUnavailable, IntegrityFailure, KeyUnavailable, UnsupportedVersion
and RetryableUnavailable, plus InvalidTime, AmbiguousTime and UnsupportedTimeScale.
Unresolved source metadata may be retained with status; an operation requiring a resolved
instant returns the relevant error instead of guessing. Error detail is bounded/redacted; resource existence is concealed
where policy requires. Partial extraction is never returned as an unqualified success.
Spatial/model/import errors add UnsupportedGeometry, FrameMismatch, TransformUnavailable,
InvalidUnits, AmbiguousState, UnsupportedModel, IncompatibleProfile and ImportSourceChanged.
An unreachable path is a typed completed result, distinct from a budget-exhausted search.

Change subscriptions emit ordered permitted commit references with resumable cursors.
Delivery is at least once; clients deduplicate by revision/event identity. Lag is bounded.
If a cursor precedes retained history, return an explicit resynchronization requirement.
Revocation stops delivery and invalidates buffered inaccessible results.

## Consumer responsibility

The engine validates access, schemas and durable transitions. The consumer decides domain
approval rules, what to propose as memory, prompt construction, model/tool execution and
real-world actions. No model inference is required for storage or recovery.
Retrieved content remains untrusted input to any external model.

Consumers may be future games, tracking applications or agent systems. Asset prices are
records supplied through local import or transactions; a separate caller/ETL owns any
external acquisition. The native database API is not an exchange/broker/feed API client.
Provider accounts, credential collection, live-price refresh and trade execution are out
of scope. See [application use cases](application-use-cases.md). Local key management and
authenticated IPC remain required and are distinct from external-provider credentials.

## Integration modes

### Derived-index mode

Caller-owned records and source versions remain authoritative. An adapter imports explicitly
authorized immutable snapshots and stable IDs. Clearing/rebuilding this database cannot
mutate the source. Reconciliation reports missing/changed records and respects deletions.
Do not turn this index into a second silent source of truth.

### Authoritative-storage mode

The engine owns named record classes under its transaction/retention contract. Switching
requires an explicit migration decision, complete ID/evidence mapping, snapshot/export
verification, cutover plan, and rollback boundaries that preserve deletion policy.
Other operational or credential stores need not be replaced.

## Generic adapter acceptance fixture

Use a synthetic “project notebook” client, not a branded downstream application:

1. Import an exact document version plus an unknown binary; verify byte round-trip.
2. Parse a supported document in an isolated worker; unsupported binary stays explicit.
3. Propose an evidence-backed relationship, then accept through configured policy.
4. Restart engine and client; retrieve it with a resolvable source citation.
5. Correct it and query valid-time versus recorded-time history.
6. Run a hypothetical dependency-delay branch; main state remains unchanged.
7. Revoke permissions during a read/parse, and prove inaccessible output is rejected.
8. Purge source and derivations, restart, rebuild indexes and attempt stale backup restore.
9. Prove a second namespace cannot read originals, extracted chunks, graph topology or
   existence-sensitive metadata.

Maintain separate derived-index and authoritative-mode fixtures. API/schema compatibility
must be tested across supported versions before describing integration as plug-and-play.
