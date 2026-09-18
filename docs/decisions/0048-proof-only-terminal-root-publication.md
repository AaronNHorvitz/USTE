# Decision 0048 — Proof-only terminal-root publication

Date: 2026-09-17

Status: accepted as bounded T-20 write-path groundwork. T-20 remains open because the live graph
and coordinator metadata remain memory-resident, recovery is not larger-than-memory and BM-01/
BM-06 are unqualified.

## Context

Decisions 0035 through 0038 allow one graph transaction to load a complete bounded authenticated
proof, prepare without hidden I/O, derive exact changes for all eight `graph-state-v1` families and
commit the proof-bound result through the authoritative journal. Postcommit root publication still
borrowed the complete live `GraphSnapshot` twice: once for the canonical logical-state digest and
again for expected family descriptors. That defeated the otherwise bounded proof/merge path and
could not serve a future disk base plus request-sized overlay.

## Decision

The storage merge primitive gains a provisional exact-output visitor. It observes each ordered
key/value pair before encryption while the target run remains unrooted. Visitor effects are not
authority and must stay private until source terminal authentication, target write/size/file sync
and directory sync all succeed. A visitor error can leave only an opaque unreferenced scratch run;
it cannot publish a root or change journal authority.

`GraphStateRootDelta` now privately retains its proof-derived target family counts. Postcommit
publication checks the exact base anchor, durable outcome and current certificate, then validates
the entries emitted by each of the eight authenticated merges. It requires the exact metadata
entry, decodes current/history/policy records, checks key/content scope and revision bindings,
checks adjacency/provenance/reverse framing and reverse kind/state/role/version/revision
constraints, checks every output descriptor's family count, and reproduces the existing
`USTE-GRAPH-LOGICAL-STATE-V1` digest from the actual merged primary families. It no longer obtains
or scans the live reducer snapshot.

Canonical history framing places a record's version count before its version bytes. The validator
therefore retains at most one record's encoded history frames. `GraphStateRootMergeLimits` requires
an explicit nonzero bound for that record-local buffer in addition to the eight existing family
merge budgets; exceeding it fails before root publication. The bound may be selected up to the
existing run carrier limit and is never inferred from total retained graph size.

The safety argument is inductive. The base is an already semantically admitted, journal-anchored
root. The opaque plan comes from a complete transaction proof and binds exact before/after deltas,
request result, base and target revisions. Each merge authenticates the complete base run and
requires every declared before-value exactly. Target counts, canonical primary-state digest and
successful terminal merge authentication therefore validate the new root without trusting a
second full-state projection. The journal remains the sole commit authority and the root remains a
rebuildable cache.

## Evidence and limits

The storage test proves the provisional visitor sees byte-identical ordered output to a later
authenticated run read. The graph unit tests reproduce the canonical digest from all emitted
families, reject malformed entries in each secondary family and reject both a secondary count
mismatch and a one-byte history-group budget. The end-to-end disk-index fixture exercises
create, replacement, relationship retraction, assertion creation, policy replacement, insert/
replace/delete merges and empty output families. It rejects wrong outcomes, undersized merge
budgets and the record-local history bound, observes no target root after failure/restart, retries,
then proves the proof-only root and independently full-published root both reconstruct to the exact
live state.

This removes the complete-state postcommit validator, not the complete live reducer. Ordinary
publication, coordinator retry/transaction/blob-owner maps, recovery reconstruction, root
admission and consumer snapshots remain memory-resident. Frozen `graph-state-v1` still rewrites
one terminal run per nonempty family and is not a persistent multi-run overlay. Exact BM-01,
BM-06, streaming disk-base admission and the live base/overlay lifecycle remain required.

## Verification

~~~text
cargo test -p uste-storage --lib --locked \
  journal::tests::authenticated_index_merge_streams_exact_deltas_and_publishes_only_terminal_output -- --exact
# 1 passed
cargo test -p uste-graph --lib --locked \
  merged_output_stream
# 2 passed
cargo test -p uste-graph --test disk_index --locked
# 4 passed
cargo clippy -p uste-storage -p uste-txn -p uste-graph --all-targets --locked -- -D warnings
# passed
~~~
