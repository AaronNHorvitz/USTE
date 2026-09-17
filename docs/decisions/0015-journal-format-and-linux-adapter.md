# Decision 0015 — Journal format and Linux adapter profile

Date: 2026-09-17

Status: accepted for storage format 1.0 and the T-13 implementation. This refines
[Decisions 0004](0004-storage-publication-and-recovery.md),
[0013](0013-crypto-envelope-and-key-interface.md) and
[0014](0014-io-capability-and-fault-profile.md). It does not qualify ext4, controller power loss,
nonempty blob inventories or whole-state rollback detection.

## Linux capability boundary

The production adapter is limited to x86_64 Linux and receives an already-open root-directory
descriptor as its authority. It is pinned to `rustix 1.1.5` with only `std` and `fs`; normal builds
use its direct Linux syscall backend and introduce no native `links` package. First-party storage
remains `unsafe_code = forbid`; rustix and its Linux syscall/buffer boundary are explicitly trusted
and reviewed rather than described as safe by the lint.

The adapter requires Linux 5.6 or newer `openat2`. Child opens use `BENEATH`, `NO_SYMLINKS`,
`NO_MAGICLINKS` and `NO_XDEV`, plus `O_NOFOLLOW|O_CLOEXEC`; regular files also use `O_NONBLOCK` so
a substituted FIFO cannot hang the process. New directories and files use 0700 and 0600. Object
types are checked after open. No-replace publication uses `renameat2(RENAME_NOREPLACE)` with no
check/rename fallback. Positional I/O uses `pread`/`pwrite`, data publication uses `fdatasync`, full
file and directory publication uses `fsync`, and recovery truncation uses `ftruncate`.

An ownership guard uniquely owns a separately opened `LOCK` descriptor and holds nonblocking
exclusive `flock`. It is not cloneable; close, drop or process exit releases it. Missing `LOCK` is
not recreated during open, and diagnostic contents never authorize stealing a live lock.
The default constructor admits only the currently evidenced Btrfs magic. A separately named ext4-candidate
constructor requires the ext-family magic and an explicit caller profile. That magic cannot
distinguish ext2, ext3 and ext4, so the caller must independently establish the mount type and a
recorded mount/device trial remains required before claiming ext4 support. The supplied root is
duplicated with close-on-exec before use. Current evidence exercises only the Btrfs reference
runner; full qualification is still pending.

## Format 1.0 layout and contexts

Public names are `LOCK`, `KEY`, `MANIFEST`, `CERTIFICATES` and random
`j-<32-lowercase-hex>` journal segments. Logical record names and hashes are not exposed in paths.
`KEY` is a 1-through-65,536-byte encoding owned by the selected trusted key adapter. Storage accepts adapter
envelopes through an explicit durable-envelope interface; the supported operator implementation is
Decision 0013's portable recovery envelope.

All following objects use database scope, storage format 1.0 and small 4 KiB encrypted frames.
One small encoded envelope is exactly 4,161 bytes. Decision 0013 role `09` is assigned to creation
manifests. Its bootstrap context uses the expected database ID and public key epoch, with zero
object ID, sequence and writer incarnation, avoiding circular dependence on encrypted manifest
fields.

The encrypted 128-byte `UMAN` manifest contains format and crypto versions, database ID, nonzero
writer incarnation, key epoch, certificate-log ID, initial segment ID, profile/suite/frame tags,
the 256 MiB segment cap, 4,161-byte certificate size and 16 MiB exact group cap. All reserved bytes
are zero. Duplicate public/context fields must agree after authentication.

| UMAN offset | Bytes | Meaning |
|---:|---:|---|
| 0 | 4 | `UMAN` |
| 4 | 4 | storage major/minor `01 00`, crypto major/minor `01 00` |
| 8 | 16 | database ID |
| 24 | 16 | initial writer-incarnation ID |
| 40 | 8 | key epoch, unsigned big endian |
| 48 | 16 | certificate-log ID |
| 64 | 16 | initial journal-segment ID |
| 80 | 4 | storage profile `01`, crypto suite `01`, frame `01`, zero flags |
| 84 | 8 | segment cap `268435456` |
| 92 | 8 | certificate record bytes `4161` |
| 100 | 8 | exact group cap `16777216` |
| 108 | 20 | reserved zeros |

`CERTIFICATES` starts with an encrypted `UCLG` header authenticating database, log and writer IDs.
Every segment starts with an encrypted `USEG` header authenticating database, segment and writer
IDs, previous segment ID and first revision. Transaction groups are the caller's exact opaque bytes
encrypted under the segment ID and global revision; storage adds no inner transaction wrapper.

