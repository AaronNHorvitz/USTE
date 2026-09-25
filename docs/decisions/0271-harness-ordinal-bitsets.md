# Decision 0271: Ordinal bitsets in the BM-01 client traversal harness

Date: 2026-09-23

Status: Accepted for the development harness; no engine, benchmark-target or default change.

Decision 0268's retained-path profile of the Decision 0267 binary placed about 15% of retained four-hop
samples inside `BTreeMap::insert` called from the benchmark harness's own `execute_query_with`,
outside every engine read. The harness composes multi-hop traversal from one-hop authorized
reads and tracked visited relationships, reachable entities, seen entities and the next frontier
in `BTreeSet<u64>` collections; a four-hop success inserts up to about 90,000 relationship
ordinals and 20,000 entity ordinals per query into ordered maps.

Track those sets in fixed-width ordinal bitsets sized from the fixture profile, with the next
frontier collected into a vector and sorted ascending before it becomes the frontier. Every
relationship and entity ordinal is already range-checked against the profile before use. The
frontier is still expanded in ascending ordinal order, exactly as the ordered set iterated, so
the sequence of engine reads and therefore every cache counter is unchanged. Result lists are
produced by ascending bit scans, which is the order the ordered sets produced. Visit, result and
visit-limit accounting, the materializer cross-checks, the oracle comparison and the independent
oracle implementation are unchanged.

This is a change to measurement-harness overhead, not to the engine. It is disclosed as such
because the harness's own time is inside the measured per-query interval. Persistent bytes, graph
results, authorization, cache configuration and engine code are unchanged.

Verification: strict all-target Clippy of the bench crate passed; the release `engine-check`
at 20 and 1,000 entities and `packed-engine-check` at 20 entities reproduced every independent
oracle output through the changed traversal (384 queries each). Decision numbers 0269 and
0270 were assigned by the owner's concurrent roadmap and publication decisions, which also
renamed this lane's branch to `main`; this decision therefore takes the next free number.
