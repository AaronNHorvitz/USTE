# Decision 0054 — Memory-first delivery checkpoint

Date: 2026-09-18

Status: accepted for planning and implementation order by owner request. No implementation,
benchmark, independent security review or consumer integration is accepted by this decision.

## Context

The existing generic integration task depends on broad R2 query, navigation and simulation
work. Meanwhile the disk-scale campaign precedes R1 acceptance. This makes a bounded local
memory experiment wait for substantial work unrelated to trying that consumer workflow.
The owner requested an explicit earlier memory-integration milestone while retaining the
native spatial-temporal database, physics kernel and full-product objective.

## Decision

Add [M1](../memory-first-milestone.md) and tasks T-63–T-68. Reconcile interrupted work and freeze
a verified bounded profile first; deliver source-backed writes, cited retrieval, correction,
revocation, restart safety and a generic local derived-index consumer demonstration next.
Resume T-20/T-19 and the complete roadmap after the milestone handoff.

M1 has its own narrow acceptance evidence and is not an alternate path to R1, R2 or release.
The original task dependencies, benchmarks, requirement gates and distribution prerequisites
are unchanged. A capped in-memory projection is permitted only with durable encrypted state,
verified recovery and enforceable total-state limits. It cannot claim disk-scale acceptance.

Consumers keep their original store authoritative. Their approval, integration admission,
model/context behavior and runtime cutover remain separate decisions. Synthetic pilot fixtures
do not qualify sensitive production storage or complete physical forgetting.

## Consequences and alternatives

Full R2 completion before any consumer experiment was rejected as the immediate delivery order,
not as a product obligation. Dropping spatial/physics scope, weakening security requirements,
calling a mock an integration, or substituting a different database engine were rejected.
Implement reusable slices in the native engine and retain the broader feature-task evidence.

No calendar promise is introduced. T-63–T-68 remain open until tested. Pending recovery edits
are preserved and reviewed independently, not swept into this planning change.
