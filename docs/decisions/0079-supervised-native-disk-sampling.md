# Decision 0079 — Supervised native disk development sampling

Date: 2026-09-18

Status: T-20 partial implementation; qualifying-profile admission/accounting and campaigns remain open.

Add `linux-disk-sample` with a persistent `linux-disk-sample-worker`. Reuse the existing parent
watchdog, bounded marker protocol and 30-second preemptive query deadline. The parent selects the
worker command and expected report schema together; only the parent may change the report's
deadline-enforced flag after verifying every start/finish pair and successful worker exit.
Timeout/error cleanup terminates and reaps only the owned worker. Tests exercise deadline cleanup
for both legacy and disk modes without changing the CLI's fixed deadline.

Share the accepted sample plan, typed outcome validation, cache-state names, aggregate digest
domain, latency grouping and percentile helpers. Preserve 96 disjoint warm-up queries, paired
empty/retained executions of all 384 measured queries, and separate success/visit-limit/result-limit
populations by depth/topology with all-topology groups. The qualifying plan remains five windows
of at least 60 seconds; development remains one complete round. Native disk commands still reject
above their development ceiling, rather than pretending development admission is qualification.

The native worker retains one authorized 64 MiB reader cache across the run. Cache deltas and
accumulation use checked arithmetic and preserve latest residency, including decreasing residency.
Only available cache counters and successful logical work are reported. Complete authenticated-I/O
accounting is explicitly `not-measured`; missing counters are not fabricated as zero or copied
from the legacy report. Storage metadata remains resident, host caches uncontrolled, process RSS
includes startup/key recovery, and budget evaluation remains `not-performed`. The report's
`engine_benchmark:true` identifies actual sampling, not a passed BM-01/BM-06 or release gate.

Strengthen parent report validation for both engines: require exact sample count/minimum window,
elapsed duration at least that minimum, bounded execution counts and `rounds * 768` agreement with
observed completed query pairs. Reject wrong engine schema, worker self-asserted enforcement and
inconsistent disk memory/accounting disclosures. Existing legacy commands and their tests remain.
The claimed deadline seconds must equal the supervisor's actual fixed deadline.

Review also hardens native bootstrap resume: before attempting policy installation on a nonempty
prefix, require exactly the expected first revision, principal, retry key and transaction ID.
Canonical request equality and expiry remain checked by the ordinary exact-retry path. An unrelated
one-revision database must not receive a fresh policy append merely because expected identities
are absent. Five negative fixtures cover foreign keys/principals/transactions, both IDs changed,
and different canonical content with matching IDs; each refusal leaves the durable frontier at one.

The real native CLI test generates its oracle bundle in another process and verifies the exact
sample digest against independent expectations, 96 warm-ups, 768 executions, 32 latency groups,
64 MiB cache budgets, nonzero empty-cache misses and zero misses for immediately retained queries
on the 20/200 fixture. Tests also cover counter rollback/overflow, unmeasured counter omission,
wrong limit classification, schema substitution and too-short qualifying-window reports. These
are development and protocol checks, not a qualifying campaign. M1 and the full roadmap remain intact.
