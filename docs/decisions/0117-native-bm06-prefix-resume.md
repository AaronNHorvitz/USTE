# Decision 0117 — Native BM-06 bounded prefix resume

Date: 2026-09-19

Status: accepted development contract; T-20 qualification remains open.

Add `bm06-linux-resume` for interrupted native materialization, retaining the two-record
ceiling and every existing qualification boundary. Resume authenticates the durable prefix,
admits/rebuilds bounded derived suffix state, verifies every existing historical payload, and
continues only missing transactions through checkpoint revision 100. A certified terminal
revision 101 is repaired and verified without adding another revision. The separate recover
phase continues to exercise the exact final-request retry.

New native fixtures bind the record count in both bootstrap identities: eight bytes
`BM06BT1\0` followed by the unsigned big-endian 64-bit count. This names the fixed Decision 0114
recipe without changing its event stream, payloads, counts or canonical request golden.
Resume requires that authenticated revision-one outcome. Current ManageSchema authorization
and exact durable-policy binding precede the trusted maintenance metadata lookup. Existing
unbound fixtures remain readable by open/recover, but are not accepted for resume; neither
missing nor expired bootstrap retry evidence permits guessing the fixture profile. The normal
retry retention policy remains unchanged.

Only an authenticated empty or policy-only prefix may use the bounded bootstrap coordinator:
one transaction, zero blob owners and a 1 MiB request allowance. A recovered policy outcome
must have the expected principal, key, transaction ID and revision before retry/publication.
Later prefixes use admitted disk graph/coordinator state and bounded suffix recovery, with no
full-memory reconstruction fallback. Complete graph-base loss still fails closed. Because the
closed native ceiling gives exactly one batch per generation, revision minus one is the exact
historical generation count; increasing that ceiling requires revisiting this prefix oracle.

`bm06-linux-create-crash-probe --pause-after-revision N` accepts 1 through 100 and flushes a
content-free durable-prefix marker before parking with ownership held. At revision one it
pauses before derived bootstrap roots are published. It is a supervised test hook, not a
standalone demo. Tests signal and reap only owned children. This is process-loss evidence,
not power-loss qualification, physical erasure, full cache-loss rebuild or benchmark acceptance.
