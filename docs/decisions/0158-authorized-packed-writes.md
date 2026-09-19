# Decision 0158 — Authorized inventory-free packed writes

Date: 2026-09-19

Status: implemented and locally verified; T-20 qualification remains open.

Expose a restricted single-writer facade around the packed coordinator, not its raw reducer,
maintenance, journal or upload surfaces. Match the existing authorized disk-write contract:
derive principal identity from authentication; authorize namespace commit, every typed request
target and policy mutation; enforce request quota/scope and reject inventories before clock or
storage access. Trusted construction fixes proof/publication limits. Consumer requests cannot
override work ceilings or obtain dependency/cardinality diagnostics.

Sample acceptance time once. Run the shared retry/collision/cancellation preflight before any
graph proof preparation. Exact eligible retries preserve their original result even with tiny
preparation limits or cancellation. Fresh writes use explicit bounded packed proof preparation
and the existing certified commit path. Content-free graph error mapping is shared with v1.

The domain's committed policy includes a certified pending change. Synchronize durable policy
immediately after certification, before attempting derived-root repair, so revocation takes effect
even if that repair fails. Errors after certification carry the exact durable outcome; unknown
journal publication stays quarantined. An older retry does not repair a different pending graph
revision. Trusted maintenance/rebase/recovery remain outside this consumer facade.

Test unauthorized/foreign/revoked identities, target permissions, quota/scope and pre-I/O refusal,
retry/expiry/collision/cancellation, content-free hidden-dependency errors, policy replacement with
failed publication, ordinary I/O failure and uncertain commit, cold retry and reference outcomes.
Inventory-aware packed upload reservations, authorized packed queries, native integration and
qualifying campaigns remain separate work. This does not close T-20 or change the pinned M1 API.
