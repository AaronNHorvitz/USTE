# Decision 0097 — Disk-coordinator committed blob reads

Date: 2026-09-19

Status: locally verified T-20 increment; consumer authorization and map-free blob recovery remain open.

Connect the privileged disk coordinator's exact admitted owner lookup to Decision 0096's
committed-reference proof and range read. Reject uncertain ownership, foreign namespace and
invalid output extent before index work. An absent or different exact reference yields no
payload. Neither path consults the storage resident blob map or constructs a complete owner map.

For an immutable base with independently admitted first-reference evidence, look up its exact
revision and authenticate that certificate against the pinned frontier. Storage gains a
revision-only proof constructor with the same chain/context/terminal-digest checks and work
limits as the expected-digest constructor; it neither trusts an unbound revision nor performs
an uncharged preliminary digest read. Callers with an independently known digest retain the
existing stricter equality check. Both constructors return the actual anchored digest and work.

New overlay owners require an explicitly bounded post-base journal scan. A legacy base lacking
first-reference evidence requires an explicitly bounded prefix scan. Retain at most one matching
anchor and complete the entire admitted range before creating a blob proof; a late error returns
no successful result. Missing correspondence is an integrity error. The range allowance counts
certificate/group bytes, including configured proof re-reads; inventory and segment-header work
retains the journal's separately documented format bounds. The final certificate proof and exact
inventory proof each have independent explicit limits. Do not advertise their sum as a single
unified byte budget or omit the discovery cost. Rebase with first-reference evidence removes the
need for the discovery scan, without changing exact first ownership or plaintext charges.

This is a raw recovery/adapter capability, not an extension of the existing metadata consumer
facade. Current principal ReadBlob authority and read quotas must precede raw calls; a separate
consumer capability remains necessary. No upload charge transfer, inventory commit authority,
storage quota bypass, format change, M1 interface change or full-RAM recovery removal follows.

Tests cover base first-reference lookup without discovery allowance, bounded new-owner discovery,
legacy prefix discovery with exact separate cold-admission work, rebase/reopen in disk-certificate
mode, mismatched references, foreign scope/invalid extent before I/O, and uncertain-commit refusal
before I/O followed by exact restart. Both base and overlay disk-certificate read paths undergo
all 31 I/O boundaries/93 error/crash attempts. The revision-only proof is compared to independently
expected exact anchors and rejects an authentic alternate certificate suffix. Existing first-owner,
retry, metadata-rebase, storage-corruption and native process/oracle suites remain passing.
