# Decision 0066 — Caller-bounded prefix-scan admission

Date: 2026-09-18

Status: T-20 query-enabling increment; no benchmark qualification.

Authorized disk graph queries need a caller-selected prefix-scan work limit, not just bounded
result cardinality. `IndexScanLimits` admits page visits, result entries and key-plus-value bytes
under the existing format maxima. Page visits include binary-search probes, sequential pages and
cache hits. The budget is charged before each page access. Zero result/byte budgets may still
prove absence; a matching entry then refuses without entry allocation or callback publication.

Before allocating a fragmented result, validate its key length plus declared complete value
length against remaining result bytes and its entry slot against remaining cardinality. This also
hardens the compatibility visitor/collection APIs, which retain their existing result limits and
use the existing absolute page bound. The collecting form never returns a partial result on error;
visitor output remains provisional until terminal success. No on-disk format or benchmark target
changes. Authentication, namespace and uncertain-state checks remain in their established layers.

The bounded collecting API is available on storage, legacy and disk coordinators. These are
privileged reads: a consumer facade must authorize before invoking them and bound aggregate work
across multiple operations. This increment does not itself implement graph-query authorization.

Regression coverage includes inclusive fragmented-value byte admission, one-byte/count/page
refusals, fully warm-cache page refusal, zero-result absence, constructor maxima and no callback
for an over-budget entry. Existing graph preparation, root publication and recovery fault suites
remain regression tests.
