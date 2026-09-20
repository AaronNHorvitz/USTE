# Decision 0187: Buffered quota admission

Date: 2026-09-20

Status: Accepted

Extend Decision 0145 with `admit_packed_quota_prefix_buffered`, following Decision 0186's
operation-local cache contract. Independently admit each of the three canonical families with
a fresh cache, dropping it before the next. Then use a separate fresh bounded cache for quota
metadata, principal totals, owner traversal and primary-owner correspondence. Accept a budget,
never caller-warmed pages; drop all caches before returning the admitted quota prefix.

Retain primary/quota scope, certificate owner, revision, profile, family cardinality, owner
bijection and checked aggregate validations. Empty-owner/zero-byte semantics and the uncached
API are unchanged. Cursor and lookup proof budgets charge hits identically to misses. Invalid
cache budgets refuse before I/O. Fixed canonical/correspondence cache reports describe sequential
phase residency, not summed concurrent memory or complete authenticated/device I/O.

Run the four existing quota-admission cases both uncached and buffered, including empty/populated
cold pairing, exact/minus-one limits, authenticated false aggregates/owners, every observed read
error/crash followed by cold restart, cursor limits and ciphertext changes after prior successful
admission. Preserve the uncached 381-fault count; derive buffered fault cases from its actual
trace. Additional one-page/larger-cache checks compare family commitments, every cursor and
lookup proof counter, bounded reports, fresh repeat behavior and actual adapter-read reduction.

Native harness integration remains a separate verified increment. This changes no persisted
format, authorization or quota semantics, establishes no larger-than-memory benchmark pass,
and leaves all accepted workload targets, M1 consumer interfaces and release gates unchanged.
