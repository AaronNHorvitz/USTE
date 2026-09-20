# Decision 0196: Explicit buffered graph and coordinator staging

Date: 2026-09-20

Status: Accepted and locally verified opt-in domain integration.

Add an optional staging cache byte budget to packed graph and coordinator staging limits.
`None` retains the existing uncached execution. `Some` must fit the existing packed-cache
limits and selects Decisions 0194–0195's fresh operation-local cache for each private batch.
At direct staging entry, admit invalid cache configuration before certificate/index I/O. Do not share pages across
families, batches, transactions or subsequent recovery attempts.

Carry this explicit option through graph genesis/delta/live publication and coordinator
primary/quota prefix staging, including their existing streamed suffix/rebase callers. Keep
all proof-work, aggregate page/write, delta, owner, quota, certificate and nonce limits.
Neither a cache hit nor a smaller actual read count increases permitted logical work.
Graph reports aggregate checked cache counters and maximum accounted residency; coordinator
reports retain fixed per-family cache reports. These are not physical I/O or total RSS.

Exact deltas, authenticated receipts, first-owner selection, authorization, retry/collision
semantics and terminal-only publication remain unchanged. Private-stage errors can leave
unreferenced packs, never a successful partial domain state. Keep uncached reference and
fault tests; add buffered reference/budget/corruption/fault/recovery coverage before selecting
this option in native development runs. No benchmark target, qualification gate, M1 consumer
contract or persistent format is changed.

Prototype Rust callers constructing these public limit structs must supply the new option;
`None` preserves their prior execution. The pinned memory-adapter consumer contract is unchanged.
