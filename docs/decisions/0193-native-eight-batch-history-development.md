# Decision 0193: Native eight-batch history development

Date: 2026-09-20

Status: Accepted; increased development ceiling locally verified, not benchmark-qualified.

Extend only packed native BM-06 admission to 4,096 records. Preserve 100 retained versions,
4,096 payload bytes per event, 512-record batches, current authorization, encryption, durable
flushes and nonce limits. The checkpoint is revision 793 and terminal revision 801; the final
generation contains eight batches. Model/legacy ceilings remain two. Qualifying 100,000 records
still refuse before I/O. This is not a memory-capacity or larger-than-memory qualification claim.

Retain the explicit 513-record test and share its scale harness with a new opt-in 4,096-record
case. Construct all 405,504 checkpoint versions, kill only the owned parked child after graph
publication at revision 794 before metadata rebase, and resume the remaining seven batches.
Verify all 409,600 versions and exact source-certificate prefixes. Explicitly replay all eight
tail groups from checkpoint 793 despite newer roots, then repeat resume and open with matching
terminal digests and no certificate appends. Retain synthetic databases and reports on either
success or failure; do not overwrite the earlier pinned evidence.

The supervising test drains stdout and stderr concurrently, retaining at most 256 KiB plus one
overflow-detection byte per pipe, to avoid deadlock as phase reports grow. Refuse oversized output;
keep existing process deadlines, owned-child kill/reap and bounded-memory semantics. A synthetic
shell-output case checks both pipes beyond ordinary pipe capacity and overflow refusal.

Run one job/test thread/heavy workload, under the established 3 GiB high/4 GiB maximum/512 MiB
swap scope, only after current RAM/swap/disk/workload checks. Keep the 1,800-second per-phase
and 5,400-second overall test deadlines. Existing 513-record resource/nonce measurements justify
attempting this bounded development step, not asserting that it will pass. Retain and investigate
failure rather than relaunching unchanged work, weakening a target or clearing nonce tracking.

Require actual passing evidence, source/binary hashes, nonce/phase observations and measured
resources before recording the ceiling as locally verified. All qualifying benchmark dimensions,
targets and reserved-host requirements remain unchanged; T-20/T-19 remain open.

The initial run constructed and verified checkpoint 793 but its fresh tail process failed
bootstrap binding. A retained-checkpoint open reproduced `USTE_BM06_PACKED_BINDING`: the binding
cursor's fixed 1 MiB budget omitted the certificate distance, which now exceeds 3 MiB. Add the
existing profile-derived maximum certificate-proof bytes, with checked arithmetic, separately
from the original 1 MiB small-policy-group allowance. Keep one selected transaction and all
identity/policy checks. This corrects work admission; no authentication or benchmark target is
relaxed. Unit arithmetic covers all development and qualifying profile sizes without running
the qualifying workload. The marker supervisor also retains bounded stderr on premature EOF,
with a direct early-exit regression, and caps marker lines at 4 KiB.

The corrected explicit run passed all five phases in 824.73 seconds. See
`docs/evidence/native-packed-history-4096-development.json` for exact source/binary provenance,
retained failure disposition, resource observations and full phase reports. Decision 0194's
later unverified core changes were not linked into this executable.
