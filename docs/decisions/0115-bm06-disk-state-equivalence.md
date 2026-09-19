# Decision 0115 — BM-06 disk-state equivalence

Date: 2026-09-19

Status: accepted and locally verified development implementation. T-20 remains open.

Connect Decision 0114's actual versioned event workload to production encrypted, authorized
disk-backed graph and coordinator state. `bm06-disk-check --records N` admits at most two records
before allocating its filesystem or key material. It deliberately uses the durable memory-model
filesystem and development key adapter; this is not native timing, a larger-than-memory run or
one of the required 30 qualification trials. No production engine format, cap or security setting
changes. A policy-only bootstrap is the sole full-memory graph transition.

The verifier writes the first 99 generations through `AuthorizedDiskWriter`, maintaining graph,
retry and transaction roots. For the final generation it supplies an intentionally inadequate
**derived publication** budget. Only a certified `CommittedPublication` with storage ResourceLimit
is accepted; successful publication or any other failure rejects the test. After restart the
ordinary disk-blob-metadata opener authenticates storage, graph and metadata roots. Private-stage
recovery must start at revision 100 and produce 101. Metadata rebase and an exact authorized retry
must not add a revision. A second cold reopen must have exact current/history/policy cardinalities
and empty coordinator overlays. Authorized historical reads compare every version, modified
revision and all 4096 payload bytes against ordinal-derived expectations. The admitted policy
explicitly grants history reads; no raw privileged lookup replaces the verification reads.

Fixture-specific admission now distinguishes two-version BM-01 from 100-version BM-06. The latter
admits at most 1,638,400 logical bytes per history group (100 * the conservative 16 KiB encoded
entry bound), a maximum 512-record preparation, and a 664-visit predecessor allowance: three
passes over at most two pages per version plus 64 search visits. This is a work ceiling, not
memory reservation, observed I/O or a latency target. The inherited 64-visit allowance failed
historical ordinal 117 in the small fixture; the final allowance is derived from the new profile,
not a weakened production test. BM-01's limits and benchmark thresholds remain unchanged.
Run scan pages are capped by the existing format maximum rather than accepting a mathematically
oversized ceiling. Constructing valid limits for 100,000 records does not authorize the capped
verifier to execute them or establish safe exact-size construction.

Remaining work is native materialization, complete independently observed accounting, broader
recovery/fault/cache controls, rewrite scaling and the reserved-host campaign. The two-record
fixture has one suffix revision, not the exact-size workload's 196. Storage catalog recovery is
explicitly zero-blob for this closed entity fixture; arbitrary-blob correctness remains covered
by its separate production tests. This path cannot be used to claim physical erasure, retained
baseline promotion, production qualification or completion of T-20/T-19.
