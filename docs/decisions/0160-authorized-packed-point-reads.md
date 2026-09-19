# Decision 0160 — Authorized packed point and historical reads

Date: 2026-09-19

Status: implemented and locally verified; T-20 qualification remains open.

Expose a restricted domain-read facade over a ready packed coordinator and the exact current
durable namespace policy. Authenticate and authorize namespace and typed request targets before
storage access. Trusted construction fixes work admission; callers cannot obtain raw tree handles,
maintenance, cache telemetry or work-budget overrides. Reuse current graph visibility rules:
embedded references are all-or-nothing, and historical reads use current authorization policy.

Current point reads use bounded packed lookup; historical reads use descending bounded seek to
the latest version at or before the requested certified revision. Reject future read views.
Check decoded identity/revision against the authenticated key and ready base. Pending publication
and outcome uncertainty cannot expose stale graph state. No graph-wide maps or v1 root forgery.

Cancellation observed before, during candidate authorization or after reading is sticky for the
request and returns no partial success. Initially support current/historical records only; graph
expansion remains explicitly unsupported until its aggregate bounded scan/lookup implementation.
Tests must cover reference equivalence, authorization/embedded references, temporal boundaries,
cancellation, limits, corrupt storage, faults, pending repair and cold reopen. Packed page caching,
native integration and larger-than-memory qualification remain separate work.
