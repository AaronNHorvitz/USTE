# Decision 0170 — Packed BM-06 history equivalence

Date: 2026-09-19

Status: implemented and locally verified; no benchmark qualification.

Add `bm06-packed-check --records N` as a separately capped development verifier. Retain the existing
one/two-record memory-model boundary and reject larger requests before allocating storage or keys.
Use the unchanged BM-06 event stream, 100 real versions, 4096-byte nonconstant payloads, at most
512 records per transaction and the frozen checkpoint/final generation boundary. No v1 command,
fixture target, native cap, benchmark threshold or production guard is weakened.

Construct only policy with the ordinary graph reducer. Open disk certificate/blob metadata and
stage packed policy genesis, then perform authorized packed writes and per-batch metadata rebase.
Stop at the actual checkpoint and certify the final batch with an intentionally insufficient
derived-publication batch allowance. Require precisely a committed publication ResourceLimit,
not any engine failure. Restart, independently admit the checkpoint graph/primary/quota triple and
stream its authenticated suffix. No recovered base digest is mislabeled as a terminal digest.

Exact-retry the certified tail, cold-admit the terminal triple, and verify every historical record
through the authorized packed reader: ID, version, creation/modification revisions, lifecycle,
schema/type and all payload bytes. Explicit zero-overlay origin recovery traverses 100 groups in
bounded 64-certificate windows. Reverify history, exact-retry every data batch at the terminal clock
without an outcome map, and cold-admit again to compare the full v1 logical-state digest.

Generalize fixture-limit construction by graph shape, preserving every existing BM-01 value. BM-06
allows 100 versions and 100 × 16 KiB per semantic history group; keys/individual values retain the
existing 512-branch/16 KiB bounds. Historical point reads use the bounded reverse predecessor cursor,
not an unbounded scan of all versions. Arithmetic tests admit the qualifying fixture's dimensions
without constructing a database or claiming performance/resource qualification.

The filesystem, entropy and credential wrapper are development models. Reports disclose zero
qualifying trials and incomplete authenticated I/O accounting; no latency or larger-than-memory
claim follows from these checks. Native packed BM-06 integration, exact-scale construction and
reserved-host qualification remain required T-20 work.

Full experiment regression passes 89 active tests with two pre-existing ignored campaigns and
strict Clippy. Both one- and two-record histories pass; the CLI verifies the two-record path and
pre-allocation refusal. See PROGRESS for exact commands, tested baseline and resource limits.
