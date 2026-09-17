# Decision 0017 — Blob chunks, inventories and resumable upload state

Date: 2026-09-17

Status: accepted for the T-15 foundation. This refines Decisions 0004, 0005, 0007, 0015 and
0016 without defining artifact graph records, authorization policy, quota accounting or orphan
collection.

## Ownership and publication

The journal owner also owns blob encryption, staging and publication. It does not expose the
database key or an ambient path. A caller receives an opaque upload token containing its
database/namespace scope and random 128-bit upload identity. The blob identity is the first 128
bits of SHA-256 over `USTE-BLOB-ID-V1\0`, the database ID, namespace ID and upload identity. This
one-way domain derivation prevents a different upload identity from deliberately minting alternate
valid ciphertext under an existing blob/chunk context. A token is an unforgeability aid, not
authorization; T-16 must authorize every upload, resume, finish and read.

Input is accumulated in at most one 1 MiB plaintext buffer. Complete chunks are encrypted with the
namespace, blob identity and zero-based chunk number in `crypto-v1` context, written to a private
staging name, data-synced and directory-synced. A staging name is immutable: an existing object is
accepted only after authentication and exact plaintext comparison. An uncertain flush quarantines
that live handle; the caller must resume from its durable byte offset rather than resend against
the old handle. The 16 GiB single-blob cap therefore admits at most 16,384 chunks. Zero-byte blobs
have zero chunks and the SHA-256 digest of empty input.

Finish flushes a final partial chunk, changes every staging name to an immutable random-blob name
without replacement, then syncs the database directory. It then publishes an authenticated,
data/directory-synced final marker binding the upload, blob, scope, byte length, chunk count and
content digest. Abort similarly publishes an authenticated terminal marker after staging removal.
These markers make zero-byte finalization and acknowledged abort durable across restart. A failed
or interrupted finish is resumable: authenticated staging and final chunks are scanned in sequence,
and a partial staging chunk that preceded finalization remains durable and is treated as the sealed
terminal chunk. Repeated resume without finish therefore cannot shorten accepted input. Once any
immutable final name, partial terminal chunk or final marker is observed, further writes are rejected
and only finish may continue. Abort removes only known unsealed staging chunks and syncs their
directory. Durable finalized orphans are never exposed and remain T-35 garbage-collection input.

This decision assigns `crypto-v1` roles `0A` to `BlobInventory`, `0B` to `BlobManifest` and `0C`
to `BlobInventoryName`. Role `0C` is used only by the vault's separate HKDF public-token domain; its
32-byte result is an opaque filename token and never an encryption key. This refines Decision 0013's
assigned-role registry without exposing the logical inventory digest in a directory entry.
The compatibility-controlled HKDF expand input is, in order, the literal
`USTE crypto-v1 public opaque token`, the 88-byte canonical `crypto-v1` context with database scope,
role `0C`, zero object/sequence, current epoch/writer, format 1.0 and the 4 KiB frame tag, followed
by the complete 32-byte inventory digest. The existing Decision 0013 extract salt and database
master key remain the HKDF salt/input key material. The fixed-master test vector pins the result.

The authenticated `UBMF` 1.0 marker plaintext is exactly 128 bytes:

| Offset | Bytes | Meaning |
|---:|---:|---|
| 0 | 4 | `UBMF` |
| 4 | 2 | major/minor `01 00` |
| 6 | 1 | state: final `01`, aborted `02` |
| 7 | 1 | reserved zero |
| 8 | 16 | namespace ID |
| 24 | 16 | upload ID |
| 40 | 16 | derived blob ID |
| 56 | 8 | final byte length, big-endian; zero for abort |
| 64 | 4 | final chunk count; zero for abort |
| 68 | 4 | reserved zeros |
| 72 | 32 | original-byte SHA-256; zeros for abort |
| 104 | 24 | reserved zeros |

## Canonical inventory and commit binding

A `UBIN` 1.0 inventory contains one namespace and up to 100,000 blob references sorted strictly by
random blob identity. Each reference fixes the blob identity, exact byte length, chunk count and
SHA-256 of the original bytes. The 32-byte header is followed by 64-byte entries:

| Offset | Bytes | Meaning |
|---:|---:|---|
| 0 | 4 | `UBIN` |
| 4 | 4 | major/minor `01 00`, two reserved zeros |
| 8 | 16 | namespace ID |
| 24 | 4 | reference count, big-endian |
| 28 | 4 | reserved zeros |
| 32 + n*64 | 16 | blob ID |
| +16 | 8 | exact byte length |
| +24 | 4 | chunk count |
| +28 | 4 | reserved zeros |
| +32 | 32 | original-byte SHA-256 |

The inventory identity is SHA-256 of those canonical bytes. The empty inventory retains the
existing SHA-256-of-empty constant and has no inventory object. A nonempty inventory is encrypted
under the database-wide `BlobInventory` role and stored under its key-derived opaque name token.
The object and containing directory are synced before the transaction group begins. Before doing
so, the owner authenticates and rehashes every named immutable blob.

The commit certificate binds the inventory digest. Recovery resolves that digest, authenticates
and decodes the canonical inventory, verifies every named chunk and complete original-byte digest,
and only then exposes any logical replay callback. Missing, malformed or corrupt committed
inventory/blob data is `IntegrityFailure`, never an older-frontier fallback. Reads require an exact
reference reconstructed from a committed inventory, so finalized but uncommitted blobs remain
invisible.

The profile admits at most 100,000 references in one inventory, 1,000,000 unique committed blob
references per journal, 10,000,000 certificate-to-blob reference bindings per journal and 1 TiB of
logical committed blob bytes per namespace. Commit admission and both recovery passes enforce the
same limits. Recovery retains only the bounded unique-reference index and a digest-to-count map;
the validation pass authenticates and rehashes each distinct inventory/blob set once, while replay
decodes one inventory at a time and does not retain all inventory bodies.

For transactions with blobs, the idempotency request digest is SHA-256 over the literal domain
`USTE transaction request+blob inventory v1`, the big-endian request length, exact canonical
request bytes and inventory digest. Empty-inventory transactions retain the format-1.0 digest of
the exact request alone. The reducer receives the verified inventory together with request bytes;
later domain reducers must ensure their semantic blob references match it.

## Current qualification boundary

The foundation covers arbitrary multi-chunk and zero-byte round trips, irregular input slices,
commit-gated reads, restart/replay verification, exact retry, interrupted upload resume, uncertain
flush quarantine, immutable duplicate-handle behavior, repeated resume of a durable terminal chunk,
durable zero-byte final/abort markers and hard recovery failure for a missing committed chunk. The
literal `blob-inventory-v1.hex` file fixes the canonical inventory format. T-15 remains open for its
full chunk/finalization/inventory fault matrix, malformed committed inventory cases, retry-binding
negative cases and measured bounded-memory evidence.
