# Decision 0055 — Bounded memory-pilot profile

Date: 2026-09-18

Status: accepted implementation profile for T-63. It is an experimental local bound, not a
larger-than-memory, production, release or physical-erasure qualification.

## Baseline recovery disposition

The interrupted Decision 0053 work was preserved and reviewed rather than assumed valid. The
selected baseline is commit `7393def` (`feat(graph): recover journal-anchored disk state`), whose
parent is the owner-approved planning commit `1ad74fa`. No recovered file was excluded. The exact
dirty tree was inspected, formatting and diff checks passed, the focused `uste-txn` 28-test and
`uste-graph` 43-test suites passed, both crates passed clippy with warnings denied, and the complete
repository check passed under one build job, one test thread and a 4 GiB process-group ceiling.

Decision 0053 remains optional T-20 disk-state groundwork rather than the memory pilot's storage
authority. M1 uses the already accepted encrypted journal/blob/authorization path and a new bounded
in-memory derived projection. Journal commits remain authoritative; consumer source storage remains
authoritative for approval and rebuild inputs.

## Frozen `memory-pilot-v1` limits

The executable constants live in `uste_memory::PILOT_PROFILE`:

| Resource | Limit |
|---|---:|
| Durable commits / retry-ledger entries | 4,096 |
| Immutable source versions | 256 |
| Retained facts, including terminal history | 2,048 |
| Total committed source bytes | 32 MiB |
| One source version | 1 MiB |
| Trusted exact UTF-8 text per source | 64 KiB |
| One fact field | 4 KiB |
| Canonical transaction request | 128 KiB |
| Logical in-memory state | 16 MiB |
| Staged uploads / staged bytes | 8 / 2 MiB |
| Query terms / visited candidates / results | 8 / 2,048 / 32 |
| Query output | 64 KiB |
| Concurrent readers in the consumer adapter | 8 |
| Process peak RSS | 512 MiB |
| Cold recovery | 15 seconds |
| Warm-query p99 | 100 ms |
| Ingest throughput floor | 1 MiB/s |

The RSS cap covers the state, one prepare copy, one read snapshot, journal/coordinator maps, one
1 MiB encrypted upload buffer, query output, and allocator/runtime margin. Measurements must use
normal encryption, authorization and durability. The thresholds were fixed before M1 measurement.
They do not replace any existing USTE benchmark target, including the failed BM-04 throughput
target or T-20's BM-01/BM-06 requirements.

## Admission and recovery rules

Every write must reject before durable publication if its commit count, source/history counts,
source bytes, request bytes or computed logical state would exceed the profile. Recovery replays
the same checks; unknown profile/schema values fail closed. The adapter admits at most one writer,
requires a complete consumer-owned upload outbox before writing durable staging bytes, and refuses
new post-restart uploads until each outbox token is resumed/committed or aborted and reconciliation
is explicitly completed. This is a bounded pilot protocol, not general storage garbage collection.

Every query supplies the consumer authority generation. A missing, rebuilding or mismatched
generation returns a typed refusal before results. Rebuild begins by durably making the projection
unservable; only a completed exact generation may serve. Old blob bytes and journal history may
remain physically present, so read exclusion is not described as erasure.

## Resource-safe execution

Initial verification uses `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`, one heavy workload at a time,
and a user-scope cgroup with `MemoryHigh=3 GiB`, `MemoryMax=4 GiB`, and `MemorySwapMax=512 MiB` where
available. The recovery audit observed roughly 4.7 GiB available host RAM and nearly all 8 GiB swap
occupied; therefore no large-scale benchmark campaign was started.
