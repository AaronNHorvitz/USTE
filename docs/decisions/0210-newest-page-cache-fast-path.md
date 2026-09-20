# Decision 0210: Newest page-cache fast path

Date: 2026-09-20

Status: Implemented; storage, native regression and strict checks passed.

After Decision 0209's complete workspace/native verification, avoid an ordered-map lookup when
the requested complete key already equals the newest resident slot's key. The existing slot
contains the full key; no new memoization, allocation or resident allowance is needed. A newest
hit already required no LRU link update. Empty/nonmatching lookups retain the ordinary map path.

This shared internal primitive serves both legacy and packed caches. It does not skip enclosing
context, live-owner, unlocked-session, slot, authorization or budget checks. Clock advancement,
hit/miss/eviction accounting, page retrieval, plaintext lifetime, clear/overflow behavior and
all resource limits remain unchanged. Exact full-key equality is mandatory; no hash/fingerprint
or partial-context shortcut is permitted. Private map/slot consistency and ordinary `Ord`/`Eq`
consistency are the same existing cache invariants.

Add a counted-ordering test proving 10,000 repeated newest hits require no tree comparisons and
do not grow allocated metadata, while non-newest hits still update the exact eviction order.
Cover empty/zero-capacity, duplicate refusal, misses, slot replacement, complete-key mismatch
and capacity-one links. Keep the existing independent LRU traces, context/owner/lock/clear,
overflow, accounting, encrypted corruption/fault and native regression tests unchanged.

Run the full storage suite, strict workspace checks and ordinary native regression serially
under the established resource limits. Decision 0209's 730-test workspace result remains its
exact prior baseline, not a claim that this later cache change was included. Any subsequent
native comparison must identify both increments and preserve the older measurement artifacts.
No benchmark target, qualifying reservation, M1 result or consumer interface changes.

All 231 storage tests passed, including both new fast-path cases and existing independent LRU,
context, encrypted fault and accounting regressions. Strict all-feature workspace Clippy and
warnings-denied docs passed. Final scope peak 1,588,989,952 bytes/zero swap. Ordinary native
regression passed 122 active tests, five unchanged opt-in ignores, and strict Clippy; its final
scope peak was 568,975,360 bytes/zero swap. No full-workspace test rerun or performance result
is implied by these checks; commands and exact baselines are recorded in PROGRESS.md.
