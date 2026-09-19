# Decision 0145 — Packed quota cold admission

Date: 2026-09-19

Status: accepted and locally verified T-20 private admission; live integration remains open.

Independently admit a discovered packed quota projection against an opaque journal-validated
primary prefix at exactly the same namespace and certificate/revision. Recheck the primary's
live-owner/key bindings without I/O, including an empty owner ledger. Expose an explicit packed
tree binding-validation operation for this purpose; it rechecks retained certificate ownership
and key availability, not current ciphertext or domain semantics. Raw quota usage also requires
this primary binding, rather than accepting matching logical commitments from a foreign owner.

Require the exact quota profile and three explicit families. Authenticate the target certificate,
fully validate each canonical family, then decode the single metadata head. Counts must match
the primary owner cardinality and principal/owner family descriptors. Stream the complete
principal/blob ordering with a bounded cursor. Every canonical owner must exactly match the
primary first-owner lookup, including the key's principal. Ordered composite uniqueness and
equal cardinality prove a bijection without a complete owner map.

Retain only the current principal's expected and accumulated count/bytes while streaming. Every
group must equal its principal aggregate; the number of groups must equal the principal family
cardinality, excluding extra aggregates. Checked namespace bytes must equal the head. Zero-byte
owners count normally; an empty projection requires an explicit authenticated zero head. No
partial projection escapes after corruption, a late failure or a resource refusal.

Certificate, per-family canonical validation, owner cursor, per-lookup and aggregate lookup work
have explicit separate limits. Admission and usage do not publish, repair, authorize consumers or
interpret missing projections as zero. Existing v1/M1 behavior, quota policy and reservation rules
remain unchanged. Populated quota rebuilding and live/domain integration are subsequent work;
this does not qualify a benchmark or close T-20.
