# Certificate-anchored graph state root evidence

Decision 0029 adds a distinct `graph-state-v1` derived cache without changing `graph-current-v1` or
the journal/reducer formats. The publisher streams eight canonical family iterators into encrypted
immutable runs, independently checks entry counts and logical run digests, and publishes only a root
bound to the exact live certificate, reducer profile and logical state digest.

The disk integration fixture publishes current records, full record histories, adjacency,
provenance and reverse-reference families, fully scrubs seven nonempty runs, restarts the encrypted
journal and admits the same root. A separately published, authenticated and correctly bound root
with wrong metadata is declined. After a later commit, the former state root is neither admitted nor
scrubbable as current. Policy-only unit coverage pins the canonical empty-family omission, 80-byte
metadata counts and current/history key ordering; reverse values have an exact fixed 24-byte layout.
An independent canonical fixture asserts every current/history/adjacency/provenance/reverse key and
value, the omission of an empty policy family, all seven entry counts and seven fixed logical run
digests. The acceptance profile and Decision 0029 also pin metadata offsets plus every reverse
kind, state and role assignment.

Reproducible focused commands:

```console
cargo test -p uste-graph --all-targets --locked --offline
cargo clippy -p uste-graph --all-targets --locked --offline -- -D warnings
```

This evidence does not claim bounded recovery or a disk-backed live reducer. Admission currently
recomputes the complete expected projection from a full in-memory snapshot, and no state reader is
exposed. The journal remains the only authority.
