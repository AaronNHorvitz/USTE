# Storage, transactions, and recovery

Draft contract · 2026-09-17 · T-13–T-18/T-49 journal, transaction, blob, replay and typed-import
foundations are locally qualified

Owns FR-02, FR-03, FR-12, FR-16 and persistent publication rules.

Spatial observations, geometry versions, import checkpoints and physics branch events use
the same transaction protocol. Spatial/history indexes and route caches are derived state,
not extra commit authorities. Multi-body results publish atomically. See
[space](spatial-world-model.md), [physics](physics-and-motion.md) and
[ingestion](ingestion-and-unified-retrieval.md) for dependencies and visibility rules.

An import job checkpoint is logical reducer state (source/mapping binding, cursor, counts and
status) journaled through ordinary transactions. It is distinct from Decision 0020's optional
encrypted reducer-cache checkpoint used to accelerate replay. The cache may contain the logical
job ledger, but never becomes an independent commit or retry authority.

## Physical artifacts

- Versioned manifest: database identity, format/features, cryptographic/profile identifiers.
- Append-only transaction journal in bounded segments.
- Independently published authenticated commit metadata naming the durable revision/root.
- Encrypted immutable blob objects/chunks, including original and derived content.
- Verified checkpoints and optional retained authoritative baselines.
- Rebuildable graph, property, temporal, provenance, and lexical index runs.
- Private staging area and versioned policy/retention metadata.

Private payloads, filenames, hashes that expose equality, and sensitive index keys require
protection per [security](security-and-privacy.md). Fixed framing fields may remain visible
only under the documented metadata-leakage budget. No unbounded allocation follows an
untrusted length field.

## Creation and ownership

Creation builds a private temporary sibling on the same supported filesystem, flushes
required files and directory entries, atomically publishes it, then flushes its parent
before acknowledging success. Failures must not resemble a valid created database.
An exclusive owner lock prevents competing writers. Stale-lock handling must not permit
two owners. Network filesystems are unsupported initially.

Decisions 0014/0015 implement the handle-relative interface, deterministic volatile/durable fault
model and x86_64 Linux adapter. The adapter uses descriptor-rooted `openat2`, no-replace rename,
explicit file/directory syncs and a unique-descriptor nonblocking ownership lock. Current host
evidence covers Btrfs and a separately identified local ext4 mount; the ext-family magic alone is
not accepted as proof that an arbitrary caller-supplied mount is ext4.

## Transaction lifecycle

1. Receive authenticated, bounded request and idempotency key.
2. Stage operations privately; stream blob data into encrypted staging objects.
3. At commit, validate current permissions, schemas, quotas, endpoints, and read/version
   preconditions. Constrained predicates must either be conflict-validated or rejected.
4. Make newly referenced immutable blobs durable, including directory entries.
5. Append the complete atomic transaction group and its idempotency outcome; flush journal.
6. Publish and flush authenticated commit metadata naming that complete group.
7. Publish a coherent in-memory revision/index root, then acknowledge a durable receipt.

Batch commits may share flushes but preserve complete groups and ordered receipts.
There is no acknowledged non-durable write mode in the initial product.
An in-process failure after step 6 is recovered as committed, not reported as rolled back.
An uncertain client outcome returns OutcomeUnknown with a transaction lookup/retry path.
Persisted idempotency keys prevent duplicate effects after lost responses; Decision 0003 fixes
their retention period and maximum retry horizon.

Commit revisions remain authoritative when wall clocks repeat, jump or move backward.
The commit wall observation is sampled at a profile-defined boundary before journal encoding
and is informational, not a claim about the exact flush or acknowledgment instant. Persist
accepted normalized timestamps and their interpretation profiles with the transaction;
recovery must not resample clocks or reinterpret source dates. See [time and ordering](time-and-ordering.md).

Readers see a pinned committed revision, not staging. Current authorization is checked
separately; revocation can invalidate an otherwise valid reader handle. Failed validation
publishes no partial revision. Bounded queues apply backpressure instead of dropping writes.

