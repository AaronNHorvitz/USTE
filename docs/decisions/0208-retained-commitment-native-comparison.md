# Decision 0208: Native comparison after commitment reuse

Date: 2026-09-20

Status: Passed read-only development comparison; nonqualifying.

After Decision 0207's full workspace/native regression, measure the separately pinned
`184b54bc7f508e8668e6850febabb766ef1442c4` binary against Decision 0206's retained synthetic
20,000-entity store and independent oracle summary. Do not rematerialize or alter its data,
64 MiB caches, 384 query shapes, typed refusals or resource budgets. Retain separate output
files so the original correctness pass and sampling timeout remain reproducible evidence.

Fresh resource admission precedes the workload: one heavy process group, 3 GiB soft/4 GiB hard
memory and 512 MiB swap limits, unchanged 1,800-second command deadline with 10-second TERM
grace. Compare exact outputs, state/certificate digests, logical/adapter work, cache counters
and resource reports before interpreting elapsed time. Host/device cache state is uncontrolled;
neither this scale nor this reservation qualifies BM-01 or T-20.

No sampling retry is automatically authorized by this experiment plan. Inspect the changed
query behavior first; only a separately admitted workload supported by new implementation and
measured progress may follow. Never extend the prior deadline or relabel missing latency
populations as passing. Preserve all artifacts on failure and continue safe implementation.

The [exact report](../evidence/retained-commitment-native-comparison.json) passed all 384 oracle
outcomes with identical output/state/certificate digests, visits, result bytes, cache counters,
adapter I/O and vault work. Query-only time changed from 870,407 to 793,165 ms (8.9% lower);
setup from 69,958 to 61,895 ms. Total wall 855.22 s, RSS 265,676 KiB, scope peak 277,741,568
bytes/zero swap. This is an uncontrolled-host development comparison, not a latency qualification.
Do not extrapolate that this modest improvement guarantees completion of the prior sampling
workload. Verify the source-only Decision 0209 follow-up next, then admit another measurement
with its own exact version if appropriate. No unchanged sampling retry ran.
