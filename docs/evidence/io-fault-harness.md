# T-12 I/O capability and fault-harness evidence

Date: 2026-09-17 · Product toolchain: Rust/Cargo 1.95.0 · Target: x86_64-unknown-linux-gnu

Reviewed implementation: commit `00d0b1a8d876a4caf4c8ebdff82854a72af203a8` · tree
`740a9d3d842817b5f16321e84fe763f549a4715a`. Two read-only Codex agent audits compared the
capabilities, durability model, fault semantics, tests and claims with Decisions 0004/0014, FR-03,
NFR-03 and T-12. Findings about a tmpfs/racy subprocess test, child cleanup, sticky crashes,
read-progress coverage, directory-sync failure, mutable restart escape and a premature lock claim
were corrected before this commit. Both final delta audits reported no blocker, high or medium
finding. This is automated implementation review, not independent recovery assessment.

This record closes T-12's narrow capability and deterministic fault-harness scope. It does not
provide the supported Linux filesystem adapter, exclusive owner lock, journal, creation protocol,
certificate recovery or power-loss proof; those remain T-13.

## Implemented contract

- Safe first-party `uste-storage` code depends only on `uste-types` and `std`, forbids unsafe code,
  and contains no production ambient-path adapter. Filesystem algorithms receive opaque file and
  directory handles plus a single validated relative entry name.
- The filesystem trait separates create/open, positional read/write, data/full file sync,
  no-replace rename and directory sync. Exact read/write helpers retry only `Interrupted`, advance
  only by checked progress and reject zero write progress, premature EOF and adapter over-report.
- Clock and storage-randomness capabilities are explicit. Scripted observations preserve wall-clock
  rollback rather than turning time into commit order. Cryptographic key/nonce entropy remains in
  `uste-crypto`, not this trait.
- The deterministic memory adapter tracks volatile/durable bytes separately from volatile/durable
  names. Restart discards unsynchronized state and changes the handle generation. File sync alone
  can leave an orphan; a rename is not durable until directory sync.
- Fault plans select one-based per-operation occurrences and inject exact errors, bounded short
  reads/writes, zero/over-reported progress, or sticky crashes before/after successful calls.
  Invalid occurrence/action combinations are rejected. Crash state clears only after the owned
  restartable adapter successfully restarts; there is no mutable-inner/resume bypass.
- `acceptance/r1/io-faults.tsv` pins 15 scenarios: interrupted/short/invalid progress, disk full,
  file/directory flush failures, both rename crash sides, crash-after-data-sync, stale handles,
  deterministic replay, clock/random failures and process kill.

## Verification

~~~text
cargo clippy -p uste-storage --all-targets --all-features -- -D warnings
cargo test -p uste-storage --all-features -- --test-threads=1
# passed: 10 deterministic integration tests, 1 SIGKILL integration test and doc tests

findmnt -no FSTYPE,TARGET --target target/tmp
# btrfs /var/home

bash scripts/check.sh
# passed: format/clippy/test/doc, documentation/task graph, R0/publication/content suites;
# 56 workspace tests passed

CARGO_DENY_BIN=/tmp/uste-t09-tools/bin/cargo-deny bash scripts/check_supply_chain.sh
# all five lockfiles: 0 advisory/license/source errors; only the two documented
# miniz_oxide duplicate warnings in parser experiment graphs
~~~

The process test writes and data-syncs a file beneath Cargo's Btrfs-backed target scratch directory,
syncs its containing directory, signals readiness over a pipe, sends actual SIGKILL to the writer,
asserts signal 9 and reopens exact bytes. RAII guards kill/wait and clean the exact generated path on
panic/timeout. Reviewers additionally repeated this test five consecutive times successfully.

## Deliberate limits and T-13 handoff

The memory adapter is a reference state machine with ordinary infallible test-model allocations; it
is not a filesystem emulator. The host test demonstrates process-loss wiring after successful
flushes, not controller-cache loss or every journal publication boundary. Ext4 is untested.

T-13 must extend the boundary with exclusive ownership locking and a reviewed Linux implementation
of handle-relative no-follow open/create, no-replace rename, file/directory sync and stable error
mapping. It must then run the Decision 0004 crash matrix against encrypted journal/certificate bytes,
including real process kills and hard committed corruption. No supported filesystem or durable
database claim follows from T-12 alone.
