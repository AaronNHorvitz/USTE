# Implementation progress and handoff

Updated: 2026-09-17 · Branch: `codex/uste-implementation`

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
- The Rust files are design-vector experiments, not a database executable or production format.

## Next dependency-permitted work

T-08 is unblocked: scaffold the root Rust workspace and quality automation, then implement T-09
bounded canonical types/encoding against the frozen vectors. T-10 reference model, T-11 crypto
boundary and T-12 I/O fault adapters follow T-09 in dependency order. T-62 remains independent
and must not be represented as complete without owner-administered evidence.
