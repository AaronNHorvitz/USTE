# Decision 0190: Bounded native history construction steps

Date: 2026-09-20

Status: Accepted; development-only, no scale or qualification gate changed.

Add packed native `create-prefix` and `resume-prefix` commands with required
`--through-revision R`. Accept only policy revision one or a complete generation boundary
`1 + g * batches_per_version`, at or before the frozen checkpoint. Retain the 513-record
native ceiling and all model/legacy ceilings. Reject invalid targets and incompatible,
missing or duplicate flags before filesystem access. Creation never overwrites an existing
database. Ordinary create/resume/tail/recovery commands retain their behavior.

Resume authenticates the existing profile and certified frontier. Refuse a target below that
frontier before generation work or derived-base reconstruction. Independently admit/replay
the existing prefix, verify every historical payload, exact-retry earlier deterministic
requests and append only the remaining batches through the selected target. A policy-only
target verifies its empty history. Report the actual frontier, verified counts, cold digest
and `construction_target_revision` (null for ordinary commands). An incomplete construction
prefix remains ineligible for ordinary open; resume/rebuild explicitly handle such prefixes.

This allows bounded, clean construction jobs; it is not writer rotation or permission to
bypass session exhaustion. Decision 0013's probabilistic nonce/session guarantees remain
unchanged. Ordinary reopen retains the journal writer incarnation. An exhausted session
requires the specified durable new writer incarnation and new vault, not clearing nonce
tracking or automatic reopen. Full rotation remains required by the preserved roadmap.

Verify separate-process policy-only creation, exact repeated target/certificate stability,
rewind and overwrite refusal, ordinary checkpoint completion, and two generations of the
smallest native multi-batch profile. Invalid target/flag and cap-first checks remain pre-I/O.
These are correctness tests under capped process groups, not larger-than-memory construction,
complete I/O accounting, recovery latency qualification or a new benchmark reservation.
