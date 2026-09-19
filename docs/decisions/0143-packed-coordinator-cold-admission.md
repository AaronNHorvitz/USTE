# Decision 0143 — Packed coordinator cold admission

Date: 2026-09-19

Status: accepted and locally verified T-20 cold admission; live integration remains open.

Admit a discovered Decision 0142 coordinator root only after complete canonical validation of its
four explicit families and journal correspondence. Require the exact profile, namespace and family
set; retry and transaction cardinalities equal the root revision, and owner/witness cardinalities
are equal. Independently authenticate the claimed target certificate against the live frontier.
Reducer and state-digest claims are not validated here and cannot authorize domain installation.

Stream revision one through the target using the bounded certificate window. At each revision,
exact retry and transaction-ID lookups must match the journal's principal, idempotency key and
complete outcome. Since outcomes include a unique revision, collisions cannot satisfy two
transactions; exact cardinalities exclude extra entries without constructing comparison maps.

For every inventory reference require the exact owner reference and a witness revision no later
than this transaction. When the witness equals this revision, require the owner principal to
match and increment the distinct-first-owner tally. Inventories have unique blob IDs and each
blob has one witness, so a blob contributes at most once. Requiring the terminal tally to equal
both owner/witness cardinalities excludes phantom owners and witnesses. A too-late witness fails
the earliest actual occurrence; a too-early or nonexistent witness cannot contribute its required
tally. These checks establish earliest-reference semantics without owner maps or per-owner full
journal rescans.

Admission bounds each full-family validation, certificate-target authentication, total journal
groups/encoded bytes, per-transaction inventory references, each lookup and aggregate lookup
pages/encoded bytes. All provisional handles remain private until the cursor finishes and every
count comparison passes. No index writes, repairs or root publication occur. Errors discard the
candidate; callers choose explicit rebuild, never a fabricated empty metadata base.

The result is the same opaque private coordinator prefix as inductive construction. It still
requires separate quota projection, domain pairing, live installation and current authorization.
This does not close T-20 or qualify BM-01/BM-06.
