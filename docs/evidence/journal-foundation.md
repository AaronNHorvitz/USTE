# T-13 journal/recovery foundation evidence

Date: 2026-09-17 · scope: partial T-13 evidence, not task completion or production qualification

Reviewed implementation: commit `65b0ed023799585de0c52f09c8b8aa5e7fb7d4f5` · tree
`456e7cb632a62d3f0dada1ca61dcab476b2efb4b`. Two read-only Codex agent audits compared the
implementation, recovery ordering, Linux capability boundary, tests and claims with Decisions
0004/0013/0014/0015, FR-03 and T-13. Findings were corrected before the recorded commit, and the
final targeted review found no high-severity correctness or security blocker. This is automated
implementation review, not independent recovery or security assessment.

Qualification delta: commit `6b0b89d0760e8a01d45675eb014fd1c3dffb1358` · tree
`9d38fa93e710c97aee05f9c0d05c2c442a8eb7af`. A further read-only delta review checked the
exhaustive corruption offsets, error/short-progress matrices, committed-error classification,
test-only filesystem snapshots and Linux process-loss/lock scenarios and found no blocker or
high-severity issue.

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
- every initial creation/commit operation returns injected non-crash I/O failure without false
  durability, and every initial journal short-write/read position is retried to exact completion;
- every encoded byte of `KEY`, `MANIFEST`, `UCLG` and `USEG`, plus missing/truncated bootstrap
  objects, fails closed; every byte of a late committed certificate and group is rejected before
  any replay callback, and a corrupt later record prevents pending tail repair;
- the real portable recovery wrapper uses `OsEntropy`, persists through `KEY`, rejects a wrong
  Argon2id credential and reopens the exact group with its authenticated logical digest. The Btrfs
  SIGKILL test separately uses a deterministic test key adapter for child reproducibility.
- real Btrfs child processes are killed after group sync before certification and after certificate
  sync. Recovery respectively removes the uncertified tail or replays the exact new frontier; a
  competing process is rejected while the writer lives and admitted after SIGKILL releases `flock`.

`cargo clippy -p uste-storage --all-targets -- -D warnings`, documentation validation and the
62-task dependency-graph check also pass. Full-workspace and supply-chain results are recorded in
`PROGRESS.md` after the coherent increment gate.

## Honest limits and remaining T-13 work

T-13 stays open. Real-process creation publication boundaries and the ext4 mount/device trial are
not yet complete. The current SIGKILL cases demonstrate process loss after group/certificate
sync, not controller cache loss or power-cut behavior. `fstatfs` cannot itself establish ext4,
local-device or hardware semantics.
Failed creation and rollover may leave hidden siblings or unreferenced segments; they are never
committed, but enumeration and bounded reclamation are explicitly assigned to T-35. The 1 GiB
certificate log fails closed after 258,047 commits; T-35 owns its rollover.
