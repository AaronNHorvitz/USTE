# Decision 0125 — Stream native paired-base recovery metadata

Date: 2026-09-19

Status: accepted implementation contract; T-20 qualification remains open.

The shared BM-01/BM-06 development adapter now selects Decision 0120's private per-revision
metadata staging whenever independently admitted graph and primary metadata bases have the same
revision. This includes ordinary open, repair/resume and explicit origin rebuild. A ready open
allows zero suffix revisions in both graph and metadata recovery. Repair retains the unchanged
fixture-derived revision, encoded-byte, proof, merge and cache limits. No cumulative retry or
transaction-ID overlay is populated on this paired path.

Keep ordinary coordinators' existing bounded overlay capacity for subsequent authorized writes.
That capacity is distinct from the zero recovery-overlay requirement. Explicit origin rebuild
continues to construct with zero live overlay capacity. Diagnostics distinguish live capacity,
recovery capacity and private metadata merge work; model selection follows the admitted path,
not the CLI phase name. Merge statistics are partial logical/index work, not complete authenticated
I/O or physical device I/O.

A graph base ahead of the newest paired metadata/transaction roots continues to use the existing
bounded suffix-overlay path. Do not invent an intermediate logical-state digest or silently roll
back the graph selection to force pairing. Missing graph roots still refuse ordinary open/repair;
origin reconstruction remains explicit. Private metadata must complete its existing terminal
publication before new writes. Exact retry, fixture binding, authorization, current policy,
certificate corruption refusal and durable publication semantics do not change.

Model/native regression checks cover paired one/multi/zero-step recovery, unpaired graph/metadata
admission, retained-root repair, exact retries and real owned-child process loss/resume. Native
construction after paired recovery exercises the retained live-write capacity. BM-01 remains capped
at 20,000 development entities and BM-06 at two development records. This integration neither runs
nor qualifies the exact-size campaigns, solves immutable-family rewrite/proof amplification, nor
changes M1's pinned consumer handoff or any release gate.
