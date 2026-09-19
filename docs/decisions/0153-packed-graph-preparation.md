# Decision 0153 — Bounded packed graph preparation

Date: 2026-09-19

Status: accepted and locally verified T-20 proof preparation; commit/live integration open.

Prepare graph transactions from an opaque admitted packed base and explicit owner-bound I/O.
Reuse the existing request/reference limits, required-record closure, history/reverse target
selection and pure graph reducer. Only required current records, history groups for read-view
predicates and reverse owners for deletions are retained, under existing logical proof ceilings.
No complete graph snapshot, ambient I/O in the pure reducer, or fabricated v1 root anchor is used.

Bind the new opaque prepared result to the exact packed base's scope, certificate, ordered-state
commitment, counts and policy. Preserve the existing canonical request binding and journal result
digest. This is not permission to commit: live state validation, consumer authorization and
durable publication remain mandatory integration steps.

Packed reads have explicit per-lookup and aggregate operation/page/encoded-byte/candidate bounds.
Prefix cursors exhaust the exact prefix, with seek probes and boundary witnesses consuming work
but not result budget. Typed history/reverse decoding checks keys, scope and revisions before
pure preparation. Corrupt or resource-exhausted reads must not become absence or a partial proof.
Return separate packed primitive work; do not invent legacy fragment/cache statistics. The reused
logical preparation report leaves legacy run/cache counters zero; those zeros are not a no-I/O
claim. Cached admitted metadata/policy is charged to logical proof retention without rereading
unchanged bytes. Every tree's owner/key/scope binding is rechecked before preparation, even for
a policy-only request.

Reference tests must compare accepted results and rejection semantics with the in-memory reducer,
cover dependencies, historical read-view predicates, reverse-dependent deletion, corrections and
policy changes, and exercise limits, corruption, owner mismatch and observed read failures.
This increment does not close packed cold semantic admission, live integration or T-20 campaigns.
