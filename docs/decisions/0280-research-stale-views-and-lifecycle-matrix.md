# Decision 0280: Research read views go stale on newer commits; lifecycle matrix

Date: 2026-09-26

Status: Accepted for the development implementation; closes a Decision 0278 defect; DB-R03 in
progress.

While drafting DB-R03's adversarial lifecycle matrix, comparison with the memory pilot showed a
defect in Decision 0278: a `ResearchState` read view taken before a newer commit still answered
from its old snapshot. A view taken before `RevokeSource` could therefore keep returning the
revoked excerpt after the revocation committed, contradicting the contract that revocation
applies to every later read. The pilot prevents this with a process-local runtime revision shared
between live state and snapshots.

Adopt the same mechanism. `ResearchState` carries a shared runtime revision that `publish`
advances; every research read first checks that its snapshot is current and otherwise fails with
the new `ResearchReadError::StaleView`, so the caller must take a fresh view under current
authority. A rebuild keeps the shared revision. Equality of research states compares logical
fields only, so replay and reopen equivalence tests are unaffected. Persistent bytes, the codec,
admission rules, digests and authorization requirements are unchanged.

Add a DB-R03 lifecycle matrix through the authorized durable coordinator: contradicting claims
both stay visible with an explicit edge; a correction chain supersedes each predecessor while an
earlier knowledge revision still shows it active; a view taken before a newer commit is stale;
after revocation both current and historical views withhold the excerpt and report revocation;
cross-scope search is refused at the authorization boundary; a per-record policy denial makes old
views fail as stale policy and new views omit the denied citation without revealing it; after a
rebuild the old generation is stale and the new one is rebuilding; the rebuild survives a
filesystem restart and reopen, and completes with only rebuilt records visible.

Source deletion and purge remain T-34 work; this profile supports revocation and rebuild only.

Verification. The lifecycle test was run against the unfixed reducer (only the error variant it
names added) and failed at the stale-view assertion; with the fix, it and every other
`uste-memory` test pass. The full `scripts/check.sh` run started on `7c607e3` was interrupted by
a session authentication expiry after formatting, strict workspace Clippy and 51 tests in its
first six test binaries passed; it has no final result and is not reported as a pass. Since
`4464541`, whose tree already ran every workspace and isolated-experiment test, code changed only
in `uste-memory`, whose sole dependent is `uste-memory-adapter`; tests of the unchanged crates and
experiments were therefore not repeated. The invalidated and tree-wide stages were rerun on this
tree: formatting, strict workspace Clippy, all `uste-memory` and `uste-memory-adapter` targets,
warnings-denied workspace rustdoc, experiment manifest formatting, fuzz metadata, the docs and
task-graph checks, the R0 vectors and the storage-publication model. The known legacy BM-06
process-test deadline failure recorded in Decision 0273 is in the unchanged t20-bench and stands.
