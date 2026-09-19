# Decision 0072 — Single-pass first-owner admission evidence

Date: 2026-09-18

Status: T-20 partial implementation. [Decision 0073](0073-first-reference-rebase.md) subsequently
adds incremental maintenance; qualification remains open.

Add the optional native encrypted index profile `coordinator-first-reference-v1`, identified by
SHA-256 of `USTE coordinator-first-reference-v1`:
`9a09b5a30b98472a5f95f1f233f846b6dc9759ea2a11eca0bfd3fb791515f21d`.
It contains exactly one nonempty family (1). Each strictly ordered unique key is a 16-byte blob ID;
each value is the earliest committed reference revision encoded as an unsigned big-endian 8-byte
integer, nonzero and no greater than the root revision. Existing metadata, transaction, journal
and M1 profiles are unchanged. Empty-owner metadata needs no auxiliary proof and uses the existing
single-pass empty-owner admission path.

The root must share the complete scope/revision/certificate/reducer/logical-state anchor with the
metadata and transaction roots. Authenticate its entire run under explicit page/entry/byte limits.
Require its count to equal the owner count, and look up every streamed metadata owner in the proof
index. This establishes exact key-set correspondence without a complete owner map.

Then stream the authoritative journal prefix once. Check every retry and inventory reference
against the existing indexes as before. For each reference, the claimed first revision must not
exceed the current revision. At equality, verify the recorded owner against the journal principal
and increment a scalar first-match count. The terminal count must equal the owner count. Canonical
inventories prohibit duplicate IDs within a revision, so a blob contributes at most one equality
match. This proves both earliest occurrence and existence: a fabricated earlier claim cannot pass
merely by preceding every real occurrence. No provisional base escapes if any later read fails.

This removes the owner-count multiplier from journal replay during base admission. It still does
bounded disk lookups per owner/reference, and separately admitted transaction indexes retain their
own pass/budgets. Existing callers can use the compatibility admission path; it keeps the explicit
`(owners + 1) * revisions` journal-work ceiling. New admission needs only `revisions` groups.

`publish_coordinator_first_reference_index` is deliberately a legacy full-coordinator bridge. It
streams one authenticated prefix while retaining a caller-bounded temporary ID/revision map,
compares it with the legacy first-owner map, then publishes the sorted native encrypted run/root.
It is not a larger-than-memory builder. Root publication uses existing certificate binding and
durability rules. At introduction, disk overlays/rebase did not maintain this optional evidence: after rebase,
an old proof cannot admit a newer root. That next implementation remains required, along with
removal of storage's resident certificate/blob metadata and qualifying BM-01/BM-06 campaigns.

Tests pin the profile hash and encoding; reject malformed widths, zero/future revisions, missing
IDs, authenticated later-owner/first-reference substitutions and false earlier claims; exercise
exact versus one-short group budgets and legacy publication owner/group/byte bounds; and inject an
I/O failure at every observed cold admission read. Existing compatibility admission and recovery
fault tests remain mandatory. This is neither full T-20 acceptance nor production qualification.
