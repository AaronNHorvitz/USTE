# Decision 0090 — Retain structural validation of immutable cached pages

Date: 2026-09-19

Status: T-20 partial implementation; qualification remains open.

Decision 0089 removes page-parser allocation, but repeated cache hits still revalidate every
fragment and all padding. Retain an optional fixed-size `PageLayout` alongside each immutable
cached plaintext page: used length, fragment count and final key start/end offsets. Only complete
successful structural parsing installs it. No history, fragment vector, copied keys or additional
heap allocation is retained. Failed parsing never marks a page validated.

Every cached read still validates the complete existing header context: size, magic, format,
family, reserved byte, revision, page ordinal, index profile and run object. In particular, cache
identity alone is not used as evidence of the requested family. The page bytes are privately owned
and immutable for their cache lifetime, so successful padding/fragment/local-order validation may
be reused. Actual returned fragments and cross-page/value continuity are still checked by their
existing iterators and read operations. Cache clear, eviction and clock-overflow invalidation drop
the layout with its zeroizing page; an inserted replacement always starts unvalidated. Scrub still
clears the cache before rereading durable encrypted pages. Uncached streaming reads fully parse
each newly authenticated page.

The four scalar offsets/counts fit within Decision 0086's existing 1 KiB per-entry logical metadata
allowance; the layout/capacity regression checks the actual inline payload. Default/public cache
budgets and capacity are not increased, and logical accounting remains distinct from allocator/RSS
measurement. Existing adapter and cached primitive telemetry meanings remain unchanged: fragment
counts describe operation enumeration, not the parser's internal validation pass.

Tests compare both initial cached reads and repeated hits with the collecting parser for every
single-byte mutation, require failed pages to remain unvalidated, reject family/object/profile/
revision/page substitutions after successful memoization, and prove clear/eviction cannot carry
validation to a malformed replacement. Full index, authorization, fault/recovery, native exact
oracle and one-page eviction suites remain required. This changes neither persisted format nor
durability/authorization contracts and does not qualify BM-01/BM-06 or remove resident journal maps.
