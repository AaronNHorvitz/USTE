# Decision 0120 — Stream inventory-free recovery metadata to private disk roots

Date: 2026-09-19

Status: accepted implementation contract; T-20 and benchmark qualification remain open.

Add an opt-in paired-base recovery path that stages primary coordinator metadata after each
certified transaction instead of retaining a cumulative suffix map. It requires the admitted
domain and metadata bases at the same revision, zero existing blob owners and no optional
first-reference/quota projection. A certified inventory refuses before domain advancement; none
is silently stripped. Existing general recovery, first-owner behavior and optional accounting
paths are unchanged. This closed inventory-free contract matches the BM-01/BM-06 graph workloads,
not every future rich-content or blob-owning reducer.

For every authenticated suffix request, raw retry and transaction-ID lookups against the current
private disk base reject collisions, including an ID introduced in an earlier step of this same
recovery. The coordinator independently checks exact external-preparation binding and result
digest before domain advancement. The domain supplies the validated current logical-state anchor.
Bounded scratch merges append exactly one retry entry and one transaction-ID entry, update the
fixed metadata count and verify exact insertion/output counts. New roots have generation zero and
replace only the private handles. No cumulative outcome/transaction/owner map is populated.
The unchanged primary metadata profiles, outcome expiry fields and key ordering are retained.

`InventoryFreeMetadataRecoveryLimits` bounds total revisions and each per-family merge; the
existing transaction cursor has one shared encoded-range allowance. Domain proof/delta/merge
limits and the caller-owned bounded cache remain independent. At most one transaction's metadata
is processed at a time; this is not a claim that the caller's domain preparation or all filesystem
buffers occupy one record. Total merge work is bounded by admitted steps times per-step limits.
Immutable-family rewrite amplification and certificate-proof amplification remain real costs.

Only complete range consumption and terminal domain validation permit a coordinator to escape.
The graph publishes only its current terminal root. Private terminal metadata marks rebase required
and blocks new writes until its existing durable publication protocol succeeds. Exact retry and
authorized reads continue through the normal coordinator/facades, including expiry and revocation.
Raw diagnostics report private merge counts/logical bytes, not complete authenticated or device I/O.

The explicit native `bm06-linux-rebuild` now selects this path with zero outcome-overlay capacity.
Its 100-step reconstruction reports zero admitted overlays and 300 private metadata runs rather
than the Decision 0119 development path's 100 retained outcomes. The literal primary-family totals
are 10,400 output entries and 1,572,300 logical key/value bytes across those merges; encrypted bytes,
graph work and genesis staging are separate. The native two-record ceiling is unchanged. Ordinary
open/recover/resume keep their existing admitted-base/bounded-overlay semantics.

Reference tests cover zero/one/three suffix steps, ordinary and private origin bases, exact outcome
and transaction lookups, foreign-principal concealment, expiry, current authorization and durable
rebase. Forged but newly storage-authenticated histories duplicate retry keys or transaction IDs
from genesis or an earlier staged suffix step; both the streaming path and full-replay comparator
refuse them. Tests additionally cover real certified inventory refusal, wrong prepared binding,
paired-base/count/byte admission, late committed-certificate corruption and atomic report overflow.
The fault matrix schedules 936 I/O error/crash cases: 930 inject failures and six optional missing
operations have no successful crash-after boundary. Failed attempts preserve only old/terminal graph
roots and leave intermediate coordinator roots undiscoverable; fresh origin recovery succeeds.

This removes cumulative recovery metadata from this inventory-free path, not all USTE scalability
limitations. Generic inventory-bearing origin staging, immutable-run construction scaling, complete
measurement accounting and the exact-size reserved-host campaigns remain work. No acceptance target,
M1 handoff, authoritative baseline, physical erasure or release gate changes.