`UCLG` is 64 bytes: magic at 0, major/minor at 4/5, two reserved zeros at 6, database ID at 8,
log ID at 24, writer ID at 40 and eight reserved zeros at 56. `USEG` is 80 bytes: magic at 0,
major/minor at 4/5, two reserved zeros at 6, database/segment/writer/previous-segment IDs at
8/24/40/56 and unsigned-big-endian first revision at 72.

Each commit certificate is a fixed 192-byte `UCER` plaintext in one 4,161-byte envelope. It contains
the contiguous revision, SHA-256 of the complete previous encoded certificate, segment ID, group
sequence/offset/encoded length, SHA-256 of the complete encoded group, canonical blob-inventory
digest and caller-supplied canonical logical-event digest. Hashes cover encoded ciphertext bytes.
T-13 accepts only SHA-256 of the empty blob inventory; T-15 must add durable blob verification
before nonempty inventories are admitted. The certificate log is capped at 1 GiB in this slice;
reaching its 258,047-certificate capacity fails before group publication. T-35 owns certificate-log
rollover and storage-orphan reclamation before this operational limit can be raised.

| UCER offset | Bytes | Meaning |
|---:|---:|---|
| 0 | 4 | `UCER` |
| 4 | 4 | major/minor `01 00`, flags/reserved zeros |
| 8 | 8 | contiguous nonzero revision |
| 16 | 32 | SHA-256 of previous encoded certificate, zero for revision 1 |
| 48 | 16 | journal segment ID |
| 64 | 8 | group sequence, equal to revision |
| 72 | 8 | group file offset |
| 80 | 8 | complete encoded group length |
| 88 | 32 | SHA-256 of complete encoded group |
| 120 | 32 | canonical blob-inventory digest |
| 152 | 32 | canonical logical-event digest |
| 184 | 8 | reserved zeros |

`acceptance/r1/journal-v1.tsv` pins SHA-256 goldens for all four plaintext structures. The maximum
16 MiB exact group encodes to at most 16,781,377 bytes because Decision 0013 authenticates an inner
eight-byte length and pads to the next 4 KiB frame.

## Publication and recovery

Creation writes a random hidden sibling, creates and locks `LOCK`, writes and fully syncs `KEY`,
`MANIFEST`, the certificate header and initial segment header, syncs the temporary directory,
renames without replacement and syncs the parent. Success is returned only after the parent sync.

A commit encrypts a bounded group, creates/syncs a new segment and directory entry if required,
writes the complete group, data-syncs the segment, writes the fixed certificate and data-syncs the
certificate log. Only then does the in-memory frontier advance. Any write or sync uncertainty after
publication begins poisons that writer; it cannot append again until close and recovery resolve the
durable frontier.

Recovery holds `LOCK`, authenticates all bootstrap objects, and validates the complete certificate
chain and all referenced groups before invoking a replay visitor. A second bounded pass streams
each group with its authenticated revision, blob-inventory digest and logical-event digest. A final
fragment smaller than one certificate slot is unacknowledged and is
truncated and synced. Complete malformed certificates—including the last—are hard integrity
failures. A complete certificate naming missing, short, mislocated or corrupt data is also a hard
failure; recovery never chooses a convenient older prefix. Bytes after the certified end of every
referenced segment are uncommitted, are reported, truncated and synced before the writer resumes.

Deleting an exact committed certificate suffix or rolling back the complete directory remains
undetectable without an external anchor, as Decisions 0004/0005 already state. SIGKILL tests prove
process-loss wiring, not controller power-loss behavior.

Failed creation and rollover can leave hidden siblings or unreferenced random segments. They are
never treated as committed, but the current capability interface cannot enumerate/reclaim them;
repeated failures can therefore accumulate storage garbage. T-35 owns bounded enumeration and safe
reclamation after checking committed roots. Writer-incarnation rotation and writable-clone/restore
admission remain T-35/T-36 lifecycle work rather than claims of this immutable creation manifest.

## Acceptance and remaining qualification

`acceptance/r1/journal-v1.tsv` pins the implemented layout and recovery invariants. Deterministic
tests cover every initial creation/commit and rollover crash-before/crash-after operation boundary,
every initial non-crash creation/commit error, every initial short-read/write position, exact
replay, two-writer exclusion, failed and lost certificate-sync outcomes, poisoned writers, tail
repair and byte-exhaustive bootstrap/late-commit corruption. Linux tests cover modes, positional
I/O, symlink/type rejection, no-replace behavior and lock lifetime. Btrfs subprocess tests cover
SIGKILL after group sync and after certificate sync, including live cross-process exclusion and
lock release on death. T-13 remains open for real-process creation boundaries and the ext4
mount/device trial.
