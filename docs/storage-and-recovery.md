# Storage, transactions, and recovery

Draft contract · 2026-09-17 · T-13/T-14 journal and transactions implemented; T-15 blob
qualification in progress

Owns FR-02, FR-03, FR-12, FR-16 and persistent publication rules.

Spatial observations, geometry versions, import checkpoints and physics branch events use
the same transaction protocol. Spatial/history indexes and route caches are derived state,
not extra commit authorities. Multi-body results publish atomically. See
[space](spatial-world-model.md), [physics](physics-and-motion.md) and
[ingestion](ingestion-and-unified-retrieval.md) for dependencies and visibility rules.

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

## Commit metadata and corruption

Decision 0004 selects an append-only authenticated commit-certificate log rather than redundant
mutable commit-root slots. A simple
“highest valid slot wins” rule may silently select an older frontier after corruption of a
previously acknowledged newer slot and is therefore not the v1 recovery rule. The selected
certificate chain distinguishes an incomplete final unacknowledged tail under its declared
failure model; complete-certificate damage or missing referenced data fails closed.

Decision 0015 fixes the format-1.0 manifest, authenticated log/segment headers, exact opaque group
envelopes and fixed 4,161-byte encrypted certificates. T-13 currently accepts only the canonical
empty blob inventory; T-15 adds nonempty inventory publication and verification.

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

Disk indexes provide entity lookup, both adjacency directions, property/type lookup,
valid/recorded-time filters, and provenance dependency lookup. Time-partitioned history alone
is not an adequate graph adjacency index. Each index run carries schema and revision coverage.
Queries merge only compatible committed runs under one revision.

Bound cache size, merge fan-in, query scratch space, snapshots/reader pins, and compaction
backlog. Include allocator/RSS measurements: logical cache accounting alone is insufficient.
Materialized summaries record covered revisions and invalidation dependencies. Corrections
and late events invalidate affected summaries; stale results are labeled or excluded.

## Blob publication and garbage collection

Blob inventory participates in transaction visibility. A committed version cannot reference
an incomplete upload. Orphan durable blobs after a failed metadata commit are collected only
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
