# Decision 0004 — Storage publication, recovery and supported platform

Date: 2026-09-16

Status: accepted for storage profile `linux-local-v1`.

Closes D-01 and is the decision artifact for T-02.
[Decision 0014](0014-io-capability-and-fault-profile.md) fixes the T-12 capability and deterministic
fault-harness contract used to implement and test these publication rules. [Decision 0015](0015-journal-format-and-linux-adapter.md)
fixes the format-1.0 journal records and production Linux syscall profile.

## Failure model and platform

The first supported target is one owning process on x86_64 Linux, local Btrfs or ext4, with
regular files, advisory `flock`, atomic same-directory rename, and `fdatasync`/`fsync`
semantics honored by the kernel, filesystem and storage device. The tested reference is
Fedora 44, kernel 7.1.10, Btrfs 7.1 on local NVMe. Network, FUSE, overlay and removable
filesystems are unsupported. Arbitrary controller lies, post-flush media loss, RAM faults,
whole-directory rollback and hostile replacement of every file are outside this local-only
failure model; detected authentication or structural failure is never silently downgraded.

## Layout and publication

The database contains an immutable creation manifest, `LOCK`, append-only journal segments,
an append-only commit-certificate log, immutable encrypted blobs, and rebuildable index runs.
All files are relative to a directory handle; symlinks and path traversal are rejected.
Segments and objects have random 128-bit identities and fixed-size authenticated headers.
Frames use checked lengths, sequence numbers and a trailer over the complete ciphertext.

Creation writes a temporary sibling, flushes every required file and the directory, renames
the sibling to its final name, then flushes the parent. An exclusive nonblocking lock remains
held for the owner's lifetime. Lock metadata is diagnostic only; a PID or age never permits
stealing a live kernel lock.

Commit follows this state machine:

1. finalize and flush every newly referenced immutable blob, then its containing directory;
2. append one complete encrypted transaction group to the current journal segment;
3. `fdatasync` the segment;
4. append an authenticated certificate containing previous-certificate digest, revision,
   transaction group location/digest, blob inventory digest and logical event digest;
5. `fdatasync` the certificate log, then publish the in-memory root and acknowledge.

A segment rollover creates and flushes the new segment and directory entry before use. No
acknowledged v1 commit depends on rename-overwrite behavior. Commit certificates form a
hash-chained, strictly contiguous frontier; the journal is not committed merely because a
valid-looking group exists.

## Recovery and the non-rollback argument

Recovery authenticates the manifest, scans the certificate log from its authenticated start,
requires contiguous revisions and previous digests, and verifies every named group and blob.
A partial final certificate or group at physical EOF is an unacknowledged crash tail and may
be quarantined. Malformation before the tail, a sequence gap, a complete certificate naming
missing data, or authentication failure is `IntegrityFailure`; recovery does not select an
older convenient frontier. Rebuildable indexes are ignored and regenerated if absent.

Under the stated flush model, step 5 cannot acknowledge before all earlier data and its
certificate are durable. Therefore a crash can expose the previous frontier or the new
frontier before acknowledgment, but cannot lose an acknowledged certificate. A later torn
or missing acknowledged certificate violates the platform/media assumption and fails closed
when detected; USTE does not claim protection from wholesale rollback without an external
anchor. This is the explicit distinction requested by D-01, not a “highest valid slot” rule.

Checkpoints are immutable caches until a retention transaction promotes one as a baseline.
Their inventory, revision, epoch, schema/profile set and canonical logical digest must match
independent replay. Immutable disk indexes use 16 KiB authenticated pages in sorted runs;
roots are referenced only by committed maintenance events and remain rebuildable.

## Crash matrix

| Crash/fault boundary | Permitted recovery outcome |
|---|---|
| before blob flush | no commit; staging garbage only |
| after blob flush, before journal append | no commit; orphan eligible after root scan |
| during group append or before segment flush | ignore/quarantine final incomplete tail |
| after group flush, before certificate append | group is uncommitted garbage |
| during certificate append | accept only a complete authenticated certificate; else prior frontier |
| after certificate flush, before response | commit is durable; retry/outcome lookup returns it |
| missing/corrupt data named by complete certificate | hard integrity failure, never rollback |
| corrupt/missing index run | rebuild from retained authority |

Short writes and `EINTR` are retried only with checked progress; zero progress, `ENOSPC`, quota
failure and flush failure abort before a certificate. Directory flush errors prevent creation,
rollover or blob publication acknowledgment.

## Alternatives and acceptance

Dual mutable root slots were rejected because damage to the newest slot is indistinguishable
from interrupted publication without stronger assumptions. SQLite/RocksDB were rejected by
the product direction. Copy-on-write filesystem snapshots are optional operator backups, not
commit truth.

The crash boundaries above are enumerated in `acceptance/r0/storage-crash.tsv`. T-12/T-13
must implement deterministic short-write/flush faults plus real process-kill tests on both
supported filesystems before R1 can pass. Ext4 remains “candidate supported” until its trial
runs; only the reference Btrfs runner is evidenced at R0.
