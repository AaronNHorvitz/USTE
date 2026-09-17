# Decision 0039 — Authorized index measurement boundary

Date: 2026-09-17

Status: accepted as T-20 benchmark instrumentation. T-20 and BM-01 remain open.

## Context

The encrypted current-graph index already owned a bounded decrypted-page cache, and the raw
storage reads returned authenticated page, cache-hit, fragment and result-byte statistics. The
authorization-preserving consumer path discarded those statistics and offered no explicit way to
start another sample with an empty USTE cache. A production BM-01 runner would therefore have had
to infer I/O from latency or bypass the consumer authorization boundary.

## Decision

Each admitted `AuthorizedGraphIndex` now owns one synchronized runtime containing its decrypted
page cache and cumulative, saturating diagnostic counters. The report includes:

- configured and currently accounted cache bytes plus page hits, misses and evictions;
- successfully completed authorized reads and completed raw index operations; and
- authenticated pages read, fragments visited and logical result bytes from those operations.

It never exposes index keys, plaintext or record identities. Its operation, fragment and byte
counters are nevertheless candidate-dependent and can reveal hidden cardinality. Report and clear
therefore exist only on `AuthorizedCoordinator`, require a fresh `ManageSchema` check and accept an
opaque root capability bound to the issuing coordinator instance. They are privileged operator
telemetry, not consumer-query output. An outcome-uncertain coordinator rejects both operations
before checking the possibly stale in-memory policy. Operations that complete and later encounter a
graph/policy error still contribute their real storage work; the top-level completed-read count
advances only for a successful authorized read.

An authorized clear operation zeroizes and discards the handle's decrypted pages while retaining
all cumulative counters. This makes before/after deltas reproducible and prevents counter resets
from concealing prior work. Policy revocation and a root from another coordinator instance reject
both reporting and clearing. It controls only USTE's userspace cache. It does not evict or claim
control over kernel, filesystem, controller or device caches. Tests cover unauthorized,
policy-revoked, foreign-coordinator and outcome-uncertain rejection.

BM-01 tooling must execute through the authorized encrypted disk-read facade and report counter
deltas per sample. Reports must distinguish an empty USTE cache from a process/host storage-cache
condition; `AuthorizedCoordinator::clear_index_cache` alone must not be labeled a fully cold host
run. Normal durability and authorization remain enabled during materialization and measurement.

## Consequences and limits

The accepted query path can now provide direct cache/I/O evidence without privileged raw reads.
One-hop adjacency still composes multiple authenticated index operations, so operation counts are
not graph-visit counts. Multi-hop BM-01 traversal must retain its independent oracle and explicit
global visit/result budgets.

This decision adds no qualifying performance result. The exact 100,000-entity/1,000,000-
relationship run still requires the reserved reference-runner envelope, release builds, repeated
samples, RSS/environment capture and exact oracle agreement. The live reducer and recovery remain
full-memory boundaries, and BM-06 remains unqualified.

## Verification

~~~text
cargo test -p uste-graph --test authorized_graph --locked
# 3 passed; first/warm/re-cleared counters plus unauthorized, revoked, foreign-instance and
# outcome-uncertain report/clear rejection
cargo test -p uste-txn --all-targets --locked
# 27 passed; 0 failed
cargo test -p uste-graph --all-targets --locked
# 40 passed; 0 failed
cargo clippy -p uste-txn -p uste-graph --all-targets --locked -- -D warnings
# passed
bash scripts/check.sh
# 262 workspace tests and 11 isolated t20-bench tests passed; format, strict clippy, rustdoc,
# docs (98 links, 95 active IDs, 146 definitions), task graph, R0 vectors, storage publication
# model and isolated builds passed
~~~

Codex agent review found the original direct-handle diagnostics could bypass revocation and leak
candidate cardinality. The issuer-bound privileged facade and uncertainty checks above resolve
those findings; final review reported no remaining high- or medium-severity issue. This is agent
review, not independent external security certification.
