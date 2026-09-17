# Implementation progress and handoff

Updated: 2026-09-17 · Branch: `codex/uste-implementation`

Latest verified implementation: `8f035d7` (completed local T-17 transactional evidence graph over
the T-13–T-16 journal/transaction/blob/authorization foundation). Review was performed by Codex
agents and does not represent independent external security certification.

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
  streaming recovery. T-13's empty-inventory profile is extended by T-15's verified nonempty blob
  inventories.
- Verified exact byte replay, competing-owner rejection, durable lost-response recovery, previous-
  frontier recovery after certificate-sync failure, incomplete-tail repair and hard failure for
  complete certificate/group corruption. A Btrfs child was SIGKILLed after certificate sync and
  the production adapter reopened the exact committed group.
- Completed T-14 with Decision 0016 and `uste-txn`: owned prepare/publish changes, encrypted canonical
  transaction groups, coherent pinned readers, namespace/principal retry scope, durable outcome
  reconstruction, expiry and uncertain-handle quarantine. Literal group/malformed recovery,
  complete initial publication-fault, short-I/O, both cancellation-boundary, 32-caller conflict,
  restart and lost-response retry tests pass.
- Completed T-15's local acceptance with Decision 0017: bounded encrypted blob chunks, temporary-
  to-canonical staging, per-chunk authenticated progress witnesses, paired terminal witnesses,
  key-derived opaque inventory names, canonical inventory publication, commit-gated range reads and
  bounded recovery-time verification. Missing first/middle/last acknowledged chunks, single
  terminal-copy loss, malformed canonical objects, cross-context replay and foreign handles fail
  closed without token resurrection or silent shortening.
- Exercised error, no-space, short/zero progress and crash-before/after behavior across chunk,
  progress, multi-chunk finish, both terminal copies, abort cleanup, inventory, group and certificate
  publication. Blob-bearing lost-response retry, exact/+1 storage-profile limits and same-byte
  upload/namespace isolation pass.
- Measured the release-built production Linux-adapter path with normal Argon2id recovery wrapping,
  encryption and durability: a 12 GiB commit/reopen/full-hash round trip used 267,636 KiB peak RSS,
  1.06653x disk bytes and reproduced SHA-256
  `7eb969346fd20004fd1bb01f0ba1a8b356aea4c684ba22b1f8fde7c0014589c4`. Ingest was 95.923 MiB/s,
  below BM-04's 250 MiB/s target, so BM-04 remains unpassed despite T-15 correctness/RSS closure.
- Completed T-16 with Decision 0018 and a storage-independent, safe-Rust `uste-policy` kernel:
  trusted-adapter authentication mints opaque issuing-kernel-bound principals; absent policy denies;
  action permissions are independent; record rules narrow namespace grants; and policy revisions
  invalidate leases. The consumer transaction request has no caller-controlled principal field.
- Added the mandatory authorized transaction facade. It checks namespace and reducer-declared
  record requirements before state/index/storage access, rejects cross-namespace reducer targets,
  binds views/uploads to their issuing coordinator and exposes no generic reducer snapshot.
  Transaction-ID outcomes are owner-filtered, and denied unknown/existing paths use content-free
  failures. A review-found same-scope fabricated resume-token capability was closed by requiring
  authenticated durable journal evidence for every token absent from the in-memory ledger.
- Enforced checked exact-plaintext quotas for request bytes, staging, committed unique blobs, live
  handles and range reads. Failed writes reconcile accepted bytes; finalize retains the reservation;
  abort releases it; commit moves it once. Recovery reconstructs first-publication ownership and
  committed charges. Zero-byte reservations are capped at 32 and inventory lookup has a blob index.
- Format 1.0 cannot enumerate abandoned uncommitted reservations, so a recovered authorized
  coordinator conservatively rejects new upload starts. Evidenced known uploads, including a
  zero-byte final marker, remain resumable for reconciliation/abort; T-35 owns complete enumeration.
- Completed T-17 with Decision 0019 and the safe-Rust `uste-graph` production reducer: typed scoped
  entities/evidence/assertions/relationships, strict canonical requests, revision history, explicit
  valid time, lifecycle/corrections, reference closure and exact bounded cascade/retract deletion.
- Added atomic ordered outgoing/incoming adjacency and evidence-provenance indexes with independent
  rebuild checks. Cycles, self-loops, parallel edges and terminal transitions retain deterministic
  behavior. A separate 1,000,000-candidate visit ceiling stops before over-limit allocation.
- Added durable native graph policy bootstrap/replacement/history and reducer-owned authorized
  projections. Missing/mismatched durable policy fails closed, consumer install is rejected, stale
  idempotent replacement retries preserve the current version, revocation stales views and an
  uncertain commit quarantines existing views.
- Closed review-found graph disclosure paths: corrections require read authority on the new ID;
  candidate caps apply after filtering; direct/traversal/provenance results authorize every embedded
  endpoint, evidence, correction and nested property reference without returning hidden counts.
- Bound recovery receipts to complete canonical affected-record and policy bytes. Future policy and
  record history revisions fail explicitly. The production reducer agrees on shared modeled record
  state with the independent ordered-map oracle after each of 160 generated transactions, validates
  its derived-index rebuild separately, and restores graph/policy state after encrypted restart.
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
# workspace format/clippy/test/doc pass; 152 workspace tests including 64 uste-storage, 13
# uste-crypto, 13 uste-graph, 4 uste-policy and 25 uste-txn tests;
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
policy checks. The T-15 12 GiB component measurement is recorded, but BM-04's throughput target and
remaining mixed workload have not passed.

## Limitations and external prerequisites

- T-62 requires the repository owner/administrator to enable and harmlessly test GitHub private
  vulnerability reporting. The previously observed GitHub CLI token is invalid and was not
  retried; SSH Git access is not administrative access. This blocks executable distribution and
  T-44, but it does not block local implementation, integration or artifact preparation.
- R0 decisions do not provide implementation, achieved benchmark performance or production
  security evidence. Transitive unsafe validation, the T-23 supervisor and actual BM results
  remain later-gate work.
- The canonical type/codec kernel, test-only logical oracle, envelope/key boundary, encrypted
  journal, transaction/blob coordinator, authorization foundation and transactional graph reducer
  are implemented, but there is no database executable, disk graph index, replay checkpoint or
  production qualification. Graph policy is durable; the trusted adapter must supply its exact
  current copy at open. Production graph snapshots still retain full record history and derived
  indexes in memory until T-20. The oracle intentionally scans records and is not a scalable
  implementation. The fuzz runner requires nightly Rust plus a C++ compiler, both confined to
  development tooling.
- T-13 local acceptance is complete on the reference Btrfs runner and the independently identified
  ext4 mount `/var/mnt/archive_vault` (`/dev/sda1`). These SIGKILL tests do not simulate controller
  cache loss or actual power loss. The certificate log fails closed at 1 GiB pending later
  T-35 maintenance/rollover design.

## Next dependency-permitted work

Begin dependency-permitted T-18 deterministic replay and verified cache snapshots over the T-17
graph reducer. Bind checkpoint identity to journal frontier, reducer/profile versions and canonical
state, prove rebuild equivalence and corruption fallback without model/parser/network dependency.
T-62 remains independent and must not be represented as complete without owner-administered
evidence. BM-04 performance optimization remains later acceptance work and is not silently treated
as passed.
