# T-63 bounded memory-pilot baseline evidence

Selected baseline: `7393def` on `codex/uste-implementation`, descended directly from planning commit
`1ad74fa`. The interrupted Decision 0053 changes were preserved, reviewed, verified and committed;
none were silently included or discarded. The worktree was clean after that commit and push.

## Recovery verification

~~~text
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo test -p uste-txn --all-targets --locked --offline
# 28 passed; 0 failed
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo test -p uste-graph --all-targets --locked --offline
# 43 passed; 0 failed
CARGO_BUILD_JOBS=1 cargo clippy -p uste-txn -p uste-graph --all-targets --locked --offline -- -D warnings
# passed
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 bash scripts/check.sh
# passed; docs=ok (120 links, 117 active IDs, 152 definitions); task graph=ok (68 tasks)
~~~

All commands ran one at a time in a user-scope cgroup capped at 4 GiB memory and 512 MiB swap.
The full check included the workspace tests, documentation, R0 vectors, fault harnesses, content
fixtures and the nonqualifying scaled T-20 experiment tests. Exact-scale BM-01/BM-06 were not run.

## Selected pilot profile

Decision 0055 and `uste_memory::PILOT_PROFILE` freeze the admission and measurement limits before
M1 implementation/measurement. Unit tests make internal widening or inconsistent limits fail.
The projection is intentionally memory-resident within its total-state cap. Journal/blob bytes are
encrypted and durable; consumer source storage and approval generation remain authoritative.

## Host preflight

The recovery audit observed Fedora 44 on Btrfs with approximately 62 GiB physical RAM, only
4.7–4.9 GiB available, and essentially all 8 GiB zram swap occupied. About 998 GiB remained free
on the workspace filesystem. No competing USTE Cargo/rustc/benchmark process was present. An
unrelated AgentMage llama server was observed and left untouched.

The next dependency-permitted action is T-64: implement the bounded source/fact reducer and the
consumer-outbox upload reconciliation needed for safe new ingestion after restart.
