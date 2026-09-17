# Implementation progress and handoff

Updated: 2026-09-17 · Branch: `codex/uste-implementation`

Latest reviewed implementation: `65b0ed0` (partial T-13 encrypted journal/recovery foundation;
T-13 remains open). Review was performed by Codex agents and does not represent independent
external security certification.

## Completed this increment

- Recorded owner-authorized Decision 0011, separating local development governance (T-06)
  from operational private-reporting verification (new T-62) without claiming verification.
- Corrected reporting participation to include the reporter and authorized repository security/
  triage participants, rather than the inaccurate maintainer-only description.
- Closed T-06/T-07 and R0 at the development decision/evidence scope after aligning stale
  pre-decision specification text and rerunning its actual prerequisites.
- Added a task-graph check proving the 62-task graph is acyclic, T-08 does not inherit T-62,
  and R4 decision T-44 does inherit T-62. R2/R3 are explicitly local readiness gates and no
  executable distribution is allowed while T-62 is open.
- Completed T-08 with a Rust 1.95.0/resolver-3 workspace, initial safe-Rust `uste-types` crate,
  root lockfile, read-only CI, active-document/reference validation and reproducible quality/
  supply-chain scripts. Excluded experiment workspaces retain their independent pinned locks.
- Completed T-09 with Decision 0012, bounded generic values, typed/scoped identities, UTC
  instants, nonzero monotonic commit revisions and a strict canonical format-1.0 codec.
- Completed T-10 with an independent `uste-testkit` oracle using ordered maps, clone-and-publish
  transactions, retained revision snapshots and scans rather than future production reducers.
- Completed T-11 with Decision 0013 and a safe first-party `uste-crypto` boundary around the pinned
  RustCrypto profile: exact object/recovery envelopes, fixed authenticated context, padded in-place
  AEAD, bounded nonce sessions, redacted lockable key ownership and a portable fixed-cost Argon2id
  recovery adapter.
- Added literal `crypto-v1` profile vectors and fail-closed coverage for every context field and
  ciphertext mutation, truncation, wrong key/password/database, entropy/nonce failures, locked use,
  maximum payload, authenticated malformed recovery padding and writer-incarnation separation.
- Corrected review findings before binding evidence: caller-sized crypto buffers now reserve
  fallibly, rejected credentials zeroize, resource failures remain retryable, guarded-memory is not
  claimed and future clone/restore/rotation/hardening responsibilities stay explicit.
- Completed T-12 with Decision 0014 and safe handle-relative filesystem, clock and randomness
  capabilities plus a deterministic volatile/durable memory adapter and per-operation fault plans.
- Covered interrupted and short I/O, zero/over-reported progress, disk full, failed file/directory
  sync, crash-before/after rename and data flush, sticky crash/restart, stale handles, wall rollback,
  random failure and exact repeatability. Crash state cannot be cleared without adapter restart.
- Added a guarded real-process scenario under Btrfs-backed Cargo target scratch: the child flushes
  file bytes and the directory, signals over a pipe, is killed with SIGKILL, and exact bytes reopen.
  This is process-loss harness evidence, not a power-loss or production-filesystem claim.
- Began T-13 with Decision 0015 and the pinned `rustix 1.1.5` x86_64 Linux adapter: descriptor-rooted
  no-symlink opens, checked object types, positional I/O, 0600/0700 creation, no-replace rename,
  explicit data/full/directory syncs, recovery truncation and unique-FD nonblocking ownership.
- Implemented encrypted creation manifest, authenticated log/segment headers, exact opaque group
  envelopes, fixed hash-chained commit certificates, poisoned uncertain writers and bounded
  streaming recovery. T-13 accepts only the canonical empty blob inventory until T-15.
- Verified exact byte replay, competing-owner rejection, durable lost-response recovery, previous-
  frontier recovery after certificate-sync failure, incomplete-tail repair and hard failure for
  complete certificate/group corruption. A Btrfs child was SIGKILLed after certificate sync and
  the production adapter reopened the exact committed group.
- Modeled scoped entity/evidence/assertion/relationship records, evidence-backed relationship
  lifecycle, explicit correction preconditions, final-state reference closure, bitemporal reads,
  bounded reject/cascade/retract deletion and typed atomic failures.
- Executed every literal assertion transition plus the complete valid/invalid action matrix; added
  deterministic bounded entity and evidence-backed graph histories with an independently asserted
  claim/correction projection.
- Enforced the `limits-v1` 10,000-operation and conservative 100,000-reference request caps before
  state cloning. The operation wire-size cap remains with the later admission/codec boundary.
- Added independent literal R1 goldens for all assigned value tags and numeric/time/reference
  endpoints; malformed length/integer/version/tag/order/UTF-8/depth inputs fail closed.
- Added a fixed 262,144-node aggregate budget and fallible decode allocation after review found
  that per-container/wire limits alone permitted heap amplification. Adversarial nested input now
  rejects before the violating container reserves its declared children.
- Added isolated pinned `cargo-fuzz` targets for arbitrary malformed frames and generated valid
  trees, a reproducible runner and bounded CI smoke campaigns. The native fuzz runtime remains a
  test-only graph outside the safe-Rust product workspace.
- Read the complete current requirement set, both accepted product decisions and every
  current domain specification. No applicable USTE `AGENTS.md` exists.
- Preserved the clean starting worktree and created the requested branch from `be5f7a4`.
- Added Decisions 0003–0010 covering the v1 data/time, storage, cryptography/retention,
  content-worker, benchmark/limit, governance, spatial and physics profiles.
