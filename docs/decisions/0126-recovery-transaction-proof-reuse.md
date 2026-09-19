# Decision 0126 — Retain transaction certificate proofs for private staging

Date: 2026-09-19

Status: accepted implementation contract; T-20 qualification remains open.

Add an explicit forward journal-range visitor that lends the certificate proof already computed
and charged by Decision 0095's disk-certificate path. Existing visitors delegate without changing
order, budgets, callback boundaries or persisted bytes. Resident-anchor reads supply no proof.
The reverse visitor remains unchanged. Every proof-bearing group has already passed its exact
certificate-to-frontier, group and inventory checks before the callback; the whole range remains
provisional until successful completion. No proof is fabricated from an unauthenticated digest.

The authenticated transaction cursor retains a clone of this opaque fixed-size owner-bound proof
alongside its one bounded decoded transaction. Transaction equality still compares all transaction
content, not transient proof presence or owner identity. Proofs are neither serialized nor disclosed
in transaction diagnostics. This is not a certificate-history cache.

Private staging reuses retained evidence after checking the transaction's exact revision/digest,
scope and the proof's exact live owner/frontier. Both staging entry points reject invalid bound
evidence without fallback or I/O. Unbound transactions preserve their existing resident/explicit-I/O
behavior. A clone holds neither keys nor a filesystem ownership lock. Reopen, foreign owner,
frontier advancement or poisoned ownership invalidates staging exactly as in Decision 0094.

As with other admitted proof-bound roots, an existing receipt is not continuous on-disk monitoring.
It can still be used after later byte tampering; fresh cursor/proof acquisition and cold recovery
must reject that tampering. No transaction, domain, retry, first-owner, quota, terminal range or
publication check is removed. Intermediate roots remain private. Proof reuse does not permit a
coordinator to escape a failed or incomplete suffix.

The shared range byte report is unchanged and counts proof reads once when they actually occur.
Stage acquisition with retained evidence reads zero certificate bytes instead of repeating a
suffix proof for each graph/primary/first-reference/quota stage. This reduces repeated work but
does not eliminate the cursor's triangular forward proof work, immutable-family rewrites or
incomplete whole-system I/O accounting. Existing profile allowances remain upper bounds; no
benchmark threshold, native development cap, qualification requirement or M1 handoff changes.

Regression coverage must include all subranges and exact/minus-one budgets in both storage modes,
proof-to-group binding, zero-I/O staging reuse, unbound fallback, foreign/reopened-owner refusal,
content equality, fresh corruption refusal, every observed disk cursor read error, and the existing
complete suffix/rebase fault and native process-loss suites.
