# Graph terminal-root delta evidence

Decision 0033 connects bounded graph transaction changes to the authenticated storage merge while
preserving the journal as the only transaction authority. The public maintenance flow prepares an
opaque no-I/O delta plan against an admitted current `graph-state-v1` root, commits normally, then
requires the exact durable outcome before merging and independently validating all eight terminal
families.

Focused verification on 2026-09-17:

~~~text
cargo test -p uste-graph --test disk_index graph_state_delta_root_matches_full_projection_and_reconstructs -- --nocapture
# 1 passed; 0 failed
cargo test -p uste-graph --test disk_index
# 3 passed; 0 failed
cargo clippy -p uste-graph --all-targets -- -D warnings
# passed
bash scripts/check.sh
# workspace format/clippy/test/doc pass; 253 workspace tests and all isolated acceptance suites pass
~~~

The fixture begins with records, evidence, an accepted relationship and policy at revision three,
then publishes and semantically admits a full base root. A one-delta preparation budget fails before
record encoding. A count limit of 18 passes the seven-delta mandatory preflight but fails while the
19th retained coalesced delta is charged; an exact-minus-one logical-byte limit also fails, while
the exact 19-delta/logical-byte budget succeeds. Revision four simultaneously replaces an entity property with a record
reference, retracts the accepted relationship, creates an evidence-backed assertion with two graph
roles to the same target, and replaces policy. A corrupted outcome digest is rejected before merge.

An undersized per-family merge limit then fails after the durable commit. The live state remains at
revision four, no target root is visible, and restart replays the committed transaction exactly.
The test process deliberately retains the non-durable plan across the storage-adapter restart;
because publication borrows it, an in-process retry under adequate limits is possible without
pretending the journal transaction failed. Real process loss cannot recover this opaque plan and
must use a complete-root rebuild from recovered state.

The valid retry copy-merges every family, applies canonical insertions/replacements/tombstones,
removes now-empty outgoing/incoming runs, verifies the six remaining descriptors against the live
post-commit graph and publishes one exact certificate-bound root. The test admits that root,
publishes a separate complete projection at the same anchor, admits both, reconstructs the delta
candidate to exact state equality and reopens the journal after simulated restart with both roots
still semantically admissible. Storage-level late-corruption and ten create/write/size/sync crash
boundaries remain covered by Decision 0032's merge tests.

This evidence does not close T-20. Delta preparation is transaction-bounded, and storage merge has
bounded buffers, but the current reducer and independent expected-descriptor pass retain and scan
the full graph in memory. Decision 0034 subsequently makes composite ingest preparation
request-bounded too, while root discovery/reconstruction, live reducer maps and coordinator
metadata remain full-memory. Decision 0035 later adds a bounded explicit-I/O current-record proof
and pure preparation phase, but does not yet derive this root delta from its partial view. The
derived plan limit is debited during retained family-map construction,
but graph prepare and one-record reference-role coalescing remain under the graph's existing
operation/reference caps. No allocator/RSS proof or BM-01/BM-06 result is claimed.
