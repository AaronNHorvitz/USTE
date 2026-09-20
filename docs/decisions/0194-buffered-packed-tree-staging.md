# Decision 0194: Fresh buffered packed-tree staging

Date: 2026-09-20

Status: Accepted and locally verified opt-in maintenance primitive.

Add `JournalStore::stage_packed_tree_batch_proven_buffered` alongside the unchanged uncached
API. The caller supplies the existing certified target, canonical/private staged base and exact
sorted deltas, plus an explicit cache byte budget within the existing packed-cache bounds.
Create a fresh operation-local cache bound to the validated certificate owner and current key
session. Never accept caller-warmed pages or retain this cache across stage calls.

Share all input admission, scope/profile/family/revision/owner checks and the existing bounded
copy-on-write planner/emitter. Reuse only immutable source pages during planning; every selected
node still validates its typed record, parent commitment, path and exact before/after transition.
Dirty-node handling and final reachable-node serialization remain unchanged. Finish all
preconditions before creating output, and return no successful staged prefix on failure.
No journal commit or root publication is added, and uncertain orphan packs are not deleted.

Every source-node inspection charges one page and its encoded bytes even on a cache hit.
Existing proof-work ceilings, metadata accounting, delta/pack bounds and nonce-session limits
remain unchanged. Cache residency has its separately declared budget; it is not hidden in the
64 MiB planner metadata allowance. Return fixed cache counters beside the original staging
result; neither is complete allocator, RSS or physical-device accounting. Cache pages drop at
operation end under the existing best-effort secret-buffer lifetime rules.

Require uncached/reference equivalence for insert/replace/delete operations, exact and minus-one
proof budgets, one-page/larger cache bounds, actual decrypt reduction, repeat-call fresh misses,
empty results, pre-I/O invalid budget/owner/lock refusal, late conflicts/ciphertext corruption,
and every observed input/output error/crash boundary with intact prior root and journal.
Preserve all existing uncached fault counts and tests. Domain/benchmark integration requires
separate work and evidence; this API alone makes no native throughput or qualification claim.
