# Explicit-I/O graph preparation evidence

Decision 0035 adds a bounded authenticated current-record proof phase without changing canonical
graph, journal, checkpoint or index formats.

## Implemented behavior

- Unsupported deletion and historical `ReadView` predicates fail before root/page access.
- A current admitted `graph-state-v1` root supplies exact authenticated policy and record lookups.
  Positive and negative proofs are distinct, and decoded IDs/revisions/scope are checked.
- Proof retention is bounded by unique record count, reference-occurrence count and logical
  key/value bytes. The supplied `PageCache` remains caller-bounded and index work is reported.
- Create/replace/correction inputs prove their syntactic references. Non-correction assertion and
  relationship transitions additionally prove every reference retained from the current record.
- `GraphDiskPreparationView::prepare` has no I/O capability. It rechecks closure completeness and
  reuses the production reducer's validation, overlay, policy and result-digest logic.

## Verification

~~~text
cargo test -p uste-graph --test disk_index bounded_disk_preparation_supports_current_history_reverse_and_stale_roots -- --exact
# 1 passed
cargo clippy -p uste-graph --all-targets -- -D warnings
# passed
bash scripts/check.sh
# workspace format/clippy/test/rustdoc/docs pass; 262 workspace tests; documentation links=91,
# active IDs=88, definitions=146; 62-task graph remains acyclic with T-62 distribution-only
~~~

The fixture proves a four-record create closure containing three positive and one negative proof,
an existing assertion transition whose unchanged references expand to four positive proofs, exact
budget acceptance, one-less proof/reference/logical-byte rejection, occupied correction-ID
rejection, result-digest equality with the actual encrypted durable commit, stale-root rejection,
and explicit deletion/`ReadView` rejection. Review-found correction-ID and preallocation/work-limit
holes were corrected before this evidence was accepted.

## Deliberate boundary

This is a privileged preparation foundation, not a consumer API, live disk-backed reducer,
publication path or benchmark result. Decision 0036 subsequently adds bounded complete deletion
and historical-precondition proofs, and Decision 0037 derives a `GraphStateRootDelta` from the
complete proof. The coordinator still owns a complete in-memory reducer; index exact-lookups have
format bounds but no new caller-selected per-lookup page limit; logical proof bytes are not RSS.
T-20 and BM-01/BM-06 remain open.
