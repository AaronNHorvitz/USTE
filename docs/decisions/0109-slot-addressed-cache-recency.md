# Decision 0109 — Slot-addressed cache recency

Date: 2026-09-19

Status: implemented and locally verified; native comparison pending. T-20 remains open.

Replace Decision 0086's second ordered map for LRU age with safe slot-addressed predecessor/next
links. Keep one ordered map from the complete existing cache identity to a slot index. A hit does
one logarithmic key lookup and constant-time link updates, then uses that exact slot to borrow
the page; it no longer removes/inserts an age-tree entry or repeats the key lookup. Touching the
newest slot requires no link changes. This is an internal representation change, not a new cache
policy, disk format, authorization shortcut or performance qualification.

Slots contain the immutable zeroizing page owner and its validated layout. Full-cache insertion
reuses the oldest slot, drops/zeroizes its prior page and invalidates that layout. No stale slot
handle escapes a cache borrow. Before capacity is reached, a fallible vector reservation precedes
slot insertion; links and map entries are private, safe-Rust-only implementation state. Clear
drops both map and vector, including unused vector capacity. Preserve clock-overflow clearing,
hit/miss/eviction counters, exact LRU ordering, duplicate/size rejection before clock advance,
and complete context validation on each read. Failed/successful primitive telemetry is unchanged.

Keep the existing 8 KiB fixed plus 16 KiB page/1 KiB metadata logical allowances and 3,854-page
default capacity. Tests count actual reserved slot capacity and conservative doubled inline map
entries against those allowances, in addition to inline type-size checks and independent LRU
traces. These are not exact allocator/RSS measurements; process-group limits remain mandatory.
No cache or request budget is raised to accommodate the representation.

Require the existing 20,000-access independent vector traces, context separation, malformed-page
and layout-memoization tests, clock overflow, duplicate admission and encrypted eviction cases.
Add sustained hit/eviction checks proving stable slot identity on hits, bounded reused capacity
and capacity release on clear. Run full affected core/native regression before commitment, then
compare exact native oracle/output/page work and measured resources against the preserved baseline.
No speedup, pressure result or T-20 completion is inferred from the algorithm alone.

The 15 focused parser/cache tests and encrypted dense-page/eviction case pass in the unoptimized
profile. Full assertion-enabled workspace regression passes 382 tests, including 123 storage
tests and all fault matrices, with no ignores. Native regression passes 51 active unit and three
process tests; its two existing exact-profile oracle ignores remain. Strict workspace/native
Clippy and storage rustdoc pass. PROGRESS.md records commands, versions and resource limits.