Decision 0018's authorized facade now performs that separate current-policy check. Consumer commit
requests omit the principal; the facade derives it from a trusted authenticated principal before
calling the raw coordinator. Raw coordinator/storage methods remain privileged recovery and adapter
capabilities. Versioned leases gate every access to a pinned revision and every subsequent upload
operation. The recovered transaction index retains its owner, and the recovered unique-blob set
retains first-commit ownership for per-principal quota reconstruction.

Staged and committed blob quotas count exact logical plaintext octets. A failed write reconciles
the handle's observed accepted-byte delta; finalize does not release staging; accepted abort does;
successful commit moves each unique reference to committed accounting once. T-17 persists graph
namespace policy in the same journal and requires an exact trusted-adapter match on authorized
open. Format 1.0 cannot
enumerate every abandoned upload reservation, so a reopened authorized coordinator denies new
starts while permitting evidenced-token resume/abort; T-35 must replace this conservative rule
with complete reconciliation.

## Commit metadata and corruption

Decision 0004 selects an append-only authenticated commit-certificate log rather than redundant
mutable commit-root slots. A simple
“highest valid slot wins” rule may silently select an older frontier after corruption of a
previously acknowledged newer slot and is therefore not the v1 recovery rule. The selected
certificate chain distinguishes an incomplete final unacknowledged tail under its declared
failure model; complete-certificate damage or missing referenced data fails closed.

Decision 0015 fixes the format-1.0 manifest, authenticated log/segment headers, exact opaque group
envelopes and fixed 4,161-byte encrypted certificates. Decision 0017 extends this with canonical
nonempty encrypted inventories and authenticates and rehashes their exact committed chunks before
logical replay.

Crash safety assumes correctly implemented supported filesystem flush/rename semantics and
storage honoring durability requests. Arbitrary media destruction is not survivable without
backups. Never advertise universal power-loss or rollback protection.

## Recovery

Verify manifest/version/key context, establish the committed root, verify required retained
history and referenced objects, and replay complete groups after an eligible checkpoint.
Required committed corruption or missing committed blobs are hard integrity errors.
Uncommitted journal tails and unreferenced staging objects are recoverable garbage; never
discard data identified as committed merely because a checksum or footer is malformed.

Recovery must not allocate from unchecked lengths, run parsers, call models, or contact
network endpoints. A missing optional search index is rebuilt, not confused with missing
source evidence. Structural validation and authentication precede logical reconstruction.

## Checkpoints and canonical state

A checkpoint records its exact revision, history epoch, object inventory, schema/profile
versions, and logical digest. Construct it from a frozen committed view or retained replay,
then publish using temp/write/flush/rename semantics. Its state must equal reference replay
at the labeled revision. A self-consistent but logically wrong checkpoint must be caught by
independent scrub/replay tests.

An invalid cache checkpoint can be declined only when sufficient retained authority exists
to rebuild. After compaction promotes a checkpoint to an authoritative baseline, its
corruption is not silently recoverable by pretending erased history still exists.
Ciphertext identity is not the canonical logical state digest.

## Indexing and bounded resources

Decision 0025 implements the first `index-v1` slice: immutable sorted runs use exact 16 KiB
authenticated plaintext pages, roots alternate between two key-derived opaque names and every root
is bound to the exact existing certificate, reducer/logical digest and index profile. Root recovery
fully scrubs referenced durable pages before selecting a fallback or overwrite slot. The first
`graph-current-v1` projection provides current record lookup, both adjacency directions and
provenance. It independently compares the exact family set, counts and digests with the frozen graph
snapshot before admission, and rechecks the live journal frontier on every raw read.

The default decrypted-page cache is 64 MiB and rejects budgets over 256 MiB. Cache identity includes
database, namespace, epoch, writer, revision, profile, generation, run and page; diagnostics redact
keys and plaintext, and scrub bypasses cached bytes. Exact layout, scan and object limits are pinned
in Decision 0025 and `acceptance/r1/index-v1.tsv`.

