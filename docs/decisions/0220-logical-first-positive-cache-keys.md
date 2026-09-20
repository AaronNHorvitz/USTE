# Decision 0220: Logical-first private positive-cache keys

Date: 2026-09-20

Status: Implementation in progress; uncompiled and unverified. Excluded from D0219 measurements.

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
