# USTE — Architecture

Database design draft 1.6 · 2026-09-17 · R0 design ready; T-08–T-18/T-45 foundation implemented

## Component boundaries

~~~text
Rust client / authenticated local service
                 |
Authorization + schema + admission limits
                 |
Transaction coordinator and ordered durable journal
                 |
Deterministic reducers + graph/temporal/spatial indexes
                 |
Queries, source retrieval, snapshots, branch views

Streaming blob store <--- authorized artifact commits
       |
Restricted parsing / OCR / media / embedding workers
       |
Validated derived artifacts and provenance transactions
~~~

Workers cannot write directly to storage or grant permissions. Source bytes, derived content,
and graph state share transaction visibility but may use different physical layouts.
The content pipeline is specified in [content ingestion](docs/content-ingestion-and-parsing.md).

The database and physics kernel are native local components usable by future games, agent
systems and tracking applications. Asset-price observations use ordinary typed records and
relationships, not a dedicated network subsystem. Data acquisition, provider authentication
and any trading logic belong to a separate consumer/ETL outside the engine. No such connector
is required by the implementation plan. Database API calls remain distinct from outbound
provider API calls; local encryption/IPC authorization are not provider credentials.
See [application use cases](docs/application-use-cases.md).

## Proposed Rust workspace

The workspace now implements `uste-types`, `uste-time`, `uste-policy`, `uste-crypto`,
`uste-storage`, `uste-txn`, `uste-graph`, `uste-replay` and the independent `uste-testkit`; later
rows remain planned component boundaries, not claims of implemented directories or a fixed public
API.

| Crate | Responsibility |
|---|---|
| uste-types | Stable identities, schemas, canonical encodings, bounded values |
| uste-time | Pinned UTC normalization, source timestamp envelopes and explicit local presentation |
| uste-policy | Principal/capability checks, labels, retention decisions |
| uste-storage | Journal, commit metadata, blob segments, checkpoints, I/O adapters |
| uste-txn | Commit sequencing, optimistic validation, idempotency, reader revisions |
| uste-graph | Native graph records, adjacency/property/temporal indexes, traversal |
| uste-spatial | Implemented R1 typed geometry and versioned-frame history; later transform evaluation, predicates and native indexes |
| uste-motion | Observations, trajectories, state-at-time, uncertainty and correction dependencies |
| uste-content | Artifact lifecycle, parser protocol, derivation and citation validation |
| uste-query | Typed plans, budgets, ranking, explain output, lexical retrieval |
| uste-replay | Deterministic reducers, logical hashing, recovery replay |
| uste-sim | Branches, model identity, virtual clock, pure simulation scheduling |
| uste-physics | Pure bounded kinematics/contact models under pinned numerical profiles |
| uste-ingest | Implemented R1 typed atomic batch/job ledger; later mapping/CLI orchestration through normal transaction authority |
| uste-api | Consumer API and generic integration adapter contracts |
| uste-cli / uste-service | Operations and authenticated local IPC |
| uste-testkit | Independent reference model, adversarial fixtures, fault simulation |

Dependency direction follows authority: types do not depend on host adapters; reducers do
not acquire files, time, randomness, model handles, or network access. Storage is not allowed
to interpret document instructions. Platform workers receive only narrow capabilities.

## Cross-component invariants

1. An acknowledged commit is durable under the declared platform/failure model.
2. A public reader sees one coherent committed revision; staged changes are private.
3. Journal records and all required blob objects are durable before their references publish.
4. Graph records, both adjacency directions, and idempotency outcomes agree atomically.
5. Derived indexes can be rebuilt; retained authoritative evidence cannot be fabricated.
6. Historical visibility never restores revoked current authorization.
7. Replay uses recorded inputs, not a fresh model interpretation.
8. Branches and parser outputs cannot silently promote themselves to authoritative knowledge.
9. Encryption, retention, deletion, and resource accounting include derived and temporary data.
10. Unsupported content is preserved or rejected explicitly, never reported as understood.
11. UTC normalizes known instants; revisions order commits. Uncertain source dates never
    become invented facts, and replay never reinterprets them using current timezone rules.
