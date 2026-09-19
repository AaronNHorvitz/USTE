# Decision 0104 — Authenticated reverse journal validation

Date: 2026-09-19

Status: implemented and locally verified. T-20 remains open.

Decision 0095's forward range visitor independently proves each certificate through the pinned
frontier before exposing its group. Preserve that contract and forward replay ordering. Add a
separate descending-revision visitor for order-independent validation, retaining one certificate,
group and inventory at a time, without a certificate history map.

Authenticate the requested last certificate to the exclusively owned frontier once. Then read
each selected certificate in descending order, requiring its ciphertext digest to equal the
expected digest before decoding or exposing its group. The decoded successor supplies the next
expected predecessor digest. Check exact revision/group sequence, genesis predecessor, segment
header, encrypted group length/digest/context and inventory exactly as in the forward visitor.
In resident-anchor mode the last digest comes from the admitted map instead of a disk suffix
proof. No transient proof crosses recovery owners or changes the pinned frontier.

For selected range [A, B] and frontier F, disk mode reads F-B+1 proof certificates plus B-A+1
selected certificates, not a sum of separately re-proven suffixes. Debit every one of these reads
and every selected group envelope from the same encoded-byte budget. Group count is admitted
before I/O; certificate proof limits remain explicit. Segment headers/inventory metadata retain
their existing independent format bounds. Reports are partial logical adapter-work accounting,
not physical device I/O. Callback-produced results remain provisional until whole-call success;
each exposed group is already bound to the frontier, even if a later read or callback fails.

Expose a corresponding authenticated transaction recovery visitor. Use it only for the
transaction-ID index's journal-to-index correspondence phase, which compares each independent
transaction against its unique ID entry and verifies full-run cardinality. Forward reducers,
first-owner construction, retry ordering, resumable forward cursors and other admission passes
are unchanged. Existing profile-derived upper bounds remain valid; no benchmark target or work
allowance is increased. Other forward proof scans can still be quadratic and are not claimed fixed.

Tests compare every subrange in resident/disk modes, exact/minus-one byte budgets, descending
contents, no resident history, pre-I/O refusal, authenticated fork substitution, callback-time
corruption and all observed read/open/metadata fault boundaries followed by restart. Transaction
tests include nonempty inventories and exact reduced proof budgets. Full storage, transaction,
replay, graph and native fault/oracle regressions pass; see PROGRESS.md for commands and limits.
