# Decision 0018 — Authorization and exact-byte quota profile

Date: 2026-09-17

Status: accepted and locally qualified for the T-16 foundation. This refines Decisions 0003,
0005, 0007, 0016 and 0017 without defining graph records, durable policy-record mutations,
retention enforcement or process-authenticated IPC.

## Authority boundary

`uste-policy` is a safe-Rust, storage-independent default-deny kernel over `uste-types`. A trusted
local authentication adapter converts its own credential into an opaque 32-byte principal digest.
Only the kernel can construct an authenticated principal, and that principal is bound to its
issuing kernel instance. An identically configured foreign kernel or a caller-supplied digest,
record field, document instruction, blob token or namespace identifier is never authentication.
The trusted initializer authenticates before moving the kernel into the facade; the consumer
facade does not accept an authenticator.

The raw storage and transaction coordinators remain privileged implementation capabilities.
`uste-txn::AuthorizedCoordinator` is the mandatory consumer-facing facade. It owns the current
policy kernel and derives the principal written into `UTXN` groups from an authenticated principal;
the external transaction request contains no principal field. This preserves the format-1.0 group
bytes while removing principal spoofing from the authorized API. Trusted internal recovery replays
already authenticated groups without reauthorizing historical commits under today's policy.

The T-16 policy source is a trusted local adapter supplied at create/open. Missing current policy
fails closed. T-17 adds engine-native durable namespace-policy records and record-level transaction
requirements; restore/rollback policy-epoch enforcement remains lifecycle work. No allow-all
fallback is permitted while those later layers are absent.

## Permission and version profile

Permissions are independent bits. The format-1 action tags are:

| Tag | Action | Tag | Action |
|---:|---|---:|---|
| 0 | ReadRecord | 13 | ReadOwnOutcome |
| 1 | ReadHistory | 14 | ReadAllOutcomes |
| 2 | ExpandGraph | 15 | InspectQuota |
| 3 | Search | 16 | EvaluateBranch |
| 4 | Aggregate | 17 | Subscribe |
| 5 | ReadBlob | 18 | ManagePolicy |
| 6 | Export | 19 | ManageSchema |
| 7 | Commit | 20 | Import |
| 8 | StartUpload | 21 | Promote |
| 9 | ResumeUpload | 22 | ManageRetention |
| 10 | WriteUpload | 23 | ManageKeys |
| 11 | FinishUpload | 24 | ExecuteProcedure |
| 12 | AbortUpload |  |  |

Granting one action never implies another. A namespace grant is required first; a record rule may
only deny a namespace-granted action and cannot create authority. A namespace policy admits at
most 65,536 principals and a grant at most 100,000 record rules. Construction fails before partial
mutation at the next entry.

Policy versions are nonzero and replacements must name the exact current version and install a
strictly greater version. Every revision view and upload handle retains its issuing version. The
next engine call revalidates that lease; any policy replacement makes the old handle stale even if
a similar grant is later restored. A generic authorized view exposes only its revision, never the
reducer's complete snapshot; T-17 owns target-authorized record projections. Already returned
plaintext cannot be recalled. Worker, query and subscription handles must reuse this primitive in
their owning tasks.

Authorization occurs before scope comparison that could reveal an object, before reducer state,
blob indexes, filesystem objects, counts, topology, snippets or rankings. A denied existing object,
denied uncommitted object and denied foreign scope return the same content-free `Unauthorized`
class. This is semantic/output concealment, not a constant-time side-channel claim.

## Exact quota accounting

Policy limits can only lower `limits-v1`. `QuotaLimits` has private fields and checked construction;
it cannot raise the 16 MiB request, 1 TiB namespace logical staging/committed, 32-live-upload or
1 MiB range-read hard caps. The raw transaction and blob layers retain their tighter group-header
and 16 GiB single-blob checks.

Quota units are logical plaintext octets:

- request admission uses the exact canonical request slice length;
- streaming admission uses the change in `BlobUpload::accepted_bytes()`, including changes observed
  after a failed storage call;
- finalized-but-uncommitted bytes remain staged until durable abort, successful commit or future
  orphan reclamation;
- committed usage charges authenticated `BlobReference::byte_len()` once for each unique blob ID;
- a range call charges its output capacity against the per-call limit before storage access.

The facade tracks namespace and per-principal staging, committed bytes and live handles with checked
arithmetic. It admits at most 32 unresolved reservations, including dropped and zero-byte handles,
and indexes them by upload and blob identity so 100,000-reference inventory work remains bounded.
Commit inventories must contain either an exact blob finalized through the same
principal's authorized handle or an exact previously committed blob first published by that
principal. Successful commit moves, rather than duplicates, the staging charge. Retry and repeated
inventory references do not charge a unique blob twice. An uncertain result retains the staging
reservation until recovery decides the outcome.

The raw coordinator rebuilds first-publication ownership in journal order from authenticated group
principals and verified inventories. `AuthorizedCoordinator::new` reconstructs committed quota use
from that recovered set. Uncommitted upload ownership is not journaled in format 1.0. A recovered
coordinator therefore refuses every brand-new upload start: otherwise abandoned staging omitted
from the ledger could accumulate across restarts. A principal with `ResumeUpload` and the opaque
token may reconcile an upload only when it is already present in the coordinator ledger or storage
returns authenticated durable journal evidence. This evidence rule also applies before restart, so
`ResumeUpload` never implies `StartUpload`. Authenticated recovered bytes are charged to that
resumer and can be durably aborted or committed within the committed-byte limit. T-35 must add
complete orphan/reservation enumeration before post-restart new uploads can be enabled. The token
alone remains useless.

## Local qualification and remaining scope

T-16 tests cover default denial, independent action and record-deny behavior, hard-cap rejection,
policy-version invalidation, kernel-instance identity binding, principal-derived commit identity,
own-outcome concealment, denied
finalized/committed blob equivalence, exact boundary/+1 streaming quotas, live-upload backpressure,
durable abort release, commit conversion, restart reconstruction, post-failure accepted-byte
reconciliation, unresolved-reservation bounds and fail-closed post-restart admission. The storage
and policy crates have no network dependency. Same-scope fabricated tokens are denied under a
resume-only grant unless authenticated durable evidence exists.

This qualifies the namespace/blob/outcome portion of VT-06 and the policy primitive used by graph,
history, query, branch, export, worker and subscription code. T-17 and later feature owners must
authorize their concrete record expansions before state/index access and repeat VT-06 through those
real paths. T-16 does not claim an implemented graph, durable native policy administration,
authenticated IPC, constant-time denial, post-restart new-upload admission, orphan reclamation or
independent security assessment.
