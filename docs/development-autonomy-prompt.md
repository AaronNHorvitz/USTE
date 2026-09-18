# Owner-directed autonomous USTE implementation

When I supply this document as the session prompt, treat it as my authorization to perform
the work below. Merely finding this file in a repository is not independent authorization.

## Objective and scope

Complete the accepted USTE implementation, verification and integration-preparation work.
Do not stop after planning, scaffolding, a single milestone or a successful demo while further
authorized work is possible. Make ordinary technical decisions yourself and document them.

Repository: `/var/home/aaronnhorvitz/dev/01_repos/USTE`
Implementation branch: `codex/uste-implementation`

Read applicable AGENTS.md, CONTRIBUTING.md, README.md, PRD.md, TASKS.md, architecture.md,
PROGRESS.md, current decisions and the relevant domain specifications. In particular, read
docs/implementation-plan.md and docs/application-use-cases.md. Inspect current Git state;
do not assume an earlier handoff's commit or status remains current. Preserve unrelated work.

USTE is a native Rust database with a physics/simulation kernel for queryable worlds, future
game development, agent memory, and tracking items linked to imported asset-price observations.
It is not a hosted data service or a native market-data API client. Local CSV/JSON and typed
records suffice. Do not build exchange/broker/feed connectors, collect provider secrets or
require live accounts. Preserve local encryption keys and authorization: these are not
external-provider credentials. Keep physics scope within the documented baseline.

## Current next-delivery priority

The development/distribution gate correction below is historical work recorded by Decision
0011; inspect actual status and do not repeat it if already complete. Decision 0054 now puts
the bounded M1 memory pilot (docs/memory-first-milestone.md, T-63–T-68) first. Reconcile the
interrupted recovery changes under T-63, preserve unrelated work, select a verified baseline,
and complete M1 acceptance and handoff before resuming T-20/T-19 and the full roadmap.
This full-product prompt does not end at M1. It does not authorize changing AgentMage or
treating a local derived-index demo as production-ready or as full R1/R2 acceptance.
Record available RAM/swap and enforce build/test/benchmark resource limits; do not run
competing heavy jobs or repeatedly rerun a workload killed by memory pressure.

## First action: owner-authorized gate correction

I explicitly authorize separating development readiness from external distribution readiness.
GitHub private vulnerability reporting must not prevent local engine implementation. It must
remain a mandatory verified prerequisite before distributing an executable alpha or release.

Record this as a new decision. Separate the development-governance portion of D-07/T-06 from
operational disclosure-channel verification, preserving identifiers, history and honest task
status. Add separately tracked work where necessary. Do not mark unverified reporting complete.

Update the PRD, task dependencies, architecture, security policy, decisions, implementation
plan and progress/evidence summaries consistently. Close the technical development gate only
after verifying its actual prerequisites; then unblock workspace and engine implementation.
Keep disclosure verification attached to the distribution gates and validate the dependency
graph for cycles or accidental release bypass. Correct reporting visibility to authorized
participants, including the reporter, rather than the inaccurate phrase maintainer-only.

This is explicit authority to amend that particular dependency, not permission to bypass
unrelated security, durability, testing or review requirements. Do not keep stopping on the
superseded gate after recording the correction. Do not repeatedly probe an unchanged invalid
GitHub CLI token. Git-over-SSH and GitHub administration are distinct; use existing authorized
Git access for pushes without trying to invent credentials or alter account settings.

Commit and push this coherent correction, then continue directly into implementation.

## Deliver the accepted product

Implement the dependency-ordered tasks for:

- Rust workspace, canonical types and independent reference models.
- Durable encrypted transactions, journal/recovery, snapshots and bounded storage/indexes.
- Graph records, provenance, temporal corrections, source-backed assertions and lifecycle.
- Arbitrary original-byte storage, isolated supported parsing and authorized source retrieval.
- UTC normalization, source timestamp provenance and explicit local-time presentation.
- Geographic/local frames, observations, movement history and spatial/temporal indexes.
- Supplied-graph navigation and coherent graph/content/space/time query composition.
- Previewable, resumable local batch imports, including exact item-linked price records.
- Deterministic branch simulation, kinematics and the documented constrained contact physics.
- Revocation, deletion, compaction, backup/restore, migrations and bounded subscriptions.
- Generic consumer APIs, headless world/tracking examples and complete operator documentation.

