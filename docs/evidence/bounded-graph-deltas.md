# Bounded graph transaction delta evidence

Decision 0027 replaces the production graph reducer's prepared full snapshot with an ordered
before/after delta. A 1,024-record regression fixture updates one record and proves that the
prepared value retains exactly one change, its result digest matches the prior canonical
`USTE-GRAPH-RESULT-V1` construction, and incremental publication equals a test-only full-index
rebuild byte for byte.

A second regression prepares two revision-2 changes from the same revision-1 base, publishes one,
then verifies that publication of the stale delta panics before any live-state mutation. Prepared
deltas are not cloneable and bind their exact scope and base revision.

The existing graph suites continue to cover atomic failure, corrections, policy history,
read-view predicates, embedded references, cycles, self-loops, parallel edges and exact cascade
deletion. Derived-index rebuild checks compare outgoing, incoming and provenance indexes with the
record state. Cascade regression additionally proves that retracting an accepted relationship
removes adjacency while preserving its evidence provenance.

Reproducible focused commands:

```console
cargo test -p uste-graph --locked --offline
cargo clippy -p uste-graph --all-targets --locked --offline -- -D warnings
```

This evidence does not claim a disk-backed graph reducer, bounded checkpoint decoding or T-20
completion. Entity deletion still scans retained records, and explicit snapshots still clone the
full in-memory state. The next profile needs a general reverse-reference family before deletion can
use bounded prefix reads.
