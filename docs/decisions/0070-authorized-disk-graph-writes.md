# Decision 0070 — Authorized inventory-free disk graph writes

Date: 2026-09-18

Status: T-20 partial consumer implementation. Blob inventories/uploads and qualification remain open.

`AuthorizedDiskWriter` borrows the exclusive disk coordinator and current policy kernel mutably.
Trusted construction selects bounded proof/delta/publication limits and a private 64 KiB cache.
The request has no principal field: authentication supplies the journaled principal. Commit checks
namespace permission, exact committed policy, canonical request bytes, reducer-owned requirements
and namespace containment before clock or disk access. This first writer rejects every blob
inventory; it cannot bypass unfinished staging/owner/quota admission by accepting raw references.

Use Decision 0069's shared preflight before external graph preparation. Exact retries, expired
tombstones, transaction collisions and capacity refusal therefore precede proof reads, including
when expected graph versions have already advanced. Sample the adapter clock once and reuse that
observation for check and actual commit, preserving pre-preparation acceptance time. The actual
commit rechecks admission and validates the prepared proof; it still certifies before publishing
state. Cancellation follows the established retry/preparation/certification boundaries.

Graph preparation uses the existing bounded proof, delta and terminal-root merge implementations,
not a complete snapshot. Domain errors must be content-free. In particular, graph dependency
failures are mapped through the existing reducer error classification: hidden record identities
and dependency counts never escape in preparation diagnostics.

A pending graph state retains the policy from its certified transaction. Writer authorization
compares against that policy, and the kernel is synchronized immediately after certification,
before derived-root publication. A revoked principal cannot exploit a failed root repair to keep
writing. Reads still require a ready root and fail closed while it is pending.

`CommittedPolicy` and `CommittedPublication` errors explicitly carry the known durable outcome
plus the policy/repair failure. They are not pre-commit rejection or `OutcomeUnknown`. An exact
authorized retry can repair its own pending root without preparing the already-applied mutation;
an older retry does not repair or acknowledge a different pending write. A newly revoked caller
cannot retry through revoked authority; trusted recovery/repair remains available to the adapter.
Actual journal publication uncertainty still poisons the coordinator and returns `OutcomeUnknown`.

Tests cover denied targets/foreign authentication/byte quotas before I/O or clock access, read
failure before commit, independent reference result digests, one clock sample, exact retry with
cancellation and unusable preparation limits, transaction collision, expiry, preparation refusal,
content-free hidden-dependency failure, durable revocation despite root budget/I/O failure, exact
repair, journal-sync uncertainty and cold reference replay with authenticated principal identity.
Metadata rebase remains trusted maintenance; this writer does not automatically flush overlays.
No M1 API or pinned handoff changes, no authoritative migration and no production/BM qualification.
