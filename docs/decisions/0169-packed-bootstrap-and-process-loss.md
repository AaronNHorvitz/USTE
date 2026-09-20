# Decision 0169 — Packed bootstrap binding and native process-loss controls

Date: 2026-09-19

Status: implemented and locally verified; no benchmark qualification.

New native packed BM-01 stores bind their policy bootstrap's retry and transaction identities to
the frozen fixture version and entity count: eight bytes `BM01PK1\0`, followed by the entity count
as unsigned big-endian 64 bits. Other data-batch identities, request encoding, graph semantics and
the existing v1/model commands are unchanged. This is fixture metadata, not a new consumer API.

Explicit resume may initialize a verified empty store, which has no prior certified profile, or
recover a correctly bound policy-only prefix. Only those zero/one-revision prefixes may enter the
ordinary reducer: at most one outcome, zero blob owners and 1,048,576 replay bytes. Check the sole
receipt's principal, retry key, transaction ID and revision before attempting the same canonical
policy request as an exact retry. Preserve expiry/collision behavior. Reopen disk certificate/blob
metadata, stage the bounded policy genesis and continue packed writes. Never apply this path to
larger prefixes or reinterpret legacy unbound policy identities. Existing data-bearing legacy
fixtures remain admissible through their authenticated evidence marker.

Add an explicitly named create-crash-probe command for a test-owned child. Emit one content-free,
flushed marker and park at revision zero after durable database creation, revision one after policy
acknowledgement but before genesis publication, or a data revision after successful authorized graph
publication but before coordinator metadata rebase. The supervisor sends SIGKILL only to its child,
waits/reaps it, and owns timeout cleanup. This is not a power-loss, torn-sector or qualification claim.
Existing storage/graph fault-injection suites retain their finer durability-boundary coverage.

Keep the first opener's repaired-tail/ignored-journal observations even when materialization later
performs cold terminal admission. Report original recovered frontier, bounded-bootstrap use,
selected base and suffix groups; do not substitute the second opener's zero repair counts.

Process tests cover all five revision boundaries of a native 20/200 fixture, wrong dimensions after
certification without changing authority, a three-byte incomplete certificate tail, unchanged
certified-prefix bytes, final reference digest, no duplicate terminal commits and all 384 oracle
queries after each restart. Future pause revisions and qualifying-size requests fail before I/O.
The 20,000-entity admission ceiling is unchanged and is not a measured packed qualification.
Packed BM-06 history, complete authenticated I/O, exact-scale construction and reserved-host
qualification remain required T-20 work.

Verification: all four packed CLI tests pass, including five actual SIGKILL/restart cases; all
68 active experiment library tests pass with two pre-existing ignored campaigns. Strict Clippy
passes. PROGRESS records exact commands, scopes and retained resource/qualification limits.
