# Decision 0220: Logical-first private positive-cache keys

Date: 2026-09-20

Status: Implemented and locally verified. Excluded from D0219 measurements; no performance claim.

D0216/D0218 retain cold-query regressions for the optional positive-cache partitions. Do not
reinterpret those results or change the default. One bounded CPU-cost hypothesis is that ordered
cache searches repeatedly compare the same 175-byte root identity before reaching the logical
key. Put the logical key first and the complete fixed-length identity last in this private
in-memory lookup encoding. This is not an on-disk format, public key ordering or hash change.

Equality remains exact and injective: encoded length minus 175 determines the logical-key length;
equal complete byte strings therefore have equal keys and every identity byte. Preserve all
database/namespace/profile/family/revision/root/commitment components. No digest, shortened tag,
probabilistic comparison, trusted caller assertion or root-admission shortcut substitutes for them.

The single fallible zeroizing allocation has the same requested capacity and exact total length.
Retained charge, shared key ownership, value zeroization, proof-work charging, cache budgets and
oversized bypass remain unchanged. Ordered-map iteration is private and not used for eviction;
the separate stamp map still determines exact LRU. Owner/session and current authorization checks
remain ahead of lookup. Default page-only and uncached paths are unchanged.

Require independent variable-byte LRU/counter/accounting equality, all identity substitutions,
variable-length/prefix/maximum-length key injectivity and direct prefix-layout checks. Preserve
all existing resource/fault/corruption/authorization tests and run full workspace/native gates
before adopting the change. Reduced shared-prefix length is not a measured performance gain:
native timing and work-count comparison remain separate, with failures/regressions retained.
No M1 interface, authoritative migration, erasure guarantee or benchmark threshold changes.

The resumed 2026-09-20 verification passed the focused lookup tests, 761 workspace tests, strict
workspace Clippy, warnings-denied documentation, and 136 active native release tests with five
unchanged opt-in ignores plus strict native Clippy. Formatting, documentation and task-graph
checks also passed. The sandbox could not create the originally planned nested transient scope
after restart, so both sequential one-job/one-thread gates ran inside the verified enclosing
`uste-codex.scope` (5 GiB high, 6 GiB maximum, 512 MiB swap maximum) with an inherited 4 GiB
virtual-address limit. The enclosing scope reached 2,504,941,568 bytes peak and zero swap; that
shared peak is a session bound, not an isolated workload RSS measurement.
