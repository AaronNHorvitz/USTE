# Decision 0036 — Complete bounded graph preparation proofs

Date: 2026-09-17

Status: accepted as T-20 preparation groundwork. T-20 remains open because the live coordinator,
terminal-root delta derivation and recovered reducer still require complete in-memory state, and
BM-01/BM-06 are unqualified.

## Context

Decision 0035 separated explicit authenticated I/O from pure graph preparation, but excluded
deletion and historical `ReadView` predicates. Deletion needs a complete reverse-dependency bucket
for its target; a historical predicate needs the complete version prefix needed to select the last
record at or before its declared revision. Treating either an unscanned bucket or unproven history
as empty would be unsound.

## Decision

The disk preparation loader now derives two additional target sets from the consumed transaction:

- every `ReadView` predicate record receives a complete family-3 history-prefix proof as well as
  its current-record proof;
- every deleted entity receives a complete family-7 reverse-owner bucket. Declared cascade owners
  and the references retained by their terminal versions also receive current-record proofs.

History and reverse scans use the caller-owned authenticated page cache. Aggregate history-version,
reverse-reference and logical-byte limits are checked by the storage scan before collection; each
individual prefix is additionally subject to the frozen 64 MiB index result cap. Empty buckets can
be proven with a zero entry budget, while a nonempty bucket fails at the first excess entry.
Decoded history keys, record IDs and modified revisions are cross-checked. Reverse keys, reserved
bytes, owner versions and revisions are decoded into the existing private reducer descriptors.

The I/O-free phase rechecks that every required current, history and reverse proof bucket exists,
then supplies those partial maps to the unchanged graph reducer. All existing operation variants,
correction preconditions and `Expected` variants are now supported by this proof path.

## Consequences and limits

No wire, journal, checkpoint, reducer-profile or `graph-state-v1` format changes. The root remains
a derived cache admitted against the authoritative current reducer. Prefix scans collect one
bounded bucket rather than stream directly into reducer evaluation, logical proof bytes are not an
RSS measurement, and the partial preparation result is not yet a coordinator commit capability or
root-delta source. A live persistent base/overlay, recovery without a complete reducer and
qualifying BM-01/BM-06 runs remain T-20 work.

Decisions 0037 and 0038 subsequently use this proof for terminal-root derivation and an exact
request-bound authoritative commit. Live publication and recovery remain complete-state
operations.
