# T-13 journal/recovery foundation evidence

Date: 2026-09-17 · scope: partial T-13 evidence, not task completion or production qualification

## Implemented

- Decision 0015 format-1.0 encrypted manifest, certificate-log header, segment headers, exact opaque
  transaction groups and fixed 4,161-byte commit certificates.
- Random nonzero writer/log/segment identities from an explicit cryptographic entropy capability;
  role-separated manifest bootstrap context and hash-chained certificate frontier.
- Temporary-sibling creation with file and directory syncs, no-replace publication and parent sync.
- Non-cloneable exclusive ownership guards in the deterministic memory model and production Linux
  adapter. Restart generation tokens prevent a stale model guard from releasing a later owner.
- Poisoned-writer behavior after uncertain journal/certificate publication; no append may continue
  until reopen determines the durable frontier.
- Bounded streaming recovery with exact certificate slots, authenticated group replay, incomplete
  certificate-tail repair, uncertified referenced-segment-tail removal and fail-closed complete
  corruption. A full validation pass precedes callbacks; replay exposes authenticated logical and
  blob-inventory digests, and callers publish derived state only after successful return.
- x86_64 Linux descriptor-rooted `rustix 1.1.5` adapter using `openat2`, `pread`/`pwrite`,
  `fdatasync`/`fsync`, `ftruncate`, `renameat2(RENAME_NOREPLACE)` and unique-FD nonblocking `flock`.
  Symlinks, mount crossings and wrong object types fail closed; files/directories use 0600/0700.
  The supplied authority is duplicated close-on-exec; default admission is Btrfs-only and the
  explicitly named ext4-candidate path still requires independent mount qualification.

The journal admits only the canonical empty blob inventory. Nonempty inventories remain T-15 and
cannot be supplied as an unverifiable digest. The generic durable key-envelope interface exists for
trusted adapters; the supported operator implementation remains Decision 0013's portable Argon2id
recovery wrapper.

## Verification in this increment

`cargo test -p uste-storage --all-targets` passes:

- exact arbitrary-byte group replay in contiguous revision order;
- competing writer rejection and correct guard release;
- crash after successful certificate sync recovers the new frontier;
- certificate-sync failure recovers the previous frontier, reports/removes the durable uncertified
  group and rejects further appends on the poisoned handle;
- incomplete final certificate bytes are truncated and synced;
- complete final-certificate mutation and certified-group mutation are hard integrity errors rather
  than rollback opportunities;
- real Linux positional I/O, modes, symlink rejection, no-replace rename and independent lock FD;
- a child appends and syncs a real encrypted journal/certificate on the Btrfs runner, signals the
  parent, is killed with SIGKILL, and the production adapter reopens the exact committed bytes.
- every initial creation, commit and segment-rollover operation is crashed immediately before and
  after in the deterministic durability model, selecting only absent/previous/exact-new outcomes;
- the real portable recovery wrapper uses `OsEntropy`, persists through `KEY`, rejects a wrong
  Argon2id credential and reopens the exact group with its authenticated logical digest. The Btrfs
  SIGKILL test separately uses a deterministic test key adapter for child reproducibility.

`cargo clippy -p uste-storage --all-targets -- -D warnings`, documentation validation and the
62-task dependency-graph check also pass. Full-workspace and supply-chain results are recorded in
`PROGRESS.md` after the coherent increment gate.

## Honest limits and remaining T-13 work

T-13 stays open. Injected non-crash errors and short-progress cases at every journal-specific
boundary, exhaustive bootstrap/header corruption, cross-process lock death and additional real
process publication boundaries, and the ext4 mount/device trial are not yet complete. The current
SIGKILL case demonstrates process loss after a synced certificate, not controller cache loss or
power-cut behavior. `fstatfs` cannot itself establish ext4, local-device or hardware semantics.
Failed creation and rollover may leave hidden siblings or unreferenced segments; they are never
committed, but enumeration and bounded reclamation are explicitly assigned to T-35. The 1 GiB
certificate log fails closed after 258,047 commits; T-35 owns its rollover.
