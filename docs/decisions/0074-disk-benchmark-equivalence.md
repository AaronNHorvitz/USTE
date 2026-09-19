# Decision 0074 — Disk-backed development oracle path and authorized cache controls

Date: 2026-09-18

Status: T-20 partial implementation; Linux runner integration and qualification remain open.

Add `disk-engine-check` to the existing benchmark experiment. Preserve the accepted materializer,
identity mapping, maximum-10,000-operation batches, 212-revision qualifying plan, query corpus,
independent oracle and output digest. Factor only the engine adjacency callback out of the existing
client-composed traversal; both engines retain the same global visit/result limits and checks.

Bootstrap a policy-only `GraphState`, publish graph/coordinator/transaction roots, drop it, and
restart before admitting any fixture records. Subsequent writes use `GraphDiskLiveState`,
`DiskCommitCoordinator` and `AuthorizedDiskWriter`, with bounded proof/delta/root publication and
metadata rebase after each batch. Final restart independently admits graph and coordinator roots
without reconstructing full logical maps; queries use `AuthorizedDiskReader`. No full graph
snapshot exists after policy bootstrap, and reopened coordinator overlays must be empty.

The new verifier keeps the existing 1,000-entity development ceiling and rejects qualifying size
before storage work. Its 20/200 fixture must match all 384 measured queries and the unchanged
`46f1bdb3138d6325e4c0f56b5fd3bbf5ff092d816e8a0f6c23acd15687b910b5` digest. This is a durable memory
fault-model adapter with development entropy/key wrapping and an in-process independent oracle,
not Linux durability or latency/RSS evidence. Storage's own certificate/blob metadata remains
resident. Reports explicitly retain `engine_benchmark:false` and disclose these boundaries.

Add trusted-construction cache sizing to `AuthorizedDiskReader`, retaining its 64 KiB default and
storage's existing inclusive maximum. The benchmark selects the accepted 64 MiB cache; consumers
cannot resize it through reads or change work limits. Candidate-dependent counters and clearing
require current `ManageSchema` authorization before locking/accessing cache state. Clearing zeroizes
the resident page buffers through the existing cache implementation, leaves cumulative counters
intact and makes no kernel/filesystem/controller/device-cache claim. Metadata outcome APIs retain
their original fixed private cache and lookup limits. Cache counts are not represented as complete
authenticated I/O accounting or qualifying timing measurements.

Tests preserve oracle equality, reject over-profile development requests, check cache size refusal,
deny report/clear without maintenance authority, preserve cache state on denial and retain counters
across an authorized clear. Existing exact-minus query work limits still count cache hits. This
does not change M1's pinned interface, qualification or handoff. Linux pending-root resume and the
exact five-window BM-01 campaign remain separate required work; no acceptance threshold changes.
