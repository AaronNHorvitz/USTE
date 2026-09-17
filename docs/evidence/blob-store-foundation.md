# T-15 encrypted blob-store qualification evidence

Date: 2026-09-17 · scope: completed local T-15 acceptance, not production qualification

## Implemented

- Decision 0017's random upload and namespace-derived blob identities, 1 MiB encrypted chunks,
  16 GiB admission cap and bounded range reads.
- Temporary/write/sync/no-replace-rename/sync publication to immutable staging. Resume discards
  only temporary objects, authenticates canonical staging plus per-chunk progress witnesses and
  quarantines uncertain handles. Missing first, middle or last acknowledged chunks fail closed.
- Retryable immutable finalization plus paired authenticated final and abort witnesses. One missing
  terminal copy is repaired; malformed canonical copies fail closed. Abort intent becomes durable
  and terminal before idempotent cleanup, so cleanup errors cannot lose an accepted abort. Duplicate
  handles reconcile both states and cannot publish contradictory final/abort terminals.
- Canonical sorted `UBIN` inventories with exact byte length, chunk count and original-byte digest;
  inventories use key-derived opaque disk names and every referenced chunk becomes durable before
  journal publication.
- Certificate inventory binding and two-pass recovery verification before logical replay. Reads
  accept only exact references reconstructed from committed inventories.
- Hard bounds cover 32 live upload buffers per database/owner process, 100,000 references per
  inventory, 1,000,000 unique blobs per journal, 10,000,000 reference bindings per journal and
  1 TiB of logical blob bytes per namespace. Inventory construction stops at cap plus one rather
  than collecting an unbounded iterator.
- Transaction idempotency binds the inventory digest; reducers receive the verified inventory on
  initial commit and recovery.
- Upload handles are bound to database, key epoch and writer incarnation before any filesystem I/O.
  Equal bytes under distinct upload/namespace identities retain isolated encrypted object names.

## Fault, corruption and retry qualification

The deterministic memory adapter exercises injected `Io`, `NoSpace`, short/zero progress,
`CrashBefore` and `CrashAfter` outcomes at the applicable occurrences across:

- full-chunk and progress-witness temporary creation/write/length/sync, directory sync, canonical
  rename and final directory sync, always recovering an exact durable offset without duplicate input;
- partial-terminal and three-chunk finish, staging-to-final renames, directory synchronization and
  both final-witness publications, with retry/resume returning the one exact reference;
- paired abort-witness publication and cleanup, including a durable terminal handle when cleanup
  fails and an idempotent in-process cleanup retry;
- inventory creation/write/length/sync/directory sync, journal group publication and
  certificate publication, never exposing a certificate with a dangling object;
- certificate-sync ambiguity, where recovery re-synchronizes an authenticated complete frontier
  before acknowledging it.

Additional negative tests reject changed retry inventories, foreign-store handles, competing
staging plaintext, authenticated but malformed `UBMF`/`UBPG`/`UBIN`, missing/truncated terminal
copies, mutated/truncated/appended committed chunks and inventories, missing acknowledged staging
or committed chunks, and valid ciphertext replayed under another blob context.
Range tests cover cross-chunk reads, exact EOF, offsets beyond EOF and output larger than the
remaining object. Finalized and aborted markers, upload-cap leases and exact retry behavior survive
store reopen.

## Measured encrypted large-object round trip

The checked-in `blob_stream_probe` example uses the production Linux adapter, portable Argon2id
recovery wrapping, normal XChaCha20-Poly1305 chunk encryption and durability calls. It streams one
deterministic 12 GiB object without retaining it, commits its inventory, drops and reopens the
store, verifies recovery, checks range behavior and streams the complete original-byte hash again.

~~~text
cargo build --release -p uste-storage --example blob_stream_probe
probe_root=$(mktemp -d --tmpdir=. .t15-12g.XXXXXX)
/usr/bin/time -v target/release/examples/blob_stream_probe \
  "$probe_root" 12884901888
find "$probe_root" -depth -delete

logical bytes:        12,884,901,888 (12 GiB)
encrypted disk bytes: 13,742,174,810 (1.06653x logical)
ingest:               128.103253 s (95.923 MiB/s logical)
recovery:             33.439120 s
full verify/read:     33.547010 s (366.292 MiB/s logical)
maximum RSS:          267,636 KiB
swaps:                0
SHA-256:              7eb969346fd20004fd1bb01f0ba1a8b356aea4c684ba22b1f8fde7c0014589c4
exit status:          0
~~~

Runner: Fedora 44, Linux 7.1.10, x86_64, Btrfs workspace on local NVMe, Intel i9-13900KF,
64 GiB RAM, Rust/Cargo 1.95.0, release profile. The explicit temporary dataset was deleted after
measurement. The 95.923 MiB/s ingest result is below Decision 0007's 250 MiB/s target. Therefore
BM-04 has **not** passed: this is only its large-object correctness, RSS and amplification evidence.
The mixed 100,000-small-object workload, performance optimization, cleanup/reclamation measurement
and production-filesystem/power-loss qualification remain later work.

## Reproducible verification

~~~text
cargo test -p uste-storage --lib
# 46 passed; 0 failed
cargo test -p uste-txn --all-targets
# 12 passed; 0 failed
bash scripts/check.sh
# workspace format/clippy/test/doc and repository checks pass
~~~

T-16 owns principal authorization and actual-byte quota policy; T-17 owns artifact records; T-35
owns durable orphan enumeration and reclamation. The local result does not claim controller-cache
power-loss safety, independent external security review or executable distribution readiness.
