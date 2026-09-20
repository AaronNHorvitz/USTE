# Decision 0185: Packed multi-batch continuation

Date: 2026-09-19

Status: Accepted

Generalize Decision 0172's continuation target without changing construction/tail intent.
An authenticated prefix at or before the frozen checkpoint finishes construction only through
that checkpoint. A prefix strictly after it has already begun the final generation and must
finish the full frontier. Reject zero/out-of-profile revisions. Never rewind a partial tail
to the preceding checkpoint or implicitly start the final generation during construction resume.

Share a generic packed continuation helper between the native driver and bounded model tests.
Require a published base and a complete-generation target within the profile and recovery group
budget, at or after the current base. Before requesting batches, verify every existing history
version against the fixture. Stream the original batches, validating sequence and using ordinary
authorized exact retry/publication/metadata rebase. Retain the observation point after successful
graph publication and before metadata rebase; an observation error stops immediately. There is
no retained request/history/outcome collection and no replacement retry identity.

The 513-record model cold-opens a partial second generation, refuses malformed targets and batch
sequences, exact-retries existing batches, and appends the remaining batch. It then interrupts
before metadata rebase, cold-recovers the certified suffix from the preceding complete triple,
and finishes exact retries without advancing the certified frontier. The clock supplies one
observation per authorized attempt, including retries, preserving retention admission. All 1,026
payload versions and the independent reference v1 digest must match with zero retry overlays.
This small-prefix test exercises the same continuation helper, not the full 100-generation
native workload. Separate arithmetic tests cover all prefixes of boundary-sized profiles and
literal qualifying final-tail boundaries 19,405/19,406/19,600/19,601.

Native/model full-history caps remain two. Existing native process-loss coverage remains required.
This change is neither physical power-loss evidence nor native multi-batch process-loss or
larger-than-memory qualification. Safe larger construction, resource/accounting evidence and
reserved-host BM-01/BM-06 campaigns remain open with unchanged targets and release gates.
