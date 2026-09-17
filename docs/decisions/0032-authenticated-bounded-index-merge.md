# Decision 0032 — Authenticated bounded index merge

Date: 2026-09-17

Status: accepted as T-20 scratch-merge groundwork. T-20 remains open; no disk-backed live reducer,
larger-than-memory recovery result or BM-01/BM-06 qualification is claimed.

## Context

Decision 0031 can recover an exact graph/coordinator root pair, but ordinary graph state remains a
complete in-memory map. Moving toward a disk base plus bounded overlay requires a way to apply exact
changes without collecting an authenticated base run or its replacement in memory. The frozen
`index-v1` root admits one run per family and binds every run to the root revision and profile, so
an older descriptor cannot be reused in a newer root and multiple overlay runs cannot be installed
without defining a new profile.

## Decision

Trusted maintenance gains a single-base merge primitive. It accepts an optional authenticated
`index-v1` base root, a nonzero family, the current journal revision, the unchanged index profile,
explicit resource limits and a fallible ordered stream of `IndexDelta` values. A delta contains one
key, an optional exact `before` value and an optional `after` value:

- `before = None` requires the key to be absent;
- a present `before` is compared byte-for-byte with the authenticated base;
- `after = None` is a tombstone; and
- both sides absent, empty/oversize keys, oversize values, duplicate keys and non-increasing keys
  fail closed.

The merge retains one stable source handle, authenticates every source page, enforces exact source
length/order/count/logical digest and writes the target with the unchanged encrypted `index-v1`
page encoder. It retains only one assembled source entry, one delta, one decrypted source page,
one output page and bounded fragment/key buffers. Limits independently cap source pages/entries/
logical bytes, delta count/logical bytes and output entries/logical bytes. The report separates
base reads, delta bytes, insertions, replacements, deletions and output size.

The target is invisible until a separate domain layer validates its complete semantics and
publishes an exact certificate-bound root. Empty logical output returns no descriptor and creates
no empty run. Failure after target creation can leave an opaque unreferenced partial run for T-35
reclamation, but cannot publish a root or change journal authority. The transaction coordinator
exposes the primitive only as trusted maintenance and rejects uncertain state; the journal requires
the target revision to equal its current frontier and any base certificate to belong to its exact
authenticated chain.

## Format and integration consequences

No durable format changes. Runs produced by a merge are ordinary `index-v1` runs. Because the
frozen root binds each descriptor to its target revision and allows one run per family, a terminal
new-revision root must rewrite every nonempty family, including unchanged families through an
empty-delta copy merge. A future persistent multi-run base/overlay manifest would require a new
versioned profile rather than reinterpreting `index-v1`.

Graph integration must next translate bounded `PreparedGraph` before/after changes into coalesced
family deltas, merge and independently validate every terminal family, recompute the canonical
graph state digest and publish graph/coordinator roots only at one exact journal anchor. A live
disk-backed reducer also needs an explicit coordinator-controlled fallible read/prepare capability;
filesystem I/O must not be hidden in the current side-effect-free `TransactionState::prepare` or
the infallible post-durability `publish` boundary.

## Verification and limits

The storage tests cover a multi-page base value, insert/replace/delete at ordered positions, exact
output visitation, all-tombstone empty output, construction without a base, wrong-before and
non-increasing delta rejection, and source corruption discovered after provisional target output.
A target fault matrix covers crash-before/after create, write, exact-size, file-sync and directory-
sync boundaries; restart exposes only the previously published root. Existing run/root and journal
tests continue to cover authenticated layout, restart fallback and root-publication boundaries.

This primitive bounds its own live buffers and logical work; it is not an allocator/RSS proof for
its caller or iterator. It does not maintain a live graph overlay, stream graph semantic validation,
remove full-memory candidate discovery/reconstruction, change ingest's clone-based preparation,
publish a root automatically, reclaim orphan runs or qualify either T-20 benchmark.
