# T-08 workspace foundation evidence

Date: 2026-09-17 · Toolchain: Rust/Cargo 1.95.0 · Target: x86_64-unknown-linux-gnu

This record closes T-08's workspace and quality-automation scope. It does not claim that T-09
types, transactions, storage, parsing, simulation or any database executable is implemented.

## Foundation

- The root Cargo workspace uses resolver 3, edition 2024 and pinned Rust 1.95.0.
- `uste-types` is the first safe-Rust core crate. Later crates join the workspace only when their
  owning task starts, avoiding empty packages that look like implemented components.
- Workspace lint policy forbids unsafe Rust and denies incompatible idioms/lifetime issues.
- The current root workspace has no third-party packages, build scripts or native links.
- MIT OR Apache-2.0 metadata is inherited consistently and packages are non-publishable while
  the product is in local development.
- `scripts/check_docs.py` checks required documents, local links and active D/FR/NFR/T/VT/BM
  references. `scripts/check_task_graph.py` checks dependencies and the distribution-gate split.
- `scripts/check.sh` runs formatting, Clippy, tests, rustdoc and the existing R0 executable
  evidence. `scripts/check_supply_chain.sh` runs the pinned policy against every lockfile.
- CI has read-only repository permission, uses a commit-pinned checkout action and runs the same
  scripts. It installs exactly cargo-deny 0.20.2 with its lockfile.

## Verified commands

~~~text
bash scripts/check.sh
# pass: workspace compile/lint/test/doc (0 product tests at scaffold scope);
# R0 vectors 12; storage publication 4;
# fixture generator 4; content fixtures 4; docs/task graph checks pass

CARGO_DENY_BIN=/tmp/uste-gate-tools/bin/cargo-deny bash scripts/check_supply_chain.sh
# pass: zero advisory/license/source errors; documented miniz_oxide duplicate warnings only
~~~

The existing parser candidate graph still contains the recorded duplicate versions and unsafe
dependency surface. Those are not dependencies of the root workspace. T-11 and T-23/T-24 own
actual crypto/worker admission and must extend the inventory for their resolved feature graphs.
