# Decision 0069 — Shared disk commit preflight

Date: 2026-09-18

Status: T-20 write-enabling increment; not an authorized consumer write facade.

Disk-aware authorization must identify exact retries, transaction collisions and admission
failures before performing external graph proof preparation. Preparation can otherwise fail on
an already-applied request or stale expected version before the coordinator can return its exact
durable retry. Duplicating the admission rules in the consumer facade would create two contracts.

Extract the existing coordinator's read-only admission into one internal function used by both
normal commit and `DiskCommitCoordinator::check_commit`. Preserve its ordering: request shape and
scope, clock sample, base/overlay retry and expiry, transaction collision, overlay capacity,
cancellation, first-owner consistency/capacity, next revision and expiration. The extracted code
is unchanged apart from wrapping its result. The normal commit still prepares once, checks
cancellation again, certifies the journal, and then publishes state and overlays without disk
metadata reads after certification.

`DiskCommitCheck::Retry` carries the existing outcome; `Ready` carries the next revision. Neither
is authorization, a reservation, a domain validation result or a new durable acknowledgement.
Preflight never prepares state, writes the journal, reserves overlay space or transfers ownership.
A later commit repeats admission and may fail or see a different next revision. Callers must
authorize before using this privileged API. A future integrated writer can retain one sampled
clock observation across its check/preparation/commit sequence under its exclusive mutable borrow.

Regression coverage verifies exact retry before cancellation, conflicts before preparation,
expired retry tombstones, cancellation, new-owner capacity refusal, unchanged state/overlays/
frontier and zero writes under an armed write fault. The fault must subsequently fire during a
real commit; the resulting uncertain coordinator refuses preflight, and restart returns the
certified frontier. A later successful commit advances the next check's revision. Existing
commit/rebase/recovery fault matrices and graph/memory/transaction tests remain unchanged gates.
Disk-aware consumer commits, staged-upload accounting and qualification remain required.
