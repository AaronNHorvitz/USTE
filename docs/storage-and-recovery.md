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

Decision 0118 adds an opt-in origin-reconstruction building block when revision one is retained:
reauthenticate/reduce only that bounded inventory-free transaction, stage unpublished graph and
coordinator candidates, independently admit them, and stream the admitted suffix. Only terminal
roots become discoverable; private metadata requires durable rebase even with an empty suffix.
It does not support an erased authoritative prefix, lift suffix-overlay limits, automatically
enable native cache-loss fallback or qualify larger-than-memory recovery.
Decisions 0119–0120 connect explicit native inventory-free origin rebuild to per-revision private
coordinator staging with zero cumulative outcome overlays. This requires paired domain/metadata
bases and refuses inventories/optional owner projections; existing general recovery remains
unchanged. Whole-family rewrite costs and qualifying recovery campaigns remain open.
Decision 0121 adds a separate inventory-bearing primary-metadata streaming API. It retains only
one inventory's new-owner deltas, preserves first principals through disk lookups and stages the
owner family at every step. Attached first-reference/quota projections explicitly refuse on this
path; ordinary general recovery remains available. Decision 0122 adds bounded inventory-bearing
genesis reconstruction and private primary owner candidates, completing this primary-only origin
path with independent admission. Optional projection maintenance and scaling remain open.
Decision 0123 adds explicit first-reference-preserving streaming recovery and private genesis
witness staging. Earliest claims are retained with only current-inventory deltas; populated bases
without admitted witnesses refuse. Quota-preserving streaming and rewrite/accounting scale remain open.
Decision 0125 selects private metadata streaming for ordinary native/model paired graph/metadata
recovery as well as explicit origin rebuild. Mismatched bases retain bounded suffix overlays;
diagnostics distinguish recovery overlay capacity from the retained capacity for future writes.
Decision 0126 retains each forward cursor transaction's already-accounted owner-bound certificate
proof for private staging. Invalid bound evidence cannot fall back; unbound transactions retain
their existing proof path. Fresh recovery still reauthenticates, and triangular forward proof
work and immutable-family rewrites remain limitations.
Decision 0127 adds opt-in forward certificate windows of at most 64 owner-bound receipts and uses
them for private metadata recovery. Window acquisition and selected group rereads debit one shared
range allowance; lookahead never bypasses selected-certificate, group or inventory authentication.
The bounded batching reduces but does not eliminate worst-case quadratic certificate work.
Decision 0124 adds explicit quota-preserving streaming and private genesis quota candidates, with
exact first-owner charges and mandatory independent initial quota admission. Private optional roots
also require rebase when primary roots are already published. Rewrite/proof amplification and
complete accounting/qualification remain open; the paired-base paths do not remove every limit.

## Indexing and bounded resources

Decision 0129 adds a distinct encrypted packed-page framing carrier for future copy-on-write
records: fixed 16 KiB zeroizing plaintext buffers, at most 128 nonempty records, strict slot and
padding checks, and a separate object-format cryptographic context. Framing alone does not validate
tree nodes, authorize access, publish a root or provide crash recovery. Existing v1 paths are unchanged.
Decision 0130 connects it to bounded create-new immutable pack writes and exact-context reads.
Successful finish requires file and directory sync; append failures poison the writer and leave
only unreferenced staging, with no root publication. Its deterministic restart/fault coverage is
not yet Linux process qualification, typed tree validation or a cold-admitted graph profile.
Decision 0131 supplies the closed node/value-chunk grammar and bounded imported-summary checks.
Typed parsing recomputes a node's logical commitment but does not prove referenced contents or
canonical partitioning; traversal must compare each child and the complete recovered value with
their expected commitments before declaring success.
Decision 0132 connects those checks in a bounded raw packed-tree lookup: exact parent commitments,
terminal key-route proof, canonical chunk lengths and complete value hash before returning bytes.
Its caller still requires independent canonical-root admission and authorization. Linked reads
check bounded integral pack geometry without a resident pack catalog; descriptor reads retain
their exact-length requirement. This is not yet a graph/coordinator profile or root-publication path.
Decision 0133 adds private copy-on-write batches over those trees: sorted exact deltas, bounded
arena/path/read reservations, before-value proof checks before output creation, unchanged-subtree
reuse and iterative final-only serialization. The returned staged root is not a journal commit or
independently admitted manifest; integration and recovery still have to establish that authority.
Decision 0134 adds complete bounded structural/content validation: iterative ordered traversal,
canonical subtree-boundary checks and incremental value hashing. Its receipt records a successful
read of every reachable node/chunk, not domain semantics, journal authority or protection against
subsequent mutation; later operations must authenticate their own reads.
Decision 0135 adds a bounded in-process range cursor over independently admitted canonical roots,
including compressed-prefix lower-bound seek and ordered successors. It returns one completely
verified zeroizing entry per step, accounts cumulative work, and permanently refuses continuation
after errors. It does not grant consumer authorization or define a serialized resume token.
Decision 0136 frames separate encrypted packed-root manifests with at most sixteen sorted family
commitments/locators and an explicit state-commitment profile. Exact certificate and reducer claims
are carried but not admitted by the codec. No frozen v1 state digest is reinterpreted, and no new
publication/discovery or commit authority follows from a successful manifest decode.