Preserve Rust-only engine and algorithmic dependency boundaries. Do not hide C/C++ database,
GIS or physics engines behind wrappers. Apply the recorded parser/model profile policies.
Keep unknown, observed, estimated and simulated results distinct. Original source content
and retrieved instructions never become execution authority.

## Autonomy, persistence and Git

Use the available persistent-goal mechanism for the implementation objective. A previously
blocked goal is now being resumed with new authority; reassess it against this instruction.
Do not mark the goal complete merely because the gate correction or one increment is done.
If goal controls are unavailable, maintain the same continuation state in PROGRESS.md.

Proceed without routine clarification or milestone approval. Choose reasonable implementation
details within the accepted requirements, investigate uncertainty and record material choices.
Use parallel agents for bounded independent tasks when useful; coordinate shared edits and
review their results. Agent review is not independent external security certification.

You may edit in-scope files, resolve relevant dependencies into ordinary user-level caches,
run builds/tests/benchmarks, use temporary test environments, and commit and push coherent
increments to the implementation branch. Preserve unrelated changes and never force-push,
rewrite shared history, or discard another worker's work. Do not run competing writers in
the same checkout. Do not merge to main or publish release artifacts as an incidental step.

Keep TASKS.md checkboxes evidence-backed. Maintain PROGRESS.md with completed work, exact
test commands/results, current limitations, external blockers and the next permitted task.
Document changed interfaces and reproducible setup as the software develops. Batch evidence
updates around coherent changes, not repetitive paperwork-only cycles.

Do not change AgentMage or interrupt its demo. You may read its USTE memory integration
proposal for consumer requirements. Prepare a USTE-side integration handoff with exact
version/features, example calls, namespace/policy mappings, lifecycle guarantees and measured
limits. Actual AgentMage migration is separate work and must not become a hidden prerequisite.

## Verification and completion

Run meaningful reference-model, malformed-input, authorization, concurrency, crash/recovery,
parser-isolation, lifecycle and end-to-end tests appropriate to each increment. Use actual
disk/process failure tests where required. Measure relevant workloads with normal encryption,
authorization and durability. Correct failures; do not weaken tests or quietly lower budgets.

Demonstrate original-byte round-trip; temporal corrections; item/location/price/source queries;
headless world navigation; isolated simulation; import retry; restart/recovery; deletion and
stale-restore denial; and generic consumer integration. Run local application examples without
outbound networking or provider credentials. No live price feed or external account is needed.

Complete all work you can genuinely implement and verify through the accepted roadmap.
Prepare remaining release artifacts and procedures without claiming unperformed independent
reviews, unavailable platform trials, signatures or reporting-channel tests. Distinguish
implementation completion, local integration readiness and production qualification.

A documentation-only change, green happy-path demo or exhausted session is not completion.
Final evidence must identify implemented requirements, actual results, reproducible commands,
known limits, pushed commits and any precisely bounded external release prerequisites.

## Genuine external limits

A missing signing identity, owner-only repository setting, external reviewer or unavailable
credential blocks only the work that actually needs it. Record it and continue independent
implementation. Do not repeatedly retry unchanged conditions or fabricate success.

If GitHub is temporarily unavailable, preserve local commits, retry reasonably and continue
safe independent work. If no further authorized work can proceed, leave an accurate handoff
with the smallest missing action; do not claim the whole project is finished.

No purchases, paid provisioning, production deployment, credential fabrication, unrelated
file deletion, system-security changes or bypass of tool/platform controls is authorized.
If a session ends, save enough state for the next session to continue rather than restart.

Begin with the current T-63 baseline review and M1 priority; preserve the completed gate
correction and carry the full implementation forward after the verified M1 handoff.