12. Identity survives movement; geometry always names a frame and units. Unknown is not zero.
13. Observed, estimated and simulated state remain distinct; viewing never advances physics.
14. Combined queries use one coherent knowledge view and current authorization for every source.
15. Physics effects commit as whole branch-local event groups, never direct storage mutations.

The state equation is:

~~~text
logical_state(revision) =
    reduce(schema_version, retained_baseline, committed_events_through_revision)
~~~

The baseline identifies the history/deletion epoch. Replay before the retained boundary is
unavailable. Seeds are simulation inputs, not substitutes for source bytes. Logical hashes
exclude randomized ciphertext and rebuildable physical index layout.

## Transaction and storage shape

One owner process holds an exclusive database lock. Clients submit bounded transactions to
an ordered commit coordinator. Snapshot readers use pinned committed revisions; commit-time
validation checks read sets/predicates or rejects unsupported isolation patterns. Do not
claim serializability for arbitrary transactions until phantom/conflict tests demonstrate it.
The first supported transaction API uses constrained operations with explicit preconditions.

Decision 0016 implements the initial `uste-txn` correctness profile: a domain reducer produces an
owned prepared change without mutating live state, the coordinator publishes an encrypted `UTXN`
group, and only a synced certificate applies it. Pinned readers own reducer-produced snapshots. Ambiguous journal
results quarantine the coordinator until recovery; durable retry and transaction indexes rebuild
from the same groups rather than becoming a second commit authority.

Decision 0017 keeps blob authority inside that same journal owner. Namespace-scoped content is
streamed into bounded encrypted chunks, finalized immutably, summarized by a canonical encrypted
inventory under a key-derived opaque disk name and verified before its digest enters the commit
certificate. Recovery verifies all certificate-named chunks before logical replay; only exact
committed inventory references are readable. The profile bounds unique committed blobs, per-
namespace logical bytes and total certificate-to-blob bindings so recovery state and work cannot
grow outside recorded limits. A database-scoped process-local lease also caps live upload buffers
at 32 under the exclusive owner; canonical staging publication uses synchronized temporary files,
no-replace rename and authenticated progress witnesses. Paired terminal witnesses preserve final/
abort intent after one missing copy, and accepted abort intent is durable before cleanup begins.

Decision 0018 puts the storage-independent default-deny kernel in `uste-policy` and the mandatory
consumer facade in `uste-txn`. The facade derives the journaled principal from a trusted
authentication adapter, authorizes before state/index/filesystem access, revalidates versioned
revision-view/upload leases, conceals foreign outcomes and accounts exact staged/committed plaintext bytes.
Raw coordinator and storage handles are privileged internal capabilities. Reducers without a native
policy may use the trusted local adapter at open. Decision 0019 makes graph policy engine-native and
durable: authorized graph open requires an exact recovered-policy match, while privileged raw
bootstrap is the only initial-install path. Graph projections authorize targets and every embedded
reference before returning content; absent policy denies.
Recovered unique committed usage is rebuilt, while new uploads fail closed after reopen until T-35
can enumerate every uncommitted reservation; known evidenced tokens remain recoverable.

Decision 0019 adds `uste-graph`: strict canonical entity/evidence/assertion/relationship mutations,
revision histories, exact corrections and deletion cascades, symmetric adjacency/provenance indexes,
one-mutation-per-record revisions with read-stable correction targets, durable policy records and
reducer-owned authorized projections. Current indexes are rebuildable
in-memory correctness structures; T-20 owns disk runs and bounded caches.

Decision 0020 adds deterministic cold replay and optional journal-anchored encrypted checkpoints.
The journal writer alternates two namespace-scoped cache slots made of authenticated 1 MiB chunks
and a terminal manifest. An opaque recovered candidate must match an exact historical certificate,
reducer profile and logical digest before it can seed recovery. Seeded open authenticates the full
journal, reconstructs and compares all retry/transaction/blob-owner metadata through the anchor,
then applies the normal reducer only to the suffix. Invalid candidates fall back to the other slot
or cold replay; they never create commits or replace journal authority. The current two-open handoff
and 256 MiB in-memory cache cap are correctness choices, not T-20/BM-06 performance claims.

