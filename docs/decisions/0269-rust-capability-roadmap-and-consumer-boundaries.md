# 0269: Rust Capability Roadmap and Staged Delivery

| Field | Value |
| --- | --- |
| Status | Accepted for product direction and implementation sequencing |
| Date | 2026-09-24 |
| Authority | Repository owner's explicit development replan and restart instruction |
| License effect | None; proposed changes require the separate owner license decision |

## Decision

Adopt [the 48-capability catalogue](../../CAPABILITY-ROADMAP.md) and
[the component implementation amendment](../../IMPLEMENTATION-AMENDMENT.md).
They extend the preserved PRD and task plan; they do not claim implementation or qualify a release.

Preserve and finish the current T-20 lookup/index correctness and performance campaign, then T-19 and the existing database dependency order. Add the consumer-facing contract packages below at their real prerequisites; this does not replace the full spatial/temporal/physics roadmap.

Own storage, revisioned graph/content, evidence provenance, bounded queries, lifecycle and memory adapters. Search acquisition, LLM execution, repository mutation, agent permissions, UI and external accounts stay with consumers. No GPU or model process is needed for this lane.

Older statements restricting the worker to the previous narrow slice or leaving it indefinitely
paused are superseded by this explicit restart assignment. Existing safety, independent review,
human-only acceptance, external publication and resource boundaries remain binding. Work on
later capabilities only after dependencies, with P0 before P1 before P2 and no silent deletion
of the original roadmap. Keep one actual execution/state owner for each domain.

## Consequences

Add stable work-package crosswalks without renumbering prior tasks. Reconcile and reuse existing
code before extending it. Keep public documentation consumer-neutral. Preserve current licenses
until a separate authorized transition; no private consumer publication is authorized here.
Serialize hardware qualification and use real pinned-model evidence; planning or fake adapters
never prove that combined functionality works.