These roots are rebuildable certificate-anchored caches, not committed baselines. Privileged raw
access remains separate; current-graph consumer reads reuse the mandatory policy lease plus
top-level, candidate and embedded-reference checks through a reducer-owned indexed-read trait.
Facade publication and discovery require `ManageSchema`; admitted handles keep cache state opaque,
and mixed-direction scans share the fixed candidate and returned-byte budgets. Property/type and
valid/recorded-time lookup remain T-21/T-25 work; streaming larger-than-memory checkpoint
construction and the BM-01/BM-06 results remain required before T-20 closes. Time-partitioned
history alone is not an adequate graph adjacency index. Queries merge only compatible committed
runs under one revision.

Decision 0026 removes avoidable snapshot clones from replay metadata checks, lets graph checkpoints
flow through a fallible bounded encoder, and publishes their declared-length bytes while retaining
only one 1 MiB plaintext chunk of the new payload. A terminal manifest appears only after exact byte production;
producer failure or a short/long stream leaves no visible cache candidate. Prefix-index consumers
can likewise visit entries without collecting the full bounded result. These are transport and API
foundations: candidate selection/recovery still materializes bounded payloads, and graph/ingest prepare still
clone and rebuild complete in-memory candidates. New versioned state profiles, not a mutation of
frozen `graph-current-v1`, must remove those full-RAM assumptions before BM-06 can qualify.

Bound cache size, merge fan-in, query scratch space, snapshots/reader pins, and compaction
backlog. Include allocator/RSS measurements: logical cache accounting alone is insufficient.
Materialized summaries record covered revisions and invalidation dependencies. Corrections
and late events invalidate affected summaries; stale results are labeled or excluded.

## Blob publication and garbage collection

Blob inventory participates in transaction visibility. A committed version cannot reference
an incomplete upload. One owner process admits at most 32 live upload buffers per database.
Each encrypted chunk is synchronized under an upload-private temporary name and renamed without
replacement to immutable staging before its durable offset advances. Resume discards temporary
objects but authenticates canonical staging and its per-chunk progress witness, and never silently
replaces malformed acknowledged state. Paired terminal witnesses can repair one missing final/abort
copy while malformed canonical state fails closed. Abort synchronizes both terminal copies before
best-effort staging cleanup, so an accepted
abort remains terminal even if cleanup must be retried or deferred to orphan collection.
Orphan durable blobs after a failed metadata commit are collected only
after checking committed roots, readers, branches, derivations, retention policies, and
active uploads. Quota reservations are released on bounded cleanup paths.
Physical deduplication cannot cross namespaces by default or reveal another namespace's data.

## Compaction, history epochs, and restore

Compaction constructs and verifies new objects/indexes/baseline, durably publishes a new
root, then reclaims old objects when pins and retention permit. Crashes at every stage must
select a complete old or new root. Purge may invalidate branches/pins explicitly; an
indefinite snapshot pin cannot secretly defeat deletion.

Pruning history establishes a declared earliest retained revision and, when needed, a new
authoritative baseline. Queries before it fail explicitly. Logical equivalence is checked
for retained, permitted state; erased payloads are not retained merely to preserve old hashes.

Backups capture one consistent root plus all referenced objects, authenticated metadata,
format identities, and deletion epoch. Restore validates into a new location; never overwrite
the only copy. Apply the current deletion ledger before making restored records accessible.
A stale backup without a trusted current deletion epoch must not be silently admitted as a
current database. Offline stale copies outside engine control remain a documented limitation.

Migration produces a new verified format with rollback to the untouched old copy where policy
permits. Version upgrades cannot silently reinterpret event meanings or retain erased data
in rollback copies. Test interrupted upgrade, wrong keys, missing files, and downgrade refusal.
