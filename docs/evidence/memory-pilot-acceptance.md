# T-68 M1 acceptance evidence

Exact implementation: `b9689f37d7e728d8ad7e27d50def3541e8609132` on
`codex/uste-implementation`.

Build contract:

- Cargo package versions: workspace `0.1.0`, unpublished; default features only (no optional
  features are declared by `uste-memory` or `uste-memory-adapter`).
- `Cargo.lock` SHA-256: `7ed2b533b3c801250a89e008b4ca26c48f16400e38224d8a63447c0393aaa97b`.
- Rust: `rustc 1.95.0 (59807616e 2026-04-14)`, Cargo 1.95.0, target
  `x86_64-unknown-linux-gnu`, LLVM 22.1.2.
- Host: Fedora Linux kernel `7.1.10-200.fc44.x86_64`; Btrfs, zstd:1; 62 GiB physical RAM.
- Verification concurrency: one Cargo build job, one Rust test thread, one heavy workload; user
  cgroup `MemoryHigh=3G`, `MemoryMax=4G`, `MemorySwapMax=512M`.

## Exit matrix

| Case | Evidence and actual result |
|---|---|
| M1-A | `process_recovery::acknowledged_memory_commit_survives_sigkill_and_wrong_key_fails_closed` waited for durable revision 4, sent SIGKILL, then a fresh process recovered fact 20 and exact source bytes. The T-64 test also performs clean restart. Pass. |
| M1-B | `memory_ingest` retries the same acknowledged source request to the identical outcome, restarts with a durable 1 MiB abandoned upload, refuses new staging and premature reconciliation, resumes/aborts it, reconciles the complete outbox, then ingests source v2. Pass. |
| M1-C | Current/as-of recorded revision, exact/missing source-event time, correction predecessor, independent contradiction and evidence remain distinct. Pass. |
| M1-D | Citation resolves exact source ID/version/blob digest/byte+line locator/excerpt and the adapter returns original bytes. Changed text, malformed locators, unknown formats and unsupported queries fail explicitly. Pass. |
| M1-E | Foreign scope fails before state access; policy replacement stales old views and hides records/counts/citations; durable source revocation invalidates cached views and excludes current and retained-history reads. Pass. |
| M1-F | Real wrong key, committed ciphertext mutation and writer lock fail closed; query cancellation and candidate/output limits are typed. Shared storage fault test `zero_progress_disk_full_and_flush_failure_fail_without_false_durability` and coordinator publication matrices cover no-space/short-I/O on the exact journal/blob path. Reopened historical search agrees with the separate scan oracle. Pass without intentionally exhausting the host disk. |
| M1-G | Generation-2 begin invalidates stale views, clears logical derived state, remains `Rebuilding` after restart, and serves only after authoritative reimport/complete. Old-generation requests fail. Consumer source bytes remain unchanged. Pass; no erasure claim. |
| M1-H | `CARGO_NET_OFFLINE=true scripts/run_memory_pilot_demo.sh` runs the synthetic Btrfs demo with no cloud, model, feed or provider credential and ends `M1_DEMO_OK ... generation=2 revision=11`. Pass. |
| M1-I | Release measurement below passes every frozen T-63 threshold with encryption, authorization and durability. Pass. |
| M1-J | `docs/memory-pilot-handoff.md` pins source/toolchain/lock/features, supported operations, mapping prerequisites, rollback and remaining full-product work. Pass; actual consumer registration remains separate. |

## Commands and results

~~~text
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 \
  cargo test -p uste-memory-adapter --all-targets --locked --offline
# adapter unit: 2 passed; real process acceptance: 2 passed; 0 failed

CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 \
  cargo test -p uste-memory --all-targets --locked --offline
# unit: 5 passed; end-to-end memory_ingest: 1 passed; 0 failed

CARGO_BUILD_JOBS=1 \
  cargo clippy -p uste-memory-adapter -p uste-memory -p uste-txn \
  --all-targets --locked --offline -- -D warnings
# passed (focused invocations; complete repository check also passed)

CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 bash scripts/check.sh
# passed; documentation=ok (127 links, 124 active IDs, 152 definitions)
# task_graph=ok (68 tasks); scaled T-20: 31 passed, 2 qualifying-only ignored

CARGO_BUILD_JOBS=1 cargo build -p uste-memory-adapter \
  --bin uste-memory-demo --release --locked --offline
/usr/bin/time -v target/release/uste-memory-demo measure \
  target/memory-pilot-measure.iM9wA6
# source_bytes=1048576
# ingest_bytes_per_second=55512414
# cold_recovery_millis=355
# warm_query_p50/p95/p99_micros=0/0/0 (all samples below 1 us clock reporting)
# warm_samples=1000
# maximum resident set size=265180 KiB
# elapsed=0.76 s; swaps=0; exit=0
~~~

The measured release run stayed below the process cap. At the later handoff preflight, the desktop
still had only about 4.7 GiB available and essentially all 8 GiB zram swap occupied. No qualifying
large-scale T-20 benchmark was launched. This resource state does not block the now-complete bounded
M1 pilot; it remains relevant to the resumed BM-01/BM-06 work.
