# R0 decision review and handoff

Date: 2026-09-16

R0 is **not complete**. T-01–T-05 and T-46–T-47 are closed at their decision/evidence scope;
Decisions D-01 through D-06, D-08 and D-09 have constrained v1 profiles and literal acceptance
vectors in Decisions 0003–0010. D-07 has a selected policy,
but T-06 is blocked on repository-owner enablement and end-to-end testing of GitHub private
vulnerability reporting; the local `gh` credential is invalid and this work is not authorized
to change repository security settings. Consequently T-07 remains open and R1 implementation
tasks are not checked.

## Completed technical evidence

- Reference runner inspected: Fedora 44, kernel 7.1.10, Btrfs 7.1/local NVMe, i9-13900KF,
  64 GiB RAM, Rust/Cargo 1.95.0.
- Exact current crate metadata queried from crates.io for cryptography and parser candidates;
  no dependency is yet admitted without a resolved lockfile/source/unsafe audit.
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
~~~

Crates.io metadata was inspected with `cargo search`/`cargo info`; this selected candidates,
not a transitive dependency admission. No release, performance, independent review or
production-readiness claim follows from R0 design evidence.

## Exact next action

The repository owner enables GitHub private vulnerability reporting, submits a harmless draft
report, confirms maintainer-only visibility/response, and records reviewer/date here. Then
review the ADRs and vector test together, close T-01–07/T-46/T-47 only if accepted, and begin
T-08. Experimental code before that point must not establish a production disk format.