Decision 0025 adds the native Rust `index-v1` foundation: immutable encrypted 16 KiB logical pages,
certificate-anchored two-slot cache roots, complete-context bounded decrypted caching and a
semantically validated current-record/adjacency/provenance graph projection. The journal remains the
only commit authority. Raw projection methods stay privileged, while consumer indexed reads reuse
the mandatory policy facade and reducer-owned per-candidate filtering. Index maintenance requires
`ManageSchema`; policy-admitted roots keep their bounded cache opaque, use constant-time view/root
binding after admission and share global mixed-direction scan budgets. Streaming larger-than-memory
recovery and BM-01/BM-06 remain open T-20 work. T-35 still owns authoritative baseline switching,
compaction and orphan reclamation.

Decision 0026 adds borrow-aware reducer checkpoint access, incremental graph checkpoint encoding,
declared-length one-new-payload-chunk checkpoint publication and visitor-based index scans without
changing format-1.0 or `graph-current-v1`. The collecting APIs remain compatibility surfaces.
Decision 0027 changes graph write preparation to a bounded record overlay and publishes exact
before/after contributions without rebuilding full indexes. Preconditions remain against the
pre-transaction view and canonical formats remain unchanged. Ingest preparation and reducer
decoding still materialize/clone complete state. Decision 0028 adds a derived target/owner reverse
map and transaction-overlay reconciliation, so graph deletion no longer scans unrelated records.
The scalable design therefore uses new
versioned state profiles backed by
encrypted scratch runs, bounded overlays/tombstones and affected-closure validation; it does not
silently reinterpret the frozen current-graph projection as authoritative mutable state.

Decision 0029 introduces `graph-state-v1` as a separate complete derived-cache root over the
existing encrypted immutable-run carrier. It binds all current/history and derived families to the
exact certificate, reducer profile and logical digest, but is admitted only by comparison with the
live in-memory snapshot and cannot seed recovery. Journal authority and `graph-current-v1` remain
unchanged.

Decision 0030 separates an authenticated root candidate from a live-snapshot-admitted root. A
privileged single-handle full-run stream reconstructs private primary state, applies the shared
semantic validator, rebuilds and compares derived families, and releases the candidate only after
terminal run and logical-state digests pass. It remains outside consumer APIs and cannot create a
coordinator recovery seed without separately authenticated retry/outcome/blob-owner metadata.

Checkpoint transport now also offers opaque, certificate-anchored candidates discovered through a
bounded authentication/hash pass and a selected revalidated chunk stream. The stream may deliver
chunks before its terminal digest result, so consumers publish only after success. This removes the
transport-level full-payload requirement; reducer decoding and live state remain the full-memory
boundary.

Decision 0021 adds the capability-free `uste-time` normalization kernel. `uste-types` retains the
canonical instant pair without a timezone dependency. Strict explicit-offset and numeric-unit input
can resolve directly; named local input uses only hash-verified embedded TZDB 2026c bytes. A bounded
canonical source envelope preserves the original token, interpretation, precision, uncertainty and
accepted pair. Graph transactions journal that complete value, so replay restores the pair without
parsing text or resolving a zone. Commit revision remains authoritative when wall observations are
equal or move backward.

Decision 0023 adds `uste-ingest` as the sole composite graph/spatial/import reducer. Finalized
source and mapping blobs are immutable inputs, while one unpublished candidate owns graph changes,
optional spatial changes and private job-ledger advancement. Authorization requirements cover the
namespace, job, bindings, graph operations and every spatial external reference. Full retained
spatial closure is rechecked after graph-only changes. Composite checkpoints are bounded verified
caches; the encrypted journal and coordinator retry identity remain authoritative. CSV/JSON parsing
and mapping execution stay outside this capability-free reducer until T-54.

Begin with an append journal and rebuildable reference indexes. Decision 0025 fixes the first
immutable disk-index run, bounded-cache and versioned derived-root profile; T-35 adds atomic
compaction and authoritative baselines. No other database engine is silently introduced. Hot graph
adjacency and cold temporal history require separate access paths, not a time-partition-only layout.

