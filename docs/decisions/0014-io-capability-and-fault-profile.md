# Decision 0014 — I/O capability and deterministic fault profile

Date: 2026-09-17

Status: accepted for the T-12 interface/harness slice of `linux-local-v1`. This refines
[Decision 0004](0004-storage-publication-and-recovery.md) without claiming the T-13 journal or a
supported production filesystem adapter.

## Capability boundary

Storage algorithms receive explicit filesystem, clock and randomness capabilities. They do not use
an ambient working directory, environment-derived paths, wall clock or hidden thread RNG. A
filesystem implementation owns opaque file/directory handles. Callers pass one validated entry name
relative to an authorized directory handle; empty names, `.`, `..`, slash, NUL and names longer than
255 bytes are rejected before adapter dispatch.

The filesystem contract assigns create/open directory, create-new/open-existing file, metadata,
positional read/write, recovery truncation, data/full file sync, no-replace rename and directory
sync operations. Decision 0015 adds a separate non-cloneable exclusive-ownership capability;
`write_at` and `read_at` report checked progress. The shared exact loops retry only `Interrupted`,
advance only by reported bytes, reject an over-reporting adapter, reject zero write progress and
turn read EOF before the requested length into `UnexpectedEof`. No flush, rename, space, quota,
permission or generic I/O error is silently retried.

Errors have stable content-free codes; platform path strings and raw OS diagnostics do not cross the
boundary. Decision 0015 implements the T-13 Linux adapter with reviewed handle-relative no-follow
operations, exclusive ownership and fail-closed filesystem admission. The Rust standard library
alone does not expose every required primitive, so T-12 deliberately does not disguise a path-based
adapter as production-safe or claim a supported host implementation.

Clock observations contain normalized UTC plus monotonic ticks. Wall observations may repeat or
move backward and never order commits. Storage randomness fills caller-owned bytes explicitly; key
and AEAD nonce generation remains the separate `uste-crypto::EntropySource` boundary.

## Durability reference and fault semantics

The deterministic memory adapter keeps volatile file bytes, durable file bytes, volatile directory
entries and durable directory entries separately. File sync copies only file bytes. Directory sync
publishes names. Restart discards unsynchronized state and invalidates every prior handle. Thus a
data-flushed file may still become an unreachable orphan when its directory entry was not flushed,
and rename does not survive restart until the affected directory is flushed.

Fault scripts select a one-based occurrence of an assigned operation. A point can return an exact
error, restrict one read/write's maximum progress, return zero progress, over-report progress for a
contract-negative test, crash before the operation, or crash after a successful operation. A crash
is sticky until the harness successfully restarts its owned adapter; there is no separate resume or
mutable-inner escape. Points are one-shot, duplicate points/zero occurrences are invalid and a
short-progress action on the wrong operation is an adapter-contract error. The same plan and inputs
must produce the same result transcript.

This is a reference failure model, not a filesystem emulator or proof of power-loss behavior.
The test-only host scenario creates and flushes a file and directory on the reference Btrfs runner,
SIGKILLs the writer process, then reopens exact bytes. It demonstrates process-loss harness wiring;
it does not simulate controller cache loss or replace T-13's boundary-by-boundary journal kill tests.
Ext4 remains untested and no production filesystem support claim advances here.

## Acceptance and downstream obligations

`acceptance/r1/io-faults.tsv` pins interrupted/short/over-reported I/O, zero-progress writes,
disk-full, file/directory-flush failure, rename crashes, crash-after-data-sync, stale handles, wall
rollback, random failure and real process-kill scenarios. Tests distinguish durable data from
durable names and prove failed flushes do not become durable in the model.

T-13 consumes these interfaces for creation, journal/certificate publication and recovery, and
extends the boundary with a reviewed exclusive ownership-lock capability. It must add the real Linux
adapter, injected failure at every Decision 0004 publication boundary, hard-corruption handling and
actual process-kill recovery tests. T-45 owns production time sampling and clock rollback/restart
integration. T-36 owns backup/restore crash behavior.
