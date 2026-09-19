# Decision 0105 — Reverse metadata correspondence

Date: 2026-09-19

Status: implemented and locally verified. T-20 remains open.

Use Decision 0104's authenticated reverse range for two more order-independent admission passes:
storage blob/inventory catalog correspondence, and the coordinator's final retry/reference
correspondence pass. Each group remains individually bound to the pinned frontier before exposure;
only whole-call success admits a derived base. Formats, roots, authorization and durability do not
change. Work limits remain explicit, with all selected certificate/group reads and the initial
terminal proof charged to the same pass byte allowance.

First-reference claims remain exact: every occurrence must match the stored reference, a declared
first revision cannot be later than any occurrence, and exactly one actual occurrence must match
each declared first revision. Coordinator first occurrences must also match the stored principal.
Full run cardinality, namespace sums, inventory counts and total reference bindings still reject
missing or extra entries. These checks are commutative; descending order does not turn earliest
ownership into latest ownership. Retry correspondence likewise compares each exact journal entry
against its independently authenticated keyed value.

Leave construction, ordered reducer replay and the compatibility per-owner earliest-discovery
pass forward ordered. Storage catalog construction can still do repeated forward certificate
proofs and complete immutable merges. Compatibility owner admission can still scan the prefix
once per owner. This increment removes neither cost and introduces no aggregate quota index.

Extend catalog tests with the exact linear journal allowance and one-byte-short refusal on a
five-revision, repeated-reference, multi-namespace fixture. Extend the first-reference claim/read-
fault matrix to both resident and disk-certificate recovery with exact per-pass byte allowances;
all existing false-earliest, missing-key and malformed-value cases remain. Full affected catalog,
coordinator, graph and native regressions pass; PROGRESS.md records commands and limits.
