# Decision 0098 — Journal-derived storage blob disk catalog

Date: 2026-09-19

Status: locally verified T-20 increment. Cold recovery/append integration remains open.

## Contract

Keep the journal authoritative. Add an encrypted `index-v1` derived catalog with index and reducer
profile SHA-256(`USTE storage-blob-meta-v1`), hex
`112c37592fdf28c79c14390162887c8d9dd411a776574dd1af78665ebc33e37a`.
Its internal carrier is the database's all-zero namespace, separated by profile. This neither
reserves that namespace for users nor grants principal authority. Coordinator principal ownership
and authorized quota responses remain separate from storage's exact historical accounting.

All integer fields below are unsigned big-endian. Reserved bytes must be zero. Family 1 always
exists; other families exist exactly when their corresponding count is nonzero. No tombstones.

| Family | Key | Value |
| --- | --- | --- |
| 1 metadata | literal `storage-blob-meta-v1` (20 bytes) | 48 bytes: `SBMD`, version bytes `01 00 00 00`, blob/namespace/inventory/reference-binding counts (four u64), eight reserved bytes |
| 2 references | namespace (16), blob ID (16) | 56 bytes: first revision u64, logical byte length u64, chunks u32, four reserved bytes, content digest (32) |
| 3 namespaces | namespace (16) | exact unique committed logical blob bytes u64, including zero for an existing zero-byte-only namespace |
| 4 inventories | inventory digest (32) | reference count u32, four reserved bytes, first revision u64 |

The state digest hashes `USTE-STORAGE-BLOB-STATE-V1\0`, root revision u64, the 48-byte metadata
value, then each ascending family's byte tag, entry count u64 and logical run digest. Existing
index run/root encryption, certificate binding and durability are unchanged. The catalog cannot
make an uncommitted upload readable. Existing journal blob, binding and namespace limits remain.

## Bounded construction and admission

Rebuild fixes the current certified frontier and streams its prefix one group at a time. Retain
one format-bounded inventory and its bounded per-inventory new-reference deltas, four run
descriptors, counters and a caller-bounded page cache, not a whole-history comparator map.
The range callback temporarily clones the current inventory to release the journal read borrow
before writing scratch runs; both bounded copies coexist until that callback returns. This is
not a claim of one-inventory peak allocation or a measured RSS bound for maximum-size inventories.
Scratch roots remain unpublished until the terminal revision. Exact reference collisions fail;
repeated inventories increment reference bindings without double-charging unique blob bytes.
The first revision never changes. Empty journals require no catalog; an existing all-empty
certified prefix produces a metadata-only catalog.

Index-v1 requires every run's revision to equal its root. Therefore each staging step rewrites
even unchanged nonempty families through a bounded merge. All rewritten logical output bytes
are charged to the rebuild's aggregate allowance before each write. Per-merge base, delta and
output limits remain mandatory. Certificate proofs and one-group range reads can reread the
suffix to the pinned frontier; their actual work is charged/reported, not assumed linear.
Full-run write amplification and retained orphan runs remain limitations, not qualification.
T-35 owns reclamation; no failed/redundant scratch runs are incidentally deleted.

Before publication, independent admission authenticates the exact certificate and fully scans
all families under explicit run limits. It validates closed codec shapes, exact family/count
structure, state digest and namespace totals using one running namespace sum. A separately
bounded full journal-prefix pass checks every inventory digest/count and exact reference against
disk lookups. Every first revision must be no later than the current group. Count matches at the
claimed first revision and require those totals to equal catalog cardinalities. Canonical unique
references per inventory and one group per revision make this an exact earliest-occurrence and
no-extra-entry check without reconstructing a history map. Only then publish the terminal root.

Admission reports separate certificate bytes, range bytes, full-run pages/entries and point-read
work. Rebuild additionally reports its discovery range, staging certificate proofs, merge base
pages, logical output bytes, point lookups and independent admission. These are not device I/O
or total filesystem accounting: bounded inventory/header/payload work follows the existing
range/open contracts; root publication/fallback inspection also has separate existing bounds.
The same configured range allowance applies independently to rebuild discovery and admission;
it is not a single budget shared across those passes. No small process cap qualifies BM-01/BM-06.

Admitted bases carry live-owner certificate binding, including in legacy certificate-map mode.
Trusted exact reference and historical namespace-byte queries use disk metadata and reject
foreign databases or stale live-owner handles. They do not grant consumer ReadBlob permission.

## Remaining boundary

This increment does not remove the legacy resident blob/inventory/namespace collections from
journal open or append. Tests clearing those collections after real disk-certificate open isolate
the new rebuild/admission/read capability; they are not map-free cold-open evidence. Integrating
the admitted base, bounded suffix and append overlays, plus inventory-domain authorization and
certified quota transfer, remains dependency-permitted T-20 work. M1's pinned consumer interface
is unchanged. Larger-than-memory recovery and qualifying BM-01/BM-06 remain open.

## Local verification scope

Synthetic fixtures exercise zero-byte blobs, an ordinary blob in the all-zero user namespace,
repeated inventories, exact first revisions, cold owner rebinding, empty catalogs, explicit count/
group/range/output refusals and exact output-budget success. Closed codec/profile vectors and
five authentically re-encrypted false catalogs check earliest revision, namespace bytes, inventory
counts, binding totals and a substituted reference ID. Corrupt terminal certificates and a later
inventory fail admission.

The rebuild mutation sweep covers every observed CreateNew (21), WriteAt (22), SyncAll (21),
SetLen (21), SyncDirectory (22) and RemoveFile (1) boundary: 108 boundaries, 324 attempts across
I/O error and crash-before/after. All restart roots admit exactly; 323 attempts report failure.
The one successful RemoveFile/1/CrashAfter attempt is the existing optional absent-file cleanup
case, not an actual crash. SyncData and RenameNoReplace have zero observed calls in this path.
Admission additionally fails closed at the first/middle/last OpenExisting, Metadata and ReadAt
boundaries (27 attempts); that is selected read-boundary coverage, not an exhaustive read sweep.
Existing index/storage fault suites remain required. Exact final gate results are in PROGRESS.
