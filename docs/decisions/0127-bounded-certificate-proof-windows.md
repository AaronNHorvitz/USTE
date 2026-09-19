# Decision 0127 — Bounded certificate proof windows for forward recovery

Date: 2026-09-19

Status: implemented and locally verified; T-20 qualification remains open.

Add an opt-in forward transaction cursor with a caller-selected certificate window of 1–64.
The hard maximum is independent of history size. Existing cursors keep their original behavior;
private per-revision coordinator recovery selects 64. Resident-anchor mode retains its existing
range behavior. No persistent format, cryptographic primitive, authorization or benchmark target
changes.

For a selected window [A, B] and pinned frontier F, authenticate B through F once, then read B
down to A using the authenticated predecessor digests. Authenticate every envelope's existing
database/epoch/log/writer/revision context and genesis predecessor. No receipt escapes before
the entire window succeeds. For a one-revision window, reuse the initial proof without rereading
it. Retain at most 64 fixed-size receipts, not requests, inventories or a history-sized map.
Receipts share acquisition work; their reports must not be summed as independent reads.

Acquisition costs F−B+1 proof certificates plus B−A+1 window certificates for windows larger
than one, or F−B+1 for a singleton. Admit the fixed receipt cap, suffix proof count/byte ceilings
and combined encoded-byte ceiling before allocation/I/O. The cursor deducts the window report
once from its existing shared range allowance. Existing profile-derived triangular allowances
remain valid upper bounds, not increased latency/memory budgets.

Each forward step validates the receipt's exact owner/frontier and rereads its selected certificate,
requiring the exact admitted digest before group/inventory authentication. Group reports charge
these new selected-certificate and group reads, separately from window acquisition. Lookahead
cannot conceal later byte corruption. Owner substitution, I/O, authentication, decoding, arithmetic
or budget errors invalidate the cursor; incomplete or unconsumed windows cannot finish. Retained
transaction proofs continue to support Decision 0126's zero-I/O private-stage admission.

This reduces repeated suffix authentication in fixed-size batches. It is not linear total forward
proof work for arbitrarily long ranges: the worst case remains quadratic divided by the window
size. Immutable-family rewrites, whole-state digest scans, broader accounting, native development
caps and reserved exact-size BM-01/BM-06 qualification remain open. M1's pinned handoff is unchanged.

Verification must cover independent proof equivalence for every subrange, 64/excess receipt
admission, exact/minus-one shared bytes, window rollover, populated inventories, foreign/stale
owners, authentic fork substitution, corruption before/after lookahead, every selected read fault
with restart and the complete graph/coordinator/native recovery regressions.
