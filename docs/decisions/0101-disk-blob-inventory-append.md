# Decision 0101 — Bounded disk-backed blob inventory append

Date: 2026-09-19

Status: implemented and locally verified. Not T-20 completion or benchmark qualification.

Add a separate trusted `JournalStore::append_group_with_disk_inventory` capability to the
Decision 0099 recovery mode. The legacy append API continues to refuse nonempty inventories in
that mode; it cannot choose caller limits or treat empty history maps as absence. No persisted
format, crypto context, inventory digest or certificate semantics change.

The admitted catalog remains an immutable historical base. A bounded overlay retains only new
reference identities and their first revisions, new inventory identities/counts/first revisions,
and changed namespace byte totals (including newly created zero-byte namespaces). Scalar counts
cover the base plus overlay. Explicit per-call limits cap each overlay, inventory references,
logical payload bytes verified and each disk lookup. Existing hard unique-reference, namespace
byte, binding, inventory and certificate-log limits remain. Retrying a previously committed
inventory increases bindings, not unique references, first revisions or namespace byte charges.

Prepare a private bounded copy of the overlay before publication; peak preparation includes both
copies, not zero metadata RAM. All catalog lookups, collision checks, namespace accounting and
overlay allocations occur before inventory publication/certification. Authenticate and rehash
every payload under the existing inventory publisher. Inventory commitment is proved through
base plus overlay before deciding whether malformed existing inventory bytes may be replaced;
committed malformed inventories still fail closed without truncation. Preserve the established
missing-object recreation behavior of the existing publisher, not a new repair policy.

After the certificate's successful data sync, install the prepared overlay and binding scalar
without fallible metadata reads or allocations. Do not populate the legacy inventory set.
Existing uncertain journal publication quarantines the owner; the new current-reference API
also refuses an uncertain owner before I/O. The API is privileged identity evidence, not consumer
authorization or a direct range-read grant. Exact committed payload reads still require the
existing certificate/inventory proof path.

`refresh_disk_blob_metadata` independently stages and admits the exact journal frontier with
Decision 0098's existing limits, compares all reconstructed counts against the live counts,
and publishes the terminal catalog before releasing any pending entries. Failure retains the
old base and overlay. Cold restart reconstructs state from authenticated journal authority and
never relies on an unpersisted overlay. Refresh is currently a bounded full-prefix rebuild with
the existing write amplification and proof re-read costs, not an incremental compactor.

Expose pending-entry residency separately from legacy-history residency and historical catalog
counts. No consumer facade exposes storage telemetry. Native graph fixtures continue to prohibit
blob inventories and have zero pending blob entries. The existing graph writer is not broadened.

Local tests cover base and pending collisions, first revisions, exact admission/refusal, repeated
inventories, zero-byte namespaces, first commit, refresh/restart and corrupted committed inventory
protection. Fault injection covers every observed append boundary for empty-base, populated-base
and pending-overlay cases; first/middle/last refresh mutation faults additionally verify overlay
retention. Existing full catalog mutation and cold-open matrices remain in the regression.

The coordinator write bridge and authorized staging-to-committed charge transfer are separate
remaining work. First-owner principal identity is owned by coordinator metadata, not this storage
catalog. M1's pinned consumer contract is unchanged. T-20/T-19, larger-than-memory/BM-01/BM-06
qualification, T-35 maintenance and every accepted later requirement remain open.
