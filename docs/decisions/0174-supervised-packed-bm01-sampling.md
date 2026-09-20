# Decision 0174 — Supervised packed BM-01 sampling

Date: 2026-09-19

Status: implemented and locally verified; no benchmark qualification.

Add native `linux-packed-sample` without changing the frozen fixture, oracle, sampling plan or
budget thresholds. Reuse the Decision 0046 plan and Decision 0047 owned-worker protocol. Cold-open
and independently admit the complete packed graph/primary/quota triple before querying. Use the
authorized packed reader and a bounded 64 MiB cache; do not reconstruct full-memory graph or
coordinator metadata. The existing 20,000-entity native admission cap remains enforced before
opening paths or spawning a worker, so qualifying dimensions are still refused.

Validate the bounded oracle bundle before setup. Execute 96 disjoint warm-ups, then every measured
query as an empty-cache/identical-retained-cache pair. Development sampling executes one complete
384-query paired round. Share typed-outcome validation, digest encoding and separate latency
populations with the existing sampler. Time only engine execution, excluding marker emission and
oracle checking. Preserve the fixed 30-second query deadline and execution-count bounds.

The parent owns, kills and reaps only its worker. Reuse the bounded closed protocol and lifetime
pipe watchdog. Only successful parent finalization can set `query_deadline_enforced:true`, after
checking the packed schema, complete fixture frontier, exact dimensions, warm-up/sample counts,
marker count and child exit. Cross-engine schemas and fabricated qualification/accounting fields
are refused. No deadline/configuration weakening flags are added.

Report actual cache counters and filesystem-adapter deltas separately for each cache population.
Do not synthesize v1 primitive counters for the packed engine. Authenticated I/O remains unmeasured,
`complete_authenticated_io:false`, host caches uncontrolled, budget evaluation not performed and
qualification explicitly nonqualifying development sampling. This closes native repeated-sampling
plumbing, not T-20, packed scale, larger-than-memory evidence or BM-01/BM-06 qualification.

Tests bind the supervisor schema and claims, extend actual owned-child deadline termination to the
packed mode, and run a separate-process 20/200 campaign. Independently recompute the paired oracle
digest, require 96 warm-ups/768 measured executions/32 latency groups, verify retained-cache hits
and zero retained reads for this small fixture, and prove source certificate bytes remain unchanged.

Full standalone regression passes 103 active cases with two existing ignored campaigns, strict
Clippy and format/docs/task checks. PROGRESS records commands, timings, resource limits and the
unchanged core baseline. No existing benchmark target or ignored campaign is weakened or relabeled.
