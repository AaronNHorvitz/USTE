# Decision 0089 — Allocation-free page validation and native eviction regression

Date: 2026-09-19

Status: T-20 partial implementation; qualification remains open.

The pinned 10,000-entity native development observation at `5a19c37` matched all 384 queries,
but recorded 420,631,561 cache hits and 672.6 seconds of query work. Inspection found that every
`ParsedPage::new` collected all fragments into a temporary vector, and binary page-boundary
searches enumerated them again to obtain the last key. This is a concrete repeated cost, not a
profile-derived attribution of total runtime.

Replace the temporary vector/adjacent-window check with a complete streaming pass retaining only
the previous fragment and count. Exhaust the iterator so trailing bytes still reject. Retain the
validated final borrowed key in the ephemeral parsed-page object; boundary searches no longer
walk the fragments a second time. No persistent cache structure, logical accounting allowance,
page format, cryptography, authorization or publication rule changes. Every page visit still
validates its complete header/context, padding, fragment encoding and local ordering; cache hits
do not bypass family validation. Cross-page/value continuity checks remain at their existing layer.

The test-only old collecting validator supplies differential acceptance/error/last-key checks for
every single-byte mutation and a count/length/offset boundary matrix. All page truncations reject;
valid repeated-key fragments and empty values remain supported. Existing golden, encrypted-read,
cache, corruption, reference and publication-fault tests remain required.

The native correctness harness additionally reruns the complete 20-entity/200-relationship query
corpus after a cold reopen with a one-page cache, requiring actual evictions and identical oracle
outcomes, counts, visits, logical bytes and digests. This is a component pressure regression, not
qualifying BM-01 or larger-than-memory evidence. Only the private harness accepts that cache
budget; public commands retain 64 MiB and all accepted benchmark targets remain unchanged.
