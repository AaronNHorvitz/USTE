# Decision 0219: Supervised bounded-capacity sampling controls

Date: 2026-09-20

Status: Implemented and locally verified; no larger sampling measurement or qualification.

Extend D0218's explicit 256 MiB page-only and 128/128 MiB page/positive profiles to separately
named supervised sampling commands. Preserve both 64 MiB commands and every D0046/D0047/D0174
oracle, population, timing, complete-round, deadline and qualification boundary. This does not
select a new default or assert that either capacity/profile meets the performance targets.

Workers report distinct wide-profile schemas and exact included cache partitions. Parents bind
the selected command to its schema, budget and complete observation ledger; cross-size/mode
substitution fails closed. Wide page-only reports require explicit configuration and absent
positive-cache work, unlike the deliberately retained historical 64 MiB schema compatibility.
Wide positive reports reconcile warm-up plus all paired-state counters and terminal gauges,
using the existing atomic checked accumulation. No fabricated proof-work or physical-I/O claim.

Keep snapshots outside timed execution and the unchanged lifetime watchdog/owned-child kill-reap
protocol. Require all four real CLI sampling configurations to preserve the independent digest,
96 warm-ups, 768 development executions and 32 latency groups. Test cross-profile refusal,
mandatory configuration, counter overflow, paired gauges and exact owned-child cleanup.
No new fixture admission, result/work limit, resource ceiling, consumer API or source format.
Qualifying reserved-host campaigns and complete T-20/T-19/R2–R4 requirements remain separate.

The full native release suite passes 136 active tests with five unchanged opt-in ignores,
strict Clippy and format/docs/task checks. All four separate-process configurations preserve
the independent paired digest, populations, deadline claim and source bytes. New parent tests
reject old-budget/schema substitution, missing configuration/work, overflow and final-gauge
mismatch. Exact commands, tested source and memory limits are recorded in PROGRESS. D0216 and
D0218's cold-query regressions remain evidence, not superseded passes or default recommendations.
