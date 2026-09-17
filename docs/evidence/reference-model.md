# T-10 independent reference-model evidence

Date: 2026-09-17 · Product toolchain: Rust/Cargo 1.95.0 · Target: x86_64-unknown-linux-gnu

Reviewed implementation: commit `99536247b98c3daa39f5fecd3e1a497166dd58b6` · tree
`44bbbac64b30ab4778556228a9149a2c66478098`. Two read-only Codex agent audits compared the
implementation and tests with Decision 0003, the data model, VT-01 and the literal R0 vectors.
Their findings about relationship semantics, delete closure, correction preconditions, reference
limits and generated-history coverage were corrected before the recorded commit. Their final
audits reported no blocker. This is automated implementation review, not independent human review
or security certification.

This record closes T-10's independent logical-oracle scope. It does not claim a production
transaction coordinator, durable storage, authorization, encryption, indexed graph, executable
database or distribution readiness.

## Implemented contract

- `uste-testkit` depends only on `uste-types` and `std`. It deliberately uses ordered maps,
  full-state cloning, scans and retained snapshots rather than sharing a future production
  reducer, adjacency index, journal or transaction algorithm.
- One successful nonempty transaction advances `CommitRevision` exactly once and publishes only
  after every operation and final-state invariant succeeds. Any typed error leaves latest state,
  history and revision unchanged. Preconditions are evaluated against the pre-transaction root.
- Expected absence, exact record versions and exact retained-read-view predicates are modeled.
  Correction creates a separately identified proposed claim and requires its own declared
  precondition; it never silently changes or auto-supersedes the accepted predecessor.
- Entity, immutable evidence, assertion and relationship records carry explicit scope and checked
  versions. Relationships are evidence-backed assertions with valid time, lifecycle and correction
  linkage rather than evidence-free edges. Current endpoints and nested generic-value references
  must close over visible records in the same namespace.
- Assertion and relationship lifecycle is the closed Decision 0003 state machine. Purge is
  rejected as a lifecycle action. Entity deletion rejects by default; explicit bounded cascade
  retracts accepted dependent claims, refuses proposed claims and fails closed for entity-property
  references that cannot be safely transformed.
- Valid time preserves `Unknown` separately from explicit unbounded half-open intervals. Membership
  returns `None` for unknown; a bounded start is inclusive and a bounded end is exclusive. Exact
  retained-revision reads prevent late assertions and corrections from leaking into older views.
- The model enforces 10,000 operations and a conservative 100,000 record-reference occurrences per
  request before cloning state. The reference count includes operation targets, new record IDs,
  predicate references, endpoints, evidence and every nested generic-value occurrence. The 16 MiB
  encoded request limit remains an admission/codec responsibility because T-10 defines no operation
  wire format.
- Seeded entity and graph generators have explicit call bounds and no clock, OS randomness or
  external dependency. Graph histories create evidence-backed proposals, accept them, create linked
  assertion and relationship corrections, then accept those corrections. Tests assert the expected
  record/status/correction projection rather than only replaying the oracle against itself.

## VT-01 and regression coverage

The integration suite executes all ten rows of `acceptance/r0/transitions.tsv` and also tests the
complete reachable status/action matrix. It covers:

- correction linkage, predecessor preservation, purge refusal and explicit correction absence;
- unknown/unbounded/one-sided/bounded intervals, empty/reversed rejection and `[start, end)` edges;
- foreign database/namespace references, including nested and terminal values;
- missing/duplicate/wrong evidence, missing endpoints and forward references closed within one
  atomic transaction;
- absent/version/read-view conflicts, originally false predicates, changed predicates, unknown
  revisions, foreign predicates and a successful unchanged predicate;
- rollback after a later operation fails, single-revision multi-operation publication, empty
  transaction refusal and both accepted resource caps;
- default deletion rejection, bounded relationship/assertion retraction, proposed blockers and
  untransformable nested entity-property references;
- late valid-time assertions and later corrections at earlier knowledge revisions; and
- two deterministic generated workloads, injected version conflict rollback, graph lifecycle
  projection and stable replay from a seed.

~~~text
cargo test -p uste-testkit --all-targets
# 14 passed; 0 failed

cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked
# passed; 33 workspace tests passed

bash scripts/check.sh
# passed: workspace checks/docs/task graph; 33 workspace, 12 R0, 4 publication,
# 4 fixture-generator and 4 content-fixture tests
~~~

## Deliberate limits and downstream use

The oracle retains a complete clone per commit and derives relationships by scans. Its memory and
query cost are intentionally unsuitable for production; this keeps later `uste-storage`,
`uste-txn` and `uste-graph` implementations algorithmically independent. Generator call caps are
test-harness safety limits, not benchmark claims.

T-14 must map durable idempotency and commit behavior onto these precondition outcomes. T-16 owns
authorization, quotas and policy-specific forbidden-content minimization. T-17 must replay the
exported histories against its independent production reducers, extend them with indexed delete and
conflict workloads, and compare observable records at every revision. Until those tasks pass, the
oracle is specification evidence rather than evidence that a database implementation agrees with it.
