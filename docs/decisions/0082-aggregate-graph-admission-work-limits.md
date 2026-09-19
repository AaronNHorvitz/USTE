# Decision 0082 — Aggregate graph admission work limits

Date: 2026-09-18

Status: T-20 partial implementation; exact-profile admission and qualification remain open.

Correct `GraphDiskBaseAdmissionLimits` configuration validation: repeated exact/predecessor
lookups are aggregate work, not a single scan of distinct run contents. The previous page and
returned-byte ceilings incorrectly reused the eight-family scan capacities. This prevents a
caller from describing valid repeated proof work independently of physical database size.

Keep mandatory explicit nonzero aggregate operation/page/byte budgets. Validate page work against
`maximum_lookup_operations * max(exact_get_page_ceiling, configured_predecessor_page_ceiling)`
and returned bytes against the analogous maximum of exact-value and configured predecessor-result
ceilings. Clamp only these theoretical configuration products to `u64::MAX` when unrepresentable;
no buffers are allocated from them. Runtime accounting still uses checked addition, includes
cache hits, and clamps each storage operation to the remaining aggregate allowance. Exhaustion
or overflow fails closed. The operation-count ceiling, history-group bounds, semantic-reference
limits and all physical scan/run/entry/value limits remain unchanged.

This is trusted maintenance configuration, not a consumer override or permission to run an
unreserved workload. It does not raise benchmark latency/resource allowances or change durable
formats. Configurations with more aggregate work than any allowed operation could perform now
fail construction instead of accepting meaningless excess headroom. Existing call sites retain
their explicit budgets; qualifying driver limits still require separate derivation and validation.

Tests distinguish repeated lookup work from physical scan capacity, exercise both exact and
predecessor maxima, reject zero/impossible budgets, cover saturated configuration products and
checked runtime overflow, and count cache hits at exact exhaustion. Existing exact-minus cold
admission tests and authenticated corruption/fault/recovery coverage remain unchanged.
