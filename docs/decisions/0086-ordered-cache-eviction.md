# Decision 0086 — Ordered cache eviction and explicit metadata allowances

Date: 2026-09-18

Status: T-20 partial implementation; cache-pressure and larger-than-memory qualification remain open.

Replace the decrypted page cache's full-map minimum-age scan on every eviction with a second
ordered map from monotonic access stamp to complete cache key. Touch, insertion and oldest-entry
removal now perform bounded numbers of logarithmic ordered-map operations rather than scanning
every resident page to select an eviction. Both maps contain exactly one entry per resident page;
there is no accumulating access log. This adds per-hit bookkeeping and is not a claim that every
workload is faster. Qualification must measure the unchanged accepted workloads.

Preserve exact LRU selection, full database/namespace/epoch/writer/revision/profile/generation/
run/page identity, cache-hit-independent query admission and zeroizing page ownership. Clear drops
both maps. Access-stamp overflow clears both maps, counts discarded pages as evictions and resumes
at stamp one, for both touch and insertion. Duplicate insertion and malformed page size are rejected
before cache mutation; rejected input buffers are zeroized on ordinary drop. No format, journal,
authorization, durability or consumer query-result change follows from eviction bookkeeping.

The old 96-byte per-page allowance was smaller than even the inline cache key plus cached-page
fields. Charge a nonempty cache an 8 KiB fixed map/header allowance plus 16 KiB plaintext and 1 KiB
metadata allowance per resident entry. These are explicit logical admission allowances for two
ordered maps, not an allocator-layout proof, a hard process-memory cap or measured RSS. Inline-type
tests check that the allowances at least cover the owned key/value/header fields; qualification
must additionally measure allocator/process residency under pressure. Caller-selected process
group limits remain separate and unchanged.

The public minimum cache budget becomes `MIN_INDEX_CACHE_BYTES = 25_600`; previously accepted
budgets below this now return `ResourceLimit`. Empty/cleared cache accounting remains zero. Default
64 MiB and maximum 256 MiB budgets are unchanged. A default cache now admits 3,854 pages, including
both maps' allowances; do not increase the benchmark cache budget to recover the previous page
count. Existing 32 KiB-or-larger small caches remain valid.

Tests compare 20,000 deterministic accesses against an independent vector LRU at capacities
1, 2, 17 and 128, checking exact contents/order/hit/miss/eviction counts and both-map invariants
after every operation. Additional tests cover every identity field, exact minimum/minus-one,
maximum/plus-one, duplicate refusal, malformed size, clock overflow on touch and insert, clear,
inline layout allowance and full default-capacity eviction. Existing encrypted/fault/recovery,
authorization/reference and native oracle tests remain required; no benchmark target is lowered.
