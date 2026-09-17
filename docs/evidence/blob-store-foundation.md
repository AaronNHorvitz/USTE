# T-15 encrypted blob-store foundation evidence

Date: 2026-09-17 · scope: partial T-15 evidence, not task completion or production qualification

## Implemented

- Decision 0017's random upload and namespace-derived blob identities, 1 MiB encrypted chunks,
  16 GiB admission cap and bounded range reads.
- Durable resumable staging, retryable immutable finalization, authenticated final/abort markers,
  uncertain-write quarantine, explicit staging abort and zero-byte content.
- Canonical sorted `UBIN` inventories with exact byte length, chunk count and original-byte digest;
  inventories use key-derived opaque disk names and every referenced chunk becomes durable before
  journal publication.
- Certificate inventory binding and two-pass recovery verification before logical replay. Reads
  accept only exact references reconstructed from committed inventories.
- Hard bounds cover 100,000 references per inventory, 1,000,000 unique blobs per journal,
  10,000,000 reference bindings per journal and 1 TiB of logical blob bytes per namespace.
- Transaction idempotency binds the inventory digest; reducers receive the verified inventory on
  initial commit and recovery.

## Focused verification

`cargo test -p uste-txn --all-targets` passes arbitrary 2+ chunk round-trip from irregular writes,
pre-commit read denial, commit/restart/replay, exact retry, one-chunk restart/resume, staging abort,
changed-inventory retry conflict, zero-byte commit/read, durable finalized/aborted upload markers
and missing-committed-chunk hard failure.
`cargo test -p uste-storage --lib blob::tests` pins literal inventory/manifest bytes, assigned
crypto roles and derived blob identity, and rejects malformed/noncanonical inventory fields.
Storage unit tests also reject alternate duplicate-handle plaintext, require resume after uncertain
flush, and preserve a durable partial terminal chunk across two restarts before finish.
Targeted storage/transaction clippy passes with warnings denied.

`bash scripts/check.sh` passes 100 workspace tests (42 `uste-storage`, 13 `uste-crypto`, 12
`uste-txn`), rustfmt, clippy with warnings denied, rustdoc, documentation validation, the 62-task
dependency graph and the R0/reference fixture suites.

## Remaining T-15 work

Complete the injected staging/finalization/inventory/commit fault matrix, authenticate malformed
inventory and chunk cases, and record measured peak RSS under a larger streamed object. T-16 owns
authorization and actual-byte quota policy; T-17 owns
artifact records; T-35 owns durable orphan enumeration and reclamation.
