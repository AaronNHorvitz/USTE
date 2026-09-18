# Decision 0046 — BM-01 repeated authorized-query sampling

Date: 2026-09-17

Status: accepted as T-20 qualification-runner groundwork. T-20 and BM-01 remain open.

## Context

Decision 0045 supplied independently generated expectations for the disjoint warm-up and measured
corpora. BM-01 still needed a runner that cannot silently shorten the accepted five 60-second
samples, distinguishes USTE cache state, validates every timed outcome and keeps expected limit
refusals out of successful-query latency percentiles.

## Decision

`linux-sample` opens the production encrypted Linux/Btrfs database and a bounded
`bm01-oracle-bundle-v1`. Exact scale admits only the accepted bundle digest and has no duration or
sample-count override: it validates one 96-query warm-up pass, then runs five samples of at least
60 seconds. A sample stops only after a complete 384-query round.

Each measured query is executed twice in order. The runner clears USTE's decrypted page cache,
times the empty-cache engine call, validates it outside the latency interval, then times and
validates the identical query with the retained USTE cache. Kernel, filesystem, controller and
device caches are not cleared. Every
successful output must match visits, relationship/entity counts, logical result bytes and digest;
every expected refusal must match its typed limit. Nearest-rank p50/p95/p99 populations are
separate by USTE cache state, success/visit-limit/result-limit outcome, depth and graph class, with
an additional all-class group per depth for the accepted one-hop and four-hop budgets.

The runner caps each sample at 2,000,000 timed executions and uses fallible latency-vector growth.
It reports successful-output visits, logical bytes and authenticated index/cache counter deltas
separately for empty and retained cache states, plus current RSS, process-lifetime peak RSS,
complete-round counts and a content-free output digest. At scaled profiles it runs exactly one
complete development round.

The 30-second query budget is checked after every returned warm-up and measured query. The current
synchronous indexed-read API cannot preempt a query that never returns, so the report states
`query_deadline_enforced:false`, `query_deadline_postchecked:true` and does not evaluate a BM-01
pass. A cancellable or supervised query boundary remains qualification work.

## Evidence and limits

A release-built 20-entity/200-relationship Btrfs smoke validated all 96 warm-up expectations and
one complete 768-execution paired round. The sample took 1,725 ms, reported 5,788 KiB current and
264,916 KiB process-lifetime peak RSS, and attributed 2,815 page reads to empty-cache executions
versus zero to retained-cache executions. It emitted aggregate plus class-specific populations. Host
caches were uncontrolled and the graph reducer remained full-memory. This is functional sampling
evidence, not an exact-size or reserved-host BM-01 result.

The exact five-minute minimum campaign has not been run. The accepted 16-CPU/24-GiB/200-GiB host
reservation, environment record, preemptive 30-second deadline and removal of the full-memory graph
boundary remain required before BM-01 or T-20 can pass. Immediately before deciding whether to
launch, the 64-GiB reference host exposed only 6.2 GiB available RAM and 212 KiB free swap; the
runner was therefore not started under a false 24-GiB reservation. Btrfs had 998 GiB free.

## Verification

~~~text
cargo test --manifest-path experiments/t20-bench/Cargo.toml --locked --offline
# 27 passed; 2 exact-profile release tests ignored
cargo clippy --manifest-path experiments/t20-bench/Cargo.toml \
  --all-targets --locked --offline -- -D warnings
# passed
~~~
