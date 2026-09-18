# Decision 0045 — BM-01 warm-up and measured oracle bundle

Date: 2026-09-17

Status: accepted as T-20 sampling-runner groundwork. T-20 and BM-01 remain open.

## Context

Decision 0043 separated measured expectations from the Linux correctness process, but the accepted
fixture also freezes 96 warm-up queries whose roots are disjoint from the 384 measured queries.
The repeated sampler cannot honestly execute that warm-up while deriving expectations internally,
reusing measured roots or leaving warm-up results unchecked.

## Decision

`oracle-bundle` builds one independent adjacency-array oracle and emits the content-free
`bm01-oracle-bundle-v1` envelope. It contains an exact `bm01-warmup-summary-v1` section followed by
the unchanged `bm01-oracle-summary-v1` measured section. Canonical byte-length headers delimit the
two nested summaries. The complete input is capped at 512 KiB; each summary retains its 256 KiB
cap, strict canonical integer/digest syntax, profile/mapping/query-corpus validation, exact query
identities and domain-separated aggregate digest.

The exact profile pins these independent outcomes:

- warm-up: 74 successful outputs, no visit-limit outcomes and 22 result-limit outcomes, summary
  digest `67d23874e04ef558e311f1d8006180b25a25481c42788b05f2deb1627155e312`;
- measured: the existing 299 successful outputs, no visit-limit outcomes and 85 result-limit
  outcomes, summary digest
  `5e9cb81200b2016ab470419021561e0a304e1eb1d6610e0633b35925b27df402`; and
- combined bundle digest
  `d52869f24d635476f86374813e754be364d0d2df470544b71221c24b96145fae`.

The R1 acceptance TSV and compiled constants pin all three. The bundle digest binds fixture counts,
both summary digests and both expectation counts. Encoding remains content-free: no query roots,
record identifiers, filesystem paths or credentials are emitted.

## Consequences and limits

A future Linux sampler can load one bounded artifact generated in a separate process, validate the
96-query stabilization pass and then measure only the disjoint 384-query corpus. The original
`oracle-summary` command and profile remain byte-compatible for the one-pass correctness phase.

This decision does not yet add repeated timing. It does not enforce the 30-second query deadline,
control host caches, reserve the accepted host resources or pass BM-01. Expected result-limit
refusals remain correctness outcomes and cannot enter successful-query latency percentiles.

## Verification

~~~text
cargo test --manifest-path experiments/t20-bench/Cargo.toml --locked --offline
# 23 passed; 2 exact-profile release tests ignored in the debug-profile suite
cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline \
  oracle_bundle::tests::qualifying_bundle_outcomes_and_digests_are_golden -- --ignored --exact
# 1 passed in 7.84 s; exact 74/0/22 and 299/0/85 splits, all digests and round-trip
cargo clippy --manifest-path experiments/t20-bench/Cargo.toml \
  --all-targets --locked --offline -- -D warnings
# passed
~~~
