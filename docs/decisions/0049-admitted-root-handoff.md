# Decision 0049 — Admitted terminal-root handoff

Date: 2026-09-17

Status: accepted as bounded T-20 write-path groundwork. T-20 remains open because cold semantic
admission, the live graph/coordinator state and recovery remain memory-resident and BM-01/BM-06
are unqualified.

## Context

Decision 0048 validates a proof-derived terminal root without consulting the complete live graph,
but the graph API discarded the resulting authenticated root descriptor and returned only its
revision/generation receipt. A caller needed `load_graph_state_roots` and a complete live snapshot
to regain the opaque `DerivedGraphStateRoot` required by the next disk preparation. This reintroduced
a full-state comparison between otherwise bounded consecutive disk transactions.

Cold candidate discovery is a different problem. It must prove current and historical reference
closure, history transitions, derived-family correspondence and policy ordering without retaining
the graph. The current complete-run callback cannot interleave authenticated lookups because it
holds the filesystem borrow. Framing/count/digest validation alone is not cold semantic admission.

## Decision

The storage root publisher can now return the exact `RecoveredIndexRoot` assembled into the
successfully synchronized manifest. The existing receipt-returning API remains and projects the
same revision/generation fields. The coordinator exposes this as trusted maintenance: it still
requires the current certificate, scope, run bindings and successful file/directory synchronization
and does not grant domain semantic authority.

After Decision 0048 has terminally validated every merged family and canonical graph digest,
`publish_graph_state_root_delta` uses that recovered-root return and wraps it directly as the
opaque `DerivedGraphStateRoot`. No post-publication manifest reread or live-snapshot comparison is
required. The returned handle can immediately serve the next explicit disk-preparation proof and
remains usable after the backing filesystem and coordinator reopen, provided the caller retained
the handle and the journal still recognizes its certificate anchor.

Returning the handle only after the durable publication succeeds preserves the existing authority
boundary. Merge output observed before terminal authentication stays provisional; a publication
error returns no handle; page reads through the handle remain authenticated; stale revision or
certificate checks still fail before preparation.

## Evidence and limits

The complete disk-preparation fixture now carries the proof-published revision-two handle across a
filesystem restart and coordinator reopen. It uses that handle directly for both an exact-minus
proof-limit failure and a successful assertion transition proof; it no longer calls the
full-snapshot `load_graph_state_roots` path at that boundary. The broader terminal-root fixture
still independently rediscovers and reconstructs the root to preserve oracle coverage.

This is a warm/retained-handle handoff, not cold semantic admission. A process that loses the
opaque handle must still reconstruct a complete `GraphState` to admit a candidate. The next
increment needs a resumable authenticated run cursor plus bounded predecessor lookup before it can
produce a cold `GraphDiskBase` without full maps. Live reducer, coordinator metadata and recovery
remain memory-resident, and frozen `graph-state-v1` remains a terminal full-family format.

## Verification

~~~text
cargo test -p uste-graph --test disk_index --locked
# 4 passed
cargo test -p uste-storage --lib --locked \
  journal::tests::every_index_root_publication_boundary_recovers_an_old_or_exact_new_root -- --exact
# 1 passed
cargo clippy -p uste-storage -p uste-txn -p uste-graph --all-targets --locked -- -D warnings
# passed
bash scripts/check.sh
# 263 workspace tests and 31 isolated t20-bench tests passed; 2 exact-profile release tests ignored
~~~
