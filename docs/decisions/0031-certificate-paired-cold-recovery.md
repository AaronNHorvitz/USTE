# Decision 0031 — Certificate-paired cold recovery metadata

Date: 2026-09-17

Status: accepted as T-20 recovery groundwork. T-20 remains open; the recovered graph reducer is
still fully memory-resident and this decision does not qualify BM-01 or BM-06.

## Context

Decision 0030 can reconstruct `graph-state-v1`, but its original API required an already-open
`CommitCoordinator`. A coordinator cannot open from that state unless retained retry outcomes,
transaction-ID bindings and first-commit blob ownership are also recovered. The existing `UCCP`
checkpoint carries all of those values with a reducer payload, but its 256 MiB monolithic reducer
frame is the boundary this root path is intended to remove.

## Decision

`coordinator-meta-v1` is a distinct optional encrypted index profile. Its root binds the exact
scope, journal revision and certificate, reducer profile and logical-state digest. The profile ID
is SHA-256 of the literal name and the byte contract is pinned in
`acceptance/r1/coordinator-meta-v1.tsv`. It contains:

1. mandatory family 1, key `coordinator-meta-v1`, with a 48-byte `UCMD` 1.0 value: reserved-zero
   header, revision, outcome/owner counts and their exact fixed logical-byte totals;
2. optional family 2 outcomes ordered by `principal[32] || idempotency-key[16]`, with a 104-byte
   value containing transaction ID, outcome revision, request/result digests, expiry and four
   reserved-zero bytes; and
3. optional family 3 owners ordered by blob ID, with an 80-byte versioned value containing byte
   length, chunk count, content digest and first committing principal.

Empty optional families are absent. Publication holds exclusive coordinator access, checks the
outcome/transaction and owner maps, streams ordered entries into immutable runs, verifies returned
counts and publishes the root only at the unchanged journal anchor. Interrupted runs are optional
orphaned cache data, not commits.

An `AuthenticatedIndexRecovery` temporarily opens and fully authenticates the storage journal
without applying reducer transactions. It owns the journal/key/lock context while trusted recovery
code loads and streams derived roots. Graph recovery requires equality of the complete cross-profile
anchor tuple—scope, revision, certificate digest, reducer profile and logical-state digest—rather
than matching profile-local generations. It reconstructs the graph and coordinator metadata
privately, then the recovery owner must be dropped. `CommitCoordinator::open_seeded` reopens the
journal, decodes every transaction group through the anchor, verifies the exact certificate and
metadata, and applies the normal result-digest-checked reducer only to the suffix.

Prefix verification migrates expected seed entries into verified maps as journal groups are
decoded. This retains the two required outcome indexes but avoids keeping an additional complete
copy of every prefix map. Blob ownership preserves the first committing principal when later
groups reference the same blob.

## Bounds and trust

Caller limits cover outcome count, owner count, aggregate entries, pages and logical bytes within
the hard domain/carrier caps. Metadata fixed-size arithmetic and descriptor family/count/page shape
are checked before outcome or owner maps are built. Full-run terminal authentication remains
provisional: no seed escapes after a late error. Candidate discovery still performs its earlier
absolute-format-bounded scrub before caller reconstruction limits apply.

The temporary reader deliberately does not treat authenticated transaction payload bytes as
semantically accepted. Only the subsequent seeded open decodes and verifies the complete prefix.
The APIs are privileged and expose retry/blob-existence metadata; they are not consumer query
surfaces. The journal remains the only commit authority.

Ordinary `BTreeMap` allocation, full `GraphState`, retained coordinator maps and root discovery are
still memory-resident and logical-byte accounting is not an RSS guarantee. Disk-backed base/overlay
state, scratch merge, ingest deltas and qualifying benchmarks remain required for T-20.
