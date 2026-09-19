# Decision 0159 — Read-only packed access

Date: 2026-09-19

Status: implemented and locally verified; T-20 qualification remains open.

Provide a scoped immutable journal borrow for already-admitted packed families. Trusted
coordinator construction pins the current certified anchor and rejects outcome uncertainty;
maintenance may reborrow its authenticated target immutably. This is not cold admission,
consumer authorization, a concurrent snapshot, or permission to stage/publish/append.

Reuse the existing scope, nonfuture revision, live certificate-owner and key-availability
checks for exact lookup and forward/reverse traversal. A cursor remains sticky-failed after
an error; even an exhausted cursor must revalidate ownership/key availability. An immutable
journal borrow prevents concurrent append through the same owner while the reader exists.
Delegate maintenance reads to this implementation to avoid two diverging security contracts.

Test read equivalence, exact work limits, read faults, foreign owner/scope, future anchors,
uncertainty, and absence of writes. Authorized packed graph queries remain a separate layer.
No packed cache or larger-than-memory qualification is implied.