Decision 0128 introduces a separate ordered logical-commitment primitive for future copy-on-write
indexes: context-separated canonical Patricia-tree hashes, bounded lookup proofs and storage-free
compare-and-swap deltas. It changes no v1 digest or persisted format and is not a live disk index
or a canonical-state admission shortcut. Versioned encrypted carrier and domain integration remain
required before it can address the whole-family rewrite bottleneck.

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

Decision 0086 replaces linear eviction selection with an ordered recency index. A nonempty cache
charges an 8 KiB fixed allowance plus 16 KiB plaintext and 1 KiB metadata per entry; these are
logical admission allowances, not measured allocator/RSS bounds. The minimum constructor budget is
25,600 bytes and the unchanged 64 MiB default admits 3,854 pages. Both cache maps remain bounded
to resident entry count. Current maintenance authority is required for the cached primitive work
reports added by Decision 0085; these include work before errors but exclude uncached cursors,
scrubs, publication and pre-primitive checks.

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
only one 1 MiB plaintext chunk of the new payload. A terminal manifest appears only after exact
byte production; producer failure or a short/long stream leaves no visible cache candidate.
Prefix-index consumers can likewise visit entries without collecting the full bounded result.

The follow-on streaming recovery transport authenticates candidate manifests and rehashes their
encrypted chunks without a complete plaintext allocation. Opaque candidates are filtered against
the authenticated certificate chain; selected bytes are revalidated and emitted while the journal
owner/key context remains live. Consumers must discard partial decoded state if the final digest or
sink fails. The legacy collecting API remains available. Current reducer decoders still materialize
complete logical state. Decision 0027 removes the graph write-path clone/rebuild by preparing
ordered before/after record deltas and updating derived-index contributions incrementally; ingest
writes and explicit snapshots remain materialized. Decision 0028 adds an incrementally maintained
in-memory target/owner reverse map, so graph delete reads target fanout plus transaction changes
instead of all records. A new versioned state profile must persist that map and remove the remaining
materialization assumptions; frozen `graph-current-v1` is not mutated. BM-06 remains unqualified.

Decision 0029 adds a separate `graph-state-v1` optional root containing metadata, current records,
record history, adjacency, provenance, reverse references and policy history. Publication and load
independently reproduce every run digest from the exact live snapshot and bind the root to the
current journal certificate. It is not a recovery seed or second authority; it initially left
bounded full-run reads, scratch merge and root-based reducer reconstruction to later T-20 work.

Decision 0030 adds a privileged full-run visitor with explicit page/entry/logical-byte budgets, one stable
file handle, one-entry assembly, exact length and terminal count/digest checks. Graph-state
candidates use it to reconstruct and semantically verify all families without collecting a run or
building a monolithic checkpoint byte buffer. Candidate discovery currently scrubs first under
absolute carrier maxima; caller `GraphStateLoadLimits` apply only to the subsequent reconstruction
pass. Ordinary `GraphState` remains full-memory, and coordinator metadata is not present in the
root.

Decision 0031 adds a separate `coordinator-meta-v1` root for ordered retry outcomes and first-blob
owners. Recovery requires its complete certificate/state anchor to equal the selected graph root;
a temporary authenticated owner streams both, is dropped, and seeded coordinator open then
reauthenticates and compares the transaction prefix before suffix replay. Prefix verification moves
entries from expected to verified maps instead of duplicating complete maps. Graph/coordinator
state and root discovery remain memory-resident, so this is not the BM-06 endpoint.

Decision 0051 permits that temporary recovery owner, or a live coordinator, to semantically admit
a cold `graph-state-v1` root without rebuilding complete graph maps. It streams all families,
retains at most one bounded history group, resolves references through bounded exact/predecessor
proofs and releases an I/O-capability-free `GraphDiskBase` only after the canonical digest and all
cross-family invariants match. Discovery and initial carrier scrub still use absolute storage
maxima, while coordinator metadata, the live reducer and suffix replay remain memory-resident.

Decision 0052 makes that base the warm reducer representation. A consuming transition verifies it
against the complete reducer and current certificate without replacing the coordinator's journal,
retry or blob-owner state. One proof-derived commit advances journal authority and leaves one
bounded pending plan; the stale base cannot serve reads or another commit. A disjoint maintenance
capability can merge runs and publish a root but cannot append journal data. Only the exact
certificate/count/policy result installs the next base, and failures remain retryable. This state
is intentionally not checkpoint-decodable: a pending crash still requires complete journal replay,
and coordinator metadata/discovery/suffix recovery remain later T-20 work.

