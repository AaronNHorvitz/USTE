# Decision 0132 — Bounded authenticated packed-tree lookup

Date: 2026-09-19

Status: accepted and locally verified T-20 raw lookup; mutation/root/domain integration open.

Connect Decisions 0128–0131 with an opt-in raw maintenance lookup. The caller must supply an
independently trusted canonical logical root, its physical locator and the exact enclosing
scope/profile/family. These arguments are not authorization capabilities or proof of journal
admission. No existing graph/coordinator or M1 profile automatically selects this path.

Follow only the key-selected path. Every page is reread under its exact encrypted context; decode
the selected typed record and recompute its logical summary before following a child. Match each
summary to its expected parent claim and the first one to the trusted root. Branch bits strictly
increase and each selected child has fewer entries than its parent. At the terminal leaf, verify
the complete borrowed Decision 0128 membership/absence proof, including key routing. Missing,
malformed, swapped or mismatching referenced data is an error, never an absent key. An empty
tree requires the exact context-specific empty commitment and no physical root.

For a present nonempty value, follow canonical chunks with strictly decreasing remaining bytes,
validate each expected length and terminal flag, and verify the final Decision 0128 value digest
before returning bytes. Return no provisional values or partial success report. Retain only one
decrypted page at a time, bounded proof metadata and the explicitly admitted zeroizing result.
Empty values remain distinct from absent keys.

Admission includes key size, path branches, total page reads, total encoded bytes and returned
value bytes. Check read/byte budgets before every filesystem operation for the next page; refuse
value overages before allocating the output or reading chunks. Reports charge every successfully
acquired page, including repeated reads of a page containing several selected records. There is
no hidden cache, cross-request state, pack enumeration or complete index reconstruction.

Add a separate raw linked-page read, retaining Decision 0130's descriptor-based exact-length read
unchanged. A linked read requires a validated locator, positive integral encrypted-page file
geometry within the pack ceiling, and an existing selected page. It does not require a resident
catalog of every pack's final size/counts: links are issued before their immutable pack finishes.
Whole trailing pages not referenced by a logical root do not define its content; selected-page
authentication and parent commitments remain mandatory. Partial trailing pages, missing selected
pages, oversized packs and operational failures are refused. This is not authority to ignore a
missing committed journal/blob or to relax existing descriptor-based scrubs.

Required tests compare membership/absence/empty and multi-chunk values against a separately
constructed reference tree, exact/minus-one budgets, caller-context substitution, authenticated
false child claims/partitioning, cycles or repeated branch bits, swapped/malformed chunks,
every observed read fault and restart. Linux process tests and bounded reachable-node write
batches, independently admitted root publication/recovery, complete accounting and qualifying
BM-01/BM-06 remain required subsequent work. No production or larger-than-memory claim follows
from a bounded lookup test.