Avoid two independent sources of commit truth. The journal owns commits; projections and
worker queues derive from it. A checkpoint is a cache until explicitly promoted to a new
recovery baseline during retention/compaction.

Decisions 0014/0015 make filesystem, clock and randomness explicit capabilities and bind the
first Linux implementation to `openat2`, no-replace rename, explicit flushes and unique-descriptor
ownership locks. Storage paths are
single validated names relative to opaque directory handles; positional I/O exposes short progress,
and file/directory flushes and no-replace rename remain separate observable operations. The T-12
memory/fault adapter remains a correctness harness; the Linux implementation is exercised on the
Btrfs reference runner and an independently identified local ext4 mount.

## Security scope

Core storage/query code is safe Rust by default. Standard-library/platform boundaries,
cryptographic implementations, native build dependencies, and optional workers are separately
inventoried. “No C/C++ database engine” is not “no C anywhere in the operating system.”
The safe-Rust `uste-crypto` boundary implements Decision 0013's versioned envelope admission,
key derivation, padding, nonce-session and adapter contracts around pinned RustCrypto primitives;
it is not itself a key store, clone detector or rotation coordinator.
No mandatory C/C++ parser or model runtime is hidden behind Rust bindings. The strict engine
requires Rust storage, graph, spatial, query and physics implementations, including algorithmic
dependencies. A native physics/GIS engine behind bindings does not satisfy that profile.

Strict-local is the default: no network egress, no telemetry, no automatic model download.
An optional external worker profile requires explicit operator activation and must not be
represented as meeting a stricter dependency profile. Engine authenticity and privacy do not
imply immunity to a compromised host or correctness of the underlying evidence.

## R0 decisions and development gate

| ID | Required decision and evidence | Owner of contract |
|---|---|---|
| D-01 | Journal/commit-root protocol, disk-index layout, locking, Linux/filesystem support, power-loss assumptions; demonstrate torn metadata cannot silently roll back acknowledged commits | Storage |
| D-02 | Cryptographic suite/library, key store, nonce uniqueness across crashes/backups/clones, metadata leakage, rotation, key-loss and rollback limits | Security |
| D-03 | Exact raw upload, query, graph, parser, branch, cache, and history limits; hardware and performance budgets | Verification |
| D-04 | Retention epochs, purge/backup interaction, holds, deduplication boundary, branch pins, historical availability | Security + storage |
| D-05 | Required parser/model/decoder candidates by format, licenses, native dependencies, sandbox controls, coverage and strict-profile feasibility | Content |
| D-06 | Commit/serialization/schema version compatibility, idempotency lifetime, baseline migration, read/write preconditions, and time codec/ranges/calendar/precision/leap handling with pinned local timezone profiles | Data + storage + [time](docs/time-and-ordering.md) |
| D-07 | Development governance: disclosure-route selection, maintainer/reviewer responsibilities, release signing, dependency admission and support policy; operational route verification is distribution task T-62 | Contributing + security policy |
| D-08 | Spatial types, geographic/local frame definitions, transforms, numeric tolerances, predicates/boundaries, disk index design, navigation costs and reference vectors | [Space](docs/spatial-world-model.md) |
| D-09 | Rust physics dependency feasibility, arithmetic/integrator/contact profile, supported shapes/forces, simulation-to-UTC mapping, budgets and deterministic restart vectors | [Physics](docs/physics-and-motion.md) |

Decisions 0003–0010 close the technical profiles, and Decision 0011 closes the development
governance split while retaining operational disclosure verification as open T-62. A choice may
be revisited only through a versioned decision and applicable compatibility/migration tests.
R0 permits implementation to begin; it does not establish a working production format,
measured capacity, distribution readiness or release qualification.

## References and authority

[PRD](PRD.md) owns scope; the focused specifications in the [README](README.md) own details.
[Decision 0001](docs/decisions/0001-product-direction.md) supersedes the historical kernel
plan; [Decision 0002](docs/decisions/0002-spatial-world-model.md) expands spatial/physics scope.
Established property graphs, temporal partitioning, and deterministic fault testing
are inspirations, not claims of implementation equivalence or new invention.
