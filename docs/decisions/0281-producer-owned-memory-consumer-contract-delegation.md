# Decision 0281: Producer-owned memory consumer contract design is admitted

Date: 2026-09-26

Status: Accepted as a scope and dependency clarification; no consumer agreement or integration.

DB-R04 asks for a restricted Rust adapter and conformance fixtures against a "pinned generic
consumer contract". The previous hold (TASKS, after Decision 0280) read that dependency as a
contract that consumers must author first. On 2026-09-26 the repository owner delegated the
design of that contract to USTE as its producer: USTE may define and pin a versioned generic Rust
memory consumer contract and synthetic conformance fixtures in this repository, derived from the
existing research-memory implementation.

The dependency is therefore split. The producer-side contract design, its producer
implementation and synthetic conformance fixtures are dependency-ready now. Consumer review of
the contract, consumer-side implementations and any combined authorized integration run remain
separate acceptance work that USTE cannot perform or approve; DB-R04 stays unchecked until they
exist. USTE does not inspect other repositories, claim consumer agreement or cross-stack
integration, or change any consumer's authority.

The contract covers only operations the implementation already supports: generation lifecycle,
source, artifact, claim and edge admission, retraction, expiry and revocation, policy
replacement, and the four bounded research reads, with their authority, scope, revisions,
citations, freshness, budgets, typed errors, cancellation and restart semantics. Its candidate
revision and fixture digest are pinned in the contract decision that implements it.