- Added literal R0 acceptance vectors and a safe-Rust standalone vector test.
- Added a storage-publication model that exhaustively tests every record cut and byte
  corruption for the proposed commit frontier.
- Resolved and compiled the pinned strict-profile dependency candidates; recorded a lockfile,
  native-link/license feasibility and the remaining unsafe/advisory review scope.
- Added the `synthetic-v1` Rust fixture-generator kernel with strict seed parsing, domain
  separation, bounded-memory emission and a pinned BLAKE3 golden digest.
- Added reproducible Fedora Kinoite/Toolbox setup and runnable synthetic commands.
- Added a committed dependency policy and ran cargo-deny 0.20.2: zero advisory, license or
  source errors; one documented duplicate-version warning in the combined parser graph.
- Materialized 19 deterministic JSON/PDF/OOXML/archive/image/audio/video fixtures with pinned
  byte counts and SHA-256 values; positive formats and hostile archive metadata are smoke-tested.
- Classified direct cryptographic/parser unsafe boundaries and selected-feature reachability;
  kept JPEG SIMD disabled and parser unsafe outside the privileged engine boundary.
- Verified unprivileged bubblewrap mount/network/user/PID isolation and in-worker address-space,
  CPU-time and descriptor limits on the reference runner; recorded cgroup/seccomp gaps.
- Completed the R0 cross-decision audit, corrected BM-04 from an inadmissible 20 GiB blob to
  a 12 GiB within-cap stream, and checked T-01–T-05/T-46–T-47 with linked evidence.
- Recorded the exact external distribution blocker without claiming disclosure verification or
  any implementation/release gate.

## Verification run

~~~text
rustc --edition=2024 --test tests/r0_vectors.rs -o /tmp/uste-r0-vectors
/tmp/uste-r0-vectors --nocapture
# 12 passed; 0 failed
rustc --edition=2024 --test experiments/storage-publication.rs -o /tmp/uste-storage-publication
/tmp/uste-storage-publication
# 4 passed; 0 failed
cargo build --manifest-path experiments/dependency-audit/Cargo.toml --locked --offline
# success
cargo test --manifest-path experiments/fixture-generator/Cargo.toml --locked --offline
# 4 passed; 0 failed
cargo test --manifest-path experiments/content-fixtures/Cargo.toml --locked --offline
# 4 passed; 0 failed
cargo-deny ... --frozen check all --show-stats
# both lockfiles: 0 errors; parser graph: one documented duplicate warning
cargo fmt ... -- --check; rustfmt --check ...
# success
python3 scripts/check_task_graph.py
# task_graph=ok tasks=62 local_implementation_gate=T-07 distribution_gate=T-62 release_gate=T-44
bash scripts/check.sh
# workspace format/clippy/test/doc pass; 72 workspace tests including 27 uste-storage tests;
# docs=ok; task graph=ok; R0/fixture tests pass
CARGO_DENY_BIN=/tmp/uste-t09-tools/bin/cargo-deny bash scripts/check_supply_chain.sh
# all five lockfiles including rustix 1.1.5: zero advisory/license/source errors;
# documented miniz_oxide warnings only
cargo +nightly-2026-08-01 fuzz run decode_v1 -- \
  -max_total_time=60 -seed=1592639215 -max_len=4096 -rss_limit_mb=1024 -print_final_stats=1
# 14,518,800 executions; 61 seconds; 513 MiB peak RSS; no crash artifact
cargo +nightly-2026-08-01 fuzz run structured_v1 -- \
  -max_total_time=60 -seed=1592639215 -max_len=4096 -rss_limit_mb=1024 -print_final_stats=1
# 1,610,094 executions; 61 seconds; 554 MiB peak RSS; no crash artifact
~~~

Reference runner observed: Fedora 44, kernel 7.1.10, Btrfs 7.1/local NVMe, Intel i9-13900KF,
64 GiB RAM, Rust/Cargo 1.95.0. Pinned lockfiles now pass cargo-deny advisory/license/source
policy checks; no benchmark measurement exists yet.

## Limitations and external prerequisites

- T-62 requires the repository owner/administrator to enable and harmlessly test GitHub private
  vulnerability reporting. The previously observed GitHub CLI token is invalid and was not
  retried; SSH Git access is not administrative access. This blocks executable distribution and
  T-44, but it does not block local implementation, integration or artifact preparation.
- R0 decisions do not provide implementation, achieved benchmark performance or production
  security evidence. Transitive unsafe validation, the T-23 supervisor and actual BM results
  remain later-gate work.
- The canonical type/codec kernel, test-only logical oracle, envelope/key boundary and initial
  encrypted durable journal path are implemented, but there is still no database executable,
  production transaction coordinator, authorization or production qualification. The
  oracle intentionally retains full snapshots and scans records;
  it is a correctness reference, not a scalable implementation. The fuzz runner requires nightly
  Rust plus a C++ compiler, both confined to development tooling.
- T-13 remains open: journal-specific error/short-progress and hard-corruption coverage, additional
  real-process boundaries, cross-process lock death and the ext4 trial are not yet evidence-backed. Current Btrfs
  SIGKILL tests do not simulate controller cache loss or actual power loss. The certificate log
  fails closed at 1 GiB pending later maintenance/rollover design.

## Next dependency-permitted work

Continue T-13 with remaining injected error/short-progress and hard-corruption cases, additional
real-process/lock-death boundaries, then run the available filesystem qualification matrix. T-62 remains
independent and must not be represented as complete without owner-administered evidence.
