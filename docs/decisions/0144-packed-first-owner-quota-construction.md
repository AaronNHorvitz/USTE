# Decision 0144 — Packed first-owner quota construction

Date: 2026-09-19

Status: accepted and locally verified T-20 private projection; live integration remains open.

Add a separate opt-in packed quota profile, SHA-256 of `USTE coordinator-packed-usage-v1`:
`d0fe0c607df22bcb3ffadacac442ed0f17d391ec5951e1467a2f3fafb3254401`.
Preserve Decision 0111's first-owner charging semantics, using three explicit families: family 1
has the single key `packed-usage-v1` with owner count, namespace bytes and principal count as three
big-endian u64 values; family 2 maps principal digest to count/bytes as two u64s; family 3 maps
principal digest plus blob ID to the unchanged canonical 80-byte owner value. Empty principal and
owner families remain explicit, and zero-byte blobs still count as owners.

Private staging takes an independently journal-validated packed primary prefix and its exact
authenticated target transaction. Genesis starts at revision one; later steps require the prior
opaque quota projection at exactly the preceding revision. This entry point does not bootstrap
a missing populated projection or treat missing accounting as zero. Existing v1 bootstrap and
rebuild APIs are unchanged; packed arbitrary-base rebuilding remains separate work.

Authenticate the primary's exact target retry outcome even for an empty inventory, refusing
foreign primary receipts before staging. Read the primary owner and witness for each bounded
inventory reference. Only owners whose
first-reference witness equals the target revision add charges, under the target principal.
The counted additions must exactly bridge prior and target primary owner counts. Update one
principal aggregate and its sorted new-owner entries with exact before/after copy-on-write
batches; update the metadata head and use checked count/byte arithmetic throughout. No prefix
is returned until all three durable stages succeed. A failure leaves the prior primary/quota
pair usable and publishes no intermediate root.

The resulting opaque projection binds the exact primary scope, certificate/revision and four
logical family commitments. Privileged usage reads recheck that pairing and authenticate the
metadata head and principal lookup before returning charges. They do not grant InspectQuota
authority or bypass current policy, reservation/reconciliation or revocation requirements.

Certificate, reference, owner, per-lookup and per-batch bounds remain explicit. Reports cover
successful primary/principal lookups and three batches, not complete filesystem I/O. Independent
cold bijection/aggregate admission, populated rebuild, live authorized integration and benchmark
qualification remain required. No v1/M1 schema or benchmark threshold changes.
