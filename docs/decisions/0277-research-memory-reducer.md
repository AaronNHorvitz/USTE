# Decision 0277: `research-memory-v1` transaction codec and bounded reducer

Date: 2026-09-25

Status: Accepted for the development implementation of DB-R02.3; DB-R02 remains open.

Decision 0276 gave every research record a canonical form, but nothing yet admitted records
through the journal or enforced the cross-record rules of
[research memory records](../research-memory-records.md): sequential source versions, reference
closure, correction and revocation lifecycles, blob inventories and namespace budgets.

Add `ResearchTransaction` (`URST` format 1.0: header, scope, generation and one mutation) and
`ResearchState`, which implements the ordinary `TransactionState` and
`AuthorizedTransactionState` contracts so the existing coordinator, journal, retry, recovery and
policy layers apply unchanged. Mutations are `BeginRebuild`, `Put` of one record, `RetractClaim`,
`ExpireClaim`, `RevokeSource` and `CompleteRebuild`; generations follow the memory pilot's
rebuild rules, and a rebuild clears all derived state.

Admission rules. Record identities are unique across kinds within the namespace. A source's next
version is exactly one more than its latest and supersedes it. Retained content must arrive with
exactly its own newly finalized blob, and records without content must arrive with no blob
inventory. Artifacts need existing unrevoked input versions. A citation must name an existing,
unrevoked, accessible source version and end within its retained bytes. A correction names an
active claim, which becomes superseded at the correcting revision; retraction and expiry apply
only to active claims; revocation is terminal and blocks new citations and artifact inputs of
that version. Edges need existing endpoints and an asserting claim or artifact. Every count,
retained-byte and per-record fan-out limit is checked before the change becomes visible, and a
refused prepare consumes no revision.

Authorization requirements: namespace `ManageSchema` for generation changes, `Commit` on each new
record and on a corrected predecessor, `ReadRecord` on every cited or referenced record, and
`ManageRetention` for revocation. The reducer never fetches, resolves, executes or authorizes what
a record describes, and its state is a rebuildable derived index, not a permission.

The specification's frozen table omitted an artifact count, which would have left artifacts
unbounded. Before the reducer admitted any artifact, `research-memory-v1` gains
`maximum_artifacts = 262,144` per namespace, and retained artifact content counts toward the same
4 GiB retained-byte limit as source content. No other limit changes.

Verification (focused, under the shared heavy-work reservation): transaction round trips,
truncation, trailing-byte, version/tag/reserved-byte and record-scope refusals; lifecycle,
inventory, generation, scope and fan-out cases; authorization requirement vectors; 40 seeded
sequences of 200 generated operations each checked after every step against an independent
reference model written from the specification, with exact-replay equality; and a durable test
that commits through the real journal with blob upload, exact retries and an inventory refusal,
restarts the filesystem, reopens, and compares the recovered state and digest with both the
pre-restart snapshot and an independent in-memory replay. The full gate was not rerun.
Queries (DB-R02.4) and pilot mapping (DB-R02.5) remain open.
