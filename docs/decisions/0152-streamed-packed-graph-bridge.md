# Decision 0152 — Streamed packed graph bridge

Date: 2026-09-19

Status: accepted and locally verified T-20 cache bridge; packed live/cold semantic integration open.

Introduce a separately typed private packed graph base constructed only by streaming an already
semantically admitted `GraphDiskBase`. Authenticate and exhaust every source family; stage bounded
insertion batches into eight explicit canonical packed families at the same certified revision.
Do not reconstruct complete graph maps, publish intermediate roots, or admit an arbitrary packed
root as valid graph state. Source bytes, counts, history and policy remain unchanged. Empty
families have explicit context-bound empty commitments. Late failure returns no graph capability;
unpublished cache packs may remain and the original source is never overwritten.

The index profile is SHA-256 of ASCII `USTE graph-packed-v1`; the new state-commitment profile is
SHA-256 of ASCII `USTE graph-ordered-state-v1`. Its digest is SHA-256 over
`USTE-GRAPH-ORDERED-STATE-V1\0`, database16, namespace16, revision-u64be, reducer-profile32,
index-profile32, then eight sorted families, each family-u8 / entries-u64be / logical-bytes-u64be /
canonical-commitment32. Physical locators, pack boundaries and publication generations are absent.
Never label this digest as the frozen graph logical-state-v1 hash. Preserve that source hash
separately and provide bounded streaming recomputation from the packed families for compatibility
checks; benchmark oracles retain their existing digest contract.

The family key/value schema remains graph-state-v1 (including its metadata key/version), not its
old immutable-run carrier. Explicit limits cover source scan entries/pages/logical bytes, batch
count/entries/bytes and packed read/write work. Compatibility export separately caps cursor work
and one history group's bytes required by the frozen v1 framing. These are admission budgets,
not measured RSS, complete physical I/O, or benchmark reservations. Reports include successful
source cursor and packed batch work, not certificate authentication or failed adapter calls.
The batch byte ceiling is checked before retaining each incoming entry; one separately bounded
source entry may be in flight. A batch whose bytes exceed its selected ceiling is refused, not
silently repartitioned.

This bridge is trusted migration of a disposable derived cache, not authoritative source migration,
cold packed semantic admission, live reducer integration or consumer authorization. Those paths
must preserve every graph constraint and current-policy rule before replacing native v1 paths.
Partition-independent commitments, exact v1 export, late corruption, failure/restart and explicit
resource refusals are required local evidence. T-20 and qualifying campaigns remain open.