Decision 0053 handles the ordinary zero/one-suffix crash window without that checkpoint codec. A
temporary authenticated owner retains only the final decoded group and supports bounded recovery
proof I/O. If the best admitted graph root is at frontier it opens ready; if exactly one revision
behind, the canonical final request deterministically rebuilds the pending plan. A separate final
journal open compares the exact base certificate and captured group, rebuilds coordinator metadata
and revalidates the prepared output before publication. Additional suffixes or an intervening
append fail closed. Coordinator maps and origin replay remain memory-resident; discovery/initial
scrub still use absolute carrier bounds.

Decision 0032 adds an authenticated bounded merge from one optional base run and sorted exact
before/after deltas into one unpublished encrypted run. Present before-values must match byte-for-
byte, absent before-values require absence and absent after-values are tombstones. The merge holds a
stable source handle through exact terminal verification and separately caps source, delta and
output work. Empty output creates no run. Errors can leave only unreferenced scratch bytes; a domain
validator and exact root publication are still required before visibility. `index-v1` remains a
single-run-per-family terminal format rather than a persistent overlay manifest.

Decision 0033 implements that validator for `graph-state-v1`. A no-I/O precommit plan encodes exact
metadata/current/history/adjacency/provenance/reverse/policy changes and binds them to an admitted
base anchor and transaction result digest. Postcommit publication requires the exact outcome,
merge-rewrites all eight families, independently projects the actual current graph, and publishes
only when family presence, counts and logical digests match. A failure leaves the journal commit
intact and at most unreferenced encrypted runs. The projection pass and live reducer remain
full-memory pending the persistent base/overlay design.

Decision 0034 removes retained-state cloning from ordinary graph, spatial and composite ingest
preparation. Spatial and ingest publication plans are request-sized and all component bases are
checked before mutation; formats and journal authority do not change. This does not remove the
full-memory live reducer, checkpoint decoder or complete semantic closure scans. A future
disk-backed preparation view must expose explicit bounded storage I/O rather than hiding reads
inside the current pure `TransactionState::prepare` contract.

Decision 0035 adds a separate privileged loader for supported current-state graph transactions.
It authenticates an admitted root, current policy and only the required positive/negative current
record closure under aggregate proof budgets and a caller-owned page cache. The returned view has
no filesystem or coordinator capability, so its consuming reducer preparation is storage-free.
Deletion and `ReadView` predicates are rejected before storage access pending bounded complete
reverse/history proof APIs. The live reducer and root-delta metadata remain full-memory.

Decision 0036 supplies those proof APIs by collecting complete authenticated family-3 history
prefixes and family-7 reverse buckets under aggregate entry/logical-byte and per-prefix result
limits. They make `ReadView` and deletion safe in the pure partial reducer. Coordinator commit/root
publication and recovered live state remain full-memory boundaries. Decision 0037 authenticates
the admitted root's canonical metadata entry in the same proof, retains its graph-state counters
and policy, and derives the bounded terminal-root delta plan from the storage-free result. The
authoritative coordinator commit originally still repeated full-state preparation. Decision 0038
adds a reducer-verified external-preparation seam: graph checks exact request bytes, revision,
policy version and touched before-values before the normal journal append/publish sequence. The
live publication target, independent root validator and recovery remain full-memory.

Decision 0039 adds cumulative authenticated page/fragment/result-byte accounting to authorized
index reads and a zeroizing userspace-cache clear. Candidate-dependent counters reveal cardinality,
so an issuer-instance-bound root plus current `ManageSchema` authorization gates both report and
clear; outcome-uncertain coordinators reject them before possibly stale policy is used. Clearing
does not evict kernel, filesystem, controller or device caches.

Decision 0040 exercises this path after memory-adapter restart and journal replay: it discovers the
persisted encrypted graph root and runs oracle-checked authorized queries. That establishes bounded
semantic recovery equivalence, not Linux durability, portable recovery cost or BM-01 scale.

Decision 0041 bounds benchmark materialization to the graph transaction maximum and pins the exact
qualifying plan at 212 durable revisions. It does not change the memory-backed live-state or
recovery boundary and therefore supplies no larger-than-memory or platform-durability evidence.

Decision 0042 adds a Linux/Btrfs benchmark owner using OS entropy and portable recovery. Stable
transaction identities allow raw journal replay followed by idempotent policy/data retry, including
interruption before the authorization facade can open. The phase publishes an index only when no
current root is admitted. Complete graph replay/root validation is still a full-memory boundary.

Decision 0043 reuses that recovered authorized view for a separate Linux correctness-query phase.
It admits a bounded external oracle summary, clears the USTE page cache between traversals and
reports authenticated disk/cache work plus process RSS. This does not alter the recovery format or
remove the full-memory graph replay/root-validation boundary; host caches remain uncontrolled.

Decision 0044 adds a benchmark-specific process-loss seam after successful durable commits. A
child flushes a content-free frontier marker and parks while retaining ownership; an external
harness SIGKILLs it and a new process uses ordinary portable recovery/idempotent resume. This does
not alter journal format, certification, recovery rules or consumer APIs.

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
