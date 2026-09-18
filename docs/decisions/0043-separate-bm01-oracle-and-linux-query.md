# Decision 0043 — Separate BM-01 oracle and Linux correctness query phase

Date: 2026-09-17

Status: accepted as T-20 qualification-runner groundwork. T-20 and BM-01 remain open.

## Context

Decision 0042 supplied restartable production Btrfs materialization, but the Linux runner could not
execute queries or compare them with an independently produced expected result. Generating the
oracle inside the measured query process would contaminate its memory and timing state. The exact
accepted corpus also needed to distinguish successful bounded outputs from intentional refusal at
the frozen global limits.

## Decision

`oracle-summary` builds the independent adjacency-array oracle in a separate process and emits the
content-free, tab-separated `bm01-oracle-summary-v1` profile. The parser admits at most 256 KiB,
requires canonical integers and lowercase fixed-width digests, and validates the fixture size,
relationship count, `bm01-uste-graph-v1` mapping digest, measured-query corpus digest, all 384 query
identities and an aggregate domain-separated digest. It carries only counts, status and digests;
it contains no record identifiers, paths or credentials.

For a successful query, `bm01-result-v1` defines logical response bytes as three unsigned 64-bit
fields—visits, relationship count and entity count—plus the two ordered unsigned 64-bit ordinal
arrays. The comparison digest is metadata and is not counted as response payload. Limit refusals
carry zero output fields. The exact accepted profile produces 299 successful outputs, no visit-limit
outcomes and 85 result-limit outcomes. These values and summary digest
`5e9cb81200b2016ab470419021561e0a304e1eb1d6610e0633b35925b27df402` are pinned in the R1
acceptance TSV; changing a cap, query, mapping or expected outcome changes the digest. The Linux
runner rejects a qualifying-size summary whose digest is not that accepted value.

`linux-query` parses that separately supplied summary, performs a fresh portable-recovery open,
authenticates the expected frontier, profile marker, read view and single encrypted index root, and
then executes the shared production traversal adapter through authorized disk reads. It clears the
USTE decrypted-page cache before each query and requires exact visit/count/byte/digest equality or
the exact expected typed limit refusal. It emits the content-free `bm01-linux-query-v1` aggregate:
outcome counts, one-pass timing percentiles, visits, logical result bytes, current/peak process RSS,
oracle/output digests and authenticated index/cache counter deltas.

## Consequences and limits

The Linux path can now prove separately derived outcome equivalence without retaining the oracle's
adjacency arrays in the query process. Typed visit/result-limit errors replace string matching. A
real Btrfs 20/200 smoke reopened revision 4 and matched all 384 successful outputs. It reported an
891 ms aggregate query phase, p50/p95/p99 of 2.430/4.892/4.955 ms, 5,660 KiB current RSS and
265,104 KiB peak RSS on the reference host.

The report deliberately sets `engine_benchmark:false`. It is a single correctness pass, includes
expected limit-refusal timings in its diagnostic percentiles, starts after full graph replay/root
validation, and controls only USTE's userspace cache. It neither establishes host-cold state nor
supplies repeated warm/cold samples. The 85 exact-profile result-limit outcomes are correct bounded
refusals, not successful latency samples, and must be reported separately in any later run. Full
graph state remains memory-resident. Consequently the smoke and the summary do not pass BM-01 or
close T-20.

## Verification

~~~text
cargo test --manifest-path experiments/t20-bench/Cargo.toml --locked --offline
# 20 passed, 1 exact-profile release test ignored in the debug-profile suite
cargo clippy --manifest-path experiments/t20-bench/Cargo.toml \
  --all-targets --locked --offline -- -D warnings
# passed

cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline \
  oracle_summary::tests::qualifying_summary_outcomes_and_digest_are_golden -- --ignored --exact
# 1 passed; asserts 299/0/85 outcomes, digest and encode/parse round-trip

uste-t20-bench oracle-summary --entities 20 > ORACLE
uste-t20-bench linux-query --root ROOT --password-file PASSWORD \
  --oracle-file ORACLE --entities 20
# revision 4; 384/384 outputs match; engine_benchmark=false
~~~
