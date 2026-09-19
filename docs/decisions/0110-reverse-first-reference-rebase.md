# Decision 0110 — Reverse first-reference rebase

Date: 2026-09-19

Status: accepted and locally verified. T-20 remains open.

Use the authenticated reverse range for the disk coordinator's bounded post-base first-reference
construction pass. Unlike commutative correspondence, earliest ownership is order-sensitive:
retain one revision and principal-match flag per already-admitted new-owner overlay entry.
Descending occurrences replace later claims. Only after the complete scan succeeds require the
exact overlay cardinality and a matching principal at every retained earliest occurrence.
An earlier wrong principal cannot be repaired by a later matching one; a later foreign reference
cannot displace a correct first owner. Reference bytes must match at every occurrence, as before.

The map contains no base owners or whole-prefix comparator. The existing maximum-owner limit
applies before scanning; each retained claim adds only a boolean to the prior revision value
(including ordinary alignment overhead), with no second collection or whole-map conversion.
Strictly descending replacement is checked explicitly. These count bounds are not maximum-RSS
qualification. Empty suffixes and owner-free bases preserve their existing behavior.

Keep the original group/encoded-byte allowances and charge the complete reverse pass, including
its terminal certificate proof in disk mode. The pass completes before any run/root publication.
Under-limit failures retain all coordinator overlays and remain retryable. Root formats, sorted
first-reference bytes, principal authorization, retry/collision semantics and durability do not
change. Legacy full-coordinator bootstrap and compatibility per-owner discovery remain forward;
this does not claim to remove every remaining construction/rewrite/accounting cost.

Tests enumerate all 256 eight-occurrence principal-match histories against independent earliest
selection, reject non-descending replacement, and require exact claim cardinality. Integration
tests repeat a new blob under a different later principal, use exact/one-short budgets in both
resident and disk-certificate modes, and independently cold-admit the resulting first revisions.
Selected read/open/metadata I/O errors and crash-before/after cases must preserve exact restart;
optional derived-cache fallback may legitimately succeed and must be reported separately.
Existing publication-fault matrices and full coordinator/replay/native regressions remain required.
