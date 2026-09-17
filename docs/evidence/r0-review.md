# R0 decision review and handoff

Date: 2026-09-17

R0 is **complete at its development decision/evidence scope**. T-01–T-07 and T-46–T-47 are
closed. Decisions D-01 through D-09 have constrained v1 profiles, governance and literal
acceptance vectors in Decisions 0003–0011. Decision 0011 separates local development governance
from external distribution readiness. GitHub private vulnerability reporting is still not
enabled/tested and T-62 remains open; no executable may be distributed until it is verified.
No repository setting was changed and the unchanged invalid `gh` credential was not retried.

## Completed technical evidence

- Reference runner inspected: Fedora 44, kernel 7.1.10, Btrfs 7.1/local NVMe, i9-13900KF,
  64 GiB RAM, Rust/Cargo 1.95.0.
- Exact cryptography/parser candidate graphs are captured in resolved experiment lockfiles and
  pass the recorded source/license/advisory policy. Final engine/worker admission still requires
  its own resolved feature graph and unsafe review.
- Canonical time/state, crash, lifecycle, parser, benchmark, spatial and physics profiles are
  closed for v1 in the ADRs; alternatives, limitations and acceptance criteria are explicit.
- Literal TSV vectors and a standalone safe-Rust test execute without external crates.
- The storage-publication experiment exhaustively cuts both fixed experimental records and
  flips every byte; incomplete unacknowledged tails recover the old frontier while every
  complete-certificate data loss/corruption fails closed.
- Nineteen generated parser fixtures have pinned byte lengths/SHA-256 values; smoke tests read
  positive PDF/ZIP/PNG/JPEG/WAV/Y4M samples and inspect archive traversal/link negatives.
- A bubblewrap 0.11.0/prlimit worker probe verifies read-only explicit input, private scratch,
  absent home/host files, no TCP egress and in-worker basic resource limits on the runner.

## Commands and results

~~~text
$ rustc --edition=2024 --test tests/r0_vectors.rs -o /tmp/uste-r0-vectors
$ /tmp/uste-r0-vectors --nocapture
running 12 tests
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ rustc --edition=2024 --test experiments/storage-publication.rs -o /tmp/uste-storage-publication
$ /tmp/uste-storage-publication
running 4 tests
test result: ok. 4 passed; 0 failed

$ cargo test --manifest-path experiments/fixture-generator/Cargo.toml --locked --offline
running 4 tests
test result: ok. 4 passed; 0 failed

$ cargo test --manifest-path experiments/content-fixtures/Cargo.toml --locked --offline
running 4 tests
test result: ok. 4 passed; 0 failed

$ cargo-deny ... --frozen check all --show-stats
# dependency candidates: advisories/licenses/sources clean; one recorded duplicate-version warning
# fixture generator: all checks clean

$ bash experiments/worker-sandbox.sh
sandbox_probe=ok input_bytes=2725

$ python3 scripts/check_task_graph.py
task_graph=ok tasks=62 local_implementation_gate=T-07 distribution_gate=T-62 release_gate=T-44
~~~

Crates.io metadata was inspected with `cargo search`/`cargo info`; this selected candidates,
not a transitive dependency admission. No release, performance, independent review or
production-readiness claim follows from R0 design evidence.

## Remaining external distribution action

For T-62, a repository owner or administrator enables GitHub private vulnerability reporting,
submits a harmless report, confirms participation by the reporter and authorized repository
security/triage participants, confirms non-public handling, and records tester/date here. That
operation does not block T-08 or later local implementation. It blocks every external executable
alpha, beta, candidate and release distribution, and it is a direct prerequisite of T-44.

## R0 conclusion and next local action

The cross-decision review, literal vectors, dependency-policy results and task-graph check pass
for the accepted design profiles. This is sufficient to close T-06/T-07 and begin T-08. It is
not implementation correctness, achieved performance, independent assessment, distribution
readiness or production qualification.
