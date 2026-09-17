# T-49 atomic import transaction evidence

Date: 2026-09-17 · implementation commit: `9ec08db` · local correctness evidence,
not independent security certification

## Implemented result

- Added safe-Rust `uste-ingest`, a single reducer owning coherent graph, spatial and private import-
  ledger state. Graph, spatial and ledger effects publish at one journal revision or not at all.
- Added closed canonical start/batch contracts, exact source and mapping blob/evidence bindings,
  stable batch identities, namespace-global source-event receipts, trusted advisory preview and
  typed durable job checkpoints.
- Composed import, graph and spatial authorization requirements without giving the reducer file,
  parser, model, credential or network capabilities.
- Enforced active graph-entity/evidence closure after composite batches and after graph-only
  changes. Import batches cannot mutate policy or delete referenced entities; T-50 retains
  transform-schema validation.
- Added canonical composite checkpoints and logical hashing for component snapshots plus private
  job, batch and source-event state. Restore rebuilds and validates ledger arithmetic, ordering,
  unique revisions, bindings and graph/spatial closure.
- Added the dedicated `SourceChanged` transaction outcome. Exact same-identity retries remain the
  coordinator's durable responsibility; alternate-identity batch replay conflicts in the ledger.

## Pinned limits and semantics

| Item | Value |
|---|---|
| Request profile | `UIRQ` 1.0, 16 MiB |
| Mapping profile | `TypedRecordsV1` |
| Rows per batch | 1–10,000 fully accepted typed rows |
| Jobs / batches | 10,000 / 1,000,000 |
| Rows per job | 1,000,000,000 |
| Composite checkpoint | 256 MiB |
| Retry identity | principal + idempotency key + transaction ID in `uste-txn` |
| Changed-input rule | exact source and mapping binding mismatch refuses continuation |

Every accepted batch contains a nonempty graph transaction, keeping the graph reducer at the
composite journal revision. Spatial changes are optional and may have a lower component frontier.
Preview executes the same candidate validation but does not reserve a revision or authorize a
later blind commit.

## Focused verification

~~~text
cargo test -p uste-ingest --locked
# 4 passed; 0 failed
cargo clippy -p uste-ingest --all-targets --locked -- -D warnings
# passed
bash scripts/check.sh
# 228 workspace tests passed; format, strict clippy, docs, references, task graph and R0 fixtures passed
/tmp/uste-t09-tools/bin/cargo-deny --locked check advisories licenses sources bans
# advisories ok; licenses ok; sources ok; bans ok
~~~

The tests reject every start/batch/graph request and checkpoint truncation, missing or inexact blob
inventories (including colliding blob IDs), changed source or mapping bindings, namespace-global
duplicate source events, dangling spatial graph references and graph deletion of live jobs or
retained spatial targets.
They prove preview leaves state unchanged, composite failure is atomic, batch/job checkpoints
round-trip with equal logical state, exact encrypted coordinator retry survives restart, a resumed
batch commits after encrypted `MemoryFileSystem` restart, alternate retry identity conflicts,
denied authorized commits publish nothing, and protected embedded job bindings are concealed.

## Deliberate boundaries

T-49 accepts already-typed records. Row payload digests are caller-declared commitments that the
reducer does not independently recompute from aggregate graph/spatial effects; T-54 must verify
canonical per-row envelopes before claiming mapping provenance. It does not parse CSV/JSON, execute a mapping manifest, advance
over rejected source rows, create error reports or provide a CLI; T-54 owns those features and the
item-linked exact-price fixture. Structural limits are not simultaneous capacity results; the
in-memory clone and 256 MiB cache can bind first. No RSS, throughput, concurrency, platform crash or
power-loss result is claimed. T-19 records R1 acceptance and operating limits. T-62 remains an
unrelated, unverified distribution prerequisite.
