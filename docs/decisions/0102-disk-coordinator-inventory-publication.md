# Decision 0102 — Disk coordinator inventory publication

Date: 2026-09-19

Status: implemented and locally verified. Not T-20 completion or benchmark qualification.

Expose Decision 0101's explicit storage writer through separate trusted ordinary and externally
prepared disk-coordinator commit methods. Caller-supplied `DiskBlobAppendLimits` accompany the
existing coordinator lookup/cache and bounded outcome/first-owner overlays. Existing commit APIs
retain their original storage selection; no implicit admission limits or full-history fallback
are introduced. Fresh nonempty requests through the new capability require disk blob recovery
mode, otherwise returning `InvalidRequest` without publication or coordinator quarantine.

Reuse the existing shared commit admission. Exact retry still precedes transaction-ID collision,
new-write capacity, cancellation and preparation, including an unusable new-write storage limit
or an unused external preparation. Expired retries remain tombstones. Resolve exact first-owner
references from coordinator base/overlay before preparation, then use the storage base/overlay
for independent collision, inventory-protection and namespace-byte admission. A later principal
referencing a blob never replaces its first owner. Reborrow the same explicit metadata cache for
admission and storage publication; no complete coordinator or storage maps are reconstructed.

External preparation retains its reducer-owned binding check. The graph consumer writer remains
inventory-free. This raw bridge does not authorize uploads, references, quota transfers or policy
changes. The existing conservative publication error/`OutcomeUnknown` contract is unchanged;
uncertain coordinators refuse state and retry access until recovery. Resource refusal publishes
no outcome. Successful certification precedes ordinary reducer/outcome/first-owner publication,
with no postcertificate disk metadata lookup.

Add a trusted storage-catalog refresh forwarder and pending-entry residency diagnostic. Storage
refresh and coordinator metadata rebase are independent: each releases only its own pending
metadata after its own verified terminal root publication. Neither bypasses a domain's pending
state or the other's required retry/owner proofs. A failed storage refresh is derived-cache
maintenance failure, not reversal of an already certified transaction.

The synthetic counter fixtures independently verify ordinary and external preparation, exact
retry with cancellation and zero storage limits, collision, expiry, first ownership, principal-
isolated transaction lookup, quota totals, independent rebases, cold suffix replay and faults at
each observed publication/read boundary. Existing resident-storage fixture limits remain unchanged;
the new disk-storage fixture explicitly admits its three owners and four journal groups.

Authorized staging-to-committed quota transfer and generic consumer inventory publication remain
the next implementation work. No M1 interface change, AgentMage migration, executable release,
larger-than-memory evidence, or BM-01/BM-06 qualification follows. T-20/T-19 and the full later
spatial, geographic, navigation, physics, content, lifecycle and security roadmap stay open.
