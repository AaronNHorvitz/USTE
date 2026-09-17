# R0 decision review and handoff

Date: 2026-09-16

R0 is **not complete**. Decisions D-01 through D-06, D-08 and D-09 have constrained v1
profiles and literal acceptance vectors in Decisions 0003–0010. D-07 has a selected policy,
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

## Commands and results

~~~text
$ rustc --edition=2024 --test tests/r0_vectors.rs -o /tmp/uste-r0-vectors
$ /tmp/uste-r0-vectors --nocapture
running 9 tests
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
~~~

Crates.io metadata was inspected with `cargo search`/`cargo info`; this selected candidates,
not a transitive dependency admission. No release, performance, independent review or
production-readiness claim follows from R0 design evidence.

## Exact next action

The repository owner enables GitHub private vulnerability reporting, submits a harmless draft
report, confirms maintainer-only visibility/response, and records reviewer/date here. Then
review the ADRs and vector test together, close T-01–07/T-46/T-47 only if accepted, and begin
T-08. Experimental code before that point must not establish a production disk format.
