# Decision 0142 — Packed coordinator prefix construction

Date: 2026-09-19

Status: accepted and locally verified T-20 private reconstruction; live integration remains open.

Add an opt-in private coordinator metadata prefix with no complete retry, transaction or owner
maps. Its index profile is SHA-256 of `USTE coordinator-packed-v1`:
`67016efc82cd60fd6d22f1afd3e2b4f8198d59b264006e7d1b2eba9696adbf7d`.
Four explicit packed families use existing canonical codecs: family 1 is principal/idempotency
key to the 104-byte outcome; family 2 is transaction ID to principal plus outcome (136 bytes);
family 3 is blob ID to the 80-byte first-owner reference/principal; family 4 is blob ID to its
first certificate revision (eight-byte big endian). Empty families remain explicit.

Only an authenticated revision-one transaction can create a prefix. Each successor requires the
same scope and exactly the next revision of an opaque previously constructed prefix. Retry and
transaction entries use exact absence-before insertion, rejecting collisions without expiration
filtering during recovery. Each named inventory reference is looked up against the prior owner
tree; existing references must match exactly and retain their first principal/revision. New owners
and witnesses are inserted together. No intermediate manifest is published; any failure returns
no new prefix and leaves the prior one usable. A prefix is evidence of coordinator metadata
correspondence, not independent reducer correctness or consumer authorization.

Caller limits bound certificate authentication, each lookup and each of four copy-on-write
batches, total owners and references in this transaction. Reference admission is capped at 512,
matching the existing single-batch contract; larger inventories explicitly refuse this profile
and are not silently truncated. Only request-sized vectors and one immutable handle per family
are retained. Existing global outcome/owner maxima remain enforced. Successful-work reports
separate owner lookup work from the four batch reports; they are not complete adapter I/O metrics.

This is a private reconstruction capability, not an installed live coordinator base. Cold cache
admission, quota projection pairing, authorized/live integration, full accounting and BM-01/BM-06
remain required. No existing v1 format, expiry/authorization rule or M1 handoff changes.
