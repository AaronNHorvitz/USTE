# Decision 0282: Candidate memory consumer contract `uste-memory-consumer` 1.0

Date: 2026-09-26

Status: Accepted as the producer's candidate contract (DB-R04 producer side); not consumer-agreed,
not integrated.

Under the delegation in Decision 0281, define the versioned generic Rust memory consumer contract
from the existing research-memory implementation, give it a producer implementation, and pin
synthetic conformance fixtures, without claiming consumer agreement.

Decision. Adopt [`uste-memory-consumer` 1.0](../memory-consumer-contract.md):

- **The trait.** `uste_memory::contract::MemoryConsumerContract` defines `write(operation,
  request, cancellation)`, `view()` and `read(view, request, cancellation)`.
- **Supporting types.** It comes with fail-closed `negotiate_contract`, consumer-chosen
  `OperationId`s, `ContractWrite` requests and a closed, content-free `ContractError` set.
- **The producer.** `uste_memory_adapter::research::ResearchMemoryProducer` implements it over
  the authorized durable coordinator, with a consumer-supplied policy kernel, principal and clock.
- **Source bytes.** The consumer supplies bytes; the producer stores them and binds the blob,
  and never exposes a blob, filesystem or coordinator capability.

Contract 1.0 admits artifacts without retained content only. It keeps no upload outbox: after
process loss, recovered-upload reconciliation completes with an empty outbox. An interrupted
upload was never committed; its staged bytes are unreferenced storage for T-35.

Retries. A replayed write returns its recorded receipt through the coordinator's idempotency
layer. A retried `PutSource` cannot replay byte-identically, because storing the bytes again
would bind a new blob. The producer therefore first looks up the operation's recorded outcome.
It then confirms through the new `SourceVersion` read that the stored version equals the request
in every descriptive field, with content compared by length and SHA-256. Only then does it return
the original receipt, without storing anything. A mismatch is `Conflict`.

That read confirms one exact source version in the current generation. It is served while the
generation is rebuilding, so retries during a rebuild resolve, and it needs `ReadRecord` on the
source. The source view gains the remaining descriptive fields: kind, run identity, route,
license, redistribution, freshness policy and media type.

Conformance. `run_conformance` drives any implementation through 34 ordered synthetic steps:

- version negotiation;
- reads before and during a generation;
- source admission, exact retry and refusal of inaccessible-with-bytes;
- citations beyond the source;
- a direct-quote claim read with freshness;
- search;
- correction with a stale view;
- an edge;
- revocation withholding excerpts;
- cancellation leaving no record;
- cross-scope denial and a zero budget;
- policy replacement with stale-policy and hidden-citation reads;
- a new generation.

The fixture is pinned by `CONFORMANCE_FIXTURE_DIGEST`, the candidate contract revision:

    b145b45ebb5668947ffb160a95a5aee8d93364bc4879f7428f31a97a196ef034

It covers the contract name and version, every step name and expected outcome, and the canonical
bytes of every fixture record.

Verification (focused, under the shared heavy-work reservation). The producer passed all 34 steps
on the durable authorized coordinator with an in-memory filesystem. It then survived a filesystem
restart and reopen, where the second generation was still rebuilding, and it accepted and
completed further writes. The digest pin, fail-closed negotiation and the confirmation read have
unit tests. All `uste-memory` and `uste-memory-adapter` tests and strict Clippy passed; the
reconciled tree-wide stages are recorded with the commit.

Open. Consumer review of this contract, consumer implementations and an authorized combined
integration run remain outside USTE's authority and keep DB-R04 unchecked. A later minor version
may add artifact content, streaming uploads or an upload outbox; any change to the step list or
fixture records changes the pinned digest and needs a new decision.
