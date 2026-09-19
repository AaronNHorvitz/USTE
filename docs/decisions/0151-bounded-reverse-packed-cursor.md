# Decision 0151 — Bounded reverse packed cursor

Date: 2026-09-19

Status: accepted and locally verified T-20 reverse traversal; disk-domain integration open.

Extend Decision 0135 with descending traversal over `(lower, upper]`. An absent upper bound
starts at the greatest key; empty lower excludes no valid nonempty key. Equal bounds are empty.
Forward traversal retains `[lower, upper)`. This explicit asymmetric contract permits inclusive
historical predecessor lookup without constructing a byte-string successor.

Reuse the authenticated route, compressed-prefix seek, value verification, cumulative budgets,
sticky error/end semantics and owner-bound wrappers. Reverse seek compares its reached leaf with
the upper bound and skips an entire too-high subtree using the first differing bit. Advance ascends
past completed left children then descends rightward in the next left subtree. Never scan the
whole later keyspace to locate a historical predecessor. The exclusive lower witness consumes
candidate/page work but no value/result budget. No extra index or tree-sized metadata is retained.

The canonical-root admission prerequisite remains mandatory; this primitive proves selected
paths and values, not independent global canonicality or graph semantics. Current policy remains
the consumer's responsibility. Verify both directions against ordered reference ranges, compressed
prefixes, exact/minus-one limits, observed read faults, late corruption and owner/key/scope refusal.
No native/M1 switch, complete-I/O claim or benchmark qualification is implied.
