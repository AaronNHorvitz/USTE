# Decision 0088 — Resumable authenticated transaction ranges

Date: 2026-09-18

Status: T-20 partial implementation; multi-revision disk-graph publication remains open.

Add an opaque `TransactionRecoveryCursor` to the existing exclusive authenticated recovery owner.
Admit an inclusive range against its scope/frontier and caller group ceiling before filesystem
access. Each step rechecks the scope and exact authenticated frontier, then rereads one certificate
and group through the existing storage authentication path, decodes canonical transaction bindings
and returns one owned bounded transaction/inventory. The cursor retains only scalar range/budget
state, never a transaction history. It is pinned to authenticated content, not a new consumer or
issuer authorization capability.

Storage now provides `JournalRangeReadReport` after complete range success, preserving the original
unit-returning visitor API as a delegating wrapper. The report counts actual encrypted certificate
and group bytes; segment headers, format-bounded inventories and blob payloads retain the existing
documented exclusions. Cursor steps deduct this actual consumption from one shared byte budget.
Splitting a range into steps cannot reset its work allowance. Overflow, admission, scope/frontier,
I/O, decoding or authentication failure invalidates the cursor. It cannot resume even after the
underlying transient fault is removed; fresh admission is required.

Only terminal finish exposes the total consumption report. Earlier yielded transactions are
provisional, and a caller must not present them as a successfully recovered coordinator. Canonical
binding/authentication does not prove retry uniqueness, collision absence, first ownership or domain
semantics. Those existing checks remain in coordinator/domain recovery and still precede exposure.

The existing `visit_transactions` now drives this cursor, so admitted disk metadata correspondence
and suffix replay use the same cumulative admission and error behavior. Temporary encrypted and
decrypted group buffers are released before the caller's index/reducer callback runs. This also
permits future trusted recovery orchestration between individual reads without keeping a journal
visitor borrow alive. It does not authorize historical root publication or journal mutation, remove
storage's resident metadata maps, relax the graph's ready/one-pending boundary, or qualify BM-06.

Tests cover three canonical transactions, owned inventory, exact group/certificate byte allowance
and minus-one refusal, two-step suffix selection, incomplete finish, every observed read error,
late certificate corruption, sticky failure after repair, fresh recovery, and scope/frontier
substitution before I/O. Existing coordinator first-owner/collision, graph pending-root and
publication-fault suites remain required regressions.
