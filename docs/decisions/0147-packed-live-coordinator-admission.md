# Decision 0147 — Packed live coordinator admission

Date: 2026-09-19

Status: implemented and locally verified opt-in T-20 live coordinator; qualification open.

Install independently admitted packed primary and quota prefixes into a separate raw coordinator
without reconstructing complete retry/transaction/owner maps. Installation consumes the exclusive
recovery owner and requires exact current-frontier scope/certificate, live tree bindings, quota
pairing and published root receipts whose profiles and physical family descriptors match those
prefixes. Primary/quota reducer and state-commitment claims must agree. A trusted domain-state
implementation must independently validate those exact claims against its ready state; matching
metadata alone does not admit a reducer. Existing graph-state-v1 digest semantics are unchanged.

Reuse the existing authoritative commit engine and read-only admission logic. Select either the
existing run-backed metadata or the new packed metadata through an internal bounded read adapter.
Overlay lookup remains first; historical-base lookup retains exact principal/idempotency outcome,
transaction-ID collision and first-owner semantics. Expiry, cancellation, request binding,
uncertainty and postcertificate publication ordering remain shared, not reimplemented.

The new coordinator retains only caller-bounded post-base outcome/transaction/first-owner overlays.
Exhaustion refuses fresh commits while exact retries remain answerable. This initial live entry
point does not silently rebase, reopen into complete maps, discard quota state or grant consumer
authorization. Dedicated packed rebase and authorized/domain adapter integration follow; they
must preserve the admitted accounting projection and exact root/visibility boundaries.

Installation and trusted raw commit tests must cover rejected roots/domain claims/owners, existing
base retries and expiry, cross-principal transaction collisions, bounded overlay refusal, exact
retries after new commits, cancellation, injected commit uncertainty and cold recovery. Legacy
run-backed admission remains covered by its unchanged reference and fault suites. No native/M1
switch, benchmark qualification or T-20 completion follows from this opt-in path alone.

Verification: six focused integration tests passed, including 60 injected commit faults and cold
reference recovery. The full workspace passed 555 tests across 47 executables, followed by
warnings-denied Clippy/docs. Tests cover base and overlay retry precedence, expiry, cross-principal
collisions, tiny owner/outcome ceilings, altered references, all three read-family corruptions,
lookup exhaustion, prepared-value binding, both cancellation boundaries and rejected installation
claims/foreign owners. Commands and process-group resource observations are in PROGRESS.md.
