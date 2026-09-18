# T-64 through T-66 bounded memory core evidence

Implementation under test: `97537e5` (`feat(memory): add bounded durable pilot core`), based on the
verified T-63 profile commit `e774558`. Decision 0056 records the implemented boundary. Tests use
synthetic data and the ordinary encrypted journal/blob/policy layers; they do not modify a consumer
runtime or claim an authoritative migration.

## T-64 — durable write and reopen

`crates/uste-memory/tests/memory_ingest.rs` creates an authorized coordinator, durably begins a
generation, uploads exact source bytes, commits the immutable source inventory and an evidence-bound
fact, completes the generation and reopens it. Retrying the same source request returns the same
outcome without another revision. Exact text length/digest, version sequence, scope, inventory,
media profile and byte/line locators validate before publication. Unit coverage rejects changed
text, unknown formats, malformed lines, every truncated canonical request and trailing data.

The test leaves an acknowledged 1 MiB upload unfinished, restarts, proves that new staging remains
closed, resumes and aborts the exact durable token, completes reconciliation from the adapter's
bounded complete outbox, and then successfully ingests source version 2. The reconciler refuses an
unresolved durable upload and cannot exceed the current policy's staging count. This resolves the
pilot's trusted-outbox case without weakening generic quotas or claiming filesystem-wide orphan
reclamation.

## T-65 — bounded retrieval and citations

The same fixture exercises identity lookup, lexical search, one-hop links and exact citations. It
checks the immutable source/version, content digest, byte/line locator, exact UTF-8 excerpt and raw
source-byte round trip. A correction preserves the predecessor for an earlier recorded revision;
the corrected fact and a separate contradiction remain independently sourced at the current view.
Exact and missing event-time states are distinct from recorded knowledge revisions.

A deliberately separate fixture scan oracle contains no `MemoryState`, query helpers or indexes and
agrees with the historical lexical result. Candidate exhaustion returns `ResourceLimit`; result
limits report truncation; cancellation returns typed `Cancelled`; unsupported and cross-namespace
requests fail before returning data. Authorization checks every result's fact, source and explicit
links before counts or content are emitted.

## T-66 — revocation and fail-closed rebuild

Source replacement excludes stale current facts while preserving history before the replacement.
Durable source revocation immediately invalidates an older view and excludes the source from fresh
current and historical reads. Policy replacement stales the old lease; a fresh view hides the fact,
search count and citation. The supported memory surface never exposes the generic raw blob
capability.

Beginning generation 2 invalidates old views, clears the logical projection and returns
`Rebuilding`. A restart between begin and reimport remains closed. Reimporting from the unchanged
authoritative fixture and completing generation 2 serves only that generation. Supplying generation
2 to a generation-1 copy returns `StaleGeneration`, so the consumer watermark prevents an old copy
from resurrecting facts. These are read-exclusion and rebuild results, not physical deletion.

## Verification

All commands ran sequentially with one Cargo build job and one Rust test thread in a user-scope
cgroup (`MemoryHigh=3G`, `MemoryMax=4G`, `MemorySwapMax=512M`):

~~~text
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 \
  cargo test -p uste-txn -p uste-memory --all-targets --locked --offline
# uste-memory: 6 passed; uste-txn: 28 passed; 0 failed

CARGO_BUILD_JOBS=1 \
  cargo clippy -p uste-txn -p uste-memory --all-targets --locked --offline -- -D warnings
# passed

CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 bash scripts/check.sh
# passed; documentation=ok (124 links, 121 active IDs, 152 definitions)
# task_graph=ok (68 tasks); scaled T-20 experiment: 31 passed, 2 qualifying cases ignored
~~~

The host still had roughly 4.8 GiB available RAM and saturated 8 GiB zram swap before verification;
no USTE build competed with these commands. T-67 is next: provide the restricted Linux Rust adapter,
durable consumer checkpoint/outbox and runnable offline demonstration. M1-A/F/H/I/J and the full
M1 qualification remain open for T-68.
