# Decision 0163 — Authorized packed cache configuration

Date: 2026-09-19

Status: implemented and locally verified; T-20 qualification remains open.

Integrate Decision 0162 into the restricted packed graph read facade as an explicit trusted
constructor option. Preserve the existing constructor's uncached behavior for compatibility,
cold diagnostics and independent fault/reference tests. A new cache-budget constructor admits
storage's fixed minimum/maximum once; consumer read requests cannot alter cache or proof budgets.
The trusted experimental `AuthorizedPackedReadState` implementation contract gains an optional
cache argument; consumer request/output types, the v1 reader and pinned M1 interfaces do not change.

Current policy/readiness, namespace, typed target permissions and pre-read cancellation are
checked before cache access. Cached plaintext is not authority: embedded references and expansion
candidates are filtered using the current policy on every request, including fully warm reads.
Pending graph repair, uncertain commit and mismatched policies remain fail-closed.

Point, historical and adjacency/evidence-support reads share a facade-owned bounded cache.
Proof-work limits remain identical to uncached execution, including aggregate expansion work.
Cache locking failure fails closed; observed cancellation remains sticky and no partial successful
result is returned. The immutable coordinator borrow prevents this facade from spanning a write.

Cache diagnostics and clearing require current namespace ManageSchema authority. Disabled cache
diagnostics return None; clearing a disabled cache is an authorized no-op. Counters are cumulative
and cardinality-sensitive, not physical device accounting. Clearing affects USTE plaintext only,
not host caches. Warm reads may use authenticated immutable bytes after a later disk mutation;
clear/uncached reads reauthenticate. No erasure or freshness guarantee is inferred from cache hits.

Verify cold/warm/uncached reference equality for points, history and expansions; hidden references,
permission/cancellation refusal before cache use, maintenance-only diagnostics, exact work limits,
late corruption and all observed read faults. Keep existing uncached tests unchanged. Native
packed fixture construction, origin rebuild and qualifying larger-than-memory campaigns remain
separate work; this decision does not change benchmark requirements or complete T-20.

Six integration tests verify the above boundaries, including 279 observed read-error/crash cases
with cold recovery, ordinary-error retries retaining only complete authenticated pages, 25,600-byte
and 64 KiB eviction fixtures, exact point/history/expansion work limits and late ciphertext mutation
in five graph families. The full workspace gate passes 647 tests across 47 executables plus Clippy
and documentation builds. Exact commands, tested baseline and measured resource limits are in PROGRESS.
